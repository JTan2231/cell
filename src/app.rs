use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, TransactionBehavior};
use serde::Serialize;
use serde_json::{Value, json};

use crate::change::{
    ChangeOperation, ConceptSelector, EvidenceDisposition, EvidenceSelector, Reconciliation,
};
use crate::cli::{
    ChangeCommand, ChangeSelectArgs, ChangeShowArgs, Cli, CliGraphDirection, Command,
    ConceptCommand, ConceptPageArgs, ConceptShowArgs, GraphArgs, InboxCommand, IntegrateArgs,
    PagedAtArgs, SearchArgs, ShakeArgs, WorkAddArgs, WorkCommand,
};
use crate::config::Config;
use crate::corpus::{
    ReconciliationRecord, ShakePlan, apply_shake, diff, get_work, list_commits,
    list_reconciliations, list_works, plan_shake, reconciliation_view, recorded_change_at,
    revision, select_reconciliation, store_work, work_view,
};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::graph::{GraphReader, NeighborDirection};
use crate::model::{
    ConceptReference, ConceptSummary, DiffEntry, GraphDirection, GraphView, LibraryStats, Page,
    PageInfo,
};
use crate::model_runner::{ModelSettings, Runner};
use crate::render::{CommandOutput, render_terminal_text};
use crate::resolver::ResolvedOperation;
use crate::{inbox, liaison, resolver, validate};

pub fn library_path(explicit: Option<&PathBuf>, config: &Config) -> Result<PathBuf, AppError> {
    let environment = std::env::var_os("ANNALS_LIBRARY");
    resolve_library_path(
        explicit.map(PathBuf::as_path),
        environment.as_deref(),
        config.library.as_deref(),
    )
}

fn resolve_library_path(
    explicit: Option<&Path>,
    environment: Option<&OsStr>,
    configured: Option<&Path>,
) -> Result<PathBuf, AppError> {
    explicit
        .map(Path::to_path_buf)
        .or_else(|| {
            environment
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| configured.map(Path::to_path_buf))
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            AppError::invalid(
                "library_not_configured",
                "no library is configured; pass --library, set ANNALS_LIBRARY, or select a configuration that defines library",
            )
        })
}

pub fn run(cli: &Cli, config: &Config, path: &Path) -> AppResult<CommandOutput> {
    match &cli.command {
        Command::Init => initialize(path),
        Command::Stats => stats(path),
        Command::Overview(args) => overview(path, args.at),
        Command::Roots(args) => roots(path, args),
        Command::Concept(command) => match command {
            ConceptCommand::Show(args) => show_concept(path, args),
            ConceptCommand::Parents(args) => concept_neighbors(path, args, GraphDirection::Parents),
            ConceptCommand::Children(args) => {
                concept_neighbors(path, args, GraphDirection::Children)
            }
            ConceptCommand::Evidence(args) => concept_evidence(path, args),
        },
        Command::Graph(args) => graph(path, args),
        Command::Shake(args) => shake(path, args, cli.json),
        Command::Validate => validate_library(path),
        Command::Backup(args) => backup(path, &args.output),
        Command::Work(command) => match command {
            WorkCommand::Add(args) => add_work(path, args),
            WorkCommand::List => work_list(path),
            WorkCommand::Show(args) => show_work(path, &args.label),
        },
        Command::Integrate(args) => integrate(path, config, args, !cli.json),
        Command::Inbox(command) => match command {
            InboxCommand::Run(args) => inbox::run(path, config, args, !cli.json),
            InboxCommand::Status => inbox::status(config),
        },
        Command::Change(command) => match command {
            ChangeCommand::Submit(args) => submit_change(path, &args.input, &args.work, args.base),
            ChangeCommand::Show(args) => show_change(path, args),
            ChangeCommand::Validate(args) => validate_change(path, args),
            ChangeCommand::Apply(args) => apply_change(path, args),
            ChangeCommand::List => change_list(path),
        },
        Command::Search(args) => search(path, args),
        Command::Log(args) => log(path, args.limit),
        Command::Diff(args) => diff_revisions(path, args.from, args.to),
        Command::Revert(args) => revert(path, args.revision),
    }
}

fn initialize(path: &Path) -> Result<CommandOutput, AppError> {
    db::init(path)?;
    Ok(CommandOutput::new(
        json!({ "library": path.display().to_string(), "revision": 0 }),
        format!(
            "Initialized Annals library {} at revision 0",
            path.display()
        ),
    )
    .mutation())
}

fn stats(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let value = LibraryStats {
        revision: revision(&connection)?,
        concept_count: count(&connection, "concepts")?,
        edge_count: count(&connection, "concept_edges")?,
        work_count: count(&connection, "works")?,
        evidence_count: count(&connection, "evidence")?,
        pending_reconciliation_count: count_where(
            &connection,
            "reconciliations",
            "status = 'pending'",
        )?,
        commit_count: count(&connection, "commits")?,
        model_run_count: count(&connection, "model_runs")?,
        database_size_bytes: fs::metadata(path).map(|metadata| metadata.len()).map_err(
            |error| {
                AppError::unexpected(
                    "database_metadata_failed",
                    format!("unable to inspect {}: {error}", path.display()),
                )
            },
        )?,
    };
    let human = format!(
        "Revision: {}\nConcepts: {}\nParent edges: {}\nWorks: {}\nEvidence links: {}\nPending reconciliations: {}\nCommits: {}\nModel runs: {}\nDatabase size: {} bytes",
        value.revision,
        value.concept_count,
        value.edge_count,
        value.work_count,
        value.evidence_count,
        value.pending_reconciliation_count,
        value.commit_count,
        value.model_run_count,
        value.database_size_bytes
    );
    Ok(CommandOutput::new(to_value(&value)?, human))
}

fn validate_library(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let report = validate::validate(&connection)?;
    if !report.valid {
        let messages = report
            .issues
            .iter()
            .map(|issue| format!("error [{}]: {}", issue.code, issue.message))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(AppError::database(
            "validation_failed",
            format!("Library is invalid\n{messages}"),
        ));
    }
    Ok(CommandOutput::new(to_value(&report)?, "Library is valid"))
}

fn backup(path: &Path, output: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    db::backup(&connection, output)?;
    Ok(CommandOutput::new(
        json!({ "output": output.display().to_string() }),
        format!("Backed up {} to {}", path.display(), output.display()),
    )
    .mutation())
}

fn add_work(path: &Path, args: &WorkAddArgs) -> Result<CommandOutput, AppError> {
    let text = read_utf8(&args.input, "work")?;
    let label = work_label(&args.input, args.name.as_deref())?;
    let mut connection = db::open_write(path)?;
    let work = store_work(&mut connection, &label, &text)?;
    let corpus_revision = revision(&connection)?;
    Ok(CommandOutput::new(
        json!({
            "work": work.label,
            "size_bytes": work.text.len(),
            "sha256": work.sha256,
            "created_at": work.created_at,
            "corpus_revision": corpus_revision
        }),
        format!(
            "Retained work {:?} ({} bytes)\nCorpus remains at revision {corpus_revision}",
            work.label,
            work.text.len()
        ),
    )
    .mutation())
}

fn work_list(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let works = list_works(&connection)?;
    let human = if works.is_empty() {
        "No retained works".to_owned()
    } else {
        works
            .iter()
            .map(|work| {
                format!(
                    "{}\t{} bytes",
                    render_terminal_text(&work.work, false),
                    work.size_bytes
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(to_value(&works)?, human))
}

fn show_work(path: &Path, label: &str) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let work = get_work(&connection, label)?;
    let view = work_view(&work);
    let data = json!({
        "work": view.summary.work,
        "size_bytes": view.summary.size_bytes,
        "sha256": view.summary.sha256,
        "created_at": view.summary.created_at,
        "headings": view.headings,
        "text": work.text
    });
    let human = format!(
        "Work: {}\nSize: {} bytes\nSHA-256: {}\nCreated: {}\n\n{}",
        render_terminal_text(&work.label, false),
        work.text.len(),
        work.sha256,
        work.created_at,
        render_terminal_text(&work.text, true)
    );
    Ok(CommandOutput::new(data, human))
}

fn integrate(
    path: &Path,
    config: &Config,
    args: &IntegrateArgs,
    forward_progress: bool,
) -> Result<CommandOutput, AppError> {
    let work = if let Some(label) = &args.work {
        let connection = db::open_read(path)?;
        get_work(&connection, label)?
    } else {
        let input = args.input.as_ref().ok_or_else(|| {
            AppError::invalid("invalid_command", "integrate requires an input or --work")
        })?;
        let text = read_utf8(input, "work")?;
        let label = work_label(input, args.name.as_deref())?;
        let mut connection = db::open_write(path)?;
        store_work(&mut connection, &label, &text)?
    };
    let quality = args.quality.unwrap_or(config.liaison.quality);
    let model = args.model.as_deref().or(config.liaison.model.as_deref());
    let settings = ModelSettings::new(quality, model);
    let runner = Runner::for_program(&config.liaison.codex);
    let record = liaison::integrate_with_runner(
        path,
        &work,
        &settings,
        forward_progress,
        args.reexamine,
        &runner,
    )?;
    if args.apply {
        match record.status.as_str() {
            "pending" => {
                let mut connection = db::open_write(path)?;
                let applied = resolver::apply_record(&mut connection, &record)?;
                return applied_output(&record, applied);
            }
            "recorded" => {}
            _ => {
                return Err(AppError::conflict(
                    "nothing_to_apply",
                    "the reusable reconciliation is not pending; use --reexamine for a fresh examination",
                ));
            }
        }
    }
    reconciliation_output(&record)
}

fn submit_change(
    path: &Path,
    input: &Path,
    work_label: &str,
    base: i64,
) -> Result<CommandOutput, AppError> {
    let document = read_utf8(input, "change request")?;
    let mut connection = db::open_write(path)?;
    let work = get_work(&connection, work_label)?;
    let record = resolver::submit_document(&mut connection, &work, base, &document, "human", None)?;
    reconciliation_output(&record)
}

fn show_change(path: &Path, args: &ChangeShowArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    if let Some(requested_revision) = args.at {
        let change = recorded_change_at(&connection, requested_revision)?;
        let human = render_recorded_change(&change)?;
        return Ok(CommandOutput::new(to_value(&change)?, human));
    }
    let record = select_reconciliation(&connection, args.work.as_deref(), false)?;
    let mut output = reconciliation_output(&record)?;
    output.quietable = false;
    Ok(output)
}

fn render_recorded_change(change: &crate::model::RecordedChangeView) -> Result<String, AppError> {
    let work = change
        .work
        .as_deref()
        .map_or_else(|| "none".to_owned(), render_quoted);
    let details = match change.kind.as_str() {
        "change" => render_recorded_reconciliation(change)?,
        "revert" => render_recorded_revert(change)?,
        "shake" => render_recorded_shake(),
        _ => {
            return Err(AppError::database(
                "invalid_commit_kind",
                format!("revision {} has an unknown change kind", change.revision),
            ));
        }
    };
    let effects = render_commit_effects(&change.effects);
    Ok(format!(
        "Applied {} at revision {}\nWork: {}\nSummary: {}\n{}\n{}\nActor: {}\nRecorded: {}",
        change.kind,
        change.revision,
        work,
        render_terminal_text(&change.summary, false),
        details,
        effects,
        render_terminal_text(&change.actor, false),
        render_terminal_text(&change.created_at, false),
    ))
}

fn render_recorded_shake() -> String {
    "Submitted request: transitively reduce the concept graph".to_owned()
}

fn render_recorded_reconciliation(
    change: &crate::model::RecordedChangeView,
) -> Result<String, AppError> {
    let reconciliation: Reconciliation = serde_json::from_value(change.submitted_request.clone())
        .map_err(|error| {
        AppError::database(
            "invalid_commit_request",
            format!(
                "revision {} has an invalid reconciliation: {error}",
                change.revision
            ),
        )
    })?;
    let resolved: Vec<ResolvedOperation> =
        serde_json::from_value(change.resolved_operations.clone()).map_err(|error| {
            AppError::database(
                "invalid_commit_operations",
                format!(
                    "revision {} has invalid resolved operations: {error}",
                    change.revision
                ),
            )
        })?;
    Ok(format!(
        "Submitted operations ({}):\n{}\nResolved operations ({}):\n{}\n{}",
        reconciliation.operations().len(),
        render_requested_operations(reconciliation.operations()),
        resolved.len(),
        render_resolved_operations(&resolved, reconciliation.operations()),
        render_annotations(reconciliation.annotations()),
    ))
}

fn render_recorded_revert(change: &crate::model::RecordedChangeView) -> Result<String, AppError> {
    let target = change
        .submitted_request
        .get("revert_revision")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            AppError::database(
                "invalid_commit_request",
                format!("revision {} has an invalid revert request", change.revision),
            )
        })?;
    Ok(format!("Submitted request: revert revision {target}"))
}

fn render_commit_effects(effects: &[DiffEntry]) -> String {
    let rendered = effects
        .iter()
        .map(|entry| format!("  {}", render_diff_entry(entry)))
        .collect::<Vec<_>>()
        .join("\n");
    format!("Material effects ({}):\n{rendered}", effects.len())
}

fn validate_change(path: &Path, args: &ChangeSelectArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let record = select_reconciliation(&connection, args.work.as_deref(), true)?;
    let resolved = resolver::validate_record(&connection, &record)?;
    let reconciliation: Reconciliation = serde_json::from_str(&record.submitted_request)?;
    let human = format!(
        "Valid pending reconciliation for {}\nBase revision: {}\nSummary: {}\nResolved operations ({}):\n{}\n{}\nApplication: ready",
        render_quoted(&record.work_label),
        record.base_revision,
        render_terminal_text(&record.summary, false),
        resolved.operations.len(),
        render_resolved_operations(&resolved.operations, reconciliation.operations()),
        render_annotations(reconciliation.annotations()),
    );
    Ok(CommandOutput::new(
        json!({
            "work": record.work_label,
            "base_revision": record.base_revision,
            "status": "valid",
            "summary": record.summary,
            "operations": resolved.operations
        }),
        human,
    ))
}

fn apply_change(path: &Path, args: &ChangeSelectArgs) -> Result<CommandOutput, AppError> {
    let mut connection = db::open_write(path)?;
    let record = select_reconciliation(&connection, args.work.as_deref(), true)?;
    let applied = resolver::apply_record(&mut connection, &record)?;
    applied_output(&record, applied)
}

fn change_list(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let reconciliations = list_reconciliations(&connection)?;
    let human = if reconciliations.is_empty() {
        "No recorded reconciliations".to_owned()
    } else {
        reconciliations
            .iter()
            .map(|reconciliation| {
                format!(
                    "{}\t{}\t{}\t{}",
                    reconciliation.status,
                    render_terminal_text(&reconciliation.work, false),
                    reconciliation.base_revision,
                    render_terminal_text(&reconciliation.summary, false)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(to_value(&reconciliations)?, human))
}

fn reconciliation_output(record: &ReconciliationRecord) -> Result<CommandOutput, AppError> {
    let view = reconciliation_view(record)?;
    let reconciliation: Reconciliation = serde_json::from_value(view.request.clone())?;
    let operation_count = view.request["operations"].as_array().map_or(0, Vec::len);
    let data = json!({
        "work": view.work,
        "base_revision": view.base_revision,
        "status": view.status,
        "summary": view.summary,
        "operation_count": operation_count,
        "annotations": view.annotations,
        "reconciliation": view.request,
        "created_at": view.created_at,
        "applied_revision": view.applied_revision
    });
    let heading = match view.status.as_str() {
        "applied" => "Applied reconciliation",
        "superseded" => "Superseded reconciliation",
        "recorded" => "Reconciliation recorded",
        _ => "Pending reconciliation",
    };
    let status = match view.status.as_str() {
        "applied" => view.applied_revision.map_or_else(
            || "applied".to_owned(),
            |revision| format!("applied at revision {revision}"),
        ),
        "superseded" => "superseded".to_owned(),
        "recorded" => "recorded".to_owned(),
        _ => "valid".to_owned(),
    };
    let corpus_state = if view.status == "recorded" {
        format!("\nCorpus remained at revision {}", view.base_revision)
    } else {
        String::new()
    };
    let human = format!(
        "{heading} for {}\nBase revision: {}\nSummary: {}\nOperations ({}):\n{}\n{}\nStatus: {status}{corpus_state}",
        render_quoted(&view.work),
        view.base_revision,
        render_terminal_text(&view.summary, false),
        reconciliation.operations().len(),
        render_requested_operations(reconciliation.operations()),
        render_annotations(reconciliation.annotations()),
    );
    Ok(CommandOutput::new(data, human).mutation())
}

fn render_requested_operations(operations: &[ChangeOperation]) -> String {
    operations
        .iter()
        .enumerate()
        .flat_map(|(index, operation)| render_requested_operation(index, operation))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_requested_operation(index: usize, operation: &ChangeOperation) -> Vec<String> {
    let number = index + 1;
    match operation {
        ChangeOperation::CreateConcept {
            handle,
            label,
            parents,
            evidence,
        } => {
            let mut lines = vec![format!(
                "  {number}. Create concept {} as ref {}",
                render_quoted(label),
                render_terminal_text(handle, false)
            )];
            lines.push(format!("     Parents: {}", render_selectors(parents)));
            append_evidence_selectors(&mut lines, evidence);
            lines
        }
        ChangeOperation::AddParent { concept, parent } => vec![format!(
            "  {number}. Add parent {} to {}",
            render_selector(parent),
            render_selector(concept)
        )],
        ChangeOperation::RemoveParent { concept, parent } => vec![format!(
            "  {number}. Remove parent {} from {}",
            render_selector(parent),
            render_selector(concept)
        )],
        ChangeOperation::AddEvidence { concept, evidence } => {
            let mut lines = vec![format!(
                "  {number}. Add evidence to {}",
                render_selector(concept)
            )];
            append_evidence_selectors(&mut lines, evidence);
            lines
        }
        ChangeOperation::RemoveEvidence { concept, evidence } => {
            let mut lines = vec![format!(
                "  {number}. Remove evidence from {}",
                render_selector(concept)
            )];
            append_evidence_selectors(&mut lines, evidence);
            lines
        }
        ChangeOperation::RewordConcept {
            concept,
            label,
            evidence_disposition,
        } => vec![
            format!(
                "  {number}. Reword {} to {}",
                render_selector(concept),
                render_quoted(label)
            ),
            format!(
                "     Evidence disposition: {}",
                render_evidence_disposition(*evidence_disposition)
            ),
        ],
        ChangeOperation::RetireConcept {
            concept,
            replacement,
        } => vec![
            format!("  {number}. Retire {}", render_selector(concept)),
            format!(
                "     Replacement: {}",
                replacement
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), render_selector,)
            ),
        ],
    }
}

fn render_resolved_operations(
    operations: &[ResolvedOperation],
    requested: &[ChangeOperation],
) -> String {
    operations
        .iter()
        .enumerate()
        .flat_map(|(index, operation)| {
            render_resolved_operation(index, operation, requested.get(index))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_resolved_operation(
    index: usize,
    operation: &ResolvedOperation,
    _requested: Option<&ChangeOperation>,
) -> Vec<String> {
    let number = index + 1;
    match operation {
        ResolvedOperation::CreateConcept {
            concept,
            parents,
            evidence_quotes,
        } => {
            let mut lines = vec![format!(
                "  {number}. Create concept {}",
                render_reference(concept)
            )];
            lines.push(format!("     Parents: {}", render_references(parents)));
            append_resolved_quotes(&mut lines, evidence_quotes);
            lines
        }
        ResolvedOperation::AddParent { concept, parent } => vec![format!(
            "  {number}. Add parent {} to {}",
            render_reference(parent),
            render_reference(concept)
        )],
        ResolvedOperation::RemoveParent { concept, parent } => vec![format!(
            "  {number}. Remove parent {} from {}",
            render_reference(parent),
            render_reference(concept)
        )],
        ResolvedOperation::AddEvidence { concept, quotes } => {
            let mut lines = vec![format!(
                "  {number}. Add evidence to {}",
                render_reference(concept)
            )];
            append_resolved_quotes(&mut lines, quotes);
            lines
        }
        ResolvedOperation::RemoveEvidence { concept, quotes } => {
            let mut lines = vec![format!(
                "  {number}. Remove evidence from {}",
                render_reference(concept)
            )];
            append_resolved_quotes(&mut lines, quotes);
            lines
        }
        ResolvedOperation::RewordConcept {
            id,
            before,
            after,
            evidence_disposition,
        } => vec![
            format!(
                "  {number}. Reword {id}: {} -> {}",
                render_quoted(before),
                render_quoted(after)
            ),
            format!(
                "     Evidence disposition: {}",
                render_evidence_disposition(*evidence_disposition)
            ),
        ],
        ResolvedOperation::RetireConcept {
            concept,
            replacement,
            removed_parents,
            removed_children,
        } => vec![
            format!("  {number}. Retire {}", render_reference(concept)),
            format!(
                "     Replacement: {}",
                replacement
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), render_reference)
            ),
            format!(
                "     Removed parents: {}",
                render_references(removed_parents)
            ),
            format!(
                "     Removed children: {}",
                render_references(removed_children)
            ),
        ],
    }
}

fn render_selectors(selectors: &[ConceptSelector]) -> String {
    if selectors.is_empty() {
        return "none (root)".to_owned();
    }
    selectors
        .iter()
        .map(render_selector)
        .collect::<Vec<_>>()
        .join(", ")
}

fn append_evidence_selectors(lines: &mut Vec<String>, evidence: &[EvidenceSelector]) {
    for selector in evidence {
        lines.push(format!("     Evidence: {}", render_quoted(&selector.quote)));
        if let Some(path) = &selector.within_heading {
            lines.push(format!("       Within heading: {}", render_path(path)));
        }
        for (label, value) in [
            ("Preceded by", selector.preceded_by.as_deref()),
            ("Followed by", selector.followed_by.as_deref()),
        ] {
            if let Some(value) = value {
                lines.push(format!("       {label}: {}", render_quoted(value)));
            }
        }
    }
}

fn append_resolved_quotes(lines: &mut Vec<String>, quotes: &[String]) {
    for quote in quotes {
        lines.push(format!("     Evidence: {}", render_quoted(quote)));
    }
}

fn render_selector(selector: &ConceptSelector) -> String {
    match selector {
        ConceptSelector::Existing { id } => id.to_string(),
        ConceptSelector::New { handle } => {
            format!("new ref {}", render_quoted(handle))
        }
    }
}

fn render_reference(reference: &ConceptReference) -> String {
    format!("{} ({})", render_quoted(&reference.label), reference.id)
}

fn render_references(references: &[ConceptReference]) -> String {
    if references.is_empty() {
        "none".to_owned()
    } else {
        references
            .iter()
            .map(render_reference)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn render_path(path: &[String]) -> String {
    if path.is_empty() {
        "root".to_owned()
    } else {
        path.iter()
            .map(|segment| render_quoted(segment))
            .collect::<Vec<_>>()
            .join(" › ")
    }
}

fn render_quoted(text: &str) -> String {
    format!("“{}”", render_terminal_text(text, false))
}

fn render_evidence_disposition(disposition: EvidenceDisposition) -> &'static str {
    match disposition {
        EvidenceDisposition::Retain => "retain existing evidence",
        EvidenceDisposition::Remove => "remove existing evidence",
    }
}

fn render_annotations(annotations: &[String]) -> String {
    if annotations.is_empty() {
        "Annotations: none".to_owned()
    } else {
        format!(
            "Annotations:\n{}",
            annotations
                .iter()
                .map(|annotation| { format!("  - {}", render_terminal_text(annotation, false)) })
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

fn applied_output(record: &ReconciliationRecord, applied: i64) -> Result<CommandOutput, AppError> {
    let request: Reconciliation = serde_json::from_str(&record.submitted_request)?;
    let reconciliation: Value = serde_json::from_str(&record.submitted_request)?;
    Ok(CommandOutput::new(
        json!({
            "work": record.work_label,
            "base_revision": record.base_revision,
            "revision": applied,
            "status": "applied",
            "summary": record.summary,
            "annotations": request.annotations(),
            "reconciliation": reconciliation
        }),
        format!(
            "Applied reconciliation at revision {applied}:\n{}\n{}",
            render_terminal_text(&record.summary, false),
            render_annotations(request.annotations())
        ),
    )
    .mutation())
}

fn overview(path: &Path, requested: Option<i64>) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let reader = GraphReader::new(&connection);
    let graph = requested.map_or_else(|| reader.head(), |revision| reader.at(revision))?;
    let value = graph.overview()?;
    let human = format!(
        "Corpus revision {}\n{} concepts · {} scope edges\n{} roots · {} leaves · {} shared concepts\n{} evidence links",
        value.revision,
        value.concept_count,
        value.edge_count,
        value.root_count,
        value.leaf_count,
        value.shared_concept_count,
        value.evidence_count
    );
    Ok(CommandOutput::new(to_value(&value)?, human))
}

fn roots(path: &Path, args: &PagedAtArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let graph = GraphReader::new(&connection).paged_at(args.at, args.cursor.as_deref())?;
    let requested = graph.revision();
    let roots = graph.roots_page(cli_page_limit(args.limit)?, args.cursor.as_deref())?;
    let human = render_summary_page("Roots", &roots, requested);
    Ok(CommandOutput::new(
        json!({ "revision": requested, "roots": roots }),
        human,
    ))
}

fn show_concept(path: &Path, args: &ConceptShowArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let reader = GraphReader::new(&connection);
    let graph = args
        .at
        .map_or_else(|| reader.head(), |revision| reader.at(revision))?;
    let requested = graph.revision();
    let detail = graph.concept_detail(args.id, args.preview_limit)?;
    let evidence_preview = if detail.evidence.items.is_empty() {
        "  None".to_owned()
    } else {
        detail
            .evidence
            .items
            .iter()
            .map(|item| {
                format!(
                    "  {} — {}",
                    render_quoted(&item.work),
                    render_quoted(&item.quote)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let human = format!(
        "{} ({}) @ r{}\n{} parents · {} children · {} evidence links\n\nParents preview ({} of {})\n{}\n\nChildren preview ({} of {})\n{}\n\nEvidence preview ({} of {})\n{}",
        render_terminal_text(&detail.summary.label, false),
        detail.summary.id,
        requested,
        detail.summary.parent_count,
        detail.summary.child_count,
        detail.summary.evidence_count,
        detail.parents.page.returned,
        detail.parents.page.total,
        render_reference_page(&detail.parents),
        detail.children.page.returned,
        detail.children.page.total,
        render_reference_page(&detail.children),
        detail.evidence.page.returned,
        detail.evidence.page.total,
        evidence_preview
    );
    Ok(CommandOutput::new(
        json!({ "revision": requested, "concept": detail }),
        human,
    ))
}

fn concept_neighbors(
    path: &Path,
    args: &ConceptPageArgs,
    direction: GraphDirection,
) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let graph =
        GraphReader::new(&connection).paged_at(args.page.at, args.page.cursor.as_deref())?;
    let requested = graph.revision();
    let neighbor_direction = match direction {
        GraphDirection::Parents => NeighborDirection::Parents,
        GraphDirection::Children => NeighborDirection::Children,
        GraphDirection::Both => {
            return Err(AppError::invalid(
                "invalid_direction",
                "concept neighbors must be parents or children",
            ));
        }
    };
    let reference = graph.reference(args.id)?;
    let page = graph.neighbor_page(
        args.id,
        neighbor_direction,
        cli_page_limit(args.page.limit)?,
        args.page.cursor.as_deref(),
    )?;
    let heading = match direction {
        GraphDirection::Parents => "Parents",
        GraphDirection::Children => "Children",
        GraphDirection::Both => {
            return Err(AppError::invalid(
                "invalid_direction",
                "concept neighbors must be parents or children",
            ));
        }
    };
    let human = render_reference_page_heading(heading, &reference, &page, requested);
    let data = match direction {
        GraphDirection::Parents => {
            json!({ "revision": requested, "concept": reference, "parents": page })
        }
        GraphDirection::Children => {
            json!({ "revision": requested, "concept": reference, "children": page })
        }
        GraphDirection::Both => unreachable!("both was rejected above"),
    };
    Ok(CommandOutput::new(data, human))
}

fn concept_evidence(path: &Path, args: &ConceptPageArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let graph =
        GraphReader::new(&connection).paged_at(args.page.at, args.page.cursor.as_deref())?;
    let requested = graph.revision();
    let reference = graph.reference(args.id)?;
    let evidence = graph.evidence_page(
        args.id,
        cli_page_limit(args.page.limit)?,
        args.page.cursor.as_deref(),
    )?;
    let body = if evidence.items.is_empty() {
        "  None".to_owned()
    } else {
        evidence
            .items
            .iter()
            .map(|item| {
                format!(
                    "  {} — {}",
                    render_quoted(&item.work),
                    render_quoted(&item.quote)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let human = format!(
        "Evidence for {} at revision {requested} — showing {} of {}\n{body}{}",
        render_reference(&reference),
        evidence.page.returned,
        evidence.page.total,
        render_continuation(&evidence.page, requested)
    );
    Ok(CommandOutput::new(
        json!({ "revision": requested, "concept": reference, "evidence": evidence }),
        human,
    ))
}

fn graph(path: &Path, args: &GraphArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let reader = GraphReader::new(&connection);
    let graph = args
        .at
        .map_or_else(|| reader.head(), |revision| reader.at(revision))?;
    let direction = match args.direction {
        CliGraphDirection::Parents => GraphDirection::Parents,
        CliGraphDirection::Children => GraphDirection::Children,
        CliGraphDirection::Both => GraphDirection::Both,
    };
    let output = graph.graph_view(args.id, direction, args.depth, args.max_nodes)?;
    let human = render_graph(&output);
    Ok(CommandOutput::new(to_value(&output)?, human))
}

fn search(path: &Path, args: &SearchArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let graph = GraphReader::new(&connection).paged_at(args.at, args.cursor.as_deref())?;
    let requested = graph.revision();
    let output = graph.search(
        &args.query,
        args.within,
        cli_page_limit(args.limit)?,
        args.cursor.as_deref(),
    )?;
    let body = if output.results.items.is_empty() {
        "  None".to_owned()
    } else {
        output
            .results
            .items
            .iter()
            .enumerate()
            .map(|(index, result)| {
                format!(
                    "  {}. {} ({}) — {} parent(s), {} child(ren), {} evidence link(s){}",
                    index + 1,
                    render_terminal_text(&result.concept.label, false),
                    result.concept.id,
                    result.concept.parent_count,
                    result.concept.child_count,
                    result.concept.evidence_count,
                    if result.concept.shared {
                        " [shared]"
                    } else {
                        ""
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let human = format!(
        "Search at revision {requested} — showing {} of {}\n{body}{}",
        output.results.page.returned,
        output.results.page.total,
        render_continuation(&output.results.page, requested)
    );
    Ok(CommandOutput::new(to_value(&output)?, human))
}

fn cli_page_limit(limit: usize) -> Result<usize, AppError> {
    if (1..=200).contains(&limit) {
        Ok(limit)
    } else {
        Err(AppError::invalid(
            "invalid_limit",
            "a CLI page limit must be between 1 and 200",
        ))
    }
}

fn render_summary_page(heading: &str, page: &Page<ConceptSummary>, revision: i64) -> String {
    let body = if page.items.is_empty() {
        "  None".to_owned()
    } else {
        page.items
            .iter()
            .map(|item| {
                format!(
                    "  {} ({}){}",
                    render_terminal_text(&item.label, false),
                    item.id,
                    if item.shared {
                        format!(" [shared: {} parents]", item.parent_count)
                    } else {
                        String::new()
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "{heading} at revision {revision} — showing {} of {}\n{body}{}",
        page.page.returned,
        page.page.total,
        render_continuation(&page.page, revision)
    )
}

fn render_reference_page(page: &Page<ConceptReference>) -> String {
    if page.items.is_empty() {
        "  None".to_owned()
    } else {
        page.items
            .iter()
            .map(|item| format!("  {}", render_reference(item)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn render_reference_page_heading(
    heading: &str,
    concept: &ConceptReference,
    page: &Page<ConceptReference>,
    revision: i64,
) -> String {
    format!(
        "{heading} of {} at revision {revision} — showing {} of {}\n{}{}",
        render_reference(concept),
        page.page.returned,
        page.page.total,
        render_reference_page(page),
        render_continuation(&page.page, revision)
    )
}

fn render_continuation(page: &PageInfo, revision: i64) -> String {
    page.next_cursor
        .as_ref()
        .map_or_else(String::new, |cursor| {
            format!("\nMore at revision {revision}: rerun with --at {revision} --cursor {cursor}")
        })
}

fn render_graph(graph: &GraphView) -> String {
    let nodes = graph
        .nodes
        .iter()
        .map(|node| {
            format!(
                "  {} — {} — distance {}",
                node.summary.id,
                render_quoted(&node.summary.label),
                node.distance
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let edges = if graph.edges.is_empty() {
        "  None".to_owned()
    } else {
        graph
            .edges
            .iter()
            .map(|edge| format!("  {} -> {}", edge.parent_id, edge.child_id))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let frontier = if graph.frontier.is_empty() {
        "  None".to_owned()
    } else {
        graph
            .frontier
            .iter()
            .map(|entry| {
                format!(
                    "  {} — {} unreturned parent(s), {} unreturned child(ren)",
                    entry.id, entry.unreturned_parent_count, entry.unreturned_child_count
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let coverage = if graph.node_limit_reached {
        format!(
            "Node limit reached at {}; expand again from a frontier concept",
            graph.max_nodes
        )
    } else {
        format!("Complete through requested depth {}", graph.depth)
    };
    let direction = match graph.direction {
        GraphDirection::Parents => "parents",
        GraphDirection::Children => "children",
        GraphDirection::Both => "both",
    };
    format!(
        "Graph around {} at revision {}\nDirection: {} · depth: {} · max nodes: {}\n\nNodes ({})\n{}\n\nEdges ({})\n{edges}\n\nFrontier\n{frontier}\n\n{coverage}",
        graph.seed,
        graph.revision,
        direction,
        graph.depth,
        graph.max_nodes,
        graph.nodes.len(),
        nodes,
        graph.edges.len()
    )
}

fn shake(path: &Path, args: &ShakeArgs, json_output: bool) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let plan = plan_shake(&connection)?;
    drop(connection);
    let report = render_shake_plan(&plan);
    if plan.removed_edges.is_empty() {
        return Ok(CommandOutput::new(
            shake_data(&plan, "unchanged", plan.base_revision),
            format!(
                "{report}\n\nThe graph is already transitively reduced; corpus remains at revision {}",
                plan.base_revision
            ),
        ));
    }
    if json_output && !args.yes {
        return Ok(CommandOutput::new(
            shake_data(&plan, "confirmation_required", plan.base_revision),
            "",
        ));
    }
    if !args.yes {
        println!("{report}\n");
        io::stdout().flush().map_err(|error| {
            AppError::unexpected(
                "report_write_failed",
                format!("unable to write shake report: {error}"),
            )
        })?;
        eprint!(
            "Remove {} transitively implied parent {}? [y/N] ",
            plan.removed_edges.len(),
            if plan.removed_edges.len() == 1 {
                "edge"
            } else {
                "edges"
            }
        );
        io::stderr().flush().map_err(|error| {
            AppError::unexpected(
                "confirmation_write_failed",
                format!("unable to write shake confirmation: {error}"),
            )
        })?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer).map_err(|error| {
            AppError::unexpected(
                "confirmation_read_failed",
                format!("unable to read shake confirmation: {error}"),
            )
        })?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return Ok(CommandOutput::new(
                shake_data(&plan, "cancelled", plan.base_revision),
                format!(
                    "Shake cancelled; corpus remains at revision {}",
                    plan.base_revision
                ),
            ));
        }
    }
    let mut connection = db::open_write(path)?;
    let new_revision = apply_shake(&mut connection, &plan)?;
    let human = if args.yes {
        format!("{report}\n\nApplied shake as revision {new_revision}")
    } else {
        format!("Applied shake as revision {new_revision}")
    };
    Ok(CommandOutput::new(shake_data(&plan, "applied", new_revision), human).mutation())
}

fn render_shake_plan(plan: &ShakePlan) -> String {
    let edges = plan
        .removed_edges
        .iter()
        .map(|edge| {
            format!(
                "  {} → {}",
                render_reference(&edge.parent),
                render_reference(&edge.child)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let edge_list = if edges.is_empty() {
        String::new()
    } else {
        format!("\n\nEdges to remove:\n{edges}")
    };
    format!(
        "Shake revision {}\n\n{} of {} parent edges will be removed.\n{} parent edges will remain.\nConcepts, evidence, and ancestor/descendant reachability will remain unchanged.{edge_list}",
        plan.base_revision,
        plan.removed_edges.len(),
        plan.edge_count_before,
        plan.edge_count_after
    )
}

fn shake_data(plan: &ShakePlan, status: &str, revision: i64) -> Value {
    json!({
        "status": status,
        "base_revision": plan.base_revision,
        "revision": revision,
        "edge_count_before": plan.edge_count_before,
        "removed_edge_count": plan.removed_edges.len(),
        "edge_count_after": plan.edge_count_after,
        "removed_edges": plan.removed_edges
    })
}

fn log(path: &Path, limit: usize) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let commits = list_commits(&connection, limit)?;
    let head = revision(&connection)?;
    let human = if commits.is_empty() {
        "No corpus commits".to_owned()
    } else {
        commits
            .iter()
            .map(|commit| {
                format!(
                    "r{}\t{}\t{}",
                    commit.revision,
                    commit.kind,
                    render_terminal_text(&commit.summary, false)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(
        json!({ "head_revision": head, "commits": commits }),
        human,
    ))
}

fn diff_revisions(path: &Path, from: i64, to: i64) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let value = diff(&connection, from, to)?;
    let human = if value.entries.is_empty() {
        format!("No corpus difference between revisions {from} and {to}")
    } else {
        value
            .entries
            .iter()
            .map(render_diff_entry)
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(to_value(&value)?, human))
}

fn render_diff_entry(entry: &DiffEntry) -> String {
    match entry {
        DiffEntry::Created { concept } => {
            format!("Created: {}", render_reference(concept))
        }
        DiffEntry::Retired { concept } => {
            format!("Retired: {}", render_reference(concept))
        }
        DiffEntry::Reworded { id, before, after } => format!(
            "Reworded {id}: {} → {}",
            render_quoted(before),
            render_quoted(after)
        ),
        DiffEntry::ParentAdded { parent, child } => format!(
            "Parent added: {} → {}",
            render_reference(parent),
            render_reference(child)
        ),
        DiffEntry::ParentRemoved { parent, child } => format!(
            "Parent removed: {} → {}",
            render_reference(parent),
            render_reference(child)
        ),
        DiffEntry::EvidenceAdded {
            concept,
            work,
            quote,
        } => format!(
            "Evidence added: {} — {} — {}",
            render_reference(concept),
            render_quoted(work),
            render_quoted(quote)
        ),
        DiffEntry::EvidenceRemoved {
            concept,
            work,
            quote,
        } => format!(
            "Evidence removed: {} — {} — {}",
            render_reference(concept),
            render_quoted(work),
            render_quoted(quote)
        ),
    }
}

fn revert(path: &Path, target: i64) -> Result<CommandOutput, AppError> {
    let mut connection = db::open_write(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let new_revision = crate::corpus::revert(&transaction, target)?;
    transaction.commit()?;
    Ok(CommandOutput::new(
        json!({
            "revision": new_revision,
            "reverted_revision": target,
            "summary": format!("Revert revision {target}")
        }),
        format!("Applied revision {new_revision}:\nRevert revision {target}"),
    )
    .mutation())
}

pub(crate) fn read_utf8(path: &Path, description: &str) -> Result<String, AppError> {
    let bytes = if path == Path::new("-") {
        let mut bytes = Vec::new();
        io::stdin().read_to_end(&mut bytes).map_err(|error| {
            AppError::unexpected(
                "input_read_failed",
                format!("unable to read {description} from standard input: {error}"),
            )
        })?;
        bytes
    } else {
        fs::read(path).map_err(|error| {
            AppError::unexpected(
                "input_read_failed",
                format!("unable to read {description} {}: {error}", path.display()),
            )
        })?
    };
    String::from_utf8(bytes).map_err(|_| {
        AppError::invalid(
            "input_not_utf8",
            format!("{description} must be valid UTF-8"),
        )
    })
}

pub(crate) fn work_label(path: &Path, explicit: Option<&str>) -> Result<String, AppError> {
    if let Some(label) = explicit {
        return Ok(label.to_owned());
    }
    if path == Path::new("-") {
        return Err(AppError::invalid(
            "work_name_required",
            "a work read from standard input requires --name",
        ));
    }
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            AppError::invalid(
                "work_name_required",
                "the work path has no usable UTF-8 filename; supply --name",
            )
        })
}

fn count(connection: &Connection, table: &str) -> Result<u64, AppError> {
    count_where(connection, table, "1")
}

fn count_where(connection: &Connection, table: &str, condition: &str) -> Result<u64, AppError> {
    let count = connection.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE {condition}"),
        [],
        |row| row.get::<_, i64>(0),
    )?;
    u64::try_from(count)
        .map_err(|_| AppError::database("invalid_count", "database returned a negative count"))
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, AppError> {
    serde_json::to_value(value).map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::resolve_library_path;

    #[test]
    fn library_path_honors_precedence_and_ignores_an_empty_environment()
    -> Result<(), Box<dyn std::error::Error>> {
        let explicit = Path::new("explicit.db");
        let configured = Path::new("configured.db");

        assert_eq!(
            resolve_library_path(
                Some(explicit),
                Some(OsStr::new("environment.db")),
                Some(configured),
            )?,
            explicit
        );
        assert_eq!(
            resolve_library_path(None, Some(OsStr::new("environment.db")), Some(configured))?,
            Path::new("environment.db")
        );
        assert_eq!(
            resolve_library_path(None, Some(OsStr::new("")), Some(configured))?,
            configured
        );

        let error = resolve_library_path(None, Some(OsStr::new("")), None)
            .err()
            .ok_or("library resolution unexpectedly succeeded")?;
        assert_eq!(error.code(), "library_not_configured");
        Ok(())
    }
}
