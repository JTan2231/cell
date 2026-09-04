#![allow(dead_code)] // Version 1-3 readers and recovery code remain migration-compatible.

mod account;
mod classifier;
mod digest;
mod email;
mod error;
mod model;
mod source;
mod store;

#[cfg(test)]
use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use conversations::{AppServerClient, ClientConfig, StderrPolicy};
use serde_json::json;
use time::{Date, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

use crate::classifier::{ClassificationResult, Runner};
use crate::error::{AppError, AppResult};
use crate::model::{Delivery, DigestSnapshot, Observation, Run};
use crate::source::ObservationLoad;
use crate::store::{Store, default_database_path, now_unix};

const CLASSIFICATION_ATTEMPTS: usize = 3;

#[derive(Debug, Parser)]
#[command(
    name = "krisis",
    version,
    about = "Identify decisions and deliver immutable accounts to Annals"
)]
struct Cli {
    /// Use an alternate migration-compatible Krisis `SQLite` database.
    #[arg(long, global = true, env = "KRISIS_DATABASE")]
    database: Option<PathBuf>,
    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    json: bool,
    /// Exact Annals executable used for decision-account acceptance.
    #[arg(long, global = true, env = "KRISIS_ANNALS_BINARY")]
    annals_binary: Option<PathBuf>,
    /// Exact dedicated decisions-library Annals config.
    #[arg(long, global = true, env = "KRISIS_ANNALS_CONFIG")]
    annals_config: Option<PathBuf>,
    /// Expected persistent ID of the dedicated Annals decisions library.
    #[arg(long, global = true, env = "KRISIS_ANNALS_LIBRARY_ID")]
    annals_library_id: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check the database, Codex source, Nucleus, and Annals prerequisites.
    Doctor,
    /// Retired Decisions digest surface; retained only to give an explicit failure.
    #[command(hide = true)]
    Daily {
        #[command(subcommand)]
        command: DailyCommand,
    },
    /// Continuously ingest, classify, reconcile, and inspect completed turns.
    Observe {
        #[command(subcommand)]
        command: ObserveCommand,
    },
    /// Show one decision and its stable source anchors.
    Show { decision_id: String },
    /// Read the append-only decision lifecycle event stream.
    Events {
        #[command(subcommand)]
        command: EventsCommand,
    },
    /// Retired Decisions review surface; retained only to give an explicit failure.
    #[command(hide = true)]
    Review {
        #[command(subcommand)]
        command: ReviewCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DailyCommand {
    /// Read the complete source window and create or resume a durable run.
    Build(DateArgs),
    /// Freeze and print the latest complete run without network access.
    Preview(DateArgs),
    /// Send the latest complete run now with an ad-hoc idempotency key.
    Send(DateArgs),
    /// Cancel and reconcile an interrupted build before permitting a new attempt.
    Abandon(DateArgs),
    /// Build and send the most recent local 09:00 occurrence.
    Run {
        /// Confirm this invocation is the installed scheduled path.
        #[arg(long, required = true)]
        scheduled: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ObserveCommand {
    /// Set the durable post-deployment user-message boundary exactly once.
    Activate {
        /// Explicit Unix-second boundary; defaults conservatively to the next second.
        #[arg(long)]
        at: Option<i64>,
    },
    /// Durably enqueue one Stop-hook correlation from JSON on standard input.
    Ingest,
    /// Process at most one queued or resumable observation.
    Process,
    /// Show observer readiness and durable queue counts.
    Status(ObserveDateArgs),
    /// Independently discover missed completed turns for one local date.
    Reconcile(ObserveDateArgs),
    /// Mark one proven-unavailable, unbound queued source as not eligible.
    Abandon {
        observation_id: String,
        /// Confirm that the exact Stop-hook source was proven permanently unavailable.
        #[arg(long, required = true)]
        source_unavailable: bool,
    },
    /// Requeue one terminally failed observation without deleting its audit trail.
    Retry { observation_id: String },
}

#[derive(Debug, Args)]
struct DateArgs {
    /// Local calendar date in YYYY-MM-DD; defaults to yesterday.
    #[arg(long)]
    date: Option<String>,
}

#[derive(Debug, Args)]
struct ObserveDateArgs {
    /// Local calendar date in YYYY-MM-DD; defaults to today.
    #[arg(long)]
    date: Option<String>,
}

#[derive(Debug, Subcommand)]
enum ReviewCommand {
    Confirm { decision_id: String },
    Dismiss { decision_id: String },
}

#[derive(Debug, Subcommand)]
enum EventsCommand {
    /// Return an opaque cursor for the current end of the stream.
    Watermark,
    /// Read an ordered page strictly after an opaque cursor.
    Read(EventReadArgs),
}

#[derive(Debug, Args)]
struct EventReadArgs {
    /// Opaque cursor returned by watermark or an earlier event page.
    #[arg(long)]
    after: String,
    /// Maximum envelopes to return, from 1 through 1000.
    #[arg(long, default_value_t = 100)]
    limit: usize,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[allow(dead_code)]
enum OutputFormat {
    Human,
    Json,
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("krisis: {error}");
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_lines)]
fn run(cli: Cli) -> AppResult<()> {
    let database = cli.database.map_or_else(default_database_path, Ok)?;
    let annals = annals_configuration(cli.annals_binary, cli.annals_config, cli.annals_library_id)?;
    match cli.command {
        Command::Doctor => doctor(&database, annals.as_ref(), cli.json),
        Command::Daily { command: _ } => Err(AppError::new(
            "legacy_surface_retired",
            "Krisis does not build or send Decisions digests",
        )),
        Command::Observe { command } => {
            let mut store = Store::open(&database)?;
            match command {
                ObserveCommand::Activate { at } => {
                    let requested = at.unwrap_or_else(|| default_observer_baseline(now_unix()));
                    let created = store.observer_baseline_at()?.is_none();
                    let baseline = store.activate_observer(requested)?;
                    if cli.json {
                        print_json(&json!({
                            "observer_baseline_at": baseline,
                            "created": created
                        }))
                    } else {
                        println!("Observer baseline: {baseline}");
                        Ok(())
                    }
                }
                ObserveCommand::Ingest => ingest_hook(&store),
                ObserveCommand::Process => {
                    let annals = annals.as_ref().ok_or_else(|| {
                        AppError::new(
                            "annals_configuration_required",
                            "Krisis processing requires explicit Annals binary, config, and library ID",
                        )
                    })?;
                    account::doctor(annals)?;
                    if let Some(pending) = store.pending_account()? {
                        let receipt = account::accept(&pending, annals, store.state_directory())?;
                        store.record_annals_acceptance(&pending, &receipt)?;
                        print_account_delivery(&pending.account_id, &receipt, cli.json)
                    } else {
                        let result = process_one_observation(&mut store, annals)?;
                        print_process_result(result.as_ref(), cli.json)
                    }
                }
                ObserveCommand::Status(args) => {
                    let status = match args.date.as_deref() {
                        Some(value) => {
                            let date = parse_date(value)?;
                            let (start, end) = local_window(date)?;
                            store.observation_status_window(Some((start, end)))?
                        }
                        None => store.observation_status()?,
                    };
                    if cli.json {
                        print_json(&status)
                    } else {
                        println!(
                            "Observer baseline: {}\nQueued: {}\nProcessing: {}\nComplete: {}\nFailed: {}\nAccounts pending Annals: {}\nAccounts accepted by Annals: {}",
                            status
                                .observer_baseline_at
                                .map_or_else(|| "inactive".to_owned(), |value| value.to_string()),
                            status.queued,
                            status.processing,
                            status.complete,
                            status.failed,
                            status.accounts_pending_annals,
                            status.accounts_accepted_by_annals
                        );
                        for failure in &status.failures {
                            println!("Failure: {} [{}]", failure.id, failure.failure_code);
                        }
                        Ok(())
                    }
                }
                ObserveCommand::Reconcile(args) => {
                    let date = observe_date(args.date.as_deref())?;
                    let result = reconcile(&mut store, date, reconciliation_cutoff())?;
                    if cli.json {
                        print_json(&result)
                    } else {
                        println!(
                            "Reconciled {}: {} completed activities across {} root tasks; {} observations enqueued",
                            format_date(date),
                            result.activities_scanned,
                            result.threads_scanned,
                            result.observations_enqueued
                        );
                        Ok(())
                    }
                }
                ObserveCommand::Abandon {
                    observation_id,
                    source_unavailable,
                } => {
                    if !source_unavailable {
                        return Err(AppError::new(
                            "source_unavailable_confirmation_required",
                            "observe abandon requires --source-unavailable",
                        ));
                    }
                    let _processing_lock = store.wait_for_observation_processing()?;
                    let observation = store.abandon_unavailable_observation(&observation_id)?;
                    if cli.json {
                        print_json(&observation)
                    } else {
                        println!(
                            "Abandoned {} as not eligible: source unavailable",
                            observation.id
                        );
                        Ok(())
                    }
                }
                ObserveCommand::Retry { observation_id } => {
                    let observation = store.retry_observation(&observation_id)?;
                    if cli.json {
                        print_json(&observation)
                    } else {
                        println!("Requeued {}", observation.id);
                        Ok(())
                    }
                }
            }
        }
        Command::Show { decision_id } => {
            let store = Store::open(&database)?;
            let candidate = store.candidate(&decision_id)?;
            if cli.json {
                print_json(&candidate)
            } else {
                println!(
                    "{}\n{} [{}; {}]",
                    candidate.id, candidate.statement, candidate.disposition, candidate.confidence
                );
                println!("Review: {}", candidate.review_state);
                println!(
                    "Authority span: {}..{}",
                    candidate.authority_start, candidate.authority_end
                );
                for source in candidate.sources {
                    println!(
                        "Source ({}): {}/{}/{}/{} [{}; {}]",
                        source.source_role,
                        source.host_id,
                        source.thread_id,
                        source.turn_id,
                        source.item_id,
                        source.message_role,
                        source.timestamp_precision
                    );
                }
                Ok(())
            }
        }
        Command::Events { command } => {
            let store = Store::open(&database)?;
            match command {
                EventsCommand::Watermark => {
                    let watermark = store.event_watermark()?;
                    if cli.json {
                        print_json(&watermark)
                    } else {
                        println!("{}", watermark.cursor);
                        Ok(())
                    }
                }
                EventsCommand::Read(args) => {
                    let page = store.read_events(&args.after, args.limit)?;
                    if cli.json {
                        print_json(&page)
                    } else {
                        for event in &page.events {
                            println!(
                                "{}",
                                serde_json::to_string(event).map_err(|_error| AppError::new(
                                    "decision_event_invalid",
                                    "unable to encode a decision event envelope"
                                ))?
                            );
                        }
                        println!("Next cursor: {}", page.next_cursor);
                        if page.has_more {
                            println!("More events are available.");
                        }
                        Ok(())
                    }
                }
            }
        }
        Command::Review { command: _ } => Err(AppError::new(
            "legacy_surface_retired",
            "Krisis has no decision review state",
        )),
    }
}

#[derive(Debug, serde::Deserialize)]
struct StopHookInput {
    session_id: String,
    turn_id: String,
    hook_event_name: String,
}

#[derive(Debug, serde::Serialize)]
struct ProcessResult {
    observation_id: String,
    status: String,
    scope_level: i64,
    outcome: Option<String>,
}

#[derive(Clone, Copy)]
struct ProjectionFrontier {
    completion_cutoff: i64,
    admission_watermark: i64,
    window_start: i64,
    window_end: i64,
}

fn ingest_hook(store: &Store) -> AppResult<()> {
    const MAX_HOOK_BYTES: u64 = 65_536;
    if store.observer_baseline_at()?.is_none() {
        return Err(AppError::new(
            "observer_not_activated",
            "activate the observer before accepting Stop hooks",
        ));
    }
    let mut input = Vec::new();
    std::io::stdin()
        .take(MAX_HOOK_BYTES + 1)
        .read_to_end(&mut input)
        .map_err(|error| {
            AppError::new(
                "hook_input_failed",
                format!("unable to read Stop-hook input: {error}"),
            )
        })?;
    if input.len() > usize::try_from(MAX_HOOK_BYTES).unwrap_or(usize::MAX) {
        return Err(AppError::new(
            "hook_input_too_large",
            "Stop-hook input exceeds 64 KiB",
        ));
    }
    let input: StopHookInput = serde_json::from_slice(&input).map_err(|_error| {
        AppError::new(
            "hook_input_invalid",
            "Stop-hook input is not the expected JSON object",
        )
    })?;
    if input.hook_event_name != "Stop" {
        return Err(AppError::new(
            "hook_event_invalid",
            "observe ingest accepts only Stop-hook input",
        ));
    }
    validate_hook_id("session_id", &input.session_id)?;
    validate_hook_id("turn_id", &input.turn_id)?;
    let _observation = store.ingest_observation(&input.session_id, &input.turn_id)?;
    println!("{{}}");
    Ok(())
}

fn validate_hook_id(field: &str, value: &str) -> AppResult<()> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(AppError::new(
            "hook_input_invalid",
            format!("{field} must contain 1 to 512 non-control bytes"),
        ));
    }
    Ok(())
}

fn process_one_observation(
    store: &mut Store,
    annals: &account::AnnalsConfig,
) -> AppResult<Option<ProcessResult>> {
    process_one_observation_for_projection(store, None, false, Some(annals))
}

#[allow(clippy::too_many_lines)]
fn process_one_observation_for_projection(
    store: &mut Store,
    projection_frontier: Option<ProjectionFrontier>,
    wait_for_active_observer: bool,
    annals: Option<&account::AnnalsConfig>,
) -> AppResult<Option<ProcessResult>> {
    let baseline = store.observer_baseline_at()?.ok_or_else(|| {
        AppError::new(
            "observer_not_activated",
            "activate the observer before processing completed turns",
        )
    })?;
    let _processing_lock = if wait_for_active_observer {
        store.wait_for_observation_processing()?
    } else {
        store.lock_observation_processing()?
    };
    let observation = match projection_frontier {
        Some(frontier) => store.next_observation_for_projection(
            frontier.completion_cutoff,
            frontier.admission_watermark,
            frontier.window_start,
            frontier.window_end,
        )?,
        None => store.next_observation_before(None)?,
    };
    let Some(observation) = observation else {
        return Ok(None);
    };
    let annals = annals.ok_or_else(|| {
        AppError::new(
            "annals_configuration_required",
            "classification requires a verified dedicated Annals target",
        )
    })?;
    store.bind_observation_annals_target(
        &observation.id,
        &annals.expected_library_id,
        account::config_path(annals)?,
    )?;
    let loaded = match source::load_observation(
        &observation.session_id,
        observation.thread_id.as_deref(),
        &observation.turn_id,
        observation.scope_level,
        baseline,
    ) {
        Ok(loaded) => loaded,
        Err(error)
            if matches!(
                error.code,
                "conversation_source_not_completed" | "conversation_source_pending"
            ) =>
        {
            let observed_at = now_unix();
            let not_completed_at =
                (error.code == "conversation_source_not_completed").then_some(observed_at);
            store.defer_observation(
                &observation.id,
                not_completed_at,
                observed_at.saturating_add(5),
            )?;
            return Ok(Some(ProcessResult {
                observation_id: observation.id,
                status: "queued".to_owned(),
                scope_level: observation.scope_level,
                outcome: None,
            }));
        }
        Err(error) => {
            store.fail_observation(&observation.id, error.code, &error.message)?;
            return Err(error);
        }
    };
    let source = match loaded {
        ObservationLoad::NotEligible {
            host_id,
            thread_id,
            authority_occurred_at,
            source_completed_at,
        } => {
            store.mark_observation_not_eligible(
                &observation.id,
                &host_id,
                &thread_id,
                source_completed_at,
                authority_occurred_at,
            )?;
            return Ok(Some(ProcessResult {
                observation_id: observation.id,
                status: "complete".to_owned(),
                scope_level: observation.scope_level,
                outcome: Some("not_eligible".to_owned()),
            }));
        }
        ObservationLoad::Eligible(source) => source,
    };
    if let Err(error) = store.bind_observation_source(
        &observation.id,
        &source.transcript.host_id,
        &source.transcript.thread_id,
        source.source_completed_at,
        &source.source_digest,
        0,
        &source.authorities,
    ) {
        fail_observation_if_terminal(store, &observation.id, &error)?;
        return Err(error);
    }
    if projection_frontier.is_some_and(|frontier| {
        source.source_completed_at > frontier.completion_cutoff
            || !source.authorities.iter().any(|authority| {
                authority.occurred_at >= frontier.window_start
                    && authority.occurred_at < frontier.window_end
            })
    }) {
        return Ok(Some(ProcessResult {
            observation_id: observation.id,
            status: "deferred".to_owned(),
            scope_level: observation.scope_level,
            outcome: None,
        }));
    }
    let result = match classify_observation_with_retries(store, &observation, &source) {
        Ok(result) => result,
        Err(error) if classifier_failure_proves_terminal(&error) => {
            store.fail_observation(&observation.id, error.code, &error.message)?;
            return Err(error);
        }
        Err(error) => return Err(error),
    };
    if result.needs_context {
        store.advance_observation_scope(&observation.id)?;
        return Ok(Some(ProcessResult {
            observation_id: observation.id,
            status: "queued".to_owned(),
            scope_level: 1,
            outcome: None,
        }));
    }
    if let Err(error) =
        store.complete_observation_if_needed(&observation.id, &result.as_observation())
    {
        fail_observation_if_terminal(store, &observation.id, &error)?;
        return Err(error);
    }
    let outcome = if result.accounts.is_empty() && result.candidates.is_empty() {
        "no_decision"
    } else {
        "decision"
    };
    Ok(Some(ProcessResult {
        observation_id: observation.id,
        status: "complete".to_owned(),
        scope_level: observation.scope_level,
        outcome: Some(outcome.to_owned()),
    }))
}

fn classify_observation_with_retries(
    store: &mut Store,
    observation: &Observation,
    source: &source::ObservationSource,
) -> AppResult<ClassificationResult> {
    let requester_id = format!("krisis-observe-{}", observation.id);
    let runner = Runner::for_current_user();
    let baseline = store
        .observer_baseline_at()?
        .ok_or_else(|| AppError::new("observer_not_activated", "observer baseline disappeared"))?;
    let mut last_terminal_error = None;
    for local_attempt in 0..CLASSIFICATION_ATTEMPTS {
        let epoch = usize::try_from(observation.attempt_epoch).map_err(|_| {
            AppError::new(
                "observation_attempt_invalid",
                "the observation retry epoch is outside the supported range",
            )
        })?;
        let attempt = epoch
            .checked_mul(CLASSIFICATION_ATTEMPTS)
            .and_then(|base| base.checked_add(local_attempt))
            .ok_or_else(|| {
                AppError::new(
                    "observation_attempt_invalid",
                    "the observation retry identity exceeds the supported range",
                )
            })?;
        let job_id = format!(
            "krisis-observe-{}-s{}-a{}",
            observation.id.trim_start_matches("o_"),
            observation.scope_level,
            attempt
        );
        store.plan_observation_job(&observation.id, observation.scope_level, attempt, &job_id)?;
        if store.job_status(&job_id)? == "failed" {
            continue;
        }
        match runner.classify_observation(
            store,
            &requester_id,
            &job_id,
            &source.transcript,
            baseline,
            i64::MAX,
            &observation.turn_id,
            observation.scope_level == 0,
        ) {
            Ok(result) => {
                let _changed = store.mark_job(&job_id, "complete", None)?;
                return Ok(result);
            }
            Err(error) if classifier_failure_proves_terminal(&error) => {
                if store.mark_job(&job_id, "failed", Some(&error.message))? {
                    last_terminal_error = Some(error);
                } else if let Some(classification) =
                    store.persisted_observation_classification(&job_id)?
                {
                    let _changed = store.mark_job(&job_id, "complete", None)?;
                    return Ok(classification.into());
                } else {
                    return Err(AppError::new(
                        "job_state_conflict",
                        "terminal observation state changed without a durable result",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_terminal_error.unwrap_or_else(|| {
        AppError::new(
            "nucleus_job_failed",
            "all three durable Nucleus attempts ended without a valid observation classification",
        )
    }))
}

fn observation_completion_failure_terminal(error: &AppError) -> bool {
    matches!(
        error.code,
        "classification_conflict"
            | "classification_coverage_invalid"
            | "classification_confidence_invalid"
            | "observation_authority_invalid"
            | "observation_authority_missing"
            | "observation_source_conflict"
            | "observation_state_conflict"
    )
}

fn fail_observation_if_terminal(
    store: &Store,
    observation_id: &str,
    error: &AppError,
) -> AppResult<()> {
    if observation_completion_failure_terminal(error) {
        store.fail_observation(observation_id, error.code, &error.message)?;
    }
    Ok(())
}

fn print_process_result(result: Option<&ProcessResult>, json_output: bool) -> AppResult<()> {
    if json_output {
        return print_json(&match result {
            Some(result) => json!({"processed": true, "observation": result}),
            None => json!({"processed": false}),
        });
    }
    match result {
        Some(result) => println!(
            "{} {} (scope {})",
            result.observation_id, result.status, result.scope_level
        ),
        None => println!("No observation ready"),
    }
    Ok(())
}

fn reconcile(
    store: &mut Store,
    date: Date,
    completed_cutoff: i64,
) -> AppResult<source::ReconcileResult> {
    let baseline = store.observer_baseline_at()?.ok_or_else(|| {
        AppError::new(
            "observer_not_activated",
            "activate the observer before reconciling completed turns",
        )
    })?;
    let (window_start, window_end) = local_window(date)?;
    source::reconcile_window(store, baseline, window_start, window_end, completed_cutoff)
}

fn reconciliation_cutoff() -> i64 {
    now_unix().saturating_sub(1)
}

fn default_observer_baseline(now: i64) -> i64 {
    now.saturating_add(1)
}

fn abandon(store: &mut Store, date: Date, json_output: bool) -> AppResult<()> {
    let _operation_lock = store.lock_run_operations()?;
    let report_date = format_date(date);
    let (run, jobs) = store.prepare_abandon(&report_date)?;
    if let Err(error) = Runner::for_current_user().reconcile_abandonment(&jobs) {
        if error.code == "abandonment_job_unavailable" {
            store.restore_build_after_unresolved_admission(&run.id, &jobs)?;
        }
        return Err(error);
    }
    store.finish_abandon(&run.id, &jobs)?;
    if json_output {
        print_json(&json!({
            "run_id": run.id,
            "report_date": report_date,
            "status": "failed",
            "reason": "abandoned",
            "jobs_reconciled": jobs.len()
        }))
    } else {
        println!(
            "Abandoned {} for {} after reconciling {} Nucleus jobs",
            run.id,
            report_date,
            jobs.len()
        );
        Ok(())
    }
}

fn doctor(
    database: &Path,
    annals: Option<&account::AnnalsConfig>,
    json_output: bool,
) -> AppResult<()> {
    let annals = annals.ok_or_else(|| {
        AppError::new(
            "annals_configuration_required",
            "doctor requires explicit Annals binary, decisions config, and expected library ID",
        )
    })?;
    let store = Store::open(database)?;
    let schema_version = store.schema_version()?;
    let observer_baseline_at = store.observer_baseline_at()?;
    let observer = store.observation_status()?;
    let mut conversations = AppServerClient::spawn(ClientConfig {
        stderr_policy: StderrPolicy::Suppress,
        ..ClientConfig::default()
    })
    .map_err(|_error| {
        AppError::new(
            "conversation_source_failed",
            "unable to start the Codex conversation source; inspect Conversations diagnostics",
        )
    })?;
    let source = conversations.doctor().map_err(|_error| {
        AppError::new(
            "conversation_source_failed",
            "Codex conversation source is not ready; inspect Conversations diagnostics",
        )
    })?;
    Runner::for_current_user().doctor()?;
    account::doctor(annals)?;
    if json_output {
        print_json(&json!({
            "ok": true,
            "schema_version": schema_version,
            "observer_baseline_at": observer_baseline_at,
            "observer": observer,
            "conversation_source": source,
            "nucleus": "ready",
            "annals_library_id": annals.expected_library_id
        }))
    } else {
        println!(
            "ready: schema v{schema_version}, observer {} (queued {}, processing {}, failed {}), {} visible conversations, Nucleus ready, Annals library {}",
            observer_baseline_at.map_or_else(|| "inactive".to_owned(), |value| value.to_string()),
            observer.queued,
            observer.processing,
            observer.failed,
            source.visible_threads,
            annals.expected_library_id
        );
        Ok(())
    }
}

#[derive(Debug, serde::Serialize)]
struct BuildResult {
    run_id: String,
    report_date: String,
    source_manifest_hash: String,
    coverage_cutoff_at: i64,
    observation_admission_watermark: i64,
    observations_covered: usize,
    candidate_count: usize,
    high_count: usize,
    medium_count: usize,
}

fn build(store: &mut Store, date: Date, wait_for_active_observer: bool) -> AppResult<BuildResult> {
    let baseline = store.observer_baseline_at()?.ok_or_else(|| {
        AppError::new(
            "observer_not_activated",
            "activate the observer before building an observation projection",
        )
    })?;
    let (day_start, window_end) = local_window(date)?;
    let window_start = day_start.max(baseline);
    let report_date = format_date(date);
    let completed_cutoff = reconciliation_cutoff();
    let mut reconciliation_passes = 0_usize;
    let admission_watermark = loop {
        let reconciled = reconcile(store, date, completed_cutoff)?;
        let pass_admission_watermark = store.observation_admission_watermark()?;
        while process_one_observation_for_projection(
            store,
            Some(ProjectionFrontier {
                completion_cutoff: completed_cutoff,
                admission_watermark: pass_admission_watermark,
                window_start,
                window_end,
            }),
            wait_for_active_observer,
            None,
        )?
        .is_some()
        {}
        reconciliation_passes += 1;
        if reconciliation_passes >= 2 && reconciled.observations_enqueued == 0 {
            break pass_admission_watermark;
        }
    };
    let projection = store.project_observations(
        &report_date,
        window_start,
        window_end,
        completed_cutoff,
        admission_watermark,
    )?;
    let candidates = store.candidates_for_run(&projection.run.id)?;
    let high_count = candidates
        .iter()
        .filter(|candidate| candidate.confidence == "high")
        .count();
    let medium_count = candidates
        .iter()
        .filter(|candidate| candidate.confidence == "medium")
        .count();
    Ok(BuildResult {
        run_id: projection.run.id,
        report_date,
        source_manifest_hash: projection.source_manifest_hash,
        coverage_cutoff_at: completed_cutoff,
        observation_admission_watermark: admission_watermark,
        observations_covered: projection.observations_covered,
        candidate_count: high_count + medium_count,
        high_count,
        medium_count,
    })
}

#[cfg(test)]
fn classify_thread_with_retries<F>(
    store: &mut Store,
    run_id: &str,
    thread_key: &str,
    base_job_id: &str,
    mut classify: F,
) -> AppResult<ClassificationResult>
where
    F: FnMut(&mut Store, &str) -> AppResult<ClassificationResult>,
{
    let mut last_terminal_error = None;
    for attempt in 0..CLASSIFICATION_ATTEMPTS {
        let (correlation_key, job_id) =
            classification_attempt_identity(thread_key, base_job_id, attempt);
        store.plan_job(run_id, &correlation_key, &job_id)?;
        if store.job_status(&job_id)? == "failed" {
            continue;
        }
        match classify(store, &job_id) {
            Ok(result) => {
                store.mark_job(&job_id, "complete", None)?;
                return Ok(result);
            }
            Err(error) if classifier_failure_proves_terminal(&error) => {
                if store.mark_job(&job_id, "failed", Some(&error.message))? {
                    last_terminal_error = Some(error);
                } else if let Some(candidates) = store.persisted_classification(&job_id)? {
                    let _ = store.mark_job(&job_id, "complete", None)?;
                    return Ok(ClassificationResult {
                        candidates,
                        accounts: Vec::new(),
                        authority_verdicts: Vec::new(),
                        needs_context: false,
                    });
                } else {
                    return Err(AppError::new(
                        "job_state_conflict",
                        "terminal classification state changed without a durable result",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_terminal_error.unwrap_or_else(|| {
        AppError::new(
            "nucleus_job_failed",
            "all three durable Nucleus attempts ended without a valid classification",
        )
    }))
}

#[cfg(test)]
fn classification_attempt_identity(
    thread_key: &str,
    base_job_id: &str,
    attempt: usize,
) -> (String, String) {
    if attempt == 0 {
        return (thread_key.to_owned(), base_job_id.to_owned());
    }
    (
        format!("{thread_key}\nretry:{attempt}"),
        format!("{base_job_id}-retry{attempt}"),
    )
}

// A terminal classifier error is execution evidence that permits the next
// deterministic attempt. Every other error leaves the current correlation
// resumable and returns without allocating another Nucleus job.
fn classifier_failure_proves_terminal(error: &AppError) -> bool {
    matches!(
        error.code,
        "classification_incomplete" | "nucleus_job_failed"
    )
}

#[cfg(test)]
fn merge_candidates(
    candidates: Vec<crate::model::Candidate>,
) -> AppResult<Vec<crate::model::Candidate>> {
    let mut merged = BTreeMap::<String, crate::model::Candidate>::new();
    for candidate in candidates {
        if let Some(existing) = merged.get(&candidate.id) {
            let agrees = existing.decided_at == candidate.decided_at
                && existing.precision == candidate.precision
                && existing.statement == candidate.statement
                && existing.disposition == candidate.disposition
                && existing.confidence == candidate.confidence
                && existing.rationale == candidate.rationale
                && existing.supersedes_id == candidate.supersedes_id
                && existing.authority.host_id == candidate.authority.host_id
                && existing.authority.item_id == candidate.authority.item_id
                && existing.authority_start == candidate.authority_start
                && existing.authority_end == candidate.authority_end;
            if !agrees {
                return Err(AppError::new(
                    "classification_conflict",
                    format!(
                        "duplicate canonical authority span produced conflicting candidate {}",
                        candidate.id
                    ),
                ));
            }
        } else {
            merged.insert(candidate.id.clone(), candidate);
        }
    }
    Ok(merged.into_values().collect())
}

fn freeze(store: &Store, date: Date) -> AppResult<DigestSnapshot> {
    let run = store.latest_complete_run(&format_date(date))?;
    freeze_run(store, &run)
}

fn freeze_run(store: &Store, run: &Run) -> AppResult<DigestSnapshot> {
    if run.status != "complete" {
        return Err(AppError::new(
            "run_incomplete",
            "only a complete source and classification run can be frozen",
        ));
    }
    let candidates = store.candidates_for_run(&run.id)?;
    let cutoff = store.run_coverage_cutoff(&run.id)?;
    let (subject, body) = digest::render(&run.report_date, cutoff, &candidates);
    store.snapshot(run, &subject, &body)
}

fn run_scheduled(store: &mut Store, email_binary: &Path, json_output: bool) -> AppResult<()> {
    let occurrence = scheduled_occurrence()?;
    let occurrence_text = format_date(occurrence);
    if let Some(delivery) = store.delivery_for_occurrence(&occurrence_text)? {
        if delivery.status == "accepted" {
            return print_delivery(&delivery, json_output);
        }
        let snapshot = store.snapshot_for_delivery(&delivery)?;
        let delivery = deliver(store, email_binary, delivery, snapshot)?;
        return print_delivery(&delivery, json_output);
    }
    let report_date = occurrence.previous_day().ok_or_else(|| {
        AppError::new("local_date_failed", "scheduled report date is out of range")
    })?;
    let _ = build(store, report_date, true)?;
    let run = store.latest_complete_run(&format_date(report_date))?;
    let snapshot = freeze_run(store, &run)?;
    let delivery = store.begin_delivery(&run, Some(&occurrence_text))?;
    let delivery = deliver(store, email_binary, delivery, snapshot)?;
    print_delivery(&delivery, json_output)
}

fn deliver(
    store: &Store,
    email_binary: &Path,
    mut delivery: Delivery,
    _current_snapshot: DigestSnapshot,
) -> AppResult<Delivery> {
    if delivery.status == "accepted" {
        return Ok(delivery);
    }
    let snapshot = store.snapshot_for_delivery(&delivery)?;
    let email_binary = email_binary.to_str().ok_or_else(|| {
        AppError::new("email_binary_invalid", "Email CLI path is not valid UTF-8")
    })?;
    match email::send(email_binary, &delivery.idempotency_key, &snapshot) {
        Ok(email_id) => {
            store.finish_delivery(&delivery.id, Ok(&email_id))?;
            "accepted".clone_into(&mut delivery.status);
            delivery.email_id = Some(email_id);
            Ok(delivery)
        }
        Err(error) => {
            let _ = store.finish_delivery(&delivery.id, Err(&error.message));
            Err(error)
        }
    }
}

fn requested_date(value: Option<&str>) -> AppResult<Date> {
    if let Some(value) = value {
        return parse_date(value);
    }
    local_now()?
        .date()
        .previous_day()
        .ok_or_else(|| AppError::new("local_date_failed", "previous date is out of range"))
}

fn observe_date(value: Option<&str>) -> AppResult<Date> {
    value.map_or_else(|| local_now().map(time::OffsetDateTime::date), parse_date)
}

fn scheduled_occurrence() -> AppResult<Date> {
    let now = local_now()?;
    if now.hour() >= 9 {
        Ok(now.date())
    } else {
        now.date().previous_day().ok_or_else(|| {
            AppError::new(
                "local_date_failed",
                "scheduled occurrence date is out of range",
            )
        })
    }
}

fn local_now() -> AppResult<OffsetDateTime> {
    OffsetDateTime::now_local().map_err(|error| {
        AppError::new(
            "local_time_unavailable",
            format!("unable to determine machine-local time: {error}"),
        )
    })
}

fn local_window(date: Date) -> AppResult<(i64, i64)> {
    let next = date
        .next_day()
        .ok_or_else(|| AppError::new("local_date_failed", "next date is out of range"))?;
    Ok((
        local_midnight(date)?.unix_timestamp(),
        local_midnight(next)?.unix_timestamp(),
    ))
}

fn local_midnight(date: Date) -> AppResult<OffsetDateTime> {
    let primitive = PrimitiveDateTime::new(date, Time::MIDNIGHT);
    let probe = primitive.assume_utc();
    let offset = UtcOffset::local_offset_at(probe).map_err(|error| {
        AppError::new(
            "local_time_unavailable",
            format!("unable to resolve local offset for {date}: {error}"),
        )
    })?;
    Ok(primitive.assume_offset(offset))
}

fn parse_date(value: &str) -> AppResult<Date> {
    let mut parts = value.split('-');
    let year = parts.next().and_then(|part| part.parse::<i32>().ok());
    let month = parts.next().and_then(|part| part.parse::<u8>().ok());
    let day = parts.next().and_then(|part| part.parse::<u8>().ok());
    if parts.next().is_some()
        || value.len() != 10
        || value.as_bytes().get(4) != Some(&b'-')
        || value.as_bytes().get(7) != Some(&b'-')
    {
        return Err(AppError::new(
            "invalid_date",
            format!("{value:?} is not a YYYY-MM-DD calendar date"),
        ));
    }
    let month = month
        .and_then(|month| time::Month::try_from(month).ok())
        .ok_or_else(|| {
            AppError::new(
                "invalid_date",
                format!("{value:?} is not a YYYY-MM-DD calendar date"),
            )
        })?;
    Date::from_calendar_date(
        year.ok_or_else(|| {
            AppError::new(
                "invalid_date",
                format!("{value:?} is not a YYYY-MM-DD calendar date"),
            )
        })?,
        month,
        day.ok_or_else(|| {
            AppError::new(
                "invalid_date",
                format!("{value:?} is not a YYYY-MM-DD calendar date"),
            )
        })?,
    )
    .map_err(|_| {
        AppError::new(
            "invalid_date",
            format!("{value:?} is not a YYYY-MM-DD calendar date"),
        )
    })
}

fn format_date(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn default_email_binary() -> AppResult<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| AppError::new("home_unavailable", "HOME must be an absolute path"))?;
    Ok(home.join(".local/bin/email"))
}

fn annals_configuration(
    binary: Option<PathBuf>,
    config: Option<PathBuf>,
    expected_library_id: Option<String>,
) -> AppResult<Option<account::AnnalsConfig>> {
    match (binary, config, expected_library_id) {
        (None, None, None) => Ok(None),
        (Some(binary), Some(config), Some(expected_library_id)) => {
            Ok(Some(account::AnnalsConfig {
                binary,
                config,
                expected_library_id,
            }))
        }
        _ => Err(AppError::new(
            "annals_configuration_incomplete",
            "Annals binary, decisions config, and expected library ID must be supplied together",
        )),
    }
}

fn print_account_delivery(
    account_id: &str,
    receipt: &account::AnnalsReceipt,
    json_output: bool,
) -> AppResult<()> {
    if json_output {
        print_json(&json!({
            "processed": true,
            "kind": "annals_acceptance",
            "account_id": account_id,
            "library_id": receipt.library_id,
            "job_id": receipt.job_id,
            "accepted_at": receipt.accepted_at
        }))
    } else {
        println!("Delivered {account_id} to Annals job {}", receipt.job_id);
        Ok(())
    }
}

fn print_build(result: &BuildResult, json_output: bool) -> AppResult<()> {
    if json_output {
        print_json(result)
    } else {
        println!(
            "Built {} through completion cutoff {} and observation watermark {}: {} decisions, {} possible across {} observations ({})",
            result.report_date,
            result.coverage_cutoff_at,
            result.observation_admission_watermark,
            result.high_count,
            result.medium_count,
            result.observations_covered,
            result.run_id
        );
        Ok(())
    }
}

fn print_snapshot(snapshot: &DigestSnapshot, json_output: bool) -> AppResult<()> {
    if json_output {
        print_json(snapshot)
    } else {
        println!("Subject: {}\n\n{}", snapshot.subject, snapshot.body);
        Ok(())
    }
}

fn print_delivery(delivery: &Delivery, json_output: bool) -> AppResult<()> {
    if json_output {
        print_json(delivery)
    } else {
        println!(
            "{} {} ({})",
            delivery.status,
            delivery.email_id.as_deref().unwrap_or("no email ID"),
            delivery.idempotency_key
        );
        Ok(())
    }
}

fn print_json(value: &impl serde::Serialize) -> AppResult<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value)
            .map_err(|error| AppError::new("json_encode_failed", error.to_string()))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::classifier::ClassificationResult;
    use crate::model::{Candidate, Confidence, Disposition, MessageRole, Precision, SourceMessage};
    use crate::store::Store;

    use crate::error::AppError;

    use super::{
        classification_attempt_identity, classifier_failure_proves_terminal,
        classify_thread_with_retries, default_observer_baseline, fail_observation_if_terminal,
        format_date, merge_candidates, parse_date,
    };

    fn candidate(statement: &str, thread_id: &str) -> Candidate {
        Candidate {
            id: "d_same".to_owned(),
            decided_at: 10,
            precision: Precision::Item,
            statement: statement.to_owned(),
            disposition: Disposition::Adopt,
            confidence: Confidence::High,
            rationale: None,
            supersedes_id: None,
            authority_start: 0,
            authority_end: 6,
            authority: SourceMessage {
                host_id: "host".to_owned(),
                thread_id: thread_id.to_owned(),
                turn_id: format!("turn-{thread_id}"),
                item_id: "canonical-item".to_owned(),
                role: MessageRole::User,
                text: "Use it.".to_owned(),
                occurred_at: 10,
                precision: Precision::Item,
            },
            context: Vec::new(),
        }
    }

    #[test]
    fn parses_strict_calendar_date() -> Result<(), Box<dyn std::error::Error>> {
        let date = parse_date("2026-08-31")?;
        assert_eq!(format_date(date), "2026-08-31");
        assert!(parse_date("08/31/2026").is_err());
        Ok(())
    }

    #[test]
    fn default_activation_excludes_the_current_whole_second() {
        assert_eq!(default_observer_baseline(100), 101);
        assert_eq!(default_observer_baseline(i64::MAX), i64::MAX);
    }

    #[test]
    fn deterministic_completion_conflict_becomes_failed_and_retryable()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("decisions.db"))?;
        let observation = store.ingest_observation("session", "turn")?;
        let error = AppError::new("classification_conflict", "private model detail");
        fail_observation_if_terminal(&store, &observation.id, &error)?;
        let failed = store.observation(&observation.id)?;
        assert_eq!(failed.status, "failed");
        assert_eq!(
            failed.failure_code.as_deref(),
            Some("classification_conflict")
        );
        let retried = store.retry_observation(&observation.id)?;
        assert_eq!(retried.status, "queued");
        assert_eq!(retried.attempt_epoch, 1);
        Ok(())
    }

    #[test]
    fn only_observed_terminal_classifier_failures_allow_a_new_attempt() {
        assert!(classifier_failure_proves_terminal(&AppError::new(
            "nucleus_job_failed",
            "terminal"
        )));
        assert!(classifier_failure_proves_terminal(&AppError::new(
            "classification_incomplete",
            "terminal"
        )));
        assert!(!classifier_failure_proves_terminal(&AppError::new(
            "nucleus_request_failed",
            "uncertain"
        )));
        assert!(!classifier_failure_proves_terminal(&AppError::new(
            "nucleus_timeout",
            "uncertain"
        )));
    }

    #[test]
    fn retry_identity_preserves_attempt_zero_and_is_deterministic() {
        assert_eq!(
            classification_attempt_identity("host\nthread", "job", 0),
            ("host\nthread".to_owned(), "job".to_owned())
        );
        assert_eq!(
            classification_attempt_identity("host\nthread", "job", 1),
            ("host\nthread\nretry:1".to_owned(), "job-retry1".to_owned())
        );
        assert_eq!(
            classification_attempt_identity("host\nthread", "job", 2),
            ("host\nthread\nretry:2".to_owned(), "job-retry2".to_owned())
        );
    }

    #[test]
    fn uncertain_retry_resumes_its_existing_durable_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("state/decisions.db");
        let mut store = Store::open(&database)?;
        let run = store.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        let mut first_calls = Vec::new();
        let first = classify_thread_with_retries(
            &mut store,
            &run.id,
            "host\nthread",
            "job",
            |store, job_id| {
                first_calls.push(job_id.to_owned());
                store.mark_job(job_id, "submitted", None)?;
                if job_id == "job" {
                    Err(AppError::new("nucleus_job_failed", "terminal"))
                } else {
                    let durable_candidate = candidate("Use it.", "thread");
                    store.persist_classification_receipt(
                        job_id,
                        "call-retry1",
                        r#"{"accepted":true,"candidate_count":1}"#,
                        false,
                        Some(std::slice::from_ref(&durable_candidate)),
                    )?;
                    Err(AppError::new("nucleus_request_failed", "uncertain"))
                }
            },
        );
        let Err(error) = first else {
            return Err("uncertain attempt unexpectedly completed".into());
        };
        assert_eq!(error.code, "nucleus_request_failed");
        assert_eq!(first_calls, ["job", "job-retry1"]);
        assert_eq!(store.job_status("job")?, "failed");
        assert_eq!(store.job_status("job-retry1")?, "complete");
        assert!(store.job_status("job-retry2").is_err());
        assert!(
            store
                .classification_receipt("job", "call-retry1")?
                .is_none()
        );
        assert!(
            store
                .classification_receipt("job-retry1", "call-retry1")?
                .is_some()
        );

        drop(store);
        let mut store = Store::open(&database)?;
        let resumed = store.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        assert_eq!(resumed.id, run.id);
        let mut resumed_calls = Vec::new();
        let result = classify_thread_with_retries(
            &mut store,
            &resumed.id,
            "host\nthread",
            "job",
            |store, job_id| {
                resumed_calls.push(job_id.to_owned());
                store.mark_job(job_id, "submitted", None)?;
                let candidates = store.persisted_classification(job_id)?.ok_or_else(|| {
                    AppError::new(
                        "classification_incomplete",
                        "missing durable classification",
                    )
                })?;
                Ok(ClassificationResult {
                    candidates,
                    accounts: Vec::new(),
                    authority_verdicts: Vec::new(),
                    needs_context: false,
                })
            },
        )?;
        assert_eq!(result.candidates.len(), 1);
        assert!(result.candidates[0].authority.text.is_empty());
        assert_eq!(resumed_calls, ["job-retry1"]);
        assert_eq!(store.job_status("job-retry1")?, "complete");
        assert!(store.job_status("job-retry2").is_err());
        Ok(())
    }

    #[test]
    fn success_receipt_wins_before_terminal_failure_and_prevents_retry()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("state/decisions.db"))?;
        let run = store.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        let mut calls = Vec::new();
        let result = classify_thread_with_retries(
            &mut store,
            &run.id,
            "host\nthread",
            "job",
            |store, job_id| {
                calls.push(job_id.to_owned());
                store.mark_job(job_id, "submitted", None)?;
                let durable_candidate = candidate("Use it.", "thread");
                store.persist_classification_receipt(
                    job_id,
                    "call",
                    r#"{"accepted":true,"candidate_count":1}"#,
                    false,
                    Some(std::slice::from_ref(&durable_candidate)),
                )?;
                Err(AppError::new("nucleus_job_failed", "stale terminal"))
            },
        )?;
        assert_eq!(calls, ["job"]);
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(store.job_status("job")?, "complete");
        assert!(store.job_status("job-retry1").is_err());
        Ok(())
    }

    #[test]
    fn terminal_failure_wins_before_late_receipt_and_advances_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("state/decisions.db"))?;
        let run = store.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        let mut calls = Vec::new();
        let result = classify_thread_with_retries(
            &mut store,
            &run.id,
            "host\nthread",
            "job",
            |store, job_id| {
                calls.push(job_id.to_owned());
                store.mark_job(job_id, "submitted", None)?;
                if job_id == "job" {
                    Err(AppError::new("nucleus_job_failed", "terminal"))
                } else {
                    Err(AppError::new("nucleus_request_failed", "uncertain"))
                }
            },
        );
        let Err(error) = result else {
            return Err("uncertain retry unexpectedly completed".into());
        };
        assert_eq!(error.code, "nucleus_request_failed");
        assert_eq!(calls, ["job", "job-retry1"]);
        assert_eq!(store.job_status("job")?, "failed");
        assert_eq!(store.job_status("job-retry1")?, "submitted");
        assert!(store.job_status("job-retry2").is_err());

        let durable_candidate = candidate("Use it.", "thread");
        let late = store.persist_classification_receipt(
            "job",
            "call",
            r#"{"accepted":true,"candidate_count":1}"#,
            false,
            Some(std::slice::from_ref(&durable_candidate)),
        );
        assert_eq!(
            late.err().map(|error| error.code),
            Some("classification_receipt_late")
        );
        assert_eq!(store.job_status("job")?, "failed");
        Ok(())
    }

    #[test]
    fn terminal_retry_exhaustion_stops_after_three_durable_attempts()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("state/decisions.db"))?;
        let run = store.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        let mut calls = Vec::new();
        let result = classify_thread_with_retries(
            &mut store,
            &run.id,
            "host\nthread",
            "job",
            |store, job_id| {
                calls.push(job_id.to_owned());
                store.mark_job(job_id, "submitted", None)?;
                Err(AppError::new("nucleus_job_failed", "terminal"))
            },
        );
        let Err(error) = result else {
            return Err("terminal attempts unexpectedly completed".into());
        };
        assert_eq!(error.code, "nucleus_job_failed");
        assert_eq!(calls, ["job", "job-retry1", "job-retry2"]);
        for job_id in &calls {
            assert_eq!(store.job_status(job_id)?, "failed");
        }

        let mut unexpected_calls = 0;
        let resumed = classify_thread_with_retries(
            &mut store,
            &run.id,
            "host\nthread",
            "job",
            |_store, _job_id| {
                unexpected_calls += 1;
                Ok(ClassificationResult {
                    candidates: Vec::new(),
                    accounts: Vec::new(),
                    authority_verdicts: Vec::new(),
                    needs_context: false,
                })
            },
        );
        let Err(error) = resumed else {
            return Err("exhausted attempts unexpectedly resumed".into());
        };
        assert_eq!(error.code, "nucleus_job_failed");
        assert_eq!(unexpected_calls, 0);
        let (_abandoning, correlated_jobs) = store.prepare_abandon("2026-08-31")?;
        assert_eq!(correlated_jobs.len(), 3);
        assert!(correlated_jobs.iter().all(|job| job.admitted));
        assert_eq!(
            correlated_jobs
                .iter()
                .map(|job| job.nucleus_job_id.as_str())
                .collect::<Vec<_>>(),
            ["job", "job-retry1", "job-retry2"]
        );
        Ok(())
    }

    #[test]
    fn merges_identical_fork_candidates_but_refuses_semantic_conflict() {
        let merged = merge_candidates(vec![
            candidate("Use it.", "original"),
            candidate("Use it.", "fork"),
        ]);
        assert_eq!(merged.ok().map(|values| values.len()), Some(1));
        let conflict = merge_candidates(vec![
            candidate("Use it.", "original"),
            candidate("Use something else.", "fork"),
        ]);
        assert_eq!(
            conflict.err().map(|error| error.code),
            Some("classification_conflict")
        );
    }
}
