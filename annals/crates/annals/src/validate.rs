use rusqlite::Connection;

use crate::change;
use crate::corpus::{self, CorpusState};
use crate::error::AppError;
use crate::model::{ValidationIssue, ValidationReport};
use crate::resolver;
use crate::revision_store;

#[derive(Debug)]
struct StoredCommit {
    revision: i64,
    kind: String,
    reconciliation_id: Option<i64>,
    reverted_revision: Option<i64>,
    actor: String,
}

#[derive(Debug)]
struct StoredRetryEvent {
    id: i64,
    from_job_id: String,
    through_job_id: String,
    state: String,
    active_slot: i64,
    created_at: String,
    ready_at: Option<String>,
    completed_at: Option<String>,
    last_halted_at: Option<String>,
    last_halt_code: Option<String>,
    last_halt_message: Option<String>,
}

#[derive(Debug)]
struct StoredRetryItem {
    ordinal: i64,
    original_job_id: String,
    original_sequence: i64,
    original_ingestion_id: i64,
    original_completed_at: String,
    original_error_code: String,
    original_error_message: String,
    original_work_id: Option<i64>,
    child_job_id: Option<String>,
    child_sequence: Option<i64>,
    child_ingestion_id: Option<i64>,
    actual_original_id: Option<i64>,
    actual_original_delivery_key: Option<String>,
    actual_original_channel: Option<String>,
    actual_original_status: Option<String>,
    actual_original_completed_at: Option<String>,
    actual_original_error_code: Option<String>,
    actual_original_error_message: Option<String>,
    actual_original_work_id: Option<i64>,
    actual_child_id: Option<i64>,
    actual_child_delivery_key: Option<String>,
    actual_child_channel: Option<String>,
    actual_child_status: Option<String>,
    actual_child_result: Option<String>,
    actual_child_error_code: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetryOutcome {
    NotAttempted,
    Processing,
    Applied,
    Recorded,
    Failed,
    Skipped,
    Invalid,
}

/// Read-only, full replay validation of the library.
///
/// No materialized corpus is compared because none exists.  The validator
/// rebuilds every revision through the same `CorpusState` reducer used by
/// reads, pending application, diff, shake, and revert.
pub fn validate(connection: &Connection) -> Result<ValidationReport, AppError> {
    let mut issues = Vec::new();
    check_sqlite_integrity(connection, &mut issues)?;
    check_foreign_keys(connection, &mut issues)?;
    check_schema_boundary(connection, &mut issues)?;
    check_library_identity(connection, &mut issues)?;
    check_work_hashes(connection, &mut issues)?;
    check_tool_artifacts(connection, &mut issues)?;
    check_effect_identity_rules(connection, &mut issues)?;

    let commits = load_commits(connection)?;
    let states = replay_commits(connection, &commits, &mut issues);
    check_commit_provenance(connection, &commits, &states, &mut issues);
    check_reconciliations(connection, &commits, &mut issues)?;
    check_drafts_and_runs(connection, &mut issues)?;
    check_ingestions(connection, &mut issues)?;
    check_inbox_retries(connection, &mut issues)?;

    Ok(ValidationReport {
        valid: issues.is_empty(),
        issues,
    })
}

fn check_sqlite_integrity(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection.prepare("PRAGMA integrity_check")?;
    for row in statement.query_map([], |row| row.get::<_, String>(0))? {
        let result = row?;
        if result != "ok" {
            issue(
                issues,
                "sqlite_integrity",
                format!("SQLite integrity check reported: {result}"),
            );
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
        let (table, rowid, parent, constraint) = row?;
        issue(
            issues,
            "foreign_key_violation",
            format!("table {table} row {rowid:?} violates foreign key {constraint} to {parent}"),
        );
    }
    Ok(())
}

fn check_schema_boundary(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let version =
        connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?;
    if version != crate::db::CURRENT_SCHEMA_VERSION {
        issue(
            issues,
            "schema_version_mismatch",
            format!(
                "library schema version {version} is not {}",
                crate::db::CURRENT_SCHEMA_VERSION
            ),
        );
    }
    let forbidden_tables = [
        "concepts",
        "concept_edges",
        "evidence",
        "revision_snapshots",
        "revision_concepts",
        "revision_edges",
        "revision_evidence",
    ];
    for name in forbidden_tables {
        if connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
            [name],
            |row| row.get::<_, bool>(0),
        )? {
            issue(
                issues,
                "forbidden_materialized_corpus",
                format!("forbidden materialized corpus table {name:?} exists"),
            );
        }
    }
    for (table, column) in [
        ("commits", "after_snapshot"),
        ("commits", "submitted_request"),
        ("commits", "resolved_operations"),
        ("reconciliations", "submitted_request"),
        ("reconciliations", "resolved_reconciliation"),
        ("request_operations", "operation"),
    ] {
        let found = connection.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)"),
            [column],
            |row| row.get::<_, bool>(0),
        )?;
        if found {
            issue(
                issues,
                "forbidden_operational_json",
                format!("forbidden operational column {table}.{column} exists"),
            );
        }
    }
    Ok(())
}

fn check_library_identity(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let rows = connection.query_row(
        "SELECT COUNT(*), COALESCE(MIN(singleton), 0), COALESCE(MAX(singleton), 0),
                COALESCE(MIN(length(library_id)), 0), COALESCE(MAX(length(library_id)), 0)
         FROM library_identity",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        },
    )?;
    if rows != (1, 1, 1, 32, 32) {
        issue(
            issues,
            "invalid_library_identity",
            "the library must contain exactly one valid identity row",
        );
    }
    Ok(())
}

fn check_work_hashes(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection
        .prepare("SELECT id, label, normalized_label, text, sha256 FROM works ORDER BY id")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    for row in rows {
        let (id, label, normalized, text, digest) = row?;
        if crate::index::normalize(&label) != normalized {
            issue(
                issues,
                "work_label_normalization_mismatch",
                format!("work {id} has a noncanonical normalized label"),
            );
        }
        if corpus::sha256_hex(text.as_bytes()) != digest {
            issue(
                issues,
                "work_hash_mismatch",
                format!("immutable work {id} does not match its SHA-256 digest"),
            );
        }
    }
    Ok(())
}

fn check_tool_artifacts(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection.prepare(
        "SELECT model_run_id, sequence, arguments, arguments_sha256, result, result_sha256
         FROM tool_calls ORDER BY model_run_id, sequence",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;
    for row in rows {
        let (run, sequence, arguments, arguments_hash, result, result_hash) = row?;
        if corpus::sha256_hex(arguments.as_bytes()) != arguments_hash
            || corpus::sha256_hex(result.as_bytes()) != result_hash
        {
            issue(
                issues,
                "tool_artifact_hash_mismatch",
                format!("model run {run} tool call {sequence} has a changed audit artifact"),
            );
        }
    }
    Ok(())
}

fn check_effect_identity_rules(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut duplicates = connection.prepare(
        "SELECT concept_id, COUNT(*) FROM concept_effects
         WHERE effect = 'create' GROUP BY concept_id HAVING COUNT(*) <> 1",
    )?;
    for row in duplicates.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))? {
        let (id, count) = row?;
        issue(
            issues,
            "concept_identity_reused",
            format!("concept c{id} has {count} create effects"),
        );
    }
    for (table, label) in [
        ("concept_effects", "concept"),
        ("parent_edge_effects", "parent edge"),
        ("evidence_link_effects", "evidence link"),
    ] {
        let sql = format!(
            "SELECT revision FROM {table} GROUP BY revision
             HAVING MIN(ordinal) <> 0 OR MAX(ordinal) + 1 <> COUNT(*)"
        );
        let mut statement = connection.prepare(&sql)?;
        for row in statement.query_map([], |row| row.get::<_, i64>(0))? {
            issue(
                issues,
                "effect_ordinal_gap",
                format!("revision {} has a noncanonical {label} effect order", row?),
            );
        }
    }
    Ok(())
}

fn load_commits(connection: &Connection) -> Result<Vec<StoredCommit>, AppError> {
    let mut statement = connection.prepare(
        "SELECT revision, kind, reconciliation_id, reverted_revision, actor
         FROM commits ORDER BY revision",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(StoredCommit {
            revision: row.get(0)?,
            kind: row.get(1)?,
            reconciliation_id: row.get(2)?,
            reverted_revision: row.get(3)?,
            actor: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn replay_commits(
    connection: &Connection,
    commits: &[StoredCommit],
    issues: &mut Vec<ValidationIssue>,
) -> Vec<Option<CorpusState>> {
    let mut states = vec![Some(CorpusState::empty())];
    for (index, commit) in commits.iter().enumerate() {
        let expected_revision = i64::try_from(index + 1).unwrap_or(i64::MAX);
        if commit.revision != expected_revision {
            issue(
                issues,
                "commit_sequence_mismatch",
                format!(
                    "commit position {expected_revision} contains revision {}",
                    commit.revision
                ),
            );
        }
        let Some(before) = states.last().and_then(Option::as_ref) else {
            states.push(None);
            continue;
        };
        let effects = match revision_store::load_revision_effects(connection, commit.revision) {
            Ok(effects) => effects,
            Err(error) => {
                issue(
                    issues,
                    "invalid_commit_effect",
                    format!(
                        "revision {} effects cannot be read: {error}",
                        commit.revision
                    ),
                );
                states.push(None);
                continue;
            }
        };
        if effects.is_empty() {
            issue(
                issues,
                "empty_commit",
                format!("revision {} has no corpus effects", commit.revision),
            );
        }
        match before.reduced(&effects) {
            Ok(after) => {
                if revision_store::derive_effects(before, &after) != effects {
                    issue(
                        issues,
                        "noncanonical_commit_effects",
                        format!("revision {} effects are not canonical", commit.revision),
                    );
                }
                if let Err(error) = corpus::validate_snapshot(connection, &after) {
                    issue(
                        issues,
                        "invalid_replayed_corpus",
                        format!(
                            "revision {} violates corpus invariants: {error}",
                            commit.revision
                        ),
                    );
                }
                states.push(Some(after));
            }
            Err(error) => {
                issue(
                    issues,
                    "invalid_commit_effect",
                    format!("revision {} cannot be reduced: {error}", commit.revision),
                );
                states.push(None);
            }
        }
    }
    states
}

fn check_commit_provenance(
    connection: &Connection,
    commits: &[StoredCommit],
    states: &[Option<CorpusState>],
    issues: &mut Vec<ValidationIssue>,
) {
    for (index, commit) in commits.iter().enumerate() {
        let (Some(before), Some(after)) = (
            states.get(index).and_then(Option::as_ref),
            states.get(index + 1).and_then(Option::as_ref),
        ) else {
            continue;
        };
        match commit.kind.as_str() {
            "change" => check_change_commit(connection, commit, after, issues),
            "shake" => match corpus::transitive_reduction(before) {
                Ok((expected, removed)) if !removed.is_empty() && expected == *after => {}
                Ok(_) => issue(
                    issues,
                    "invalid_shake_commit",
                    format!(
                        "revision {} is not the transitive reduction of its parent",
                        commit.revision
                    ),
                ),
                Err(error) => issue(
                    issues,
                    "invalid_shake_commit",
                    format!(
                        "revision {} shake cannot be recomputed: {error}",
                        commit.revision
                    ),
                ),
            },
            "revert" => check_revert_commit(commit, commits, states, before, after, issues),
            other => issue(
                issues,
                "invalid_commit_kind",
                format!("revision {} has unknown kind {other:?}", commit.revision),
            ),
        }
    }
}

fn check_change_commit(
    connection: &Connection,
    commit: &StoredCommit,
    after: &CorpusState,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(id) = commit.reconciliation_id else {
        issue(
            issues,
            "invalid_change_commit",
            format!("revision {} has no reconciliation", commit.revision),
        );
        return;
    };
    let record = match corpus::reconciliation_by_id(connection, id) {
        Ok(record) => record,
        Err(error) => {
            issue(
                issues,
                "invalid_change_commit",
                format!(
                    "revision {} reconciliation cannot be read: {error}",
                    commit.revision
                ),
            );
            return;
        }
    };
    if record.status != "applied"
        || record.applied_revision != Some(commit.revision)
        || record.base_revision != commit.revision - 1
        || record.actor != commit.actor
    {
        issue(
            issues,
            "invalid_change_commit",
            format!(
                "revision {} reconciliation provenance is inconsistent",
                commit.revision
            ),
        );
    }
    match resolver::replay_record(connection, &record) {
        Ok(resolved) if resolved.resulting_snapshot == *after => {}
        Ok(_) => issue(
            issues,
            "reconciliation_replay_mismatch",
            format!(
                "revision {} does not match its typed reconciliation",
                commit.revision
            ),
        ),
        Err(error) => issue(
            issues,
            "invalid_reconciliation",
            format!(
                "revision {} reconciliation cannot be replayed: {error}",
                commit.revision
            ),
        ),
    }
}

fn check_revert_commit(
    commit: &StoredCommit,
    commits: &[StoredCommit],
    states: &[Option<CorpusState>],
    before: &CorpusState,
    after: &CorpusState,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(target) = commit.reverted_revision else {
        issue(
            issues,
            "invalid_revert_commit",
            format!("revision {} has no revert target", commit.revision),
        );
        return;
    };
    let target_index = match usize::try_from(target) {
        Ok(value) if target > 0 && value <= commits.len() && target < commit.revision => value,
        _ => {
            issue(
                issues,
                "invalid_revert_commit",
                format!(
                    "revision {} has invalid revert target {target}",
                    commit.revision
                ),
            );
            return;
        }
    };
    let (Some(target_before), Some(target_after)) = (
        states.get(target_index - 1).and_then(Option::as_ref),
        states.get(target_index).and_then(Option::as_ref),
    ) else {
        return;
    };
    match corpus::invert_snapshot_change(target, target_before, target_after, before) {
        Ok(expected) if expected == *after => {}
        Ok(_) => issue(
            issues,
            "invalid_revert_commit",
            format!(
                "revision {} is not the inverse of revision {target}",
                commit.revision
            ),
        ),
        Err(error) => issue(
            issues,
            "invalid_revert_commit",
            format!(
                "revision {} revert cannot be recomputed: {error}",
                commit.revision
            ),
        ),
    }
}

#[allow(clippy::too_many_lines)]
fn check_reconciliations(
    connection: &Connection,
    commits: &[StoredCommit],
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection.prepare("SELECT id FROM reconciliations ORDER BY id")?;
    let ids = statement
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let head = commits.last().map_or(0, |commit| commit.revision);
    for id in ids {
        let record = match corpus::reconciliation_by_id(connection, id) {
            Ok(record) => record,
            Err(error) => {
                issue(
                    issues,
                    "invalid_reconciliation",
                    format!("reconciliation {id} cannot be read: {error}"),
                );
                continue;
            }
        };
        if record.base_revision < 0 || record.base_revision > head {
            issue(
                issues,
                "invalid_reconciliation_base",
                format!(
                    "reconciliation {id} names missing revision {}",
                    record.base_revision
                ),
            );
            continue;
        }
        let request = match change::load_request(connection, record.request_id) {
            Ok(request) => request,
            Err(error) => {
                issue(
                    issues,
                    "invalid_reconciliation_request",
                    format!("reconciliation {id} request is invalid: {error}"),
                );
                continue;
            }
        };
        if request.summary() != record.summary {
            issue(
                issues,
                "reconciliation_metadata_mismatch",
                format!("reconciliation {id} summary is not derived consistently"),
            );
        }
        let base = match corpus::snapshot_at(connection, record.base_revision) {
            Ok(base) => base,
            Err(error) => {
                issue(
                    issues,
                    "invalid_reconciliation_base",
                    format!("reconciliation {id} base cannot be replayed: {error}"),
                );
                continue;
            }
        };
        let resolved = match resolver::replay_record(connection, &record) {
            Ok(resolved) => resolved,
            Err(error) => {
                issue(
                    issues,
                    "invalid_reconciliation",
                    format!("reconciliation {id} cannot be replayed: {error}"),
                );
                continue;
            }
        };
        let changed = !resolver::snapshots_corpus_equal(&base, &resolved.resulting_snapshot);
        match record.status.as_str() {
            "recorded" if changed || record.applied_revision.is_some() => issue(
                issues,
                "invalid_recorded_reconciliation",
                format!("recorded reconciliation {id} has corpus effects"),
            ),
            "pending" | "superseded" if !changed || record.applied_revision.is_some() => issue(
                issues,
                "invalid_unapplied_reconciliation",
                format!("reconciliation {id} has inconsistent unapplied status"),
            ),
            "applied" => {
                let linked = commits.iter().any(|commit| {
                    commit.reconciliation_id == Some(id)
                        && Some(commit.revision) == record.applied_revision
                });
                if !changed || !linked {
                    issue(
                        issues,
                        "invalid_applied_reconciliation",
                        format!("applied reconciliation {id} has no matching change commit"),
                    );
                }
            }
            "recorded" | "pending" | "superseded" => {}
            other => issue(
                issues,
                "invalid_reconciliation_status",
                format!("reconciliation {id} has unknown status {other:?}"),
            ),
        }
    }
    Ok(())
}

fn check_drafts_and_runs(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection.prepare(
        "SELECT d.id, d.model_run_id, d.request_id, d.status, d.version,
                q.work_id, q.base_revision, r.work_id, r.base_revision, r.status
         FROM reconciliation_drafts AS d
         JOIN reconciliation_requests AS q ON q.id = d.request_id
         JOIN model_runs AS r ON r.id = d.model_run_id
         ORDER BY d.id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, String>(9)?,
        ))
    })?;
    for row in rows {
        let (draft, run, request, status, version, work, base, run_work, run_base, run_status) =
            row?;
        if version < 1 || work != run_work || base != run_base {
            issue(
                issues,
                "invalid_reconciliation_draft",
                format!("draft {draft} does not match model run {run}"),
            );
        }
        let operations = match change::load_operations(connection, request, true) {
            Ok(operations) => operations,
            Err(error) => {
                issue(
                    issues,
                    "invalid_reconciliation_draft",
                    format!("draft {draft} operations cannot be read: {error}"),
                );
                continue;
            }
        };
        if status == "finalized" {
            if operations.iter().any(|operation| {
                operation.status != "dropped"
                    && (operation.status != "staged" || operation.operation.is_none())
            }) {
                issue(
                    issues,
                    "invalid_finalized_draft",
                    format!("finalized draft {draft} contains an incomplete active operation"),
                );
            }
            let linked = connection.query_row(
                "SELECT COUNT(*) FROM reconciliations
                 WHERE draft_id = ?1 AND request_id = ?2 AND model_run_id = ?3",
                rusqlite::params![draft, request, run],
                |row| row.get::<_, i64>(0),
            )?;
            if linked != 1 || run_status != "submitted" {
                issue(
                    issues,
                    "invalid_finalized_draft",
                    format!("finalized draft {draft} is not linked to one submitted run"),
                );
            }
        }
        if status == "open" && run_status != "running" {
            issue(
                issues,
                "invalid_open_draft",
                format!("open draft {draft} belongs to non-running model run {run}"),
            );
        }
    }

    let mut runs = connection.prepare("SELECT id, status FROM model_runs ORDER BY id")?;
    for row in runs.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })? {
        let (id, status) = row?;
        let reconciliations = connection.query_row(
            "SELECT COUNT(*) FROM reconciliations WHERE model_run_id = ?1",
            [id],
            |row| row.get::<_, i64>(0),
        )?;
        if (status == "submitted" && reconciliations != 1)
            || (status != "submitted" && reconciliations != 0)
        {
            issue(
                issues,
                "invalid_model_run_provenance",
                format!("model run {id} status {status:?} has {reconciliations} reconciliations"),
            );
        }
    }
    Ok(())
}

fn check_ingestions(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let invalid = connection.query_row(
        "SELECT COUNT(*) FROM ingestions
         WHERE (status = 'completed' AND (completed_at IS NULL OR result IS NULL OR work_id IS NULL))
            OR (status = 'processing' AND (completed_at IS NOT NULL OR result IS NOT NULL))
            OR (status = 'failed' AND (completed_at IS NULL OR error_code IS NULL OR error_message IS NULL))
            OR (result = 'applied' AND result_revision IS NULL)
            OR (result_revision IS NOT NULL AND result <> 'applied')",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if invalid != 0 {
        issue(
            issues,
            "invalid_source_delivery",
            format!("{invalid} source delivery rows violate lifecycle invariants"),
        );
    }
    Ok(())
}

fn check_inbox_retries(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let active = connection.query_row(
        "SELECT COUNT(*) FROM inbox_retry_events WHERE state <> 'completed'",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if active > 1 {
        issue(
            issues,
            "invalid_inbox_retry_event",
            format!("{active} inbox retry events are unfinished; at most one may be active"),
        );
    }

    for (column, label) in [
        ("original_ingestion_id", "original source delivery"),
        ("child_job_id", "retry child job"),
        ("child_ingestion_id", "retry child source delivery"),
    ] {
        let duplicates = connection.query_row(
            &format!(
                "SELECT COUNT(*) FROM (
                     SELECT {column} FROM inbox_retry_items
                     WHERE {column} IS NOT NULL
                     GROUP BY {column} HAVING COUNT(*) > 1
                 )"
            ),
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if duplicates != 0 {
            issue(
                issues,
                "invalid_inbox_retry_item",
                format!(
                    "{duplicates} duplicated {label} links violate direct-child retry provenance"
                ),
            );
        }
    }

    let mut statement = connection.prepare(
        "SELECT id, from_job_id, through_job_id, state, active_slot, created_at,
                ready_at, completed_at, last_halted_at, last_halt_code,
                last_halt_message
         FROM inbox_retry_events ORDER BY id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(StoredRetryEvent {
            id: row.get(0)?,
            from_job_id: row.get(1)?,
            through_job_id: row.get(2)?,
            state: row.get(3)?,
            active_slot: row.get(4)?,
            created_at: row.get(5)?,
            ready_at: row.get(6)?,
            completed_at: row.get(7)?,
            last_halted_at: row.get(8)?,
            last_halt_code: row.get(9)?,
            last_halt_message: row.get(10)?,
        })
    })?;
    let events = rows.collect::<Result<Vec<_>, _>>()?;
    for event in events {
        check_retry_event(connection, &event, issues)?;
    }
    Ok(())
}

fn check_retry_event(
    connection: &Connection,
    event: &StoredRetryEvent,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    if !retry_event_lifecycle_is_valid(event) {
        issue(
            issues,
            "invalid_inbox_retry_event",
            format!("inbox retry event {} has an incoherent lifecycle", event.id),
        );
    }

    let items = load_retry_items(connection, event.id)?;
    if items.is_empty() {
        issue(
            issues,
            "invalid_inbox_retry_event",
            format!("inbox retry event {} has no frozen retry items", event.id),
        );
        return Ok(());
    }

    let mut outcomes = Vec::with_capacity(items.len());
    for (position, item) in items.iter().enumerate() {
        let expected_ordinal = i64::try_from(position).unwrap_or(i64::MAX);
        if item.ordinal != expected_ordinal {
            issue(
                issues,
                "invalid_inbox_retry_event",
                format!(
                    "inbox retry event {} has a noncontiguous item ordinal at position {position}",
                    event.id
                ),
            );
        }
        check_retry_original(event.id, item, issues);
        let outcome = check_retry_child(event.id, item, issues);
        outcomes.push(outcome);
    }

    if items.first().map(|item| item.original_job_id.as_str()) != Some(event.from_job_id.as_str())
        || items.last().map(|item| item.original_job_id.as_str())
            != Some(event.through_job_id.as_str())
    {
        issue(
            issues,
            "invalid_inbox_retry_event",
            format!(
                "inbox retry event {} anchors do not match its first and last retry items",
                event.id
            ),
        );
    }

    if items.windows(2).any(|pair| {
        (
            &pair[0].original_completed_at,
            pair[0].original_ingestion_id,
        ) >= (
            &pair[1].original_completed_at,
            pair[1].original_ingestion_id,
        )
    }) {
        issue(
            issues,
            "invalid_inbox_retry_event",
            format!(
                "inbox retry event {} items are not in source-delivery failure order",
                event.id
            ),
        );
    }

    check_retry_membership(connection, event.id, &items, issues)?;

    let all_children_published = items.iter().all(|item| item.child_job_id.is_some());
    let any_child_delivery = items.iter().any(|item| item.child_ingestion_id.is_some());
    let any_processing = outcomes.contains(&RetryOutcome::Processing);
    let any_remaining = outcomes.iter().any(|outcome| {
        matches!(
            outcome,
            RetryOutcome::NotAttempted | RetryOutcome::Processing
        )
    });
    let state_matches_items = match event.state.as_str() {
        "preparing" => !any_child_delivery,
        "running" => all_children_published,
        "halted" => all_children_published && !any_processing,
        "completed" => all_children_published && !any_remaining,
        _ => false,
    };
    if !state_matches_items {
        issue(
            issues,
            "invalid_inbox_retry_event",
            format!(
                "inbox retry event {} state {:?} is inconsistent with its retry items",
                event.id, event.state
            ),
        );
    }
    Ok(())
}

fn retry_event_lifecycle_is_valid(event: &StoredRetryEvent) -> bool {
    let halt = match (
        event.last_halted_at.as_deref(),
        event.last_halt_code.as_deref(),
        event.last_halt_message.as_deref(),
    ) {
        (None, None, None) => Some(false),
        (Some(at), Some(code), Some(message))
            if !at.trim().is_empty() && !code.trim().is_empty() && !message.trim().is_empty() =>
        {
            Some(true)
        }
        _ => None,
    };
    if event.active_slot != 1 || event.created_at.trim().is_empty() || halt.is_none() {
        return false;
    }
    let ready = event
        .ready_at
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let completed = event
        .completed_at
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    match event.state.as_str() {
        "preparing" => {
            event.ready_at.is_none() && event.completed_at.is_none() && halt == Some(false)
        }
        "running" => ready && event.completed_at.is_none(),
        "halted" => ready && event.completed_at.is_none() && halt == Some(true),
        "completed" => ready && completed,
        _ => false,
    }
}

fn load_retry_items(
    connection: &Connection,
    event_id: i64,
) -> Result<Vec<StoredRetryItem>, AppError> {
    let mut statement = connection.prepare(
        "SELECT item.ordinal, item.original_job_id, item.original_sequence,
                item.original_ingestion_id, item.original_completed_at,
                item.original_error_code, item.original_error_message,
                item.original_work_id, item.child_job_id, item.child_sequence,
                item.child_ingestion_id,
                original.id, original.delivery_key, original.channel, original.status,
                original.completed_at, original.error_code, original.error_message,
                original.work_id,
                child.id, child.delivery_key, child.channel, child.status, child.result,
                child.error_code
         FROM inbox_retry_items AS item
         LEFT JOIN ingestions AS original ON original.id = item.original_ingestion_id
         LEFT JOIN ingestions AS child ON child.id = item.child_ingestion_id
         WHERE item.event_id = ?1
         ORDER BY item.ordinal",
    )?;
    let rows = statement.query_map([event_id], |row| {
        Ok(StoredRetryItem {
            ordinal: row.get(0)?,
            original_job_id: row.get(1)?,
            original_sequence: row.get(2)?,
            original_ingestion_id: row.get(3)?,
            original_completed_at: row.get(4)?,
            original_error_code: row.get(5)?,
            original_error_message: row.get(6)?,
            original_work_id: row.get(7)?,
            child_job_id: row.get(8)?,
            child_sequence: row.get(9)?,
            child_ingestion_id: row.get(10)?,
            actual_original_id: row.get(11)?,
            actual_original_delivery_key: row.get(12)?,
            actual_original_channel: row.get(13)?,
            actual_original_status: row.get(14)?,
            actual_original_completed_at: row.get(15)?,
            actual_original_error_code: row.get(16)?,
            actual_original_error_message: row.get(17)?,
            actual_original_work_id: row.get(18)?,
            actual_child_id: row.get(19)?,
            actual_child_delivery_key: row.get(20)?,
            actual_child_channel: row.get(21)?,
            actual_child_status: row.get(22)?,
            actual_child_result: row.get(23)?,
            actual_child_error_code: row.get(24)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn check_retry_original(event_id: i64, item: &StoredRetryItem, issues: &mut Vec<ValidationIssue>) {
    let sequence = retry_job_sequence(&item.original_job_id);
    let matches = item.actual_original_id == Some(item.original_ingestion_id)
        && item.actual_original_channel.as_deref() == Some("inbox")
        && item.actual_original_status.as_deref() == Some("failed")
        && item.actual_original_error_code.as_deref() != Some("inbox_job_skipped")
        && item.actual_original_completed_at.as_deref()
            == Some(item.original_completed_at.as_str())
        && item.actual_original_error_code.as_deref() == Some(item.original_error_code.as_str())
        && item.actual_original_error_message.as_deref()
            == Some(item.original_error_message.as_str())
        && item.original_work_id.is_some()
        && item.actual_original_work_id == item.original_work_id
        && sequence == Some(item.original_sequence)
        && item
            .actual_original_delivery_key
            .as_deref()
            .is_some_and(|key| delivery_key_matches_job(key, &item.original_job_id));
    if !matches {
        issue(
            issues,
            "invalid_inbox_retry_item",
            format!(
                "inbox retry event {event_id} item {} does not match its original failed source delivery",
                item.ordinal
            ),
        );
    }
}

fn check_retry_child(
    event_id: i64,
    item: &StoredRetryItem,
    issues: &mut Vec<ValidationIssue>,
) -> RetryOutcome {
    let child_job_is_valid = match (&item.child_job_id, item.child_sequence) {
        (None, None) => item.child_ingestion_id.is_none(),
        (Some(job_id), Some(sequence)) => {
            job_id != &item.original_job_id && retry_job_sequence(job_id) == Some(sequence)
        }
        _ => false,
    };
    let child_delivery_is_valid = match item.child_ingestion_id {
        None => item.actual_child_id.is_none(),
        Some(child_id) => {
            Some(child_id) == item.actual_child_id
                && Some(child_id) != Some(item.original_ingestion_id)
                && item.actual_child_channel.as_deref() == Some("inbox")
                && item.child_job_id.as_deref().is_some_and(|job_id| {
                    item.actual_child_delivery_key
                        .as_deref()
                        .is_some_and(|key| delivery_key_matches_job(key, job_id))
                })
        }
    };
    if !child_job_is_valid || !child_delivery_is_valid {
        issue(
            issues,
            "invalid_inbox_retry_item",
            format!(
                "inbox retry event {event_id} item {} has invalid retry child provenance",
                item.ordinal
            ),
        );
    }

    let outcome = match item.child_ingestion_id {
        None => RetryOutcome::NotAttempted,
        Some(_) if item.actual_child_id.is_none() => RetryOutcome::Invalid,
        Some(_) => match item.actual_child_status.as_deref() {
            Some("processing") => RetryOutcome::Processing,
            Some("completed") if item.actual_child_result.as_deref() == Some("applied") => {
                RetryOutcome::Applied
            }
            Some("completed") if item.actual_child_result.as_deref() == Some("recorded") => {
                RetryOutcome::Recorded
            }
            Some("failed")
                if item.actual_child_error_code.as_deref() == Some("inbox_job_skipped") =>
            {
                RetryOutcome::Skipped
            }
            Some("failed") => RetryOutcome::Failed,
            _ => RetryOutcome::Invalid,
        },
    };
    if outcome == RetryOutcome::Invalid {
        issue(
            issues,
            "invalid_inbox_retry_item",
            format!(
                "inbox retry event {event_id} item {} has an invalid retry child outcome",
                item.ordinal
            ),
        );
    }
    outcome
}

fn check_retry_membership(
    connection: &Connection,
    event_id: i64,
    items: &[StoredRetryItem],
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let Some(first) = items.first() else {
        return Ok(());
    };
    let Some(last) = items.last() else {
        return Ok(());
    };
    let mut statement = connection.prepare(
        "SELECT id FROM ingestions
         WHERE channel = 'inbox' AND status = 'failed'
           AND error_code <> 'inbox_job_skipped'
           AND (completed_at > ?1 OR (completed_at = ?1 AND id >= ?2))
           AND (completed_at < ?3 OR (completed_at = ?3 AND id <= ?4))
         ORDER BY completed_at, id",
    )?;
    let expected = statement
        .query_map(
            rusqlite::params![
                first.original_completed_at,
                first.original_ingestion_id,
                last.original_completed_at,
                last.original_ingestion_id,
            ],
            |row| row.get::<_, i64>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let stored = items
        .iter()
        .map(|item| item.original_ingestion_id)
        .collect::<Vec<_>>();
    if stored != expected {
        issue(
            issues,
            "invalid_inbox_retry_event",
            format!(
                "inbox retry event {event_id} does not contain the complete bounded failure range"
            ),
        );
    }
    Ok(())
}

fn retry_job_sequence(job_id: &str) -> Option<i64> {
    let bytes = job_id.as_bytes();
    if bytes.len() < 21 || bytes[0] != b'j' || !bytes[1..21].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let suffix = &bytes[21..];
    if !suffix.is_empty()
        && (suffix.len() < 2
            || suffix[0] != b'-'
            || !suffix[1..].iter().all(u8::is_ascii_digit)
            || std::str::from_utf8(&suffix[1..])
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .is_none_or(|value| value == 0))
    {
        return None;
    }
    std::str::from_utf8(&bytes[1..21])
        .ok()?
        .parse::<i64>()
        .ok()
        .filter(|sequence| *sequence > 0)
}

fn delivery_key_matches_job(delivery_key: &str, job_id: &str) -> bool {
    delivery_key
        .strip_prefix("inbox:")
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(stored_job_id, _)| stored_job_id == job_id)
}

fn issue(issues: &mut Vec<ValidationIssue>, code: &str, message: impl Into<String>) {
    issues.push(ValidationIssue {
        code: code.to_owned(),
        message: message.into(),
    });
}

#[cfg(test)]
mod tests {
    use rusqlite::params;
    use serde_json::json;

    use super::*;
    use crate::corpus::store_work;
    use crate::inbox_retry_store;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn empty_fresh_library_is_valid() -> TestResult {
        let directory = tempfile::tempdir()?;
        let connection = crate::db::init(&directory.path().join("annals.db"))?;
        assert_eq!(
            validate(&connection)?,
            ValidationReport {
                valid: true,
                issues: vec![]
            }
        );
        Ok(())
    }

    #[test]
    fn applied_typed_reconciliation_replays_as_valid() -> TestResult {
        let (mut connection, record) = applied_fixture()?;
        assert!(validate(&connection)?.valid);
        assert_eq!(record.applied_revision, None);
        let pending = corpus::select_reconciliation(&connection, Some("Source"), true)?;
        resolver::apply_record(&mut connection, &pending)?;
        assert!(validate(&connection)?.valid);
        Ok(())
    }

    #[test]
    fn tampered_effect_is_reported_by_replay() -> TestResult {
        let (mut connection, _) = applied_fixture()?;
        let pending = corpus::select_reconciliation(&connection, Some("Source"), true)?;
        resolver::apply_record(&mut connection, &pending)?;
        connection.execute("DROP TRIGGER concept_effects_immutable_update", [])?;
        connection.execute("UPDATE concept_effects SET label = 'Tampered'", [])?;
        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(report.issues.iter().any(|issue| {
            matches!(
                issue.code.as_str(),
                "reconciliation_replay_mismatch" | "noncanonical_commit_effects"
            )
        }));
        Ok(())
    }

    #[test]
    fn retry_events_validate_while_preparing_and_after_completion() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut connection = crate::db::init(&directory.path().join("annals.db"))?;
        failed_inbox_delivery(&connection, 1, "2026-08-27T00:00:01Z")?;
        failed_inbox_delivery(&connection, 2, "2026-08-27T00:00:02Z")?;
        let selection = inbox_retry_store::preview(
            &connection,
            "j00000000000000000001",
            "j00000000000000000002",
        )?;
        let event_id =
            inbox_retry_store::create_event(&mut connection, &selection, Some("auth repair"))?
                .event
                .id;
        assert!(validate(&connection)?.valid);

        for (ordinal, sequence) in [(0, 3_u64), (1, 4_u64)] {
            let job_id = format!("j{sequence:020}");
            inbox_retry_store::link_child_job(&connection, event_id, ordinal, &job_id, sequence)?;
        }
        inbox_retry_store::mark_running(&connection, event_id)?;
        for sequence in [3_u64, 4_u64] {
            let job_id = format!("j{sequence:020}");
            let child_id = failed_inbox_delivery(
                &connection,
                sequence,
                &format!("2026-08-27T00:00:0{sequence}Z"),
            )?;
            inbox_retry_store::link_child_delivery(&connection, event_id, &job_id, child_id)?;
        }
        inbox_retry_store::complete(&connection, event_id)?;
        assert!(validate(&connection)?.valid);
        Ok(())
    }

    #[test]
    fn retry_validation_reports_changed_original_snapshot() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut connection = crate::db::init(&directory.path().join("annals.db"))?;
        failed_inbox_delivery(&connection, 1, "2026-08-27T00:00:01Z")?;
        let selection = inbox_retry_store::preview(
            &connection,
            "j00000000000000000001",
            "j00000000000000000001",
        )?;
        inbox_retry_store::create_event(&mut connection, &selection, None)?;
        connection.execute("DROP TRIGGER inbox_retry_items_original_immutable", [])?;
        connection.execute(
            "UPDATE inbox_retry_items SET original_error_message = 'changed'",
            [],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(report.issues.iter().any(|issue| {
            issue.code == "invalid_inbox_retry_item"
                && issue.message.contains("original failed source delivery")
        }));
        Ok(())
    }

    #[test]
    fn retry_validation_reports_incomplete_frozen_range() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut connection = crate::db::init(&directory.path().join("annals.db"))?;
        for sequence in 1..=3 {
            failed_inbox_delivery(
                &connection,
                sequence,
                &format!("2026-08-27T00:00:0{sequence}Z"),
            )?;
        }
        let selection = inbox_retry_store::preview(
            &connection,
            "j00000000000000000001",
            "j00000000000000000003",
        )?;
        inbox_retry_store::create_event(&mut connection, &selection, None)?;
        connection.execute("DROP TRIGGER inbox_retry_items_no_delete", [])?;
        connection.execute(
            "DELETE FROM inbox_retry_items WHERE event_id = 1 AND ordinal = 1",
            [],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(report.issues.iter().any(|issue| {
            issue.code == "invalid_inbox_retry_event"
                && issue.message.contains("complete bounded failure range")
        }));
        Ok(())
    }

    #[test]
    fn retry_validation_rejects_retained_child_outcome() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut connection = crate::db::init(&directory.path().join("annals.db"))?;
        failed_inbox_delivery(&connection, 1, "2026-08-27T00:00:01Z")?;
        let selection = inbox_retry_store::preview(
            &connection,
            "j00000000000000000001",
            "j00000000000000000001",
        )?;
        let event_id = inbox_retry_store::create_event(&mut connection, &selection, None)?
            .event
            .id;
        let child_job_id = "j00000000000000000002";
        inbox_retry_store::link_child_job(&connection, event_id, 0, child_job_id, 2)?;
        inbox_retry_store::mark_running(&connection, event_id)?;
        let work = store_work(&mut connection, "Retry", "same source")?;
        connection.execute(
            "INSERT INTO ingestions(
                 delivery_key, source_name, channel, first_seen_at, ingested_at,
                 completed_at, status, work_id, new_work, result
             ) VALUES(?1, 'retry.txt', 'inbox', 'seen', 'ingested',
                      '2026-08-27T00:00:02Z', 'completed', ?2, 0, 'retained')",
            params![format!("inbox:{child_job_id}:seen"), work.id],
        )?;
        let child_id = connection.last_insert_rowid();
        inbox_retry_store::link_child_delivery(&connection, event_id, child_job_id, child_id)?;
        connection.execute(
            "UPDATE inbox_retry_events
             SET state = 'completed', completed_at = '2026-08-27T00:00:03Z'
             WHERE id = ?1",
            [event_id],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(report.issues.iter().any(|issue| {
            issue.code == "invalid_inbox_retry_item"
                && issue.message.contains("invalid retry child outcome")
        }));
        Ok(())
    }

    #[test]
    fn retry_validation_reports_unpublished_running_event() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut connection = crate::db::init(&directory.path().join("annals.db"))?;
        failed_inbox_delivery(&connection, 1, "2026-08-27T00:00:01Z")?;
        failed_inbox_delivery(&connection, 2, "2026-08-27T00:00:02Z")?;
        let selection = inbox_retry_store::preview(
            &connection,
            "j00000000000000000001",
            "j00000000000000000002",
        )?;
        let event_id = inbox_retry_store::create_event(&mut connection, &selection, None)?
            .event
            .id;
        inbox_retry_store::link_child_job(&connection, event_id, 0, "j00000000000000000003", 3)?;
        connection.execute(
            "UPDATE inbox_retry_events
             SET state = 'running', ready_at = '2026-08-27T00:00:03Z'
             WHERE id = ?1",
            [event_id],
        )?;

        let report = validate(&connection)?;
        assert!(!report.valid);
        assert!(report.issues.iter().any(|issue| {
            issue.code == "invalid_inbox_retry_event"
                && issue.message.contains("inconsistent with its retry items")
        }));
        Ok(())
    }

    fn failed_inbox_delivery(
        connection: &Connection,
        job_sequence: u64,
        completed_at: &str,
    ) -> Result<i64, rusqlite::Error> {
        let job_id = format!("j{job_sequence:020}");
        let text = format!("source-{job_sequence}");
        let digest = corpus::sha256_hex(text.as_bytes());
        connection.execute(
            "INSERT INTO works(label, normalized_label, text, sha256, created_at)
             VALUES(?1, ?1, ?2, ?3, 'now')",
            params![format!("work-{job_sequence}"), text, digest,],
        )?;
        let work_id = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO ingestions(
                 delivery_key, source_name, channel, first_seen_at, ingested_at,
                 completed_at, status, work_id, new_work, error_code, error_message
             ) VALUES(?1, ?2, 'inbox', ?3, ?3, ?4, 'failed', ?5, 1,
                      'model_runner_failed', 'source delivery failed')",
            params![
                format!("inbox:{job_id}:{completed_at}"),
                format!("source-{job_sequence}.txt"),
                format!("seen-{job_sequence}"),
                completed_at,
                work_id,
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }

    fn applied_fixture() -> Result<(Connection, corpus::ReconciliationRecord), AppError> {
        let directory = tempfile::tempdir()
            .map_err(|error| AppError::unexpected("tempdir_failed", error.to_string()))?;
        let path = directory.keep().join("annals.db");
        let mut connection = crate::db::init(&path)?;
        let work = store_work(&mut connection, "Source", "Exact source.")?;
        let record = resolver::submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Create a grounded concept",
                "operations": [{
                    "action": "create_concept",
                    "ref": "source",
                    "label": "Source concept",
                    "parents": [],
                    "evidence": [{"quote": "Exact source."}]
                }]
            }),
            "test",
            None,
        )?;
        Ok((connection, record))
    }
}
