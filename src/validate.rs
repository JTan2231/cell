use std::collections::BTreeMap;
use std::time::Duration;

use rusqlite::Connection;
use rusqlite::backup::Backup;
use sha2::{Digest, Sha256};

use crate::change::{ChangeOperation, Reconciliation, parse_reconciliation};
use crate::corpus::{self, Snapshot};
use crate::error::AppError;
use crate::index;
use crate::model::{DiffEntry, ValidationIssue, ValidationReport, ValidationSeverity};
use crate::resolver::{ResolvedOperation, ResolvedReconciliation, snapshots_corpus_equal};

#[derive(Debug)]
struct StoredCommit {
    revision: i64,
    parent_revision: i64,
    base_revision: i64,
    work_id: Option<i64>,
    reconciliation_id: Option<i64>,
    kind: String,
    summary: String,
    submitted_request: String,
    resolved_operations: String,
    before_snapshot: String,
    after_snapshot: String,
    metadata: String,
    actor: String,
}

#[derive(Debug)]
struct StoredReconciliation {
    id: i64,
    work_id: i64,
    base_revision: i64,
    model_run_id: Option<i64>,
    status: String,
    summary: String,
    submitted_request: String,
    resolved_reconciliation: String,
    actor: String,
    applied_revision: Option<i64>,
}

#[derive(Debug)]
struct StoredModelRun {
    id: i64,
    work_id: i64,
    base_revision: i64,
    status: String,
}

#[derive(Debug)]
struct SuccessfulSubmission {
    model_run_id: i64,
    arguments: String,
}

#[derive(Debug, PartialEq, Eq)]
struct ExpectedIndexRow {
    id: i64,
    concept_id: i64,
    label: String,
    path: String,
    normalized_label: String,
    normalized_path: String,
    content_hash: String,
    indexer_version: i64,
}

/// Validate the authoritative corpus, its append-only history, and its derived index.
///
/// Validation is deliberately read-only. Every detected invariant violation is returned in
/// the report; no canonical or derived state is repaired here.
pub fn validate(connection: &Connection) -> Result<ValidationReport, AppError> {
    let mut issues = Vec::new();

    check_sqlite_integrity(connection, &mut issues)?;
    check_foreign_keys(connection, &mut issues)?;
    check_fts_integrity(connection, &mut issues);
    check_work_hashes(connection, &mut issues)?;

    let head_revision = load_head_revision(connection, &mut issues)?;
    let commits = load_commits(connection)?;
    let history_head = check_history(connection, head_revision, &commits, &mut issues);
    check_provenance(connection, head_revision, &commits, &mut issues)?;

    let current_snapshot = match corpus::head_snapshot(connection) {
        Ok(snapshot) => Some(snapshot),
        Err(error) => {
            issues.push(error_issue(
                "invalid_materialized_snapshot",
                format!("the materialized corpus could not be read: {error}"),
            ));
            None
        }
    };

    if let Some(snapshot) = current_snapshot.as_ref() {
        if let Err(error) = corpus::validate_snapshot(connection, snapshot) {
            issues.push(error_issue(
                "invalid_corpus_snapshot",
                format!("the materialized corpus violates corpus invariants: {error}"),
            ));
        }
        check_materialized_head(head_revision, snapshot, history_head.as_ref(), &mut issues);
        check_index(connection, snapshot, &mut issues)?;
    }

    Ok(report(issues))
}

fn check_sqlite_integrity(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection.prepare("PRAGMA integrity_check")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        let result = row?;
        if result != "ok" {
            issues.push(error_issue(
                "sqlite_integrity",
                format!("SQLite integrity check reported: {result}"),
            ));
        }
    }
    Ok(())
}

fn check_foreign_keys(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    for row in rows {
        let (table, row_id, parent, constraint) = row?;
        issues.push(error_issue(
            "foreign_key_violation",
            format!(
                "{table} row {row_id:?} violates foreign key {constraint} referencing {parent}"
            ),
        ));
    }
    Ok(())
}

fn check_fts_integrity(connection: &Connection, issues: &mut Vec<ValidationIssue>) {
    let mut copy = match Connection::open_in_memory() {
        Ok(copy) => copy,
        Err(error) => {
            issues.push(error_issue(
                "fts_integrity",
                format!("could not create a temporary validation snapshot: {error}"),
            ));
            return;
        }
    };
    let backup_result = Backup::new(connection, &mut copy)
        .and_then(|backup| backup.run_to_completion(128, Duration::from_millis(1), None));
    if let Err(error) = backup_result {
        issues.push(error_issue(
            "fts_integrity",
            format!("could not copy the library for FTS validation: {error}"),
        ));
        return;
    }
    if let Err(error) = copy.execute(
        "INSERT INTO concept_fts(concept_fts, rank) VALUES ('integrity-check', 1)",
        [],
    ) {
        issues.push(error_issue(
            "fts_integrity",
            format!("FTS integrity check failed: {error}"),
        ));
    }
}

fn check_work_hashes(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement =
        connection.prepare("SELECT id, label, text, sha256 FROM works ORDER BY id")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (id, label, text, stored_hash) = row?;
        if text.trim().is_empty() {
            issues.push(error_issue(
                "empty_work",
                format!("immutable work {label:?} ({id}) contains no source text"),
            ));
        }
        let actual_hash = corpus::sha256_hex(text.as_bytes());
        if actual_hash != stored_hash {
            issues.push(error_issue(
                "work_checksum_mismatch",
                format!("immutable work {label:?} ({id}) does not match its SHA-256 digest"),
            ));
        }
    }
    Ok(())
}

fn load_head_revision(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<Option<i64>, AppError> {
    let mut statement =
        connection.prepare("SELECT singleton, revision FROM library_state ORDER BY singleton")?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    match rows.as_slice() {
        [(1, revision)] => Ok(Some(*revision)),
        [] => {
            issues.push(error_issue(
                "library_state_missing",
                "the library has no HEAD revision record",
            ));
            Ok(None)
        }
        _ => {
            issues.push(error_issue(
                "invalid_library_state",
                "the library must contain exactly one singleton HEAD revision record",
            ));
            Ok(rows
                .iter()
                .find_map(|(singleton, revision)| (*singleton == 1).then_some(*revision)))
        }
    }
}

fn load_commits(connection: &Connection) -> Result<Vec<StoredCommit>, AppError> {
    let mut statement = connection.prepare(
        "SELECT revision, parent_revision, base_revision, work_id, reconciliation_id, kind, summary, \
                submitted_request, resolved_operations, before_snapshot, after_snapshot, \
                metadata, actor \
         FROM commits ORDER BY revision",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(StoredCommit {
            revision: row.get(0)?,
            parent_revision: row.get(1)?,
            base_revision: row.get(2)?,
            work_id: row.get(3)?,
            reconciliation_id: row.get(4)?,
            kind: row.get(5)?,
            summary: row.get(6)?,
            submitted_request: row.get(7)?,
            resolved_operations: row.get(8)?,
            before_snapshot: row.get(9)?,
            after_snapshot: row.get(10)?,
            metadata: row.get(11)?,
            actor: row.get(12)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn load_reconciliations(connection: &Connection) -> Result<Vec<StoredReconciliation>, AppError> {
    let mut statement = connection.prepare(
        "SELECT id, work_id, base_revision, model_run_id, status, summary, submitted_request, \
                resolved_reconciliation, actor, applied_revision \
         FROM reconciliations ORDER BY id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(StoredReconciliation {
            id: row.get(0)?,
            work_id: row.get(1)?,
            base_revision: row.get(2)?,
            model_run_id: row.get(3)?,
            status: row.get(4)?,
            summary: row.get(5)?,
            submitted_request: row.get(6)?,
            resolved_reconciliation: row.get(7)?,
            actor: row.get(8)?,
            applied_revision: row.get(9)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn load_model_runs(connection: &Connection) -> Result<Vec<StoredModelRun>, AppError> {
    let mut statement = connection
        .prepare("SELECT id, work_id, base_revision, status FROM model_runs ORDER BY id")?;
    let rows = statement.query_map([], |row| {
        Ok(StoredModelRun {
            id: row.get(0)?,
            work_id: row.get(1)?,
            base_revision: row.get(2)?,
            status: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn load_successful_submissions(
    connection: &Connection,
) -> Result<Vec<SuccessfulSubmission>, AppError> {
    let mut statement = connection.prepare(
        "SELECT model_run_id, arguments FROM tool_calls \
         WHERE tool_name = 'submit_reconciliation' AND succeeded = 1 \
         ORDER BY model_run_id, sequence",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(SuccessfulSubmission {
            model_run_id: row.get(0)?,
            arguments: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn check_provenance(
    connection: &Connection,
    head_revision: Option<i64>,
    commits: &[StoredCommit],
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let reconciliations = load_reconciliations(connection)?;
    let model_runs = load_model_runs(connection)?;
    let submissions = load_successful_submissions(connection)?;
    let reconciliations_by_id = reconciliations
        .iter()
        .map(|reconciliation| (reconciliation.id, reconciliation))
        .collect::<BTreeMap<_, _>>();
    let commits_by_reconciliation = commits.iter().fold(
        BTreeMap::<i64, Vec<&StoredCommit>>::new(),
        |mut grouped, commit| {
            if let Some(reconciliation_id) = commit.reconciliation_id {
                grouped.entry(reconciliation_id).or_default().push(commit);
            }
            grouped
        },
    );

    for reconciliation in &reconciliations {
        check_reconciliation_payload(connection, head_revision, reconciliation, issues);
        let linked = commits_by_reconciliation
            .get(&reconciliation.id)
            .map_or(&[][..], Vec::as_slice);
        if reconciliation.status == "applied" {
            if linked.len() != 1 {
                issues.push(error_issue(
                    "reconciliation_commit_mismatch",
                    format!(
                        "applied reconciliation {} is linked to {} commits instead of exactly one",
                        reconciliation.id,
                        linked.len()
                    ),
                ));
            }
        } else if !linked.is_empty() {
            issues.push(error_issue(
                "reconciliation_commit_mismatch",
                format!(
                    "non-applied reconciliation {} is linked to a corpus commit",
                    reconciliation.id
                ),
            ));
        }
    }

    for commit in commits {
        check_commit_provenance(connection, commit, &reconciliations_by_id, commits, issues);
    }
    check_model_run_provenance(
        head_revision,
        &model_runs,
        &reconciliations,
        &submissions,
        issues,
    );
    Ok(())
}

fn check_reconciliation_payload(
    connection: &Connection,
    head_revision: Option<i64>,
    reconciliation: &StoredReconciliation,
    issues: &mut Vec<ValidationIssue>,
) {
    if head_revision.is_some_and(|head| reconciliation.base_revision > head) {
        issues.push(error_issue(
            "reconciliation_base_missing",
            format!(
                "reconciliation {} targets revision {}, which is later than corpus HEAD",
                reconciliation.id, reconciliation.base_revision
            ),
        ));
    }
    let request = check_reconciliation_request(reconciliation, issues);
    check_reconciliation_resolution(connection, reconciliation, request.as_ref(), issues);
    if reconciliation.status == "recorded" {
        check_reconciliation_replay(connection, reconciliation, issues);
    }
}

fn check_reconciliation_replay(
    connection: &Connection,
    reconciliation: &StoredReconciliation,
    issues: &mut Vec<ValidationIssue>,
) {
    let record = match corpus::reconciliation_query(
        connection,
        "SELECT r.id, r.work_id, w.label, r.base_revision, r.status, r.summary, \
                r.submitted_request, r.resolved_reconciliation, r.actor, r.created_at, \
                r.applied_revision \
         FROM reconciliations AS r JOIN works AS w ON w.id = r.work_id WHERE r.id = ?1",
        [reconciliation.id],
    ) {
        Ok(Some(record)) => record,
        Ok(None) => return,
        Err(error) => {
            issues.push(error_issue(
                "invalid_reconciliation_resolution",
                format!(
                    "reconciliation {} could not be loaded: {error}",
                    reconciliation.id
                ),
            ));
            return;
        }
    };
    let expected =
        serde_json::from_str::<ResolvedReconciliation>(&reconciliation.resolved_reconciliation);
    match (
        expected,
        crate::resolver::replay_record(connection, &record),
    ) {
        (Ok(expected), Ok(actual)) if expected == actual => {}
        (Ok(_), Ok(_)) => issues.push(error_issue(
            "reconciliation_resolution_mismatch",
            format!(
                "reconciliation {} does not resolve to its stored projection",
                reconciliation.id
            ),
        )),
        (_, Err(error)) => issues.push(error_issue(
            "reconciliation_resolution_mismatch",
            format!(
                "reconciliation {} cannot be replayed: {error}",
                reconciliation.id
            ),
        )),
        (Err(_), Ok(_)) => {}
    }
}

fn check_reconciliation_request(
    reconciliation: &StoredReconciliation,
    issues: &mut Vec<ValidationIssue>,
) -> Option<Reconciliation> {
    let request = match parse_reconciliation(&reconciliation.submitted_request) {
        Ok(request) => Some(request),
        Err(error) => {
            issues.push(error_issue(
                "invalid_reconciliation_request",
                format!(
                    "reconciliation {} has an invalid request: {error}",
                    reconciliation.id
                ),
            ));
            None
        }
    };
    if request
        .as_ref()
        .is_some_and(|request| reconciliation.summary != request.summary())
    {
        issues.push(error_issue(
            "reconciliation_request_mismatch",
            format!(
                "reconciliation {} does not match its request's summary",
                reconciliation.id
            ),
        ));
    }
    request
}

fn check_reconciliation_resolution(
    connection: &Connection,
    reconciliation: &StoredReconciliation,
    request: Option<&Reconciliation>,
    issues: &mut Vec<ValidationIssue>,
) {
    let resolved = match serde_json::from_str::<ResolvedReconciliation>(
        &reconciliation.resolved_reconciliation,
    ) {
        Ok(resolved) => Some(resolved),
        Err(error) => {
            issues.push(error_issue(
                "invalid_reconciliation_resolution",
                format!(
                    "reconciliation {} has an invalid resolved reconciliation: {error}",
                    reconciliation.id
                ),
            ));
            None
        }
    };
    let Some(resolved) = resolved.as_ref() else {
        return;
    };
    if resolved.base_revision != reconciliation.base_revision {
        issues.push(error_issue(
            "reconciliation_resolution_mismatch",
            format!(
                "reconciliation {} and its resolved reconciliation name different base revisions",
                reconciliation.id
            ),
        ));
    }
    let base = match corpus::snapshot_at(connection, reconciliation.base_revision) {
        Ok(base) => Some(base),
        Err(error) => {
            issues.push(error_issue(
                "reconciliation_base_missing",
                format!(
                    "reconciliation {} has no readable base revision: {error}",
                    reconciliation.id
                ),
            ));
            None
        }
    };

    let recorded = reconciliation.status == "recorded";
    let expected_revision = if recorded {
        reconciliation.base_revision
    } else {
        reconciliation.base_revision.saturating_add(1)
    };
    validate_historical_snapshot(
        connection,
        &resolved.resulting_snapshot,
        expected_revision,
        expected_revision,
        "reconciliation result",
        issues,
    );

    if request
        .is_some_and(|request| !operation_kinds_match(request.operations(), &resolved.operations))
    {
        issues.push(error_issue(
            "reconciliation_resolution_mismatch",
            format!(
                "reconciliation {} has resolved operations inconsistent with its request",
                reconciliation.id
            ),
        ));
    }
    if let Some(base) = base.as_ref() {
        let unchanged = snapshots_corpus_equal(base, &resolved.resulting_snapshot);
        if recorded != unchanged {
            issues.push(error_issue(
                "reconciliation_resolution_mismatch",
                format!(
                    "reconciliation {} status {:?} does not match whether its projection changes the corpus",
                    reconciliation.id, reconciliation.status
                ),
            ));
        }
    }
}

fn operation_kinds_match(requested: &[ChangeOperation], resolved: &[ResolvedOperation]) -> bool {
    requested.len() == resolved.len()
        && requested.iter().zip(resolved).all(|(request, resolution)| {
            matches!(
                (request, resolution),
                (
                    ChangeOperation::CreateConcept { .. },
                    ResolvedOperation::CreateConcept { .. }
                ) | (
                    ChangeOperation::AddEvidence { .. },
                    ResolvedOperation::AddEvidence { .. }
                ) | (
                    ChangeOperation::RemoveEvidence { .. },
                    ResolvedOperation::RemoveEvidence { .. }
                ) | (
                    ChangeOperation::MoveConcept { .. },
                    ResolvedOperation::MoveConcept { .. }
                ) | (
                    ChangeOperation::RewordConcept { .. },
                    ResolvedOperation::RewordConcept { .. }
                ) | (
                    ChangeOperation::RetireConcept { .. },
                    ResolvedOperation::RetireConcept { .. }
                )
            )
        })
}

fn check_commit_provenance(
    connection: &Connection,
    commit: &StoredCommit,
    reconciliations: &BTreeMap<i64, &StoredReconciliation>,
    commits: &[StoredCommit],
    issues: &mut Vec<ValidationIssue>,
) {
    if commit.before_snapshot == commit.after_snapshot {
        issues.push(error_issue(
            "empty_commit",
            format!("revision {} does not change the corpus", commit.revision),
        ));
    }
    match commit.kind.as_str() {
        "change" => check_reconciliation_commit(commit, reconciliations, issues),
        "revert" => check_revert_commit(connection, commit, commits, issues),
        _ => {}
    }
}

fn check_reconciliation_commit(
    commit: &StoredCommit,
    reconciliations: &BTreeMap<i64, &StoredReconciliation>,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(reconciliation_id) = commit.reconciliation_id else {
        issues.push(error_issue(
            "commit_reconciliation_mismatch",
            format!("change revision {} has no reconciliation", commit.revision),
        ));
        return;
    };
    let Some(reconciliation) = reconciliations.get(&reconciliation_id) else {
        issues.push(error_issue(
            "commit_reconciliation_mismatch",
            format!(
                "change revision {} refers to a missing reconciliation",
                commit.revision
            ),
        ));
        return;
    };
    let request_matches =
        reconciliations_equal(&commit.submitted_request, &reconciliation.submitted_request);
    let resolved_matches =
        serde_json::from_str::<ResolvedReconciliation>(&reconciliation.resolved_reconciliation)
            .ok()
            .and_then(|resolved| serde_json::to_string(&resolved.operations).ok())
            .is_some_and(|operations| json_equal(&commit.resolved_operations, &operations));
    let snapshot_matches =
        serde_json::from_str::<ResolvedReconciliation>(&reconciliation.resolved_reconciliation)
            .is_ok_and(|resolved| {
                serde_json::from_str::<Snapshot>(&commit.after_snapshot)
                    .is_ok_and(|after| after == resolved.resulting_snapshot)
            });
    let metadata_actor = serde_json::from_str::<serde_json::Value>(&commit.metadata)
        .ok()
        .and_then(|metadata| {
            metadata
                .get("reconciliation_actor")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    if reconciliation.status != "applied"
        || reconciliation.applied_revision != Some(commit.revision)
        || commit.work_id != Some(reconciliation.work_id)
        || commit.base_revision != reconciliation.base_revision
        || commit.summary != reconciliation.summary
        || commit.actor != reconciliation.actor
        || metadata_actor.as_deref() != Some(reconciliation.actor.as_str())
        || !request_matches
        || !resolved_matches
        || !snapshot_matches
    {
        issues.push(error_issue(
            "commit_reconciliation_mismatch",
            format!(
                "change revision {} does not exactly preserve its applied reconciliation",
                commit.revision
            ),
        ));
    }
}

fn check_revert_commit(
    connection: &Connection,
    commit: &StoredCommit,
    commits: &[StoredCommit],
    issues: &mut Vec<ValidationIssue>,
) {
    let request_target = json_integer(&commit.submitted_request, "revert_revision");
    let metadata_target = json_integer(&commit.metadata, "reverted_revision");
    let target = request_target.filter(|target| Some(*target) == metadata_target);
    let target_commit = target.and_then(|target| {
        commits
            .iter()
            .find(|candidate| candidate.revision == target && target < commit.revision)
    });
    let resolved_matches = revert_operations_match(connection, commit);
    if commit.reconciliation_id.is_some()
        || target_commit.is_none()
        || target_commit.is_some_and(|target| target.work_id != commit.work_id)
        || target.is_some_and(|target| commit.summary != format!("Revert revision {target}"))
        || !resolved_matches
    {
        issues.push(error_issue(
            "invalid_revert_provenance",
            format!(
                "revert revision {} does not consistently identify its target change",
                commit.revision
            ),
        ));
    }
}

fn revert_operations_match(connection: &Connection, commit: &StoredCommit) -> bool {
    let Ok(before) = serde_json::from_str::<Snapshot>(&commit.before_snapshot) else {
        return false;
    };
    let Ok(after) = serde_json::from_str::<Snapshot>(&commit.after_snapshot) else {
        return false;
    };
    let Ok(stored) = serde_json::from_str::<Vec<DiffEntry>>(&commit.resolved_operations) else {
        return false;
    };
    corpus::diff_snapshot_entries(connection, &before, &after)
        .is_ok_and(|expected| expected == stored)
}

fn check_model_run_provenance(
    head_revision: Option<i64>,
    model_runs: &[StoredModelRun],
    reconciliations: &[StoredReconciliation],
    submissions: &[SuccessfulSubmission],
    issues: &mut Vec<ValidationIssue>,
) {
    for run in model_runs {
        if head_revision.is_some_and(|head| run.base_revision > head) {
            issues.push(error_issue(
                "model_run_base_missing",
                format!(
                    "model run {} examined revision {}, which is later than corpus HEAD",
                    run.id, run.base_revision
                ),
            ));
        }
        let linked = reconciliations
            .iter()
            .filter(|reconciliation| reconciliation.model_run_id == Some(run.id))
            .collect::<Vec<_>>();
        let successful = submissions
            .iter()
            .filter(|submission| submission.model_run_id == run.id)
            .collect::<Vec<_>>();
        let should_have_submission = run.status == "submitted";
        if (should_have_submission && (linked.len() != 1 || successful.len() != 1))
            || (!should_have_submission && (!linked.is_empty() || !successful.is_empty()))
        {
            issues.push(error_issue(
                "model_run_submission_mismatch",
                format!(
                    "model run {} status {:?} does not match its recorded reconciliation and submit call",
                    run.id, run.status
                ),
            ));
            continue;
        }
        if let ([reconciliation], [submission]) = (linked.as_slice(), successful.as_slice())
            && (reconciliation.work_id != run.work_id
                || reconciliation.base_revision != run.base_revision
                || !reconciliations_equal(&reconciliation.submitted_request, &submission.arguments))
        {
            issues.push(error_issue(
                "model_run_reconciliation_mismatch",
                format!(
                    "model run {} and its submitted reconciliation have different scope or payload",
                    run.id
                ),
            ));
        }
    }
}

fn reconciliations_equal(left: &str, right: &str) -> bool {
    parse_reconciliation(left)
        .ok()
        .zip(parse_reconciliation(right).ok())
        .is_some_and(|(left, right)| left == right)
}

fn json_equal(left: &str, right: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(left)
        .ok()
        .zip(serde_json::from_str::<serde_json::Value>(right).ok())
        .is_some_and(|(left, right)| left == right)
}

fn json_integer(source: &str, field: &str) -> Option<i64> {
    serde_json::from_str::<serde_json::Value>(source)
        .ok()?
        .get(field)?
        .as_i64()
}

/// Check the linear log and return the parsed after-state at HEAD, when available.
#[allow(clippy::too_many_lines)]
fn check_history(
    connection: &Connection,
    head_revision: Option<i64>,
    commits: &[StoredCommit],
    issues: &mut Vec<ValidationIssue>,
) -> Option<Snapshot> {
    let max_revision = commits.last().map_or(0, |commit| commit.revision);
    let commit_count = i64::try_from(commits.len()).unwrap_or(i64::MAX);
    if let Some(head) = head_revision {
        if head != max_revision {
            issues.push(error_issue(
                "library_revision_mismatch",
                format!(
                    "library HEAD is revision {head}, but the highest commit is revision {max_revision}"
                ),
            ));
        }
        if head != commit_count {
            issues.push(error_issue(
                "commit_count_mismatch",
                format!(
                    "library HEAD is revision {head}, but the commit log contains {commit_count} entries"
                ),
            ));
        }
    }

    let mut previous_after = Some(Snapshot::empty());
    let mut head_after = None;
    for (offset, commit) in commits.iter().enumerate() {
        let expected_revision = i64::try_from(offset)
            .ok()
            .and_then(|offset| offset.checked_add(1))
            .unwrap_or(i64::MAX);
        if commit.revision != expected_revision {
            issues.push(error_issue(
                "commit_sequence_mismatch",
                format!(
                    "commit position {expected_revision} contains revision {}",
                    commit.revision
                ),
            ));
        }
        let expected_parent = commit.revision.checked_sub(1);
        if expected_parent != Some(commit.parent_revision) {
            issues.push(error_issue(
                "commit_parent_mismatch",
                format!(
                    "revision {} names parent {}, expected {}",
                    commit.revision,
                    commit.parent_revision,
                    expected_parent
                        .map_or_else(|| "no valid parent".to_owned(), |value| value.to_string())
                ),
            ));
        }
        if commit.base_revision != commit.parent_revision {
            issues.push(error_issue(
                "commit_base_mismatch",
                format!(
                    "revision {} targets base {}, but its parent is {}",
                    commit.revision, commit.base_revision, commit.parent_revision
                ),
            ));
        }

        let before = parse_snapshot(&commit.before_snapshot, commit.revision, "before", issues);
        let after = parse_snapshot(&commit.after_snapshot, commit.revision, "after", issues);
        if let Some(snapshot) = &before {
            validate_historical_snapshot(
                connection,
                snapshot,
                commit.parent_revision,
                commit.revision,
                "before",
                issues,
            );
        }
        if let Some(snapshot) = &after {
            validate_historical_snapshot(
                connection,
                snapshot,
                commit.revision,
                commit.revision,
                "after",
                issues,
            );
        }
        if let (Some(expected), Some(actual)) = (previous_after.as_ref(), before.as_ref())
            && actual != expected
        {
            issues.push(error_issue(
                "commit_history_mismatch",
                format!(
                    "revision {} does not begin with the preceding revision's after-state",
                    commit.revision
                ),
            ));
        }
        if head_revision == Some(commit.revision) {
            head_after.clone_from(&after);
        }
        previous_after = after;
    }

    if head_revision == Some(0) {
        Some(Snapshot::empty())
    } else {
        head_after
    }
}

fn validate_historical_snapshot(
    connection: &Connection,
    snapshot: &Snapshot,
    snapshot_revision: i64,
    commit_revision: i64,
    side: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if let Err(error) = corpus::validate_snapshot(connection, snapshot) {
        issues.push(error_issue(
            "invalid_historical_snapshot",
            format!("revision {commit_revision} has an invalid {side}-snapshot: {error}"),
        ));
    }
    for concept in &snapshot.concepts {
        if concept.created_revision > snapshot_revision
            || concept.updated_revision > snapshot_revision
        {
            issues.push(error_issue(
                "future_concept_revision",
                format!(
                    "revision {commit_revision} has a {side}-snapshot whose concept revision metadata is later than snapshot revision {snapshot_revision}"
                ),
            ));
            break;
        }
    }
}

fn parse_snapshot(
    source: &str,
    revision: i64,
    side: &str,
    issues: &mut Vec<ValidationIssue>,
) -> Option<Snapshot> {
    match serde_json::from_str(source) {
        Ok(snapshot) => Some(snapshot),
        Err(error) => {
            issues.push(error_issue(
                "invalid_commit_snapshot",
                format!("revision {revision} has an invalid {side}-snapshot: {error}"),
            ));
            None
        }
    }
}

fn check_materialized_head(
    head_revision: Option<i64>,
    current: &Snapshot,
    history_head: Option<&Snapshot>,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(head) = head_revision else {
        return;
    };
    if let Some(expected) = history_head {
        if current != expected {
            issues.push(error_issue(
                "materialized_head_mismatch",
                if head == 0 {
                    "the materialized corpus is not empty at revision 0".to_owned()
                } else {
                    format!("the materialized corpus does not match revision {head}'s after-state")
                },
            ));
        }
    } else if head > 0 {
        issues.push(error_issue(
            "materialized_head_unverifiable",
            format!("revision {head} has no parseable commit after-state"),
        ));
    }
}

fn check_index(
    connection: &Connection,
    snapshot: &Snapshot,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let status = index::status(connection)?;
    if !status.is_current() {
        issues.push(error_issue(
            "index_stale",
            format!(
                "concept index status is stale (version {:?}, {} concepts, {} indexed rows)",
                status.stored_version, status.concept_count, status.indexed_count
            ),
        ));
    }

    let paths = match corpus::paths(snapshot) {
        Ok(paths) => paths,
        Err(error) => {
            issues.push(error_issue(
                "index_paths_unavailable",
                format!("canonical concept paths could not be derived: {error}"),
            ));
            return Ok(());
        }
    };
    let expected = snapshot
        .concepts
        .iter()
        .filter_map(|concept| {
            let segments = paths.get(&concept.id)?;
            let path = segments.join(" › ");
            Some((
                concept.id,
                ExpectedIndexRow {
                    id: concept.id,
                    concept_id: concept.id,
                    label: concept.label.clone(),
                    normalized_label: index::normalize(&concept.label),
                    normalized_path: index::normalize(&path),
                    content_hash: index_content_hash(concept.id, &concept.label, &path),
                    path,
                    indexer_version: index::INDEXER_VERSION,
                },
            ))
        })
        .collect::<BTreeMap<_, _>>();

    let mut statement = connection.prepare(
        "SELECT id, concept_id, label, path, normalized_label, normalized_path, content_hash, \
                indexer_version \
         FROM concept_search ORDER BY concept_id",
    )?;
    let stored = statement
        .query_map([], |row| {
            Ok(ExpectedIndexRow {
                id: row.get(0)?,
                concept_id: row.get(1)?,
                label: row.get(2)?,
                path: row.get(3)?,
                normalized_label: row.get(4)?,
                normalized_path: row.get(5)?,
                content_hash: row.get(6)?,
                indexer_version: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let stored = stored
        .into_iter()
        .map(|row| (row.concept_id, row))
        .collect::<BTreeMap<_, _>>();

    for (concept_id, expected_row) in &expected {
        match stored.get(concept_id) {
            None => issues.push(error_issue(
                "index_row_missing",
                format!("concept {concept_id} has no derived search row"),
            )),
            Some(stored_row) if stored_row != expected_row => issues.push(error_issue(
                "index_row_mismatch",
                format!("derived search row for concept {concept_id} differs from the corpus"),
            )),
            Some(_) => {}
        }
    }
    for concept_id in stored.keys() {
        if !expected.contains_key(concept_id) {
            issues.push(error_issue(
                "orphan_index_row",
                format!("derived search row for missing concept {concept_id} is orphaned"),
            ));
        }
    }
    Ok(())
}

fn index_content_hash(id: i64, label: &str, path: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(id.to_le_bytes());
    digest.update(label.as_bytes());
    digest.update([0]);
    digest.update(path.as_bytes());
    digest.update(index::INDEXER_VERSION.to_le_bytes());
    format!("{:x}", digest.finalize())
}

fn report(issues: Vec<ValidationIssue>) -> ValidationReport {
    ValidationReport {
        valid: !issues
            .iter()
            .any(|issue| issue.severity == ValidationSeverity::Error),
        issues,
    }
}

fn error_issue(code: &str, message: impl Into<String>) -> ValidationIssue {
    ValidationIssue {
        severity: ValidationSeverity::Error,
        code: code.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use serde_json::json;

    use super::validate;
    use crate::corpus::{self, ReconciliationRecord};
    use crate::index;
    use crate::resolver;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn initialized_connection() -> Result<Connection, Box<dyn std::error::Error>> {
        let mut connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.execute_batch(include_str!("../schema.sql"))?;
        let transaction = connection.transaction()?;
        index::rebuild_all(&transaction)?;
        transaction.commit()?;
        Ok(connection)
    }

    fn applied_library() -> Result<Connection, Box<dyn std::error::Error>> {
        let mut connection = initialized_connection()?;
        let work = corpus::store_work(
            &mut connection,
            "Transactions",
            "Predicate locks prevent phantom rows.",
        )?;
        let reconciliation = resolver::submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Add predicate locking",
                "operations": [{
                    "action": "create_concept",
                    "label": "Predicate locking",
                    "evidence": [{
                        "quote": "Predicate locks prevent phantom rows."
                    }]
                }]
            }),
            "human",
            None,
        )?;
        assert_eq!(resolver::apply_record(&mut connection, &reconciliation)?, 1);
        Ok(connection)
    }

    fn record_model_reconciliation(
        connection: &mut Connection,
        run_work_id: i64,
        reconciliation_work: &crate::corpus::Work,
    ) -> Result<ReconciliationRecord, Box<dyn std::error::Error>> {
        connection.execute(
            "INSERT INTO model_runs(\
                 token, work_id, base_revision, status, model, reasoning_effort, prompt_version, \
                 created_at\
             ) VALUES('run-token', ?1, 0, 'submitted', 'test-model', 'medium', 'test', ?2)",
            rusqlite::params![run_work_id, corpus::now()?],
        )?;
        let run_id = connection.last_insert_rowid();
        let request = json!({
            "summary": "Represent the source claim",
            "operations": [{
                "action": "create_concept",
                "label": "Source claim",
                "evidence": [{"quote": reconciliation_work.text}]
            }]
        });
        let reconciliation = resolver::submit_value(
            connection,
            reconciliation_work,
            0,
            request.clone(),
            "model",
            Some(run_id),
        )?;
        let mut submitted_arguments = request;
        submitted_arguments["annotations"] = json!([]);
        connection.execute(
            "INSERT INTO tool_calls(\
                 model_run_id, sequence, tool_name, arguments, result, succeeded, created_at\
             ) VALUES(?1, 0, 'submit_reconciliation', ?2, '{}', 1, ?3)",
            rusqlite::params![
                run_id,
                serde_json::to_string(&submitted_arguments)?,
                corpus::now()?
            ],
        )?;
        Ok(reconciliation)
    }

    fn has_issue(report: &crate::model::ValidationReport, code: &str) -> bool {
        report.issues.iter().any(|issue| issue.code == code)
    }

    #[test]
    fn a_new_empty_library_is_valid_after_index_initialization() -> TestResult {
        let connection = initialized_connection()?;

        let changes_before = connection.total_changes();
        let report = validate(&connection)?;
        assert!(report.valid, "{:?}", report.issues);
        assert_eq!(connection.total_changes(), changes_before);
        Ok(())
    }

    #[test]
    fn an_applied_reconciliation_has_consistent_commit_provenance() -> TestResult {
        let connection = applied_library()?;
        let report = validate(&connection)?;
        assert!(report.valid, "{:?}", report.issues);
        Ok(())
    }

    #[test]
    fn validation_detects_a_commit_that_no_longer_matches_its_reconciliation() -> TestResult {
        let connection = applied_library()?;
        connection.execute(
            "UPDATE commits SET summary = 'Tampered summary' WHERE revision = 1",
            [],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "commit_reconciliation_mismatch"));
        Ok(())
    }

    #[test]
    fn validation_detects_tampered_revert_operations() -> TestResult {
        let mut connection = applied_library()?;
        let transaction = connection.transaction()?;
        assert_eq!(corpus::revert(&transaction, 1)?, 2);
        transaction.commit()?;
        let report = validate(&connection)?;
        assert!(report.valid, "{:?}", report.issues);

        connection.execute(
            "UPDATE commits SET resolved_operations = '[]' WHERE revision = 2",
            [],
        )?;
        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "invalid_revert_provenance"));
        Ok(())
    }

    #[test]
    fn validation_detects_a_reconciliation_payload_mismatch() -> TestResult {
        let connection = applied_library()?;
        connection.execute(
            "UPDATE reconciliations SET summary = 'Tampered summary' WHERE status = 'applied'",
            [],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "reconciliation_request_mismatch"));
        Ok(())
    }

    #[test]
    fn a_recorded_reconciliation_preserves_its_base_without_a_commit() -> TestResult {
        let mut connection = applied_library()?;
        let work = corpus::get_work(&connection, "Transactions")?;
        let reconciliation = resolver::submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Reconcile evidence already attached",
                "operations": [{
                    "action": "add_evidence",
                    "concept": {"path": ["Predicate locking"]},
                    "evidence": [{"quote": "Predicate locks prevent phantom rows."}]
                }],
                "annotations": ["This note has no application semantics."]
            }),
            "human",
            None,
        )?;

        assert_eq!(reconciliation.status, "recorded");
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM commits", [], |row| row
                .get::<_, i64>(0))?,
            1
        );
        let report = validate(&connection)?;
        assert!(report.valid, "{:?}", report.issues);
        Ok(())
    }

    #[test]
    fn validation_replays_a_recorded_reconciliation_request() -> TestResult {
        let mut connection = applied_library()?;
        let work = corpus::get_work(&connection, "Transactions")?;
        resolver::submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Reconcile evidence already attached",
                "operations": [{
                    "action": "add_evidence",
                    "concept": {"path": ["Predicate locking"]},
                    "evidence": [{"quote": "Predicate locks prevent phantom rows."}]
                }]
            }),
            "human",
            None,
        )?;
        connection.execute(
            "UPDATE reconciliations SET submitted_request = json_set(\
                 submitted_request, '$.operations[0].concept.path[0]', 'Missing') \
             WHERE status = 'recorded'",
            [],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "reconciliation_resolution_mismatch"));
        Ok(())
    }

    #[test]
    fn validation_detects_a_recorded_projection_with_a_nonrecorded_status() -> TestResult {
        let mut connection = applied_library()?;
        let work = corpus::get_work(&connection, "Transactions")?;
        resolver::submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Reconcile evidence already attached",
                "operations": [{
                    "action": "add_evidence",
                    "concept": {"path": ["Predicate locking"]},
                    "evidence": [{"quote": "Predicate locks prevent phantom rows."}]
                }]
            }),
            "human",
            None,
        )?;
        connection.execute(
            "UPDATE reconciliations SET status = 'pending' WHERE status = 'recorded'",
            [],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "reconciliation_resolution_mismatch"));
        Ok(())
    }

    #[test]
    fn validation_detects_a_submitted_run_without_a_submission() -> TestResult {
        let mut connection = initialized_connection()?;
        let work = corpus::store_work(&mut connection, "Paper", "Some source text.")?;
        connection.execute(
            "INSERT INTO model_runs(\
                 token, work_id, base_revision, status, model, reasoning_effort, prompt_version, \
                 created_at\
             ) VALUES('orphan-run', ?1, 0, 'submitted', 'test-model', 'medium', 'test', ?2)",
            rusqlite::params![work.id, corpus::now()?],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "model_run_submission_mismatch"));
        Ok(())
    }

    #[test]
    fn validation_detects_a_model_reconciliation_for_the_wrong_work() -> TestResult {
        let mut connection = initialized_connection()?;
        let examined = corpus::store_work(&mut connection, "Examined", "Examined source.")?;
        let submitted = corpus::store_work(&mut connection, "Submitted", "Submitted source.")?;
        record_model_reconciliation(&mut connection, examined.id, &submitted)?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "model_run_reconciliation_mismatch"));
        Ok(())
    }

    #[test]
    fn a_matching_model_submission_is_valid() -> TestResult {
        let mut connection = initialized_connection()?;
        let work = corpus::store_work(&mut connection, "Paper", "Some source text.")?;
        record_model_reconciliation(&mut connection, work.id, &work)?;

        let report = validate(&connection)?;
        assert!(report.valid, "{:?}", report.issues);
        Ok(())
    }
}
