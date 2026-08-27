mod budget;
mod config;
mod database;
mod protocol;
mod report;
mod types;

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::time::Duration;

use clap::Parser;
use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{
    AccountSnapshotQueryV1, JobState, ListJobsQueryV1, LogRecordV1, LogStream, LogsQueryV1,
};
use serde_json::{Value, json};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::budget::BudgetReport;
use crate::config::UsageConfig;
use crate::database::{ModelRunReceipts, RunIdentity, UsageDatabase, read_model_run_receipts};
use crate::protocol::{AccountSnapshot, ProtocolEvent, ProtocolState};

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
    if let Err(error) = sync_nucleus_usage(&config) {
        eprintln!(
            "annals-usage: Nucleus telemetry is temporarily unavailable; reporting retained observations only: {error}"
        );
    }
    let database = UsageDatabase::open(&config.database)?;
    let report = report::build_report(&config, &database, options.limit)?;
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
    let snapshot = account_snapshot(snapshot)?;
    let database = UsageDatabase::open(&config.database)?;
    database.record_quota_snapshot(
        None,
        "account/rateLimits/read",
        &serde_json::to_value(&snapshot.rate_limits)?,
    )?;
    let snapshot = serde_json::to_value(snapshot)?;
    let report = BudgetReport::new(snapshot)?;
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
    let _database = UsageDatabase::open(&config.database)?;
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
        println!("Ledger:        {}", config.database.display());
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

struct NucleusRun {
    token: String,
    codex_version: Option<String>,
    state: JobState,
    started_at_ms: i64,
    completed_at_ms: Option<i64>,
    records: Vec<LogRecordV1>,
}

fn sync_nucleus_usage(config: &UsageConfig) -> Result<(), AppError> {
    let receipts = read_model_run_receipts(&config.spool)?;
    let database = UsageDatabase::open(&config.database)?;
    let refresh = refreshable_unattributed_runs(&database, &receipts)?;
    let mut imported = database.imported_model_run_tokens()?;
    imported.retain(|token| !refresh.contains(token));
    drop(database);
    with_runtime(sync_nucleus_runs(config, imported, refresh, receipts))
}

fn refreshable_unattributed_runs(
    database: &UsageDatabase,
    receipts: &ModelRunReceipts,
) -> Result<BTreeSet<String>, AppError> {
    Ok(database
        .unattributed_model_run_tokens()?
        .into_iter()
        .filter(|token| {
            receipts
                .get(token)
                .is_some_and(|receipt| receipt.ingestion_id.is_some())
        })
        .collect())
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

async fn sync_nucleus_runs(
    config: &UsageConfig,
    mut imported: BTreeSet<String>,
    mut refresh: BTreeSet<String>,
    mut receipts: ModelRunReceipts,
) -> Result<(), AppError> {
    let client = nucleus_client(config)?;
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
            "telemetry job-list request",
        )
        .await?;
        // Annals persists the model-run token before admitting a Nucleus job. Refreshing after
        // the list response therefore sees receipts created after this report's initial snapshot,
        // including receipts that moved from processing into a terminal lane while listing.
        refresh_receipt_snapshot(config, &mut imported, &mut refresh, &mut receipts)?;
        for summary in page.jobs {
            if imported.contains(&summary.requester.id) {
                continue;
            }
            if !summary.state.is_terminal() {
                ensure_pending_nucleus_run(
                    config,
                    &summary.requester.id,
                    parse_nucleus_timestamp(&summary.created_at)?,
                    &receipts,
                )?;
                continue;
            }
            let job =
                nucleus_http_request(client.get_job(&summary.id), "telemetry job-detail request")
                    .await?;
            let attempt = job.attempts.last();
            let started_at_ms = parse_nucleus_timestamp(
                attempt.map_or(job.summary.created_at.as_str(), |attempt| {
                    attempt.started_at.as_deref().unwrap_or(&attempt.created_at)
                }),
            )?;
            let completed_at_ms = attempt
                .and_then(|attempt| attempt.completed_at.as_deref())
                .or(job.summary.completed_at.as_deref())
                .map(parse_nucleus_timestamp)
                .transpose()?;
            let mut records = Vec::new();
            if attempt.is_some() {
                let mut cursor = 0_u64;
                loop {
                    let logs = nucleus_http_request(
                        client.logs(
                            &summary.id,
                            &LogsQueryV1 {
                                after: cursor,
                                follow: false,
                                limit: Some(1_000),
                            },
                        ),
                        "telemetry log-page request",
                    )
                    .await?;
                    let count = logs.records.len();
                    cursor = logs.next_sequence;
                    records.extend(logs.records);
                    if count < 1_000 {
                        break;
                    }
                }
            }
            import_nucleus_run(
                config,
                NucleusRun {
                    token: job.summary.requester.id,
                    codex_version: attempt.map(|attempt| attempt.harness.harness_version.clone()),
                    state: summary.state,
                    started_at_ms,
                    completed_at_ms,
                    records,
                },
                &receipts,
                refresh.contains(&summary.requester.id),
            )?;
        }
        let Some(next) = page.next else {
            break;
        };
        after = Some(next);
    }
    Ok(())
}

fn refresh_receipt_snapshot(
    config: &UsageConfig,
    imported: &mut BTreeSet<String>,
    refresh: &mut BTreeSet<String>,
    receipts: &mut ModelRunReceipts,
) -> Result<(), AppError> {
    *receipts = read_model_run_receipts(&config.spool)?;
    let database = UsageDatabase::open(&config.database)?;
    let newly_refreshable = refreshable_unattributed_runs(&database, receipts)?;
    imported.retain(|token| !newly_refreshable.contains(token));
    refresh.extend(newly_refreshable);
    Ok(())
}

fn ensure_pending_nucleus_run(
    config: &UsageConfig,
    token: &str,
    started_at_ms: i64,
    receipts: &ModelRunReceipts,
) -> Result<(), AppError> {
    let mut database = UsageDatabase::open(&config.database)?;
    if database.has_model_run_token(token)? {
        return Ok(());
    }
    let identity = RunIdentity::resolve(config, Some(token), receipts)?;
    let run_id = database.begin_run(&identity, None)?;
    database.set_run_timestamps(run_id, started_at_ms, None)?;
    Ok(())
}

fn import_nucleus_run(
    config: &UsageConfig,
    run: NucleusRun,
    receipts: &ModelRunReceipts,
    force_refresh: bool,
) -> Result<(), AppError> {
    let identity = RunIdentity::resolve(config, Some(&run.token), receipts)?;
    let mut database = UsageDatabase::open(&config.database)?;
    database.transaction(|database| {
        // Another report may have completed this import after our list snapshot. Its committed
        // row wins; otherwise replacement and replay remain one crash-atomic write transaction.
        if !force_refresh && database.model_run_import_is_complete(&run.token)? {
            return Ok(());
        }
        let run_id = database.begin_or_reset_run(&identity, run.codex_version.as_deref())?;
        database.set_run_timestamps(run_id, run.started_at_ms, run.completed_at_ms)?;
        let mut recorder = RunRecorder {
            database,
            run_id,
            completed: false,
        };
        let mut protocol = ProtocolState::default();
        for record in run.records {
            if !matches!(
                record.stream,
                LogStream::HarnessInput | LogStream::HarnessOutput
            ) {
                continue;
            }
            if !record
                .schema_id
                .as_str()
                .starts_with("codex.app-server.protocol.")
            {
                recorder.database.record_exact_gap(
                    recorder.run_id,
                    &format!(
                        "Nucleus retained a Codex stream record under incompatible schema {}",
                        record.schema_id
                    ),
                )?;
                continue;
            }
            let observed_at_ms = parse_nucleus_timestamp(&record.observed_at)?;
            let message = match serde_json::from_str::<Value>(record.payload.get()) {
                Ok(message) => message,
                Err(error) => {
                    recorder.database.record_exact_gap(
                        recorder.run_id,
                        &format!("Nucleus retained an undecodable Codex record: {error}"),
                    )?;
                    continue;
                }
            };
            if record.stream == LogStream::HarnessInput {
                protocol.observe_client_message(&message);
            } else if let Some(event) = protocol.observe_server_message(&message) {
                recorder.observe(&event, observed_at_ms)?;
            }
        }
        if !recorder.completed {
            recorder.finish_incomplete(match run.state {
                JobState::Completed => "ended-without-turn-completion",
                JobState::Failed => "nucleus-job-failed",
                JobState::Cancelled => "nucleus-job-cancelled",
                JobState::Accepted | JobState::Running | JobState::WaitingOnRequester => {
                    "nucleus-job-incomplete"
                }
            })?;
        }
        Ok(())
    })
}

fn parse_nucleus_timestamp(timestamp: &str) -> Result<i64, AppError> {
    let parsed = OffsetDateTime::parse(timestamp, &Rfc3339).map_err(|error| {
        AppError::NucleusTelemetry(format!(
            "Nucleus returned an invalid attempt timestamp {timestamp:?}: {error}"
        ))
    })?;
    i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000).map_err(|_| {
        AppError::NucleusTelemetry(format!(
            "Nucleus attempt timestamp {timestamp:?} does not fit in milliseconds"
        ))
    })
}

struct RunRecorder<'a> {
    database: &'a UsageDatabase,
    run_id: i64,
    completed: bool,
}

impl RunRecorder<'_> {
    fn observe(&mut self, event: &ProtocolEvent, observed_at_ms: i64) -> Result<(), AppError> {
        match event {
            ProtocolEvent::ThreadStarted { thread_id, .. } => {
                self.database.bind_thread(self.run_id, thread_id, None)?;
            }
            ProtocolEvent::TurnStarted {
                thread_id, turn_id, ..
            } => {
                self.database
                    .bind_thread(self.run_id, thread_id, Some(turn_id))?;
            }
            ProtocolEvent::TokenUsageUpdated {
                thread_id,
                turn_id,
                usage,
            } => {
                self.database.record_token_usage(
                    self.run_id,
                    thread_id,
                    turn_id,
                    usage,
                    observed_at_ms,
                )?;
            }
            ProtocolEvent::RawResponseCompleted {
                thread_id,
                turn_id,
                response_id,
                usage,
            } => {
                if let Some(usage) = usage {
                    self.database.record_response_usage(
                        self.run_id,
                        response_id,
                        thread_id,
                        turn_id,
                        *usage,
                        observed_at_ms,
                    )?;
                } else {
                    self.database.record_exact_gap(
                        self.run_id,
                        "an upstream response omitted token usage",
                    )?;
                }
            }
            ProtocolEvent::RateLimitsUpdated { rate_limits } => {
                let payload = json!({ "rateLimits": rate_limits });
                self.database.record_quota_snapshot_at(
                    Some(self.run_id),
                    "account/rateLimits/updated",
                    &payload,
                    observed_at_ms,
                )?;
            }
            ProtocolEvent::TurnCompleted {
                thread_id,
                turn_id,
                status,
            } => {
                self.database
                    .complete_run(self.run_id, status, Some(thread_id), Some(turn_id))?;
                self.completed = true;
            }
        }
        Ok(())
    }

    fn finish_incomplete(&mut self, status: &str) -> Result<(), AppError> {
        self.database.finish_incomplete_run(self.run_id, status)?;
        self.completed = true;
        Ok(())
    }
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
         Nucleus owns Codex execution and authentication. The login command delegates to the \
         configured Nucleus executable.",
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
    Database(#[from] crate::database::DatabaseError),
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
    #[error("unsupported command {0:?}; annals-usage is no longer a Codex proxy")]
    UnsupportedCommand(String),
    #[error("health check failed: {0}")]
    Doctor(String),
    #[error("Nucleus telemetry could not be imported: {0}")]
    NucleusTelemetry(String),
    #[error("Nucleus {0} exceeded its 30-second timeout")]
    NucleusTimeout(&'static str),
}

#[cfg(test)]
mod tests {
    use super::{
        AppError, ModelRunReceipts, NucleusRun, UsageConfig, command_arguments,
        ensure_pending_nucleus_run, import_nucleus_run, nucleus_client,
        nucleus_http_request_with_timeout, parse_nucleus_timestamp, refresh_receipt_snapshot,
        with_runtime,
    };
    use crate::database::{UsageDatabase, read_model_run_receipts};
    use nucleus_core::{JobId, JobState, LogRecordV1, LogStream, PROTOCOL_VERSION_V1, SchemaId};
    use rusqlite::Connection;
    use serde_json::{Value, json, value::RawValue};
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    const LINKED_TOKEN: &str = "f925-linked-run-token";
    const LINKED_JOB: &str = "j00000000000000000342";
    const LINKED_DELIVERY: i64 = 372;

    #[test]
    fn clap_subcommand_arguments_retain_their_values() {
        let arguments = [OsString::from("--limit"), OsString::from("3")];
        assert_eq!(
            command_arguments("report", &arguments),
            ["report", "--limit", "3"].map(OsString::from)
        );
    }

    #[test]
    fn terminal_job_without_an_attempt_becomes_a_retryable_gap()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let library = directory.path().join("annals.db");
        Connection::open(&library)?.execute_batch(
            "CREATE TABLE works(id INTEGER PRIMARY KEY, label TEXT NOT NULL); \
             CREATE TABLE model_runs( \
                 id INTEGER PRIMARY KEY, work_id INTEGER NOT NULL, token TEXT NOT NULL, \
                 base_revision INTEGER NOT NULL, model TEXT NOT NULL, \
                 reasoning_effort TEXT NOT NULL \
             );",
        )?;
        let config = UsageConfig {
            nucleus: "nucleus".into(),
            nucleus_socket: None,
            database: directory.path().join("usage.db"),
            library,
            spool: directory.path().join("spool"),
            path: directory.path().join("usage.toml"),
        };

        import_nucleus_run(
            &config,
            NucleusRun {
                token: "attemptless".to_owned(),
                codex_version: None,
                state: JobState::Failed,
                started_at_ms: 1_000,
                completed_at_ms: Some(2_000),
                records: Vec::new(),
            },
            &ModelRunReceipts::new(),
            false,
        )?;

        let run = &UsageDatabase::open(&config.database)?.runs(1)?[0];
        assert_eq!(run.model_run_token.as_deref(), Some("attemptless"));
        assert_eq!(run.status, "nucleus-job-failed");
        assert_eq!(run.coverage, "gap");
        assert_eq!(run.started_at_ms, 1_000);
        assert_eq!(run.completed_at_ms, Some(2_000));
        Ok(())
    }

    #[test]
    fn pending_run_remains_linked_after_its_receipt_is_archived()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, config) = linked_fixture("processing")?;
        let receipts = read_model_run_receipts(&config.spool)?;
        let started_at_ms = parse_nucleus_timestamp("2026-08-27T12:10:00Z")?;
        ensure_pending_nucleus_run(&config, LINKED_TOKEN, started_at_ms, &receipts)?;
        let pending = UsageDatabase::open(&config.database)?.runs(1)?.remove(0);
        assert_eq!(pending.delivery_id, Some(LINKED_DELIVERY));
        assert_eq!(pending.status, "running");

        let done = config.spool.join("done");
        fs::create_dir_all(&done)?;
        fs::rename(
            config.spool.join("processing").join(LINKED_JOB),
            done.join(LINKED_JOB),
        )?;
        let archived_receipts = read_model_run_receipts(&config.spool)?;
        import_nucleus_run(
            &config,
            exact_run("response-after-archive", 10)?,
            &archived_receipts,
            false,
        )?;

        let database = UsageDatabase::open(&config.database)?;
        let run = database.runs(1)?.remove(0);
        assert_eq!(run.id, pending.id);
        assert_eq!(run.delivery_id, Some(LINKED_DELIVERY));
        assert_eq!(run.inbox_job_id.as_deref(), Some(LINKED_JOB));
        assert_eq!(run.attempt, Some(1));
        assert_eq!(run.source_name.as_deref(), Some("live.md"));
        assert_eq!(run.coverage, "exact");
        assert_eq!(run.response_count, 1);

        let report = serde_json::to_value(crate::report::build_report(&config, &database, 10)?)?;
        assert_eq!(
            report.pointer("/deliveries/0/deliveryId"),
            Some(&json!(372))
        );
        assert_eq!(
            report.pointer("/deliveries/0/coverage"),
            Some(&json!("exact"))
        );
        assert_eq!(
            report.pointer("/deliveries/0/attempts/0/deliveryId"),
            Some(&json!(372))
        );
        assert_eq!(
            report
                .pointer("/deliveries/0/attempts")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_replayed_event_times(&config.database, 10)?;
        Ok(())
    }

    #[test]
    fn archived_receipt_repairs_an_existing_exact_unattributed_import()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, config) = linked_fixture("done")?;
        import_nucleus_run(
            &config,
            exact_run("stale-response", 10)?,
            &ModelRunReceipts::new(),
            false,
        )?;
        let database = UsageDatabase::open(&config.database)?;
        let orphan = database.runs(1)?.remove(0);
        assert_eq!(orphan.delivery_id, None);
        assert_eq!(orphan.coverage, "exact");
        let mut imported = database.imported_model_run_tokens()?;
        assert!(imported.contains(LINKED_TOKEN));
        let mut receipts = ModelRunReceipts::new();
        let mut refresh = std::collections::BTreeSet::new();
        refresh_receipt_snapshot(&config, &mut imported, &mut refresh, &mut receipts)?;
        assert_eq!(refresh, [LINKED_TOKEN.to_owned()].into_iter().collect());
        assert!(!imported.contains(LINKED_TOKEN));
        assert!(receipts.contains_key(LINKED_TOKEN));
        drop(database);
        import_nucleus_run(
            &config,
            exact_run("replacement-response", 11)?,
            &receipts,
            true,
        )?;

        let database = UsageDatabase::open(&config.database)?;
        let repaired = database.runs(1)?.remove(0);
        assert_eq!(repaired.id, orphan.id);
        assert_eq!(repaired.delivery_id, Some(LINKED_DELIVERY));
        assert_eq!(repaired.inbox_job_id.as_deref(), Some(LINKED_JOB));
        assert_eq!(repaired.source_name.as_deref(), Some("live.md"));
        assert_eq!(repaired.coverage, "exact");
        assert_eq!(repaired.response_count, 1);
        assert_eq!(repaired.responses[0].response_id, "replacement-response");
        let connection = Connection::open(&config.database)?;
        for table in ["token_snapshots", "response_usages", "quota_snapshots"] {
            let count = connection.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE run_id = ?1"),
                [repaired.id],
                |row| row.get::<_, i64>(0),
            )?;
            assert_eq!(count, 1, "{table} retained stale replay rows");
        }
        assert_replayed_event_times(&config.database, 11)?;
        Ok(())
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

    fn linked_fixture(
        receipt_lane: &str,
    ) -> Result<(tempfile::TempDir, UsageConfig), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let library = directory.path().join("annals.db");
        let connection = Connection::open(&library)?;
        connection.execute_batch(
            "CREATE TABLE works(id INTEGER PRIMARY KEY, label TEXT NOT NULL); \
             CREATE TABLE model_runs( \
                 id INTEGER PRIMARY KEY, work_id INTEGER NOT NULL, token TEXT NOT NULL, \
                 base_revision INTEGER NOT NULL, model TEXT NOT NULL, \
                 reasoning_effort TEXT NOT NULL \
             ); \
             CREATE TABLE ingestions( \
                 id INTEGER PRIMARY KEY, source_name TEXT NOT NULL, status TEXT NOT NULL, \
                 result TEXT, work_id INTEGER \
             );",
        )?;
        connection.execute("INSERT INTO works(id, label) VALUES(1, 'Linked work')", [])?;
        connection.execute(
            "INSERT INTO model_runs( \
                 id, work_id, token, base_revision, model, reasoning_effort \
             ) VALUES(?1, 1, ?2, 7, 'gpt-5.6-sol', 'high')",
            (LINKED_DELIVERY, LINKED_TOKEN),
        )?;
        connection.execute(
            "INSERT INTO ingestions(id, source_name, status, result, work_id) \
             VALUES(?1, 'live.md', 'completed', 'applied', 1)",
            [LINKED_DELIVERY],
        )?;

        let spool = directory.path().join("spool");
        let job_directory = spool.join(receipt_lane).join(LINKED_JOB);
        fs::create_dir_all(&job_directory)?;
        fs::write(
            job_directory.join("job.json"),
            serde_json::to_vec(&json!({
                "id": LINKED_JOB,
                "attempts": 1,
                "ingestion_id": LINKED_DELIVERY,
                "model_run_token": LINKED_TOKEN,
                "reconciliation_id": null,
                "result_status": "applied"
            }))?,
        )?;
        let config = UsageConfig {
            nucleus: "nucleus".into(),
            nucleus_socket: None,
            database: directory.path().join("usage.db"),
            library,
            spool,
            path: directory.path().join("usage.toml"),
        };
        Ok((directory, config))
    }

    fn exact_run(response_id: &str, minute: u8) -> Result<NucleusRun, Box<dyn std::error::Error>> {
        let usage = json!({
            "inputTokens": 100,
            "cachedInputTokens": 50,
            "cacheWriteInputTokens": 0,
            "outputTokens": 20,
            "reasoningOutputTokens": 10,
            "totalTokens": 120
        });
        let records = vec![
            log_record(
                1,
                minute,
                1,
                &json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": "thread-live",
                        "turnId": "turn-live",
                        "tokenUsage": {
                            "last": usage,
                            "total": usage,
                            "modelContextWindow": 1000
                        }
                    }
                }),
            )?,
            log_record(
                2,
                minute,
                2,
                &json!({
                    "method": "rawResponse/completed",
                    "params": {
                        "threadId": "thread-live",
                        "turnId": "turn-live",
                        "responseId": response_id,
                        "usage": usage
                    }
                }),
            )?,
            log_record(
                3,
                minute,
                3,
                &json!({
                    "method": "account/rateLimits/updated",
                    "params": { "rateLimits": {} }
                }),
            )?,
            log_record(
                4,
                minute,
                4,
                &json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": "thread-live",
                        "turn": { "id": "turn-live", "status": "completed" }
                    }
                }),
            )?,
        ];
        let started_at = timestamp(minute, 0);
        let completed_at = timestamp(minute, 5);
        Ok(NucleusRun {
            token: LINKED_TOKEN.to_owned(),
            codex_version: Some("codex-cli test".to_owned()),
            state: JobState::Completed,
            started_at_ms: parse_nucleus_timestamp(&started_at)?,
            completed_at_ms: Some(parse_nucleus_timestamp(&completed_at)?),
            records,
        })
    }

    fn log_record(
        sequence: u64,
        minute: u8,
        second: u8,
        payload: &Value,
    ) -> Result<LogRecordV1, Box<dyn std::error::Error>> {
        Ok(LogRecordV1 {
            version: PROTOCOL_VERSION_V1,
            job_id: JobId::from(LINKED_JOB),
            attempt_id: None,
            sequence,
            observed_at: timestamp(minute, second),
            stream: LogStream::HarnessOutput,
            schema_id: SchemaId::from("codex.app-server.protocol.test"),
            payload: RawValue::from_string(serde_json::to_string(&payload)?)?,
            payload_digest: "fixture-digest".to_owned(),
        })
    }

    fn timestamp(minute: u8, second: u8) -> String {
        format!("2026-08-27T12:{minute:02}:{second:02}Z")
    }

    fn assert_replayed_event_times(
        database: &Path,
        minute: u8,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let connection = Connection::open(database)?;
        for (table, second) in [
            ("token_snapshots", 1_u8),
            ("response_usages", 2_u8),
            ("quota_snapshots", 3_u8),
        ] {
            let observed_at_ms = connection.query_row(
                &format!("SELECT observed_at_ms FROM {table} WHERE run_id IS NOT NULL"),
                [],
                |row| row.get::<_, i64>(0),
            )?;
            assert_eq!(
                observed_at_ms,
                parse_nucleus_timestamp(&timestamp(minute, second))?,
                "{table} did not preserve the Nucleus record timestamp"
            );
            assert!(
                observed_at_ms >= parse_nucleus_timestamp(&timestamp(minute, 0))?
                    && observed_at_ms <= parse_nucleus_timestamp(&timestamp(minute, 5))?
            );
        }
        Ok(())
    }
}
