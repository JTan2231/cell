use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::git;
use crate::model::{
    AttemptView, DocumentView, MAX_CONTRACT_UNITS, MAX_INPUT_BUNDLE_BYTES, NewRun, OpaqueMarkdown,
    PacketView, RecoveryEnvelope, ReviewScopeView, RunView,
};
use crate::nucleus::{AgentRunner, HealthSummary};
use crate::store::Store;
use crate::workflow::Workflow;

#[derive(Debug, Parser)]
#[command(
    name = "vizier",
    version,
    about = "Finite contract-bounded implementation delegation"
)]
struct Cli {
    /// Absolute path to the private Vizier `SQLite` ledger.
    #[arg(long, global = true)]
    database: Option<PathBuf>,
    /// Emit the supported machine-readable output form.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize the private ledger.
    Init,
    /// Check the ledger, Git, and exact Nucleus requester capabilities.
    Doctor,
    /// Read one retained exact Markdown document.
    Document(DocumentArgs),
    /// Submit, inspect, recover, or cancel a run.
    Run(RunArgs),
    /// Inspect or explicitly retry one Nucleus-backed attempt.
    Attempt(AttemptArgs),
}

#[derive(Debug, Args)]
struct DocumentArgs {
    #[command(subcommand)]
    command: DocumentCommand,
}

#[derive(Debug, Subcommand)]
enum DocumentCommand {
    /// Emit the exact retained Markdown body and supported metadata in JSON.
    Show { document_id: String },
}

#[derive(Debug, Args)]
struct RunArgs {
    #[command(subcommand)]
    command: RunCommand,
}

#[derive(Debug, Subcommand)]
enum RunCommand {
    /// Freeze exact inputs and synchronously drive a finite run.
    Submit(SubmitArgs),
    /// List durable runs.
    List,
    /// Show one durable run and its mechanical workflow records.
    Show { run_id: String },
    /// Alias for show.
    Status { run_id: String },
    /// Service and wait for recoverable in-flight work.
    Wait { run_id: String },
    /// Resume the exact persisted workflow after interruption.
    Resume { run_id: String },
    /// Create and synchronously drive one explicitly requested linked child.
    Continue(ContinueArgs),
    /// Record cancellation intent and cancel correlated active Nucleus jobs.
    Cancel { run_id: String },
}

#[derive(Debug, Args)]
struct ContinueArgs {
    run_id: String,
    #[arg(long)]
    request_key: String,
    #[arg(long)]
    remediation_rounds: u32,
}

#[derive(Debug, Args)]
struct SubmitArgs {
    #[arg(long)]
    repository: PathBuf,
    #[arg(long)]
    brief: String,
    #[arg(long)]
    terminology: String,
    #[arg(long = "contract", required = true)]
    contracts: Vec<String>,
    #[arg(long = "gate")]
    gates: Vec<String>,
    #[arg(long, default_value = "HEAD")]
    source: String,
    #[arg(long)]
    request_key: Option<String>,
    #[arg(long, default_value_t = 1)]
    remediation_rounds: u32,
}

#[derive(Debug, Args)]
struct AttemptArgs {
    #[command(subcommand)]
    command: AttemptCommand,
}

#[derive(Debug, Subcommand)]
enum AttemptCommand {
    Show { attempt_id: String },
    Retry { attempt_id: String },
}

#[derive(Debug, Serialize)]
struct InitOutput {
    status: &'static str,
    database: String,
}

#[derive(Debug, Serialize)]
struct DoctorOutput {
    status: &'static str,
    database: String,
    git: String,
    nucleus: HealthSummary,
}

#[derive(Debug, Serialize)]
struct RunStatusOutput {
    run: RunView,
    recovery: Option<RecoveryEnvelope>,
    documents: Vec<DocumentSummary>,
    packets: Vec<PacketView>,
    attempts: Vec<AttemptSummary>,
}

#[derive(Debug, Serialize)]
struct DocumentSummary {
    id: String,
    kind: String,
    subject_id: Option<String>,
    ordinal: u32,
    sha256: String,
    byte_len: usize,
}

#[derive(Debug, Serialize)]
struct DocumentOutput {
    id: String,
    run_id: String,
    kind: String,
    subject_id: Option<String>,
    ordinal: u32,
    sha256: String,
    markdown: String,
}

#[derive(Debug, Serialize)]
struct AttemptSummary {
    id: String,
    run_id: String,
    role: String,
    subject_id: String,
    round: u32,
    targeted: bool,
    state: String,
    nucleus_job_id: String,
    request_sha256: String,
    admitted: bool,
    domain_document_id: Option<String>,
    disposition: Option<String>,
    predecessor_attempt_id: Option<String>,
    detail: Option<String>,
    review_scope: Option<ReviewScopeView>,
}

pub fn run() -> AppResult<()> {
    let cli = Cli::parse();
    let database = cli.database.map_or_else(default_database, Ok)?;
    require_absolute(&database, "database")?;
    let store = Store::new(&database);
    match cli.command {
        Command::Init => {
            store.initialize()?;
            emit(
                cli.json,
                &InitOutput {
                    status: "initialized",
                    database: database.to_string_lossy().into_owned(),
                },
                &format!("initialized {}", database.display()),
            )
        }
        Command::Doctor => {
            store.check_ready()?;
            let git = git::require_git()?;
            let nucleus = runtime()?.block_on(AgentRunner::for_current_user().doctor())?;
            store.secure_files()?;
            emit(
                cli.json,
                &DoctorOutput {
                    status: "ready",
                    database: database.to_string_lossy().into_owned(),
                    git: git.clone(),
                    nucleus,
                },
                &format!("ready: {git}"),
            )
        }
        Command::Document(arguments) => {
            store.check_ready_readonly()?;
            match arguments.command {
                DocumentCommand::Show { document_id } => {
                    emit_document(&store.document(&document_id)?, cli.json)
                }
            }
        }
        Command::Run(arguments) => {
            store.check_ready()?;
            run_command(&store, arguments.command, cli.json)
        }
        Command::Attempt(arguments) => {
            store.check_ready()?;
            attempt_command(&store, arguments.command, cli.json)
        }
    }
}

fn run_command(store: &Store, command: RunCommand, json: bool) -> AppResult<()> {
    match command {
        RunCommand::Submit(arguments) => {
            let input = read_submit(arguments)?;
            let run = store.create_run(&input)?;
            let workflow = Workflow::new(store.clone(), AgentRunner::for_current_user());
            let result = runtime()?.block_on(workflow.drive(&run.id))?;
            emit_run_status(store, &result.id, json)
        }
        RunCommand::Continue(arguments) => {
            validate_remediation_rounds(arguments.remediation_rounds)?;
            let child = store.admit_continuation(
                &arguments.run_id,
                &arguments.request_key,
                arguments.remediation_rounds,
            )?;
            let workflow = Workflow::new(store.clone(), AgentRunner::for_current_user());
            let result = runtime()?.block_on(workflow.drive(&child.id))?;
            emit_run_status(store, &result.id, json)
        }
        RunCommand::List => {
            let runs = store.list_runs()?;
            if json {
                emit_json(&runs)
            } else {
                for run in runs {
                    println!(
                        "{}\t{}\t{}",
                        run.id,
                        run.state.as_str(),
                        run.final_ref.as_deref().unwrap_or("-")
                    );
                }
                Ok(())
            }
        }
        RunCommand::Show { run_id } | RunCommand::Status { run_id } => {
            emit_run_status(store, &run_id, json)
        }
        RunCommand::Wait { run_id } | RunCommand::Resume { run_id } => {
            let workflow = Workflow::new(store.clone(), AgentRunner::for_current_user());
            runtime()?.block_on(workflow.drive(&run_id))?;
            emit_run_status(store, &run_id, json)
        }
        RunCommand::Cancel { run_id } => {
            store.request_cancel(&run_id)?;
            runtime()?.block_on(AgentRunner::for_current_user().cancel_run(store, &run_id))?;
            emit_run_status(store, &run_id, json)
        }
    }
}

fn attempt_command(store: &Store, command: AttemptCommand, json: bool) -> AppResult<()> {
    match command {
        AttemptCommand::Show { attempt_id } => {
            let attempt = attempt_summary(store, store.attempt(&attempt_id)?)?;
            emit(
                json,
                &attempt,
                &format!(
                    "{}\t{}\t{}\t{}",
                    attempt.id, attempt.role, attempt.state, attempt.nucleus_job_id
                ),
            )
        }
        AttemptCommand::Retry { attempt_id } => {
            let workflow = Workflow::new(store.clone(), AgentRunner::for_current_user());
            let run = runtime()?.block_on(workflow.retry_attempt(&attempt_id))?;
            emit_run_status(store, &run.id, json)
        }
    }
}

fn read_submit(arguments: SubmitArgs) -> AppResult<NewRun> {
    validate_remediation_rounds(arguments.remediation_rounds)?;
    if arguments.contracts.len() > MAX_CONTRACT_UNITS {
        return Err(AppError::new(
            "too_many_contract_units",
            format!("at most {MAX_CONTRACT_UNITS} contract units are supported"),
        ));
    }
    let repository = git::canonical_repository(&arguments.repository)?;
    let source_commit = git::resolve_commit(&repository, &arguments.source)?;
    let mut stdin_used = false;
    let brief = read_markdown(&arguments.brief, &mut stdin_used)?;
    let terminology = read_markdown(&arguments.terminology, &mut stdin_used)?;
    let mut contracts = Vec::with_capacity(arguments.contracts.len());
    let mut contract_ids = BTreeSet::new();
    for value in arguments.contracts {
        let (id, file) = split_assignment(&value, "contract", "ID=FILE")?;
        validate_identifier(id, "contract ID")?;
        if !contract_ids.insert(id.to_owned()) {
            return Err(AppError::new(
                "duplicate_contract_unit",
                format!("contract unit {id} was supplied more than once"),
            ));
        }
        contracts.push((id.to_owned(), read_markdown(file, &mut stdin_used)?));
    }
    let mut gates = Vec::with_capacity(arguments.gates.len());
    let mut gate_names = BTreeSet::new();
    for value in arguments.gates {
        let (name, command) = split_assignment(&value, "gate", "NAME=COMMAND")?;
        validate_identifier(name, "gate name")?;
        if command.is_empty() {
            return Err(AppError::new(
                "gate_command_empty",
                "gate command must not be empty",
            ));
        }
        if !gate_names.insert(name.to_owned()) {
            return Err(AppError::new(
                "duplicate_gate",
                format!("gate {name} was supplied more than once"),
            ));
        }
        gates.push((name.to_owned(), command.to_owned()));
    }
    if let Some(key) = &arguments.request_key {
        validate_identifier(key, "request key")?;
    }
    let total = brief
        .as_bytes()
        .len()
        .saturating_add(terminology.as_bytes().len())
        .saturating_add(
            contracts
                .iter()
                .map(|(_, markdown)| markdown.as_bytes().len())
                .sum::<usize>(),
        );
    if total > MAX_INPUT_BUNDLE_BYTES {
        return Err(AppError::new(
            "input_bundle_too_large",
            format!("exact Markdown inputs exceed {MAX_INPUT_BUNDLE_BYTES} bytes"),
        ));
    }
    Ok(NewRun {
        id: format!("run-{}", Uuid::now_v7()),
        request_key: arguments.request_key,
        repository: repository.to_string_lossy().into_owned(),
        source_commit,
        brief,
        terminology,
        contracts,
        gates,
        remediation_limit: arguments.remediation_rounds,
    })
}

fn read_markdown(locator: &str, stdin_used: &mut bool) -> AppResult<OpaqueMarkdown> {
    let bytes = if locator == "-" {
        if *stdin_used {
            return Err(AppError::new(
                "stdin_reused",
                "at most one Markdown input may use stdin",
            ));
        }
        *stdin_used = true;
        let mut bytes = Vec::new();
        std::io::stdin()
            .take((crate::model::MAX_MARKDOWN_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        bytes
    } else {
        std::fs::read(locator).map_err(|error| {
            AppError::new(
                "markdown_read_failed",
                format!("cannot read exact Markdown from {locator}: {error}"),
            )
        })?
    };
    OpaqueMarkdown::new(bytes)
}

fn emit_document(document: &DocumentView, json: bool) -> AppResult<()> {
    if json {
        emit_json(&DocumentOutput {
            id: document.id.clone(),
            run_id: document.run_id.clone(),
            kind: document.kind.clone(),
            subject_id: document.subject_id.clone(),
            ordinal: document.ordinal,
            sha256: document.sha256.clone(),
            markdown: document.markdown.as_str().to_owned(),
        })
    } else {
        // Exact body output intentionally adds no newline or formatting.
        print!("{}", document.markdown.as_str());
        Ok(())
    }
}

fn emit_run_status(store: &Store, run_id: &str, json: bool) -> AppResult<()> {
    let run = store.run(run_id)?;
    let output = RunStatusOutput {
        run: run.clone(),
        recovery: store.recovery_envelope(run_id)?,
        documents: all_document_summaries(store, run_id)?,
        packets: store.packets(run_id)?,
        attempts: store
            .attempts(run_id)?
            .into_iter()
            .map(|attempt| attempt_summary(store, attempt))
            .collect::<AppResult<Vec<_>>>()?,
    };
    emit(
        json,
        &output,
        &format!(
            "{}\t{}\t{}",
            run.id,
            run.state.as_str(),
            run.final_ref.as_deref().unwrap_or("-")
        ),
    )
}

fn all_document_summaries(store: &Store, run_id: &str) -> AppResult<Vec<DocumentSummary>> {
    let mut values = Vec::new();
    for kind in [
        "brief",
        "terminology",
        "contract_unit",
        "unit_plan",
        "delegation_plan",
        "packet_plan",
        "plan_review",
        "implementation_handoff",
        "packet_review",
        "integration_handoff",
        "integrated_review",
    ] {
        values.extend(
            store
                .documents(run_id, kind)?
                .into_iter()
                .map(document_summary),
        );
    }
    Ok(values)
}

fn document_summary(document: DocumentView) -> DocumentSummary {
    DocumentSummary {
        id: document.id,
        kind: document.kind,
        subject_id: document.subject_id,
        ordinal: document.ordinal,
        sha256: document.sha256,
        byte_len: document.markdown.as_bytes().len(),
    }
}

fn attempt_summary(store: &Store, attempt: AttemptView) -> AppResult<AttemptSummary> {
    let review_scope = store.review_scope_for_attempt(&attempt.id)?;
    Ok(AttemptSummary {
        id: attempt.id,
        run_id: attempt.run_id,
        role: attempt.role.as_str().to_owned(),
        subject_id: attempt.subject_id,
        round: attempt.round,
        targeted: attempt.targeted,
        state: attempt.state.as_str().to_owned(),
        nucleus_job_id: attempt.nucleus_job_id,
        request_sha256: attempt.request_sha256,
        admitted: attempt.admitted,
        domain_document_id: attempt.domain_document_id,
        disposition: attempt.disposition.map(|value| value.as_str().to_owned()),
        predecessor_attempt_id: attempt.predecessor_attempt_id,
        detail: attempt.detail,
        review_scope,
    })
}

fn split_assignment<'a>(
    value: &'a str,
    label: &str,
    syntax: &str,
) -> AppResult<(&'a str, &'a str)> {
    value
        .split_once('=')
        .filter(|(name, value)| !name.is_empty() && !value.is_empty())
        .ok_or_else(|| {
            AppError::new(
                "assignment_invalid",
                format!("{label} must use {syntax} syntax"),
            )
        })
}

fn validate_identifier(value: &str, label: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(AppError::new(
            "identifier_invalid",
            format!(
                "{label} {value:?} may contain only bounded ASCII letters, digits, '.', '_', and '-'"
            ),
        ));
    }
    Ok(())
}

fn validate_remediation_rounds(value: u32) -> AppResult<()> {
    if (1..=8).contains(&value) {
        Ok(())
    } else {
        Err(AppError::new(
            "remediation_rounds_invalid",
            "remediation rounds must be between 1 and 8",
        ))
    }
}

fn require_absolute(path: &Path, label: &str) -> AppResult<()> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(AppError::new(
            "path_not_absolute",
            format!("{label} path must be absolute"),
        ))
    }
}

fn default_database() -> AppResult<PathBuf> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| AppError::new("home_unavailable", "HOME is not set; use --database"))?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("Vizier")
        .join("vizier.db"))
}

fn runtime() -> AppResult<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| AppError::new("runtime_error", error.to_string()))
}

fn emit<T: Serialize>(json: bool, value: &T, human: &str) -> AppResult<()> {
    if json {
        emit_json(value)
    } else {
        println!("{human}");
        Ok(())
    }
}

fn emit_json<T: Serialize>(value: &T) -> AppResult<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DocumentOutput, RunCommand, emit_document, run_command, split_assignment,
        validate_identifier, validate_remediation_rounds,
    };
    use crate::model::{NewRun, OpaqueMarkdown, RunState};
    use crate::store::Store;

    #[test]
    fn mechanical_assignments_preserve_command_tail() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            split_assignment("ci=FOO=a ./ci.sh", "gate", "NAME=COMMAND")?,
            ("ci", "FOO=a ./ci.sh")
        );
        Ok(())
    }

    #[test]
    fn mechanical_ids_reject_path_material() {
        assert!(validate_identifier("../outside", "contract ID").is_err());
        assert!(validate_identifier("api.v1", "contract ID").is_ok());
    }

    #[test]
    fn remediation_rounds_require_a_positive_finite_bound() {
        assert!(validate_remediation_rounds(0).is_err());
        assert!(validate_remediation_rounds(1).is_ok());
        assert!(validate_remediation_rounds(8).is_ok());
        assert!(validate_remediation_rounds(9).is_err());
    }

    #[test]
    fn document_show_keeps_raw_bytes_and_json_body_while_missing_reads_do_not_mutate()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let store = Store::new(directory.path().join("vizier.db"));
        store.initialize()?;
        let run = store.create_run(&NewRun {
            id: "run-document-cli".to_owned(),
            request_key: None,
            repository: "/tmp/repo".to_owned(),
            source_commit: "source".to_owned(),
            brief: OpaqueMarkdown::from_text("# Brief\r\n")?,
            terminology: OpaqueMarkdown::from_text("# Terms\n")?,
            contracts: vec![(
                "unit".to_owned(),
                OpaqueMarkdown::from_text("# Contract\n")?,
            )],
            gates: Vec::new(),
            remediation_limit: 1,
        })?;
        let mut documents = store.documents(&run.id, "brief")?;
        let document = documents.remove(0);
        // The raw command has no formatter or trailing newline path.
        emit_document(&document, false)?;
        let json = serde_json::to_value(DocumentOutput {
            id: document.id.clone(),
            run_id: document.run_id.clone(),
            kind: document.kind.clone(),
            subject_id: document.subject_id.clone(),
            ordinal: document.ordinal,
            sha256: document.sha256.clone(),
            markdown: document.markdown.as_str().to_owned(),
        })?;
        assert_eq!(json["markdown"], "# Brief\r\n");
        assert_eq!(
            store.document(&document.id)?.markdown.as_bytes(),
            b"# Brief\r\n"
        );
        let before = store.documents(&run.id, "brief")?.len();
        let Err(error) = store.document("document-missing") else {
            return Err("missing document unexpectedly resolved".into());
        };
        assert_eq!(error.code(), "document_not_found");
        assert_eq!(store.documents(&run.id, "brief")?.len(), before);
        Ok(())
    }

    #[test]
    fn terminal_wait_and_resume_are_dispatch_noops() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let store = Store::new(directory.path().join("vizier.db"));
        store.initialize()?;
        let run = store.create_run(&NewRun {
            id: "run-terminal-cli".to_owned(),
            request_key: None,
            repository: "/tmp/repo".to_owned(),
            source_commit: "source".to_owned(),
            brief: OpaqueMarkdown::from_text("# Brief\n")?,
            terminology: OpaqueMarkdown::from_text("# Terms\n")?,
            contracts: vec![(
                "unit".to_owned(),
                OpaqueMarkdown::from_text("# Contract\n")?,
            )],
            gates: Vec::new(),
            remediation_limit: 1,
        })?;
        store.set_run_state(&run.id, RunState::NeedsAttention, Some("durable blocker"))?;
        for command in [
            RunCommand::Wait {
                run_id: run.id.clone(),
            },
            RunCommand::Resume {
                run_id: run.id.clone(),
            },
        ] {
            run_command(&store, command, true)?;
        }
        assert_eq!(store.run(&run.id)?.state, RunState::NeedsAttention);
        assert!(store.attempts(&run.id)?.is_empty());
        Ok(())
    }
}
