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

fn issue(issues: &mut Vec<ValidationIssue>, code: &str, message: impl Into<String>) {
    issues.push(ValidationIssue {
        code: code.to_owned(),
        message: message.into(),
    });
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::corpus::store_work;

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
