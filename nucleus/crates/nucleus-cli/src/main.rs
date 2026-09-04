mod service;

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand, ValueEnum};
use nucleus_client::{ClientError, NucleusClient};
use nucleus_codex::{CodexError, CodexHarness};
use nucleus_core::{
    AccountSnapshotQueryV1, JobId, JobRequestV1, JobState, ListJobsQueryV1, LogSchemaV1,
    LogsQueryV1, SchemaId, ToolCallId, ToolCallsQueryV1, ToolResultV1, ToolsetRegistrationV1,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::service::{ServiceError, ServicePaths};

const OPERATOR_MANUAL: &str = include_str!("../../../docs/operator-manual.md");
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Parser)]
#[command(name = "nucleus", version, about = "Run and observe local agent jobs")]
struct Cli {
    /// Override the per-user nucleusd Unix socket.
    #[arg(long, global = true)]
    socket: Option<PathBuf>,

    /// Emit compact JSON instead of indented JSON.
    #[arg(long, global = true)]
    compact: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the built-in operator manual as Markdown.
    Manual,
    /// Inspect daemon availability.
    Health,
    /// Read the authenticated Codex account owned by Nucleus.
    Account {
        /// Also read account token activity; failure is reported in usageError.
        #[arg(long)]
        include_usage: bool,
        /// Seconds to wait for Nucleus's credential operation; zero is nonblocking.
        #[arg(long, default_value_t = 0)]
        wait: u32,
    },
    /// Manage Nucleus-owned Codex authentication.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Submit and inspect agent jobs.
    #[command(subcommand)]
    Jobs(JobsCommand),
    /// Register and retrieve immutable decoder and tool schemas.
    #[command(subcommand)]
    Schemas(SchemasCommand),
    /// Register and inspect requester-owned dynamic toolsets.
    #[command(subcommand)]
    Toolsets(ToolsetsCommand),
    /// Service requester-owned dynamic tool calls.
    #[command(name = "tool-calls", subcommand)]
    ToolCalls(ToolCallsCommand),
    /// Install and control the per-user macOS background service.
    #[command(subcommand)]
    Service(ServiceCommand),
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    /// Run attended Codex login after active authentication sessions settle.
    Login {
        #[arg(long)]
        device_auth: bool,
        /// Exact Codex executable; defaults to `NUCLEUS_CODEX` or `PATH`.
        #[arg(long)]
        codex: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum JobsCommand {
    /// Submit an exact version-one job request.
    Submit {
        /// Request file, or '-' for standard input.
        #[arg(default_value = "-")]
        file: PathBuf,
    },
    /// Show one job, including its frozen request and attempts.
    Show { id: String },
    /// List jobs, optionally scoped to one requester.
    List {
        #[arg(long)]
        requester: Option<String>,
        #[arg(long, requires = "requester")]
        requester_id: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        state: Option<JobStateArgument>,
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: u32,
    },
    /// Read harness-output records in durable arrival order.
    Logs {
        id: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        /// Continue long-polling until the job is terminal.
        #[arg(long, short = 'f')]
        follow: bool,
        /// Print only each raw payload, one exact JSON value per line.
        #[arg(long)]
        payload_only: bool,
    },
    /// Request cancellation; repeated requests are idempotent.
    Cancel { id: String },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum JobStateArgument {
    Accepted,
    Running,
    WaitingOnRequester,
    Completed,
    Failed,
    Cancelled,
}

impl From<JobStateArgument> for JobState {
    fn from(value: JobStateArgument) -> Self {
        match value {
            JobStateArgument::Accepted => Self::Accepted,
            JobStateArgument::Running => Self::Running,
            JobStateArgument::WaitingOnRequester => Self::WaitingOnRequester,
            JobStateArgument::Completed => Self::Completed,
            JobStateArgument::Failed => Self::Failed,
            JobStateArgument::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug, Subcommand)]
enum SchemasCommand {
    /// Register an exact version-one schema document.
    Register {
        /// Schema registration file, or '-' for standard input.
        #[arg(default_value = "-")]
        file: PathBuf,
    },
    /// Retrieve a registered schema and its exact schema document.
    Get { id: String },
}

#[derive(Debug, Subcommand)]
enum ToolsetsCommand {
    /// Register an exact version-one toolset registration.
    Register {
        /// Registration file, or '-' for standard input.
        #[arg(default_value = "-")]
        file: PathBuf,
    },
    /// Show a registered toolset identity and digest.
    Show {
        provider: String,
        name: String,
        version: u32,
    },
}

#[derive(Debug, Subcommand)]
enum ToolCallsCommand {
    /// Read pending calls from one requester's durable mailbox.
    Pending {
        job: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
        /// Long-poll duration, capped at 60 seconds.
        #[arg(long, default_value_t = 0)]
        wait: u32,
    },
    /// Post an exact version-one result for a pending call.
    Respond {
        job: String,
        call: String,
        /// Result file, or '-' for standard input.
        #[arg(default_value = "-")]
        file: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    /// Install binaries and continuously run nucleusd as a user background service.
    Install {
        /// Source nucleusd binary; defaults to a sibling of this executable.
        #[arg(long)]
        daemon: Option<PathBuf>,
        /// Codex binary; resolved now and stored as an absolute plist argument.
        #[arg(long)]
        codex: Option<PathBuf>,
        /// Existing signed-in Codex home whose auth.json is copied into Nucleus-owned state.
        #[arg(long, value_name = "DIRECTORY")]
        codex_home: Option<PathBuf>,
    },
    /// Print launchd state and daemon health.
    Status,
    /// Terminate the current daemon and ask launchd to start it again.
    Restart,
    /// Remove the background service and installed binaries, retaining all state/logs.
    Uninstall,
}

#[derive(Debug, Error)]
enum CliError {
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error(transparent)]
    Codex(#[from] CodexError),
    #[error("unable to read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("unable to write output: {0}")]
    Output(#[source] io::Error),
    #[error("unable to encode output: {0}")]
    Encode(#[source] serde_json::Error),
    #[error(
        "--socket does not apply to service commands; the LaunchAgent uses the standard per-user socket"
    )]
    ServiceSocketOverride,
    #[error("nucleusd did not become healthy before the service-start deadline: {0}")]
    HealthTimeout(String),
    #[error("nucleusd reported an unhealthy state: {0}")]
    ServiceUnhealthy(String),
    #[error(
        "new service was unhealthy ({health}); restoring the previous installation also failed: {rollback}"
    )]
    InstallHealthRollback { health: String, rollback: String },
    #[error("new service was unhealthy ({0}); the installation was rolled back")]
    InstallUnhealthyRestored(String),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstalledOutput<'a> {
    service: &'a str,
    daemon: &'a Path,
    cli: &'a Path,
    state: &'a Path,
    database: &'a Path,
    socket: &'a Path,
    logs: &'a Path,
    codex: &'a Path,
    codex_home: &'a Path,
    health: nucleus_core::HealthResponseV1,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceStatusOutput<'a> {
    loaded: bool,
    target: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    health: Option<nucleus_core::HealthResponseV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    health_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UninstalledOutput<'a> {
    service: &'a str,
    removed: bool,
    retained_state: &'a Path,
    retained_logs: &'a Path,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nucleus: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), CliError> {
    let compact = cli.compact;
    match cli.command {
        Command::Manual => print_manual(),
        Command::Service(command) => {
            if cli.socket.is_some() {
                return Err(CliError::ServiceSocketOverride);
            }
            run_service(command, compact).await
        }
        Command::Auth(command) => {
            if cli.socket.is_some() {
                return Err(CliError::ServiceSocketOverride);
            }
            run_auth(command).await
        }
        command => {
            let client = match cli.socket {
                Some(socket) => NucleusClient::new(socket)?,
                None => NucleusClient::for_current_user()?,
            };
            run_api(command, client, compact).await
        }
    }
}

async fn run_api(command: Command, client: NucleusClient, compact: bool) -> Result<(), CliError> {
    match command {
        Command::Health => {
            let health = client.health().await?;
            print_json(&health, compact)?;
            if !health.accepting_jobs || health.status != "ok" {
                return Err(CliError::ServiceUnhealthy(format!(
                    "daemon reported status {:?} and acceptingJobs={}",
                    health.status, health.accepting_jobs
                )));
            }
            Ok(())
        }
        Command::Account {
            include_usage,
            wait,
        } => print_json(
            &client
                .account_snapshot(&AccountSnapshotQueryV1 {
                    include_usage,
                    wait_seconds: wait,
                })
                .await?,
            compact,
        ),
        Command::Jobs(command) => run_jobs(command, &client, compact).await,
        Command::Schemas(command) => run_schemas(command, &client, compact).await,
        Command::Toolsets(command) => run_toolsets(command, &client, compact).await,
        Command::ToolCalls(command) => run_tool_calls(command, &client, compact).await,
        Command::Manual => unreachable!("manual is handled before client creation"),
        Command::Auth(_) => unreachable!("auth commands are handled before client creation"),
        Command::Service(_) => unreachable!("service commands are handled before client creation"),
    }
}

fn print_manual() -> Result<(), CliError> {
    io::stdout()
        .lock()
        .write_all(OPERATOR_MANUAL.as_bytes())
        .map_err(CliError::Output)
}

async fn run_auth(command: AuthCommand) -> Result<(), CliError> {
    let paths = ServicePaths::for_current_user()?;
    service::prepare_codex_home(&paths)?;
    match command {
        AuthCommand::Login { device_auth, codex } => {
            let codex = service::find_codex(codex.as_deref())?;
            let status = CodexHarness::with_codex_home(codex, &paths.codex_home)
                .login(device_auth)
                .await?;
            if status.success() {
                Ok(())
            } else {
                Err(CliError::ServiceUnhealthy(format!(
                    "Codex login exited with status {status}"
                )))
            }
        }
    }
}

async fn run_jobs(
    command: JobsCommand,
    client: &NucleusClient,
    compact: bool,
) -> Result<(), CliError> {
    match command {
        JobsCommand::Submit { file } => {
            let request: JobRequestV1 = read_json(&file)?;
            print_json(&client.submit_job(&request).await?, compact)
        }
        JobsCommand::Show { id } => print_json(&client.get_job(&JobId::new(id)).await?, compact),
        JobsCommand::List {
            requester,
            requester_id,
            parent,
            state,
            after,
            limit,
        } => {
            let query = ListJobsQueryV1 {
                requester_program: requester,
                requester_id,
                parent: parent.map(JobId::new),
                state: state.map(Into::into),
                after: after.map(JobId::new),
                limit: Some(limit),
            };
            print_json(&client.list_jobs(&query).await?, compact)
        }
        JobsCommand::Logs {
            id,
            after,
            limit,
            follow,
            payload_only,
        } => {
            follow_or_print_logs(
                client,
                JobId::new(id),
                LogsQueryV1 {
                    after,
                    follow,
                    limit: Some(limit),
                },
                payload_only,
                compact,
            )
            .await
        }
        JobsCommand::Cancel { id } => {
            print_json(&client.cancel_job(&JobId::new(id)).await?, compact)
        }
    }
}

async fn follow_or_print_logs(
    client: &NucleusClient,
    job_id: JobId,
    mut query: LogsQueryV1,
    payload_only: bool,
    compact: bool,
) -> Result<(), CliError> {
    if !query.follow && !payload_only {
        return print_json(&client.logs(&job_id, &query).await?, compact);
    }

    let stdout = io::stdout();
    let mut output = stdout.lock();
    loop {
        let page = client.logs(&job_id, &query).await?;
        let empty = page.records.is_empty();
        for record in &page.records {
            if payload_only {
                output
                    .write_all(record.payload.get().as_bytes())
                    .and_then(|()| output.write_all(b"\n"))
                    .map_err(CliError::Output)?;
            } else {
                serde_json::to_writer(&mut output, record).map_err(CliError::Encode)?;
                output.write_all(b"\n").map_err(CliError::Output)?;
            }
        }
        output.flush().map_err(CliError::Output)?;
        query.after = page.next_sequence;

        if !query.follow {
            break;
        }
        if empty {
            let job = client.get_job(&job_id).await?;
            if job.summary.state.is_terminal() {
                break;
            }
            // The daemon normally long-polls. This small delay also prevents a
            // tight loop if it returns an empty nonterminal page early.
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    Ok(())
}

async fn run_schemas(
    command: SchemasCommand,
    client: &NucleusClient,
    compact: bool,
) -> Result<(), CliError> {
    match command {
        SchemasCommand::Register { file } => {
            let schema: LogSchemaV1 = read_json(&file)?;
            print_json(&client.register_schema(&schema).await?, compact)
        }
        SchemasCommand::Get { id } => {
            print_json(&client.get_schema(&SchemaId::new(id)).await?, compact)
        }
    }
}

async fn run_toolsets(
    command: ToolsetsCommand,
    client: &NucleusClient,
    compact: bool,
) -> Result<(), CliError> {
    match command {
        ToolsetsCommand::Register { file } => {
            let registration: ToolsetRegistrationV1 = read_json(&file)?;
            print_json(&client.register_toolset(&registration).await?, compact)
        }
        ToolsetsCommand::Show {
            provider,
            name,
            version,
        } => print_json(
            &client.get_toolset(&provider, &name, version).await?,
            compact,
        ),
    }
}

async fn run_tool_calls(
    command: ToolCallsCommand,
    client: &NucleusClient,
    compact: bool,
) -> Result<(), CliError> {
    match command {
        ToolCallsCommand::Pending { job, after, wait } => print_json(
            &client
                .pending_tool_calls(
                    &JobId::new(job),
                    &ToolCallsQueryV1 {
                        after,
                        wait_seconds: wait,
                    },
                )
                .await?,
            compact,
        ),
        ToolCallsCommand::Respond { job, call, file } => {
            let result: ToolResultV1 = read_json(&file)?;
            print_json(
                &client
                    .post_tool_result(&JobId::new(job), &ToolCallId::new(call), &result)
                    .await?,
                compact,
            )
        }
    }
}

async fn run_service(command: ServiceCommand, compact: bool) -> Result<(), CliError> {
    let paths = ServicePaths::for_current_user()?;
    match command {
        ServiceCommand::Install {
            daemon,
            codex,
            codex_home,
        } => {
            let installed = service::install(
                paths,
                daemon.as_deref(),
                codex.as_deref(),
                codex_home.as_deref(),
            )?;
            let health = match wait_for_health(&installed.paths.socket).await {
                Ok(health) => health,
                Err(health_error) => {
                    if let Err(rollback) = installed.rollback() {
                        return Err(CliError::InstallHealthRollback {
                            health: health_error.to_string(),
                            rollback: rollback.to_string(),
                        });
                    }
                    return Err(CliError::InstallUnhealthyRestored(health_error.to_string()));
                }
            };
            print_json(
                &InstalledOutput {
                    service: service::SERVICE_LABEL,
                    daemon: &installed.paths.daemon,
                    cli: &installed.paths.cli,
                    state: &installed.paths.state_dir,
                    database: &installed.paths.database,
                    socket: &installed.paths.socket,
                    logs: &installed.paths.log_dir,
                    codex: &installed.codex,
                    codex_home: &installed.codex_home,
                    health,
                },
                compact,
            )
        }
        ServiceCommand::Status => {
            let status = service::status()?;
            let (health, health_error) = if status.loaded {
                let client = NucleusClient::new(&paths.socket)?;
                match tokio::time::timeout(Duration::from_secs(10), client.health()).await {
                    Ok(Ok(health)) => (Some(health), None),
                    Ok(Err(error)) => (None, Some(error.to_string())),
                    Err(_) => (None, Some("health request exceeded ten seconds".to_owned())),
                }
            } else {
                (None, Some(status.details))
            };
            print_json(
                &ServiceStatusOutput {
                    loaded: status.loaded,
                    target: &status.target,
                    health,
                    health_error,
                },
                compact,
            )
        }
        ServiceCommand::Restart => {
            service::restart()?;
            let health = wait_for_health(&paths.socket).await?;
            print_json(&health, compact)
        }
        ServiceCommand::Uninstall => {
            service::uninstall(&paths)?;
            print_json(
                &UninstalledOutput {
                    service: service::SERVICE_LABEL,
                    removed: true,
                    retained_state: &paths.state_dir,
                    retained_logs: &paths.log_dir,
                },
                compact,
            )
        }
    }
}

async fn wait_for_health(socket: &Path) -> Result<nucleus_core::HealthResponseV1, CliError> {
    let deadline = Instant::now() + SERVICE_START_TIMEOUT;
    let client = NucleusClient::new(socket)?;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(CliError::HealthTimeout("daemon did not answer".to_owned()));
        }
        let attempt_timeout = remaining;
        let last_error = match tokio::time::timeout(attempt_timeout, client.health()).await {
            Ok(Ok(health)) if health.status == "ok" => return Ok(health),
            Ok(Ok(health)) => {
                return Err(CliError::ServiceUnhealthy(format!(
                    "daemon reported status {:?}",
                    health.status
                )));
            }
            Ok(Err(error)) => error.to_string(),
            Err(_) => format!("health request exceeded {} ms", attempt_timeout.as_millis()),
        };
        if Instant::now() >= deadline {
            return Err(CliError::HealthTimeout(last_error));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn read_json<T>(path: &Path) -> Result<T, CliError>
where
    T: DeserializeOwned,
{
    let label = path.display().to_string();
    let bytes = if path == Path::new("-") {
        let mut bytes = Vec::new();
        io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|source| CliError::Read {
                path: "standard input".to_owned(),
                source,
            })?;
        bytes
    } else {
        fs::read(path).map_err(|source| CliError::Read {
            path: label.clone(),
            source,
        })?
    };
    serde_json::from_slice(&bytes).map_err(|source| CliError::Json {
        path: if path == Path::new("-") {
            "standard input".to_owned()
        } else {
            label
        },
        source,
    })
}

fn print_json<T>(value: &T, compact: bool) -> Result<(), CliError>
where
    T: Serialize,
{
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if compact {
        serde_json::to_writer(&mut output, value).map_err(CliError::Encode)?;
    } else {
        serde_json::to_writer_pretty(&mut output, value).map_err(CliError::Encode)?;
    }
    output.write_all(b"\n").map_err(CliError::Output)
}
