mod budget;
mod config;
mod protocol;
mod report;
mod types;

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::time::Duration;

use clap::Parser;
use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{AccountSnapshotQueryV1, JobState, ListJobsQueryV1, LogsQueryV1};
use thiserror::Error;

use crate::budget::BudgetReport;
use crate::config::UsageConfig;
use crate::protocol::AccountSnapshot;
use crate::report::{NucleusObservation, ReportScope};

const NUCLEUS_ACCOUNT_TIMEOUT: Duration = Duration::from_secs(30);
const NUCLEUS_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

fn main() {
    match run() {
        Ok(Outcome::Success) => {}
        Ok(Outcome::Child(status)) => {
            std::process::exit(status.code().unwrap_or(1));
        }
        Err(error) => {
            eprintln!("annals-usage: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<Outcome, AppError> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let Some(command) = arguments.first().and_then(|argument| argument.to_str()) else {
        print_help();
        return Ok(Outcome::Success);
    };
    match command {
        "-h" | "--help" | "help" => {
            print_help();
            Ok(Outcome::Success)
        }
        "-V" | "--version" | "version" => {
            println!("annals-usage {}", env!("CARGO_PKG_VERSION"));
            Ok(Outcome::Success)
        }
        "report" => {
            let options = ReportOptions::parse_from(command_arguments("report", &arguments[1..]));
            run_report(&options)?;
            Ok(Outcome::Success)
        }
        "budget" => {
            let options = BudgetOptions::parse_from(command_arguments("budget", &arguments[1..]));
            run_budget(&options)?;
            Ok(Outcome::Success)
        }
        "doctor" => {
            let options = DoctorOptions::parse_from(command_arguments("doctor", &arguments[1..]));
            run_doctor(&options)?;
            Ok(Outcome::Success)
        }
        "login" => run_login(&arguments[1..]),
        _ => Err(AppError::UnsupportedCommand(command.to_owned())),
    }
}

fn run_report(options: &ReportOptions) -> Result<(), AppError> {
    let config = UsageConfig::load(options.config.as_deref())?;
    let observations = with_runtime(load_nucleus_observations(&config, options.limit))?;
    let report = report::build_report(&config, observations, options.limit)?;
    if options.json {
        write_json(&report)?;
    } else {
        report::print_human(&report);
    }
    Ok(())
}

fn run_budget(options: &BudgetOptions) -> Result<(), AppError> {
    let config = UsageConfig::load(options.config.as_deref())?;
    let snapshot = with_runtime_timeout(
        async {
            nucleus_client(&config)?
                .account_snapshot(&AccountSnapshotQueryV1 {
                    include_usage: true,
                    wait_seconds: 0,
                })
                .await
                .map_err(AppError::from)
        },
        NUCLEUS_ACCOUNT_TIMEOUT,
        "account request",
    )?;
    let snapshot = serde_json::to_value(account_snapshot(snapshot)?)?;
    let report = BudgetReport::new(snapshot);
    if options.json {
        write_json(&report)?;
    } else {
        budget::print_human(&report);
    }
    Ok(())
}

fn run_doctor(options: &DoctorOptions) -> Result<(), AppError> {
    let config = UsageConfig::load(options.config.as_deref())?;
    let mut failures = Vec::new();
    for (name, path) in [
        ("Nucleus executable", &config.nucleus),
        ("Annals library", &config.library),
        ("inbox spool", &config.spool),
    ] {
        if !path.exists() {
            failures.push(format!("{name} is missing: {}", path.display()));
        }
    }
    let inspected = with_runtime_timeout(
        async {
            let client = nucleus_client(&config)?;
            let health = client.health().await?;
            client
                .account_snapshot(&AccountSnapshotQueryV1 {
                    include_usage: false,
                    wait_seconds: 0,
                })
                .await?;
            Ok::<_, AppError>(health)
        },
        NUCLEUS_ACCOUNT_TIMEOUT,
        "doctor request",
    );
    let mut nucleus_version = None;
    let mut codex_version = None;
    match inspected {
        Ok(health) => {
            nucleus_version = Some(health.daemon_version);
            codex_version = health.harness.map(|harness| harness.harness_version);
            if !health.accepting_jobs {
                failures.push(
                    health
                        .detail
                        .unwrap_or_else(|| "Nucleus is not accepting jobs".to_owned()),
                );
            }
            if !health.authentication.authenticated {
                failures.push(
                    health
                        .authentication
                        .detail
                        .unwrap_or_else(|| "Nucleus authentication is unavailable".to_owned()),
                );
            }
        }
        Err(error) => failures.push(format!("Nucleus account preflight failed: {error}")),
    }
    if failures.is_empty() {
        println!("annals-usage doctor: healthy");
        println!("Configuration: {}", config.path.display());
        println!("Reporting:     live Nucleus output");
        println!(
            "Nucleus:       {}",
            nucleus_version.as_deref().unwrap_or("unavailable")
        );
        println!(
            "Real Codex:    {}",
            codex_version.as_deref().unwrap_or("unavailable")
        );
        return Ok(());
    }
    Err(AppError::Doctor(failures.join("; ")))
}

fn run_login(arguments: &[OsString]) -> Result<Outcome, AppError> {
    let config = UsageConfig::load(None)?;
    let status = Command::new(&config.nucleus)
        .args(["auth", "login"])
        .args(arguments)
        .status()
        .map_err(|source| AppError::NucleusCommand {
            path: config.nucleus,
            source,
        })?;
    Ok(Outcome::Child(status))
}

async fn load_nucleus_observations(
    config: &UsageConfig,
    limit: usize,
) -> Result<Vec<NucleusObservation>, AppError> {
    let scope = ReportScope::load(config, limit)?;
    if limit == 0 {
        return Ok(Vec::new());
    }
    let client = nucleus_client(config)?;
    let mut selected = Vec::new();
    let mut unattributed = VecDeque::new();
    let mut after = None;
    loop {
        let page = nucleus_http_request(
            client.list_jobs(&ListJobsQueryV1 {
                requester_program: Some("annals".to_owned()),
                requester_id: None,
                parent: None,
                state: None,
                after,
                limit: Some(1_000),
            }),
            "report job-list request",
        )
        .await?;
        for summary in page.jobs {
            let token = &summary.requester.id;
            if scope.includes_delivery(token) {
                selected.push(summary);
            } else if scope.is_unattributed(token) {
                if unattributed.len() == scope.unattributed_limit() {
                    unattributed.pop_front();
                }
                unattributed.push_back(summary);
            }
        }
        let Some(next) = page.next else {
            break;
        };
        after = Some(next);
    }
    selected.extend(unattributed);

    let mut observations = Vec::with_capacity(selected.len());
    for summary in selected {
        let initial_job =
            nucleus_http_request(client.get_job(&summary.id), "report job-detail request").await?;
        let initial_state = initial_job.summary.state;
        let mut records = load_nucleus_output(&client, &summary.id).await?;
        let job =
            nucleus_http_request(client.get_job(&summary.id), "report job-detail request").await?;
        if output_requires_terminal_reload(initial_state, job.summary.state) {
            records = load_nucleus_output(&client, &summary.id).await?;
        }
        observations.push(NucleusObservation { job, records });
    }
    Ok(observations)
}

fn output_requires_terminal_reload(initial: JobState, refreshed: JobState) -> bool {
    !initial.is_terminal() && refreshed.is_terminal()
}

async fn load_nucleus_output(
    client: &NucleusClient,
    job_id: &nucleus_core::JobId,
) -> Result<Vec<nucleus_core::LogRecordV1>, AppError> {
    let mut records = Vec::new();
    let mut cursor = 0;
    loop {
        let page = nucleus_http_request(
            client.logs(
                job_id,
                &LogsQueryV1 {
                    after: cursor,
                    follow: false,
                    limit: Some(1_000),
                },
            ),
            "report output-page request",
        )
        .await?;
        let count = page.records.len();
        if count > 0 && page.next_sequence <= cursor {
            return Err(AppError::NucleusTelemetry(
                "Nucleus did not advance the model-output cursor".to_owned(),
            ));
        }
        cursor = page.next_sequence;
        records.extend(page.records);
        if count < 1_000 {
            break;
        }
    }
    Ok(records)
}

fn account_snapshot(
    snapshot: nucleus_core::AccountSnapshotV1,
) -> Result<AccountSnapshot, AppError> {
    Ok(AccountSnapshot {
        rate_limits: serde_json::from_value(snapshot.rate_limits)?,
        token_activity: snapshot.usage.map(serde_json::from_value).transpose()?,
        token_activity_error: snapshot.usage_error,
    })
}

fn nucleus_client(config: &UsageConfig) -> Result<NucleusClient, AppError> {
    config.nucleus_socket.as_ref().map_or_else(
        || NucleusClient::for_current_user().map_err(Into::into),
        |socket| NucleusClient::new(socket.clone()).map_err(Into::into),
    )
}

fn with_runtime<T>(future: impl Future<Output = Result<T, AppError>>) -> Result<T, AppError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(future)
}

fn with_runtime_timeout<T>(
    future: impl Future<Output = Result<T, AppError>>,
    timeout: Duration,
    operation: &'static str,
) -> Result<T, AppError> {
    with_runtime(async move {
        tokio::time::timeout(timeout, future)
            .await
            .map_err(|_| AppError::NucleusTimeout(operation))?
    })
}

async fn nucleus_http_request<T>(
    future: impl Future<Output = Result<T, ClientError>>,
    operation: &'static str,
) -> Result<T, AppError> {
    nucleus_http_request_with_timeout(future, operation, NUCLEUS_HTTP_TIMEOUT).await
}

async fn nucleus_http_request_with_timeout<T>(
    future: impl Future<Output = Result<T, ClientError>>,
    operation: &'static str,
    timeout: Duration,
) -> Result<T, AppError> {
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| AppError::NucleusTimeout(operation))?
        .map_err(Into::into)
}

fn command_arguments(name: &str, arguments: &[OsString]) -> Vec<OsString> {
    std::iter::once(OsString::from(name))
        .chain(arguments.iter().cloned())
        .collect()
}

fn write_json(value: &impl serde::Serialize) -> Result<(), AppError> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, value)?;
    output.write_all(b"\n")?;
    Ok(())
}

fn print_help() {
    println!(
        "annals-usage {}\n\n\
         Usage:\n  annals-usage report [--json] [--limit N] [--config PATH]\n  \
         annals-usage budget [--json] [--config PATH]\n  \
         annals-usage doctor [--config PATH]\n  \
         annals-usage login --device-auth\n\n\
         Reports are calculated live from Nucleus model output and Annals attribution. Nucleus \
         owns Codex execution and authentication.",
        env!("CARGO_PKG_VERSION")
    );
}

#[derive(Debug, Parser)]
struct ReportOptions {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    json: bool,
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[derive(Debug, Parser)]
struct BudgetOptions {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser)]
struct DoctorOptions {
    #[arg(long)]
    config: Option<PathBuf>,
}

enum Outcome {
    Success,
    Child(ExitStatus),
}

#[derive(Debug, Error)]
enum AppError {
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error(transparent)]
    Report(#[from] crate::report::ReportError),
    #[error(transparent)]
    Nucleus(#[from] ClientError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("unable to run the Nucleus executable {path}: {source}")]
    NucleusCommand { path: PathBuf, source: io::Error },
    #[error("unsupported command {0:?}")]
    UnsupportedCommand(String),
    #[error("health check failed: {0}")]
    Doctor(String),
    #[error("Nucleus reporting output is invalid: {0}")]
    NucleusTelemetry(String),
    #[error("Nucleus {0} exceeded its 30-second timeout")]
    NucleusTimeout(&'static str),
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::net::UnixListener;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{
        AppError, UsageConfig, command_arguments, nucleus_client,
        nucleus_http_request_with_timeout, output_requires_terminal_reload, with_runtime,
    };
    use nucleus_core::JobState;

    #[test]
    fn clap_subcommand_arguments_retain_their_values() {
        let arguments = [OsString::from("--limit"), OsString::from("3")];
        assert_eq!(
            command_arguments("report", &arguments),
            ["report", "--limit", "3"].map(OsString::from)
        );
    }

    #[test]
    fn output_is_reloaded_when_the_job_turns_terminal_during_collection() {
        assert!(output_requires_terminal_reload(
            JobState::Running,
            JobState::Completed
        ));
        assert!(output_requires_terminal_reload(
            JobState::WaitingOnRequester,
            JobState::Failed
        ));
        assert!(!output_requires_terminal_reload(
            JobState::Completed,
            JobState::Completed
        ));
        assert!(!output_requires_terminal_reload(
            JobState::Accepted,
            JobState::Running
        ));
    }

    #[test]
    fn stalled_nucleus_usage_request_is_bounded() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let socket = directory.path().join("nucleus.sock");
        let listener = UnixListener::bind(&socket)?;
        thread::spawn(move || {
            if let Ok((_stream, _)) = listener.accept() {
                thread::sleep(Duration::from_secs(2));
            }
        });
        let config = UsageConfig {
            nucleus_socket: Some(socket),
            ..UsageConfig::default()
        };
        let started = Instant::now();
        let Err(error) = with_runtime(async {
            let client = nucleus_client(&config)?;
            nucleus_http_request_with_timeout(
                client.health(),
                "test request",
                Duration::from_millis(100),
            )
            .await?;
            Ok(())
        }) else {
            return Err("a stalled Nucleus usage request unexpectedly completed".into());
        };
        assert!(matches!(error, AppError::NucleusTimeout("test request")));
        assert!(started.elapsed() < Duration::from_secs(2));
        Ok(())
    }
}
