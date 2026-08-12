use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, TransactionBehavior};
use serde::Serialize;
use serde_json::{Value, json};

use crate::change::{
    ChangeOperation, ChangeProposal, ConceptSelector, EvidenceDisposition, EvidenceSelector,
};
use crate::cli::{
    ChangeCommand, ChangeSelectArgs, ChangeShowArgs, Cli, Command, IntegrateArgs, WorkAddArgs,
    WorkCommand,
};
use crate::corpus::{
    ProposalRecord, corpus_view, diff, get_work, list_commits, list_proposals, list_works,
    proposal_view, recorded_change_at, revision, select_proposal, store_work, work_view,
};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::index;
use crate::model::{CorpusView, DiffEntry, DiffKind, LibraryStats, ValidationSeverity};
use crate::render::{CommandOutput, render_terminal_text};
use crate::resolver::ResolvedOperation;
use crate::{liaison, resolver, validate};

#[must_use]
pub fn library_path(explicit: Option<&PathBuf>) -> PathBuf {
    explicit.cloned().unwrap_or_else(|| {
        std::env::var_os("ANNALS_LIBRARY")
            .map_or_else(|| PathBuf::from("./annals.db"), PathBuf::from)
    })
}

pub fn run(cli: &Cli, path: &Path) -> AppResult<CommandOutput> {
    match &cli.command {
        Command::Init => initialize(path),
        Command::Stats => stats(path),
        Command::Validate => validate_library(path),
        Command::Backup(args) => backup(path, &args.output),
        Command::Reindex => reindex(path),
        Command::Work(command) => match command {
            WorkCommand::Add(args) => add_work(path, args),
            WorkCommand::List => work_list(path),
            WorkCommand::Show(args) => show_work(path, &args.label),
        },
        Command::Integrate(args) => integrate(path, args, !cli.json),
        Command::Change(command) => match command {
            ChangeCommand::Submit(args) => submit_change(path, &args.input, &args.work, args.base),
            ChangeCommand::Show(args) => show_change(path, args),
            ChangeCommand::Validate(args) => validate_change(path, args),
            ChangeCommand::Apply(args) => apply_change(path, args),
            ChangeCommand::List => change_list(path),
        },
        Command::Show(args) => show_corpus(path, args.at),
        Command::Search(args) => search(path, &args.query, args.limit),
        Command::Log(args) => log(path, args.limit),
        Command::Diff(args) => diff_revisions(path, args.from, args.to),
        Command::Revert(args) => revert(path, args.revision),
        Command::LiaisonServer(args) => {
            liaison::serve(path, &args.token)?;
            Ok(CommandOutput::new(Value::Null, ""))
        }
    }
}

fn initialize(path: &Path) -> Result<CommandOutput, AppError> {
    let mut connection = db::init(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    index::rebuild_all(&transaction)?;
    transaction.commit()?;
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
        work_count: count(&connection, "works")?,
        evidence_count: count(&connection, "evidence")?,
        pending_change_count: count_where(&connection, "proposals", "status = 'pending'")?,
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
        index_current: index::status(&connection)?.is_current(),
    };
    let human = format!(
        "Revision: {}\nConcepts: {}\nWorks: {}\nEvidence links: {}\nPending changes: {}\nCommits: {}\nModel runs: {}\nDatabase size: {} bytes\nIndex current: {}",
        value.revision,
        value.concept_count,
        value.work_count,
        value.evidence_count,
        value.pending_change_count,
        value.commit_count,
        value.model_run_count,
        value.database_size_bytes,
        value.index_current
    );
    Ok(CommandOutput::new(to_value(&value)?, human))
}

fn validate_library(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_validation(path)?;
    let report = validate::validate(&connection)?;
    if !report.valid {
        let messages = report
            .issues
            .iter()
            .map(|issue| {
                let severity = match issue.severity {
                    ValidationSeverity::Warning => "warning",
                    ValidationSeverity::Error => "error",
                };
                format!("{severity} [{}]: {}", issue.code, issue.message)
            })
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

fn reindex(path: &Path) -> Result<CommandOutput, AppError> {
    let mut connection = db::open_write(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let indexed = index::rebuild_all(&transaction)?;
    transaction.commit()?;
    Ok(CommandOutput::new(
        json!({ "indexed_concepts": indexed.concepts }),
        format!("Indexed {} concepts", indexed.concepts),
    )
    .mutation())
}

fn add_work(path: &Path, args: &WorkAddArgs) -> Result<CommandOutput, AppError> {
    let text = read_utf8(&args.input, "work")?;
    let label = work_label(&args.input, args.name.as_deref())?;
    let connection = db::open_write(path)?;
    let work = store_work(&connection, &label, &text)?;
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
            "Stored work {:?} ({} bytes)\nCorpus remains at revision {corpus_revision}",
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
        let connection = db::open_write(path)?;
        store_work(&connection, &label, &text)?
    };
    let record = liaison::integrate(path, &work, forward_progress)?;
    if args.apply && record.outcome == "change" && record.uncertainties.is_empty() {
        let mut connection = db::open_write(path)?;
        let applied = resolver::apply_record(&mut connection, &record)?;
        return Ok(applied_output(&record, applied));
    }
    proposal_output(&record)
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
    proposal_output(&record)
}

fn show_change(path: &Path, args: &ChangeShowArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    if let Some(requested_revision) = args.at {
        let change = recorded_change_at(&connection, requested_revision)?;
        let human = render_recorded_change(&change)?;
        return Ok(CommandOutput::new(to_value(&change)?, human));
    }
    let record = select_proposal(&connection, args.work.as_deref(), false)?;
    let mut output = proposal_output(&record)?;
    output.quietable = false;
    Ok(output)
}

fn render_recorded_change(change: &crate::model::RecordedChangeView) -> Result<String, AppError> {
    let work = change
        .work
        .as_deref()
        .map_or_else(|| "none".to_owned(), render_quoted);
    let metadata = render_terminal_text(&serde_json::to_string(&change.metadata)?, false);
    let details = match change.kind.as_str() {
        "change" => render_recorded_proposal(change)?,
        "revert" => render_recorded_revert(change)?,
        _ => {
            return Err(AppError::database(
                "invalid_commit_kind",
                format!("revision {} has an unknown change kind", change.revision),
            ));
        }
    };
    Ok(format!(
        "Applied {} at revision {}\nParent revision: {}\nBase revision: {}\nWork: {}\nSummary: {}\n{}\nActor: {}\nMetadata: {}\nRecorded: {}",
        change.kind,
        change.revision,
        change.parent_revision,
        change.base_revision,
        work,
        render_terminal_text(&change.summary, false),
        details,
        render_terminal_text(&change.actor, false),
        metadata,
        render_terminal_text(&change.created_at, false),
    ))
}

fn render_recorded_proposal(change: &crate::model::RecordedChangeView) -> Result<String, AppError> {
    let proposal: ChangeProposal = serde_json::from_value(change.submitted_request.clone())
        .map_err(|error| {
            AppError::database(
                "invalid_commit_request",
                format!(
                    "revision {} has an invalid change proposal: {error}",
                    change.revision
                ),
            )
        })?;
    let ChangeProposal::Change {
        operations,
        uncertainties,
        ..
    } = proposal
    else {
        return Err(AppError::database(
            "invalid_commit_request",
            format!(
                "revision {} stores a no-change result as a commit",
                change.revision
            ),
        ));
    };
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
        operations.len(),
        render_requested_operations(&operations),
        resolved.len(),
        render_resolved_operations(&resolved, &operations),
        render_uncertainties(&uncertainties),
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
    let resolved: Vec<DiffEntry> = serde_json::from_value(change.resolved_operations.clone())
        .map_err(|error| {
            AppError::database(
                "invalid_commit_operations",
                format!(
                    "revision {} has invalid resolved operations: {error}",
                    change.revision
                ),
            )
        })?;
    let transition = if resolved.is_empty() {
        "  No semantic difference".to_owned()
    } else {
        resolved
            .iter()
            .map(|entry| format!("  {}", render_diff_entry(entry)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(format!(
        "Submitted request: revert revision {target}\nResolved transition ({}):\n{transition}",
        resolved.len()
    ))
}

fn validate_change(path: &Path, args: &ChangeSelectArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let record = select_proposal(&connection, args.work.as_deref(), true)?;
    let resolved = resolver::validate_record(&connection, &record)?;
    let proposal: ChangeProposal = serde_json::from_str(&record.submitted_request)?;
    let ChangeProposal::Change {
        operations,
        uncertainties,
        ..
    } = proposal
    else {
        return Err(AppError::database(
            "invalid_resolved_change",
            "a pending change contains a no-change request",
        ));
    };
    let application = if uncertainties.is_empty() {
        "ready"
    } else {
        "review required"
    };
    let human = format!(
        "Valid pending change for {}\nBase revision: {}\nSummary: {}\nResolved operations ({}):\n{}\n{}\nApplication: {application}",
        render_quoted(&record.work_label),
        record.base_revision,
        render_terminal_text(&record.summary, false),
        resolved.operations.len(),
        render_resolved_operations(&resolved.operations, &operations),
        render_uncertainties(&uncertainties),
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
    let record = select_proposal(&connection, args.work.as_deref(), true)?;
    let applied = resolver::apply_record(&mut connection, &record)?;
    Ok(applied_output(&record, applied))
}

fn change_list(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let proposals = list_proposals(&connection)?;
    let human = if proposals.is_empty() {
        "No recorded changes or examinations".to_owned()
    } else {
        proposals
            .iter()
            .map(|proposal| {
                format!(
                    "{}\t{}\t{}\t{}",
                    proposal.status,
                    render_terminal_text(&proposal.work, false),
                    proposal.base_revision,
                    render_terminal_text(&proposal.summary, false)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(to_value(&proposals)?, human))
}

fn proposal_output(record: &ProposalRecord) -> Result<CommandOutput, AppError> {
    let view = proposal_view(record)?;
    let proposal: ChangeProposal = serde_json::from_value(view.request.clone())?;
    let operation_count = view.request["operations"].as_array().map_or(0, Vec::len);
    let reason = view.request.get("reason").cloned();
    let data = json!({
        "work": view.work,
        "base_revision": view.base_revision,
        "outcome": view.outcome,
        "status": view.status,
        "summary": view.summary,
        "operation_count": operation_count,
        "uncertainties": view.uncertainties,
        "reason": reason,
        "proposal": view.request,
        "created_at": view.created_at,
        "applied_revision": view.applied_revision
    });
    let human = match &proposal {
        ChangeProposal::NoChange {
            reason,
            uncertainties,
            ..
        } => format!(
            "No corpus change proposed for {}\nBase revision: {}\nSummary: {}\nReason: {}\n{}\nCorpus remains unchanged",
            render_quoted(&view.work),
            view.base_revision,
            render_terminal_text(&view.summary, false),
            render_terminal_text(reason, false),
            render_uncertainties(uncertainties),
        ),
        ChangeProposal::Change {
            operations,
            uncertainties,
            ..
        } => {
            let heading = match view.status.as_str() {
                "applied" => "Applied change",
                "superseded" => "Superseded change",
                _ => "Pending change",
            };
            let status = match view.status.as_str() {
                "applied" => view.applied_revision.map_or_else(
                    || "applied".to_owned(),
                    |revision| format!("applied at revision {revision}"),
                ),
                "superseded" => "superseded".to_owned(),
                _ if uncertainties.is_empty() => "valid".to_owned(),
                _ => "review required".to_owned(),
            };
            let corpus_state = if view.status == "applied" {
                String::new()
            } else {
                "\nCorpus remains unchanged".to_owned()
            };
            format!(
                "{heading} for {}\nBase revision: {}\nSummary: {}\nOperations ({}):\n{}\n{}\nStatus: {status}{corpus_state}",
                render_quoted(&view.work),
                view.base_revision,
                render_terminal_text(&view.summary, false),
                operations.len(),
                render_requested_operations(operations),
                render_uncertainties(uncertainties),
            )
        }
    };
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
            label,
            under,
            before,
            after,
            evidence,
        } => {
            let mut lines = vec![format!(
                "  {number}. Create concept {}",
                render_quoted(label)
            )];
            append_requested_placement(&mut lines, under.as_ref(), before.as_ref(), after.as_ref());
            append_evidence_selectors(&mut lines, evidence);
            lines
        }
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
        ChangeOperation::MoveConcept {
            concept,
            under,
            before,
            after,
        } => {
            let mut lines = vec![format!("  {number}. Move {}", render_selector(concept))];
            append_requested_placement(&mut lines, under.as_ref(), before.as_ref(), after.as_ref());
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
    requested: Option<&ChangeOperation>,
) -> Vec<String> {
    let number = index + 1;
    match operation {
        ResolvedOperation::CreateConcept {
            path,
            evidence_quotes,
        } => {
            let mut lines = vec![format!("  {number}. Create concept {}", render_path(path))];
            append_resolved_placement(&mut lines, path, requested);
            append_resolved_quotes(&mut lines, evidence_quotes);
            lines
        }
        ResolvedOperation::AddEvidence { path, quotes } => {
            let mut lines = vec![format!("  {number}. Add evidence to {}", render_path(path))];
            append_resolved_quotes(&mut lines, quotes);
            lines
        }
        ResolvedOperation::RemoveEvidence { path, quotes } => {
            let mut lines = vec![format!(
                "  {number}. Remove evidence from {}",
                render_path(path)
            )];
            append_resolved_quotes(&mut lines, quotes);
            lines
        }
        ResolvedOperation::MoveConcept {
            before,
            after,
            previous_sibling_before,
            previous_sibling_after,
        } => {
            let mut lines = vec![format!(
                "  {number}. Move {} -> {}",
                render_path(before),
                render_path(after)
            )];
            let parent = if after.len() <= 1 {
                "root".to_owned()
            } else {
                render_path(&after[..after.len() - 1])
            };
            lines.push(format!("     Parent: {parent}"));
            lines.push(format!(
                "     Previous sibling: {} -> {}",
                render_optional_path(previous_sibling_before.as_deref()),
                render_optional_path(previous_sibling_after.as_deref())
            ));
            lines
        }
        ResolvedOperation::RewordConcept {
            before,
            after,
            evidence_disposition,
        } => vec![
            format!(
                "  {number}. Reword {} -> {}",
                render_path(before),
                render_path(after)
            ),
            format!(
                "     Evidence disposition: {}",
                render_evidence_disposition(*evidence_disposition)
            ),
        ],
        ResolvedOperation::RetireConcept { path, replacement } => vec![
            format!("  {number}. Retire {}", render_path(path)),
            format!(
                "     Replacement: {}",
                replacement
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), |path| render_path(path))
            ),
        ],
    }
}

fn append_requested_placement(
    lines: &mut Vec<String>,
    under: Option<&ConceptSelector>,
    before: Option<&ConceptSelector>,
    after: Option<&ConceptSelector>,
) {
    lines.push(format!(
        "     Parent: {}",
        under.map_or_else(|| "root".to_owned(), render_selector)
    ));
    lines.push(format!(
        "     Order: {}",
        render_requested_order(before, after)
    ));
}

fn append_resolved_placement(
    lines: &mut Vec<String>,
    final_path: &[String],
    requested: Option<&ChangeOperation>,
) {
    let parent = if final_path.len() <= 1 {
        "root".to_owned()
    } else {
        render_path(&final_path[..final_path.len() - 1])
    };
    let (before, after) = match requested {
        Some(
            ChangeOperation::CreateConcept { before, after, .. }
            | ChangeOperation::MoveConcept { before, after, .. },
        ) => (before.as_ref(), after.as_ref()),
        _ => (None, None),
    };
    lines.push(format!("     Parent: {parent}"));
    lines.push(format!(
        "     Order: {}",
        render_requested_order(before, after)
    ));
}

fn render_requested_order(
    before: Option<&ConceptSelector>,
    after: Option<&ConceptSelector>,
) -> String {
    if let Some(before) = before {
        format!("before {}", render_selector(before))
    } else if let Some(after) = after {
        format!("after {}", render_selector(after))
    } else {
        "append".to_owned()
    }
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
        ConceptSelector::Existing { path } => render_path(path),
        ConceptSelector::New { label } => format!("new concept {}", render_quoted(label)),
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

fn render_optional_path(path: Option<&[String]>) -> String {
    path.map_or_else(|| "none (first)".to_owned(), render_path)
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

fn render_uncertainties(uncertainties: &[String]) -> String {
    if uncertainties.is_empty() {
        "Uncertainties: none".to_owned()
    } else {
        format!(
            "Uncertainties:\n{}",
            uncertainties
                .iter()
                .map(|uncertainty| { format!("  - {}", render_terminal_text(uncertainty, false)) })
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

fn applied_output(record: &ProposalRecord, applied: i64) -> CommandOutput {
    CommandOutput::new(
        json!({
            "work": record.work_label,
            "base_revision": record.base_revision,
            "revision": applied,
            "status": "applied",
            "summary": record.summary
        }),
        format!(
            "Applied revision {applied}:\n{}",
            render_terminal_text(&record.summary, false)
        ),
    )
    .mutation()
}

fn show_corpus(path: &Path, requested: Option<i64>) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let requested = requested.unwrap_or(revision(&connection)?);
    let view = corpus_view(&connection, requested)?;
    let human = render_corpus(&view);
    Ok(CommandOutput::new(to_value(&view)?, human))
}

fn search(path: &Path, query: &str, limit: usize) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let output = crate::corpus::search_current(&connection, query, limit)?;
    let human = if output.results.is_empty() {
        "No matches".to_owned()
    } else {
        output
            .results
            .iter()
            .enumerate()
            .map(|(index, result)| {
                format!(
                    "{}. {}\n   Evidence: {} source quotation{}",
                    index + 1,
                    result
                        .path
                        .iter()
                        .map(|segment| render_terminal_text(segment, false))
                        .collect::<Vec<_>>()
                        .join(" › "),
                    result.evidence.len(),
                    if result.evidence.len() == 1 { "" } else { "s" }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(to_value(&output)?, human))
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
                    "r{} <- r{}\t{}\t{}",
                    commit.revision,
                    commit.parent_revision,
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
    let before = entry.before.as_deref().map(render_path);
    let after = entry.after.as_deref().map(render_path);
    match entry.kind {
        DiffKind::Created => format!(
            "Created: {}",
            after.unwrap_or_else(|| "unknown path".to_owned())
        ),
        DiffKind::Retired => format!(
            "Retired: {}",
            before.unwrap_or_else(|| "unknown path".to_owned())
        ),
        DiffKind::Moved if before == after => format!(
            "Reordered: {}",
            after.unwrap_or_else(|| "unknown path".to_owned())
        ),
        DiffKind::Moved => format!(
            "Moved: {} → {}",
            before.unwrap_or_else(|| "unknown path".to_owned()),
            after.unwrap_or_else(|| "unknown path".to_owned())
        ),
        DiffKind::Reworded => format!(
            "Reworded: {} → {}",
            before.unwrap_or_else(|| "unknown path".to_owned()),
            after.unwrap_or_else(|| "unknown path".to_owned())
        ),
        DiffKind::EvidenceAdded | DiffKind::EvidenceRemoved => {
            let action = if entry.kind == DiffKind::EvidenceAdded {
                "Evidence added"
            } else {
                "Evidence removed"
            };
            let path = if entry.kind == DiffKind::EvidenceAdded {
                after
            } else {
                before
            }
            .unwrap_or_else(|| "unknown path".to_owned());
            let work = entry.work.as_deref().map(render_quoted);
            let quote = entry.quote.as_deref().map(render_quoted);
            [Some(format!("{action}: {path}")), work, quote]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" — ")
        }
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
            "parent_revision": new_revision - 1,
            "reverted_revision": target,
            "status": "applied",
            "summary": format!("Revert revision {target}")
        }),
        format!("Applied revision {new_revision}:\nRevert revision {target}"),
    )
    .mutation())
}

fn render_corpus(view: &CorpusView) -> String {
    let mut lines = vec![format!("Corpus revision {}", view.revision)];
    if view.concepts.is_empty() {
        lines.push(String::new());
        lines.push("No concepts".to_owned());
    } else {
        lines.push(String::new());
        for concept in &view.concepts {
            lines.push(format!(
                "{}{}{}",
                "  ".repeat(concept.path.len().saturating_sub(1)),
                render_terminal_text(&concept.label, false),
                if concept.evidence.is_empty() {
                    String::new()
                } else {
                    format!(
                        " [{} source{}]",
                        concept.evidence.len(),
                        if concept.evidence.len() == 1 { "" } else { "s" }
                    )
                }
            ));
        }
    }
    lines.join("\n")
}

fn read_utf8(path: &Path, description: &str) -> Result<String, AppError> {
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

fn work_label(path: &Path, explicit: Option<&str>) -> Result<String, AppError> {
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
    use serde_json::json;

    use super::{proposal_output, render_resolved_operations};
    use crate::change::{ChangeOperation, ConceptSelector, EvidenceDisposition, EvidenceSelector};
    use crate::corpus::ProposalRecord;
    use crate::resolver::ResolvedOperation;

    fn existing(path: &[&str]) -> ConceptSelector {
        ConceptSelector::Existing {
            path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        }
    }

    fn exact(quote: &str) -> EvidenceSelector {
        EvidenceSelector {
            quote: quote.to_owned(),
            within_heading: None,
            preceded_by: None,
            followed_by: None,
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn proposal_output_renders_every_semantic_field_and_escapes_controls()
    -> Result<(), Box<dyn std::error::Error>> {
        let request = json!({
            "outcome": "change",
            "summary": "Audit the complete request",
            "operations": [
                {
                    "action": "create_concept",
                    "label": "Created\u{1b}[31m",
                    "under": {"path": ["Root"]},
                    "after": {"path": ["Root", "Earlier"]},
                    "evidence": [{
                        "quote": "Exact\nquote",
                        "within_heading": ["Details"],
                        "preceded_by": "Before ",
                        "followed_by": " after"
                    }]
                },
                {
                    "action": "add_evidence",
                    "concept": {"new": "Created\u{1b}[31m"},
                    "evidence": [{"quote": "Added evidence"}]
                },
                {
                    "action": "remove_evidence",
                    "concept": {"path": ["Root", "Old"]},
                    "evidence": [{"quote": "Removed evidence"}]
                },
                {
                    "action": "move_concept",
                    "concept": {"path": ["Root", "Moved"]},
                    "under": {"path": ["Destination"]},
                    "before": {"path": ["Destination", "Anchor"]}
                },
                {
                    "action": "reword_concept",
                    "concept": {"path": ["Root", "Old wording"]},
                    "label": "New wording",
                    "evidence_disposition": "remove"
                },
                {
                    "action": "retire_concept",
                    "concept": {"path": ["Root", "Retired"]},
                    "replacement": {"path": ["Root", "Successor"]}
                }
            ],
            "uncertainties": ["Confirm\tplacement"]
        });
        let record = ProposalRecord {
            id: 1,
            work_id: 1,
            work_label: "Work\u{1b}[2J".to_owned(),
            base_revision: 7,
            status: "pending".to_owned(),
            outcome: "change".to_owned(),
            summary: "Audit the complete request".to_owned(),
            submitted_request: serde_json::to_string(&request)?,
            resolved_change: "{}".to_owned(),
            uncertainties: vec!["Confirm\tplacement".to_owned()],
            actor: "human".to_owned(),
            created_at: "2026-08-12T00:00:00Z".to_owned(),
            applied_revision: None,
        };

        let output = proposal_output(&record)?;
        let human = output.human;
        assert!(human.contains("Pending change for “Work\\u{1b}[2J”"));
        assert!(human.contains("1. Create concept “Created\\u{1b}[31m”"));
        assert!(human.contains("Parent: “Root”"));
        assert!(human.contains("Order: after “Root” › “Earlier”"));
        assert!(human.contains("Evidence: “Exact\\u{a}quote”"));
        assert!(human.contains("Within heading: “Details”"));
        assert!(human.contains("Preceded by: “Before ”"));
        assert!(human.contains("Followed by: “ after”"));
        assert!(human.contains("2. Add evidence to new concept"));
        assert!(human.contains("3. Remove evidence from “Root” › “Old”"));
        assert!(human.contains("4. Move “Root” › “Moved”"));
        assert!(human.contains("Order: before “Destination” › “Anchor”"));
        assert!(human.contains("5. Reword “Root” › “Old wording” to “New wording”"));
        assert!(human.contains("Evidence disposition: remove existing evidence"));
        assert!(human.contains("6. Retire “Root” › “Retired”"));
        assert!(human.contains("Replacement: “Root” › “Successor”"));
        assert!(human.contains("- Confirm\\u{9}placement"));
        assert!(!human.contains('\u{1b}'));
        Ok(())
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn validation_renderer_shows_resolved_paths_quotes_order_and_identity_choices() {
        let requested = vec![
            ChangeOperation::CreateConcept {
                label: "Child".to_owned(),
                under: Some(existing(&["Root"])),
                before: Some(existing(&["Root", "Later"])),
                after: None,
                evidence: vec![exact("Creation evidence")],
            },
            ChangeOperation::AddEvidence {
                concept: existing(&["Root", "Child"]),
                evidence: vec![exact("Added evidence")],
            },
            ChangeOperation::RemoveEvidence {
                concept: existing(&["Root", "Child"]),
                evidence: vec![exact("Removed evidence")],
            },
            ChangeOperation::MoveConcept {
                concept: existing(&["Old", "Moved"]),
                under: Some(existing(&["New"])),
                before: None,
                after: Some(existing(&["New", "First"])),
            },
            ChangeOperation::RewordConcept {
                concept: existing(&["New", "Moved"]),
                label: "Renamed".to_owned(),
                evidence_disposition: EvidenceDisposition::Retain,
            },
            ChangeOperation::RetireConcept {
                concept: existing(&["Old", "Retired"]),
                replacement: Some(existing(&["New", "Replacement"])),
            },
        ];
        let resolved = vec![
            ResolvedOperation::CreateConcept {
                path: vec!["Root".to_owned(), "Child".to_owned()],
                evidence_quotes: vec!["Creation evidence".to_owned()],
            },
            ResolvedOperation::AddEvidence {
                path: vec!["Root".to_owned(), "Child".to_owned()],
                quotes: vec!["Added evidence".to_owned()],
            },
            ResolvedOperation::RemoveEvidence {
                path: vec!["Root".to_owned(), "Child".to_owned()],
                quotes: vec!["Removed evidence".to_owned()],
            },
            ResolvedOperation::MoveConcept {
                before: vec!["Old".to_owned(), "Moved".to_owned()],
                after: vec!["New".to_owned(), "Moved".to_owned()],
                previous_sibling_before: Some(vec!["Old".to_owned(), "Earlier".to_owned()]),
                previous_sibling_after: Some(vec!["New".to_owned(), "First".to_owned()]),
            },
            ResolvedOperation::RewordConcept {
                before: vec!["New".to_owned(), "Moved".to_owned()],
                after: vec!["New".to_owned(), "Renamed".to_owned()],
                evidence_disposition: EvidenceDisposition::Retain,
            },
            ResolvedOperation::RetireConcept {
                path: vec!["Old".to_owned(), "Retired".to_owned()],
                replacement: Some(vec!["New".to_owned(), "Replacement".to_owned()]),
            },
        ];

        let human = render_resolved_operations(&resolved, &requested);
        assert!(human.contains("Create concept “Root” › “Child”"));
        assert!(human.contains("Parent: “Root”"));
        assert!(human.contains("Order: before “Root” › “Later”"));
        assert!(human.contains("Evidence: “Creation evidence”"));
        assert!(human.contains("Add evidence to “Root” › “Child”"));
        assert!(human.contains("Evidence: “Added evidence”"));
        assert!(human.contains("Remove evidence from “Root” › “Child”"));
        assert!(human.contains("Evidence: “Removed evidence”"));
        assert!(human.contains("Move “Old” › “Moved” -> “New” › “Moved”"));
        assert!(human.contains("Previous sibling: “Old” › “Earlier” -> “New” › “First”"));
        assert!(human.contains("Reword “New” › “Moved” -> “New” › “Renamed”"));
        assert!(human.contains("Evidence disposition: retain existing evidence"));
        assert!(human.contains("Retire “Old” › “Retired”"));
        assert!(human.contains("Replacement: “New” › “Replacement”"));
    }
}
