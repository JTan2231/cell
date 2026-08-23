use std::collections::BTreeMap;

use rusqlite::Connection;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::change::{ChangeOperation, Reconciliation, parse_reconciliation};
use crate::corpus::{self, Snapshot};
use crate::error::AppError;
use crate::model::{DiffEntry, ValidationIssue, ValidationReport};
use crate::resolver::{ResolvedOperation, ResolvedReconciliation, snapshots_corpus_equal};
use crate::revision_store;

#[derive(Debug)]
struct StoredCommit {
    revision: i64,
    work_id: Option<i64>,
    reconciliation_id: Option<i64>,
    kind: String,
    summary: String,
    submitted_request: String,
    resolved_operations: String,
    after_snapshot: String,
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

/// Validate the authoritative corpus and its append-only history.
///
/// Validation is deliberately read-only. Every detected invariant violation is returned in
/// the report; no state is repaired here.
pub fn validate(connection: &Connection) -> Result<ValidationReport, AppError> {
    let mut issues = Vec::new();

    check_sqlite_integrity(connection, &mut issues)?;
    check_foreign_keys(connection, &mut issues)?;
    check_work_hashes(connection, &mut issues)?;
    check_ingestions(connection, &mut issues)?;

    let head_revision = load_head_revision(connection, &mut issues)?;
    let commits = load_commits(connection)?;
    let history_head = check_history(connection, head_revision, &commits, &mut issues);
    check_revision_projections(connection, &commits, &mut issues);
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
    }

    Ok(report(issues))
}

fn check_revision_projections(
    connection: &Connection,
    commits: &[StoredCommit],
    issues: &mut Vec<ValidationIssue>,
) {
    for commit in commits {
        let Ok(expected) = serde_json::from_str::<Snapshot>(&commit.after_snapshot) else {
            continue;
        };
        match revision_store::load_revision_snapshot(connection, commit.revision) {
            Ok(Some(actual)) => {
                if actual != expected {
                    issues.push(error_issue(
                        "revision_projection_mismatch",
                        format!(
                            "relational graph revision {} does not match its committed after-state",
                            commit.revision
                        ),
                    ));
                }
                if let Err(error) = corpus::validate_snapshot(connection, &actual) {
                    issues.push(error_issue(
                        "invalid_revision_projection",
                        format!(
                            "relational graph revision {} violates corpus invariants: {error}",
                            commit.revision
                        ),
                    ));
                }
            }
            Ok(None) => issues.push(error_issue(
                "revision_projection_missing",
                format!(
                    "committed revision {} has no relational graph projection",
                    commit.revision
                ),
            )),
            Err(error) => issues.push(error_issue(
                "invalid_revision_projection",
                format!(
                    "relational graph revision {} could not be read: {error}",
                    commit.revision
                ),
            )),
        }
    }
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

fn check_ingestions(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection.prepare(
        "SELECT id, source_created_at, source_modified_at, first_seen_at, ingested_at, \
                completed_at, work_id, new_work, error_code, error_message \
         FROM ingestions ORDER BY id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<bool>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
        ))
    })?;
    let mut first_new_delivery = BTreeMap::new();
    for row in rows {
        let (
            id,
            source_created_at,
            source_modified_at,
            first_seen_at,
            ingested_at,
            completed_at,
            work_id,
            new_work,
            error_code,
            error_message,
        ) = row?;
        let _source_created_at = parse_ingestion_timestamp(
            id,
            "source_created_at",
            source_created_at.as_deref(),
            issues,
        );
        let _source_modified_at = parse_ingestion_timestamp(
            id,
            "source_modified_at",
            source_modified_at.as_deref(),
            issues,
        );
        let first_seen_at =
            parse_ingestion_timestamp(id, "first_seen_at", Some(&first_seen_at), issues);
        let ingested_at =
            parse_ingestion_timestamp(id, "ingested_at", ingested_at.as_deref(), issues);
        let completed_at =
            parse_ingestion_timestamp(id, "completed_at", completed_at.as_deref(), issues);

        if let (Some(first_seen_at), Some(ingested_at)) = (first_seen_at, ingested_at)
            && ingested_at < first_seen_at
        {
            issues.push(error_issue(
                "invalid_ingestion_time_order",
                format!("source delivery {id} was ingested before it was first seen"),
            ));
        }
        if let (Some(first_seen_at), Some(completed_at)) = (first_seen_at, completed_at)
            && completed_at < first_seen_at
        {
            issues.push(error_issue(
                "invalid_ingestion_time_order",
                format!("source delivery {id} completed before it was first seen"),
            ));
        }
        if let (Some(ingested_at), Some(completed_at)) = (ingested_at, completed_at)
            && completed_at < ingested_at
        {
            issues.push(error_issue(
                "invalid_ingestion_time_order",
                format!("source delivery {id} completed before it was ingested"),
            ));
        }
        if error_code.is_some() != error_message.is_some() {
            issues.push(error_issue(
                "invalid_ingestion_error",
                format!("source delivery {id} must store its error code and message together"),
            ));
        }
        if new_work == Some(true)
            && let Some(work_id) = work_id
            && let Some(previous) = first_new_delivery.insert(work_id, id)
        {
            issues.push(error_issue(
                "duplicate_new_work_ingestion",
                format!(
                    "source deliveries {previous} and {id} both claim to have created work {work_id}"
                ),
            ));
        }
    }
    Ok(())
}

fn parse_ingestion_timestamp(
    ingestion_id: i64,
    field: &str,
    value: Option<&str>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<OffsetDateTime> {
    let value = value?;
    match OffsetDateTime::parse(value, &Rfc3339) {
        Ok(timestamp) => Some(timestamp),
        Err(error) => {
            issues.push(error_issue(
                "invalid_ingestion_timestamp",
                format!(
                    "source delivery {ingestion_id} has invalid {field} timestamp {value:?}: {error}"
                ),
            ));
            None
        }
    }
}

fn load_head_revision(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<Option<i64>, AppError> {
    let mut statement = connection
        .prepare("SELECT singleton, revision, library_id FROM library_state ORDER BY singleton")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    match rows.as_slice() {
        [(1, revision, library_id)] if valid_library_id(library_id) => Ok(Some(*revision)),
        [] => {
            issues.push(error_issue(
                "library_state_missing",
                "the library has no HEAD revision record",
            ));
            Ok(None)
        }
        [(1, revision, _)] => {
            issues.push(error_issue(
                "invalid_library_id",
                "the library identity must be 32 lowercase hexadecimal characters",
            ));
            Ok(Some(*revision))
        }
        _ => {
            issues.push(error_issue(
                "invalid_library_state",
                "the library must contain exactly one singleton HEAD revision record",
            ));
            Ok(rows
                .iter()
                .find_map(|(singleton, revision, _)| (*singleton == 1).then_some(*revision)))
        }
    }
}

fn valid_library_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn load_commits(connection: &Connection) -> Result<Vec<StoredCommit>, AppError> {
    let mut statement = connection.prepare(
        "SELECT revision, work_id, reconciliation_id, kind, summary, submitted_request, \
                resolved_operations, after_snapshot, actor \
         FROM commits ORDER BY revision",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(StoredCommit {
            revision: row.get(0)?,
            work_id: row.get(1)?,
            reconciliation_id: row.get(2)?,
            kind: row.get(3)?,
            summary: row.get(4)?,
            submitted_request: row.get(5)?,
            resolved_operations: row.get(6)?,
            after_snapshot: row.get(7)?,
            actor: row.get(8)?,
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
    if corpus::snapshot_at(connection, reconciliation.base_revision).is_ok() {
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
                    ChangeOperation::AddParent { .. },
                    ResolvedOperation::AddParent { .. }
                ) | (
                    ChangeOperation::RemoveParent { .. },
                    ResolvedOperation::RemoveParent { .. }
                ) | (
                    ChangeOperation::AddEvidence { .. },
                    ResolvedOperation::AddEvidence { .. }
                ) | (
                    ChangeOperation::RemoveEvidence { .. },
                    ResolvedOperation::RemoveEvidence { .. }
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
    if let (Ok(before), Ok(after)) = (
        corpus::snapshot_at(connection, commit.revision - 1),
        serde_json::from_str::<Snapshot>(&commit.after_snapshot),
    ) && snapshots_corpus_equal(&before, &after)
    {
        issues.push(error_issue(
            "empty_commit",
            format!("revision {} does not change the corpus", commit.revision),
        ));
    }
    match commit.kind.as_str() {
        "change" => check_reconciliation_commit(commit, reconciliations, issues),
        "revert" => check_revert_commit(connection, commit, commits, issues),
        "shake" => check_shake_commit(connection, commit, issues),
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
    if reconciliation.status != "applied"
        || reconciliation.applied_revision != Some(commit.revision)
        || commit.work_id != Some(reconciliation.work_id)
        || commit.revision - 1 != reconciliation.base_revision
        || commit.summary != reconciliation.summary
        || commit.actor != reconciliation.actor
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
    let target = json_integer(&commit.submitted_request, "revert_revision");
    let target_commit = target.and_then(|target| {
        commits
            .iter()
            .find(|candidate| candidate.revision == target && target < commit.revision)
    });
    let resolved_matches = revert_operations_match(connection, commit);
    let projection_matches =
        target.is_some_and(|target| revert_projection_matches(connection, commit, target));
    if commit.reconciliation_id.is_some()
        || target_commit.is_none()
        || target_commit.is_some_and(|target| target.work_id != commit.work_id)
        || target.is_some_and(|target| commit.summary != format!("Revert revision {target}"))
        || !resolved_matches
        || !projection_matches
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

fn check_shake_commit(
    connection: &Connection,
    commit: &StoredCommit,
    issues: &mut Vec<ValidationIssue>,
) {
    let before = corpus::snapshot_at(connection, commit.revision - 1);
    let actual = serde_json::from_str::<Snapshot>(&commit.after_snapshot);
    let reduction = before.as_ref().ok().and_then(|snapshot| {
        corpus::transitive_reduction(snapshot)
            .ok()
            .map(|(after, removed)| (after, removed.len()))
    });
    let expected_operations = match (before.as_ref(), reduction.as_ref()) {
        (Ok(before), Some((after, _))) => {
            corpus::diff_snapshot_entries(connection, before, after).ok()
        }
        _ => None,
    };
    let stored_operations =
        serde_json::from_str::<Vec<DiffEntry>>(&commit.resolved_operations).ok();
    let request = serde_json::from_str::<serde_json::Value>(&commit.submitted_request).ok();
    let removed_count = reduction.as_ref().map(|(_, removed)| *removed);
    let valid = commit.work_id.is_none()
        && commit.reconciliation_id.is_none()
        && commit.actor == "human"
        && request == Some(serde_json::json!({ "operation": "transitive_reduction" }))
        && removed_count.is_some_and(|count| count > 0)
        && removed_count.is_some_and(|count| commit.summary == corpus::shake_summary(count))
        && reduction
            .as_ref()
            .zip(actual.as_ref().ok())
            .is_some_and(|((expected, _), actual)| expected == actual)
        && expected_operations.is_some()
        && expected_operations == stored_operations;
    if !valid {
        issues.push(error_issue(
            "invalid_shake_provenance",
            format!(
                "shake revision {} does not exactly preserve its transitive-reduction plan",
                commit.revision
            ),
        ));
    }
}

fn revert_projection_matches(
    connection: &Connection,
    commit: &StoredCommit,
    target_revision: i64,
) -> bool {
    let snapshots = (
        corpus::snapshot_at(connection, target_revision.saturating_sub(1)),
        corpus::snapshot_at(connection, target_revision),
        corpus::snapshot_at(connection, commit.revision - 1),
        serde_json::from_str::<Snapshot>(&commit.after_snapshot),
    );
    let (Ok(target_before), Ok(target_after), Ok(head_before), Ok(actual)) = snapshots else {
        return false;
    };
    corpus::invert_snapshot_change(target_revision, &target_before, &target_after, &head_before)
        .is_ok_and(|expected| expected == actual)
}

fn revert_operations_match(connection: &Connection, commit: &StoredCommit) -> bool {
    let Ok(before) = corpus::snapshot_at(connection, commit.revision - 1) else {
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

/// Check the linear after-state log and return the parsed snapshot at HEAD, when available.
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
        let after = parse_snapshot(&commit.after_snapshot, commit.revision, "after", issues);
        if let Some(snapshot) = &after {
            validate_historical_snapshot(connection, snapshot, commit.revision, "after", issues);
        }
        if head_revision == Some(commit.revision) {
            head_after.clone_from(&after);
        }
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

fn report(issues: Vec<ValidationIssue>) -> ValidationReport {
    ValidationReport {
        valid: issues.is_empty(),
        issues,
    }
}

fn error_issue(code: &str, message: impl Into<String>) -> ValidationIssue {
    ValidationIssue {
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
    use crate::resolver;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn initialized_connection() -> Result<Connection, Box<dyn std::error::Error>> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.execute_batch(include_str!("../schema.sql"))?;
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
                    "ref": "predicate_locking",
                    "label": "Predicate locking",
                    "parents": [],
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

    fn shaken_library() -> Result<Connection, Box<dyn std::error::Error>> {
        let mut connection = initialized_connection()?;
        let work = corpus::store_work(
            &mut connection,
            "Hierarchy",
            "Broad scope. Middle scope. Narrow claim.",
        )?;
        let reconciliation = resolver::submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Add a complete three-concept order",
                "operations": [
                    {
                        "action": "create_concept",
                        "ref": "broad",
                        "label": "Broad",
                        "parents": [],
                        "evidence": [{"quote": "Broad scope."}]
                    },
                    {
                        "action": "create_concept",
                        "ref": "middle",
                        "label": "Middle",
                        "parents": [{"new": "broad"}],
                        "evidence": [{"quote": "Middle scope."}]
                    },
                    {
                        "action": "create_concept",
                        "ref": "narrow",
                        "label": "Narrow",
                        "parents": [{"new": "broad"}, {"new": "middle"}],
                        "evidence": [{"quote": "Narrow claim."}]
                    }
                ]
            }),
            "human",
            None,
        )?;
        assert_eq!(resolver::apply_record(&mut connection, &reconciliation)?, 1);
        let plan = corpus::plan_shake(&connection)?;
        assert_eq!(corpus::apply_shake(&mut connection, &plan)?, 2);
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
                "ref": "source_claim",
                "label": "Source claim",
                "parents": [],
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
    fn a_new_empty_library_is_valid() -> TestResult {
        let connection = initialized_connection()?;

        let changes_before = connection.total_changes();
        let report = validate(&connection)?;
        assert!(report.valid, "{:?}", report.issues);
        assert_eq!(connection.total_changes(), changes_before);
        Ok(())
    }

    #[test]
    fn validation_checks_ingestion_timestamps_and_lifecycle_order() -> TestResult {
        let mut connection = initialized_connection()?;
        let work = corpus::store_work(&mut connection, "Delivered", "Source text.")?;
        connection.execute(
            "INSERT INTO ingestions(\
                 source_name, channel, source_created_at, source_modified_at, first_seen_at, \
                 ingested_at, completed_at, status, work_id, new_work, result\
             ) VALUES(\
                 'source.txt', 'manual', 'not-a-timestamp', '2026-08-20T12:00:00Z', \
                 '2026-08-20T12:00:03Z', '2026-08-20T12:00:02Z', \
                 '2026-08-20T12:00:01Z', 'completed', ?1, 1, 'retained'\
             )",
            [work.id],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "invalid_ingestion_timestamp"));
        assert!(has_issue(&report, "invalid_ingestion_time_order"));
        Ok(())
    }

    #[test]
    fn validation_checks_ingestion_errors_and_new_work_uniqueness() -> TestResult {
        let mut connection = initialized_connection()?;
        let work = corpus::store_work(&mut connection, "Delivered", "Source text.")?;
        connection.execute("DROP INDEX ingestions_one_new_work_per_work", [])?;
        connection.pragma_update(None, "ignore_check_constraints", true)?;
        connection.execute(
            "INSERT INTO ingestions(\
                 source_name, channel, first_seen_at, ingested_at, status, work_id, new_work, \
                 error_code\
             ) VALUES(\
                 'first.txt', 'manual', '2026-08-20T12:00:00Z', \
                 '2026-08-20T12:00:01Z', 'processing', ?1, 1, 'retryable'\
             )",
            [work.id],
        )?;
        connection.execute(
            "INSERT INTO ingestions(\
                 source_name, channel, first_seen_at, ingested_at, status, work_id, new_work\
             ) VALUES(\
                 'second.txt', 'manual', '2026-08-20T12:00:00Z', \
                 '2026-08-20T12:00:01Z', 'processing', ?1, 1\
             )",
            [work.id],
        )?;
        connection.pragma_update(None, "ignore_check_constraints", false)?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "invalid_ingestion_error"));
        assert!(has_issue(&report, "duplicate_new_work_ingestion"));
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
    fn validation_detects_a_tampered_relational_revision() -> TestResult {
        let connection = applied_library()?;
        connection.execute("DROP TRIGGER revision_concepts_immutable_update", [])?;
        connection.execute(
            "UPDATE revision_concepts SET label = 'Tampered', normalized_label = 'tampered' \
             WHERE revision = 1 AND concept_id = (\
                 SELECT MIN(concept_id) FROM revision_concepts WHERE revision = 1\
             )",
            [],
        )?;
        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "revision_projection_mismatch"));
        Ok(())
    }

    #[test]
    fn validation_replays_shakes_and_detects_tampered_provenance() -> TestResult {
        let connection = shaken_library()?;
        let report = validate(&connection)?;
        assert!(report.valid, "{:?}", report.issues);

        connection.execute(
            "UPDATE commits SET summary = 'Tampered summary' WHERE revision = 2",
            [],
        )?;
        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "invalid_shake_provenance"));
        Ok(())
    }

    #[test]
    fn validation_accepts_shared_dag_ancestry() -> TestResult {
        let mut connection = initialized_connection()?;
        let work = corpus::store_work(
            &mut connection,
            "Graph paper",
            "Alpha scope. Beta scope. Shared claim.",
        )?;
        let reconciliation = resolver::submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Create a shared concept",
                "operations": [
                    {
                        "action": "create_concept",
                        "ref": "alpha",
                        "label": "Alpha",
                        "parents": [],
                        "evidence": [{"quote": "Alpha scope."}]
                    },
                    {
                        "action": "create_concept",
                        "ref": "beta",
                        "label": "Beta",
                        "parents": [],
                        "evidence": [{"quote": "Beta scope."}]
                    },
                    {
                        "action": "create_concept",
                        "ref": "shared",
                        "label": "Shared",
                        "parents": [{"new": "alpha"}, {"new": "beta"}],
                        "evidence": [{"quote": "Shared claim."}]
                    }
                ]
            }),
            "human",
            None,
        )?;
        resolver::apply_record(&mut connection, &reconciliation)?;

        let report = validate(&connection)?;
        assert!(report.valid, "{:?}", report.issues);
        Ok(())
    }

    #[test]
    fn validation_reports_a_cycle_in_a_historical_after_snapshot() -> TestResult {
        let connection = applied_library()?;
        let snapshot = json!({
            "concepts": [
                {"id": 1, "label": "Predicate locking"},
                {"id": 2, "label": "Cycle peer"}
            ],
            "edges": [
                {"parent_id": 1, "child_id": 2},
                {"parent_id": 2, "child_id": 1}
            ],
            "evidence": [
                {"concept_id": 1, "work_id": 1, "start_byte": 0, "end_byte": 37}
            ]
        });
        connection.execute(
            "UPDATE commits SET after_snapshot = ?1 WHERE revision = 1",
            [serde_json::to_string(&snapshot)?],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "invalid_historical_snapshot"));
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
    fn validation_detects_a_revert_that_does_not_apply_the_inverse() -> TestResult {
        let mut connection = applied_library()?;
        let transaction = connection.transaction()?;
        assert_eq!(corpus::revert(&transaction, 1)?, 2);
        transaction.commit()?;

        let non_inverse = corpus::snapshot_at(&connection, 1)?;
        connection.execute(
            "UPDATE commits SET after_snapshot = ?1, resolved_operations = '[]' \
             WHERE revision = 2",
            [serde_json::to_string(&non_inverse)?],
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
                    "concept": {"id": "c1"},
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
                    "concept": {"id": "c1"},
                    "evidence": [{"quote": "Predicate locks prevent phantom rows."}]
                }]
            }),
            "human",
            None,
        )?;
        connection.execute(
            "UPDATE reconciliations SET submitted_request = json_set(\
                 submitted_request, '$.operations[0].concept.id', 'c999') \
             WHERE status = 'recorded'",
            [],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "reconciliation_resolution_mismatch"));
        Ok(())
    }

    #[test]
    fn validation_replays_an_applied_reconciliation_request() -> TestResult {
        let connection = applied_library()?;
        connection.execute(
            "UPDATE reconciliations SET resolved_reconciliation = json_set(\
                 resolved_reconciliation, '$.operations[0].concept.label', 'Tampered') \
             WHERE status = 'applied'",
            [],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(has_issue(&report, "reconciliation_resolution_mismatch"));
        Ok(())
    }

    #[test]
    fn validation_replays_pending_and_superseded_reconciliations() -> TestResult {
        let mut connection = applied_library()?;
        let work = corpus::get_work(&connection, "Transactions")?;
        resolver::submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Add another grounded concept",
                "operations": [{
                    "action": "create_concept",
                    "ref": "another_claim",
                    "label": "Another claim",
                    "parents": [],
                    "evidence": [{"quote": "Predicate locks prevent phantom rows."}]
                }]
            }),
            "human",
            None,
        )?;
        connection.execute(
            "UPDATE reconciliations SET resolved_reconciliation = json_set(\
                 resolved_reconciliation, '$.operations[0].concept.label', 'Tampered') \
             WHERE status = 'pending'",
            [],
        )?;

        let pending_report = validate(&connection)?;
        assert!(!pending_report.valid);
        assert!(has_issue(
            &pending_report,
            "reconciliation_resolution_mismatch"
        ));

        connection.execute(
            "UPDATE reconciliations SET status = 'superseded' WHERE status = 'pending'",
            [],
        )?;
        let superseded_report = validate(&connection)?;
        assert!(!superseded_report.valid);
        assert!(has_issue(
            &superseded_report,
            "reconciliation_resolution_mismatch"
        ));
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
                    "concept": {"id": "c1"},
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
