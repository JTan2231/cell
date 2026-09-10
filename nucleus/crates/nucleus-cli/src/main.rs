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
    /// Own deployment admission holds and inspect runtime readiness.
    #[command(subcommand)]
    Maintenance(MaintenanceCommand),
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
enum MaintenanceCommand {
    /// Prove healthy auth and harness under the named sole deployment hold.
    Health {
        run_id: String,
    },
    Hold {
        run_id: String,
    },
    Status,
    Release {
        run_id: String,
    },
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
    /// Observe state, pending call IDs, final output availability, and terminal reason.
    Status { id: String },
    /// Emit one terminal observation or timeout; never cancels the job.
    Wait {
        id: String,
        #[arg(long, default_value_t = 60)]
        timeout: u64,
    },
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
        #[arg(long, default_value_t = 20)]
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
    /// Reinstall the exact retained generation under a drained deployment hold.
    Recover {
        #[arg(long)]
        daemon: PathBuf,
        #[arg(long)]
        codex: PathBuf,
        #[arg(long)]
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
        Command::Maintenance(command) => run_maintenance(command, &client, compact).await,
        Command::Jobs(command) => run_jobs(command, &client, compact).await,
        Command::Schemas(command) => run_schemas(command, &client, compact).await,
        Command::Toolsets(command) => run_toolsets(command, &client, compact).await,
        Command::ToolCalls(command) => run_tool_calls(command, &client, compact).await,
        Command::Manual => unreachable!("manual is handled before client creation"),
        Command::Auth(_) => unreachable!("auth commands are handled before client creation"),
        Command::Service(_) => unreachable!("service commands are handled before client creation"),
    }
}

async fn run_maintenance(
    command: MaintenanceCommand,
    client: &NucleusClient,
    compact: bool,
) -> Result<(), CliError> {
    match command {
        MaintenanceCommand::Health { run_id } => {
            let health = client.health().await?;
            require_maintenance_health(client, &health, &run_id).await?;
            print_json(&health, compact)
        }
        MaintenanceCommand::Hold { run_id } => {
            print_json(&client.maintenance_hold(&run_id).await?, compact)
        }
        MaintenanceCommand::Status => print_json(&client.maintenance_status().await?, compact),
        MaintenanceCommand::Release { run_id } => {
            print_json(&client.maintenance_release(&run_id).await?, compact)
        }
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
        JobsCommand::Status { id } => {
            print_json(&client.job_status(&JobId::new(id)).await?, compact)
        }
        JobsCommand::Wait { id, timeout } => print_json(
            &client
                .wait_job(&JobId::new(id), std::time::Duration::from_secs(timeout))
                .await?,
            compact,
        ),
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

async fn require_maintenance_health(
    client: &NucleusClient,
    _health: &nucleus_core::HealthResponseV1,
    owner: &str,
) -> Result<(), CliError> {
    client.health_for_deployment(owner).await?;
    Ok(())
}

async fn deployment_service_guard(
    paths: &ServicePaths,
    command: &ServiceCommand,
) -> Result<Option<cell_maintenance::Admission>, CliError> {
    let admission = if matches!(
        command,
        ServiceCommand::Install { .. } | ServiceCommand::Recover { .. } | ServiceCommand::Restart
    ) && let Ok(owner) = std::env::var("CELL_DEPLOYMENT_RUN_ID")
    {
        let client = NucleusClient::new(&paths.socket)?;
        let gate = cell_maintenance::Gate::new(paths.state_dir.join("deployment-maintenance"));
        let drained = match client.maintenance_status().await {
            Ok(status) => status.holds == [owner.as_str()] && status.drained,
            Err(_)
                if matches!(
                    command,
                    ServiceCommand::Install { .. } | ServiceCommand::Recover { .. }
                ) =>
            {
                let status = gate
                    .status()
                    .map_err(|error| CliError::ServiceUnhealthy(error.to_string()))?;
                status.holds == [owner.as_str()]
                    && status.drained
                    && service::offline_deployment_ready(paths)?
            }
            Err(error) => return Err(error.into()),
        };
        if !drained {
            return Err(CliError::ServiceUnhealthy(
                "deployment requires its sole drained admission hold".into(),
            ));
        }
        Some(
            cell_maintenance::Gate::new(paths.state_dir.join("deployment-maintenance"))
                .enter_for(&owner)
                .map_err(|error| CliError::ServiceUnhealthy(error.to_string()))?,
        )
    } else {
        None
    };
    Ok(admission)
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep guarded service installation and health recovery in one dispatch"
)]
async fn run_service(command: ServiceCommand, compact: bool) -> Result<(), CliError> {
    let paths = ServicePaths::for_current_user()?;
    if matches!(command, ServiceCommand::Recover { .. })
        && std::env::var_os("CELL_DEPLOYMENT_RUN_ID").is_none()
    {
        return Err(CliError::ServiceUnhealthy(
            "service recovery requires its recorded deployment owner".into(),
        ));
    }
    let deployment_guard = deployment_service_guard(&paths, &command).await?;
    let command = match command {
        ServiceCommand::Recover {
            daemon,
            codex,
            codex_home,
        } => ServiceCommand::Install {
            daemon: Some(daemon),
            codex: Some(codex),
            codex_home: if paths.codex_home.join("auth.json").exists() {
                None
            } else {
                codex_home
            },
        },
        command => command,
    };
    match command {
        ServiceCommand::Recover { .. } => unreachable!("recovery normalized to installation"),
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
            let health =
                match wait_for_health(&installed.paths.socket, deployment_guard.as_ref()).await {
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
            let health = wait_for_health(&paths.socket, deployment_guard.as_ref()).await?;
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

async fn wait_for_health(
    socket: &Path,
    installation_guard: Option<&cell_maintenance::Admission>,
) -> Result<nucleus_core::HealthResponseV1, CliError> {
    let deadline = Instant::now() + SERVICE_START_TIMEOUT;
    let client = NucleusClient::new(socket)?;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(CliError::HealthTimeout("daemon did not answer".to_owned()));
        }
        let attempt_timeout = remaining;
        let last_error = match tokio::time::timeout(attempt_timeout, client.health()).await {
            Ok(Ok(health))
                if health.status == "ok"
                    && std::env::var_os("CELL_DEPLOYMENT_RUN_ID").is_none() =>
            {
                return Ok(health);
            }
            Ok(Ok(health)) => {
                if let Ok(owner) = std::env::var("CELL_DEPLOYMENT_RUN_ID") {
                    if installation_guard.is_some() {
                        // This process still holds enter_for's exclusive guard.
                        // No earlier admission or recovery guard can coexist.
                        let status = client.maintenance_status().await?;
                        if status.holds != [owner.as_str()]
                            || status.nonterminal_jobs != 0
                            || health.harness.is_none()
                            || health.harness_executable.is_none()
                            || !health.authentication.configured
                            || !health.authentication.authenticated
                            || health.detail.is_some()
                            || !health.supported_protocol_versions.contains(&1)
                            || health
                                .execution
                                .is_none_or(|capacity| capacity.active_jobs != 0)
                        {
                            return Err(CliError::ServiceUnhealthy(
                                "held installation health did not verify its owner and runtime"
                                    .into(),
                            ));
                        }
                    } else {
                        require_maintenance_health(&client, &health, &owner).await?;
                    }
                    return Ok(health);
                }
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
