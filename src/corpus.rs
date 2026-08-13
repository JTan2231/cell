#![allow(clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::change::Reconciliation;
use crate::error::AppError;
use crate::index;
use crate::model::{
    CommitView, ConceptView, CorpusView, DiffEntry, DiffKind, DiffView, EvidenceView,
    ReconciliationView, RecordedChangeView, SearchOutput, SearchResult, WorkSummary, WorkView,
};

const POSITION_STEP: i64 = 1024;

#[derive(Debug, Clone)]
pub(crate) struct Work {
    pub id: i64,
    pub label: String,
    pub text: String,
    pub sha256: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Snapshot {
    pub concepts: Vec<SnapshotConcept>,
    pub evidence: Vec<SnapshotEvidence>,
}

impl Snapshot {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            concepts: Vec::new(),
            evidence: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotConcept {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub label: String,
    pub position: i64,
    pub created_revision: i64,
    pub updated_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotEvidence {
    pub id: i64,
    pub concept_id: i64,
    pub work_id: i64,
    pub start_byte: usize,
    pub end_byte: usize,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ReconciliationRecord {
    pub id: i64,
    pub work_id: i64,
    pub work_label: String,
    pub base_revision: i64,
    pub status: String,
    pub summary: String,
    pub submitted_request: String,
    pub resolved_reconciliation: String,
    pub actor: String,
    pub created_at: String,
    pub applied_revision: Option<i64>,
}

pub(crate) fn now() -> Result<String, AppError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| AppError::unexpected("timestamp_failed", error.to_string()))
}

#[must_use]
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn revision(connection: &Connection) -> Result<i64, AppError> {
    Ok(connection.query_row(
        "SELECT revision FROM library_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?)
}

pub(crate) fn store_work(
    connection: &mut Connection,
    label: &str,
    text: &str,
) -> Result<Work, AppError> {
    validate_label(label, "work label")?;
    if text.trim().is_empty() {
        return Err(AppError::invalid(
            "empty_work",
            "an immutable work must contain non-whitespace source text",
        ));
    }
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let normalized = index::normalize(label);
    let digest = sha256_hex(text.as_bytes());
    if let Some(existing) = work_by_normalized_label(&transaction, &normalized)?
        && existing.sha256 != digest
    {
        return Err(AppError::conflict(
            "work_name_exists",
            format!("a retained work named {:?} already exists", existing.label),
        ));
    }
    if let Some(existing) = work_by_digest(&transaction, &digest)? {
        transaction.commit()?;
        return Ok(existing);
    }
    if let Some(existing) = work_by_normalized_label(&transaction, &normalized)? {
        return Err(AppError::conflict(
            "work_name_exists",
            format!("a retained work named {:?} already exists", existing.label),
        ));
    }
    let created_at = now()?;
    transaction.execute(
        "INSERT INTO works(label, normalized_label, text, sha256, created_at) \
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![label, normalized, text, digest, created_at],
    )?;
    let work = Work {
        id: transaction.last_insert_rowid(),
        label: label.to_owned(),
        text: text.to_owned(),
        sha256: digest,
        created_at,
    };
    transaction.commit()?;
    Ok(work)
}

pub(crate) fn get_work(connection: &Connection, label: &str) -> Result<Work, AppError> {
    let normalized = index::normalize(label);
    work_by_normalized_label(connection, &normalized)?.ok_or_else(|| {
        AppError::not_found(
            "work_not_found",
            format!("retained work {label:?} was not found"),
        )
    })
}

pub(crate) fn get_work_by_id(connection: &Connection, id: i64) -> Result<Work, AppError> {
    work_query(connection, "WHERE id = ?1", rusqlite::params![id])?.ok_or_else(|| {
        AppError::database(
            "work_reference_missing",
            format!("internal work {id} was not found"),
        )
    })
}

pub(crate) fn list_works(connection: &Connection) -> Result<Vec<WorkSummary>, AppError> {
    let mut statement = connection.prepare(
        "SELECT label, sha256, length(CAST(text AS BLOB)), created_at \
         FROM works ORDER BY normalized_label",
    )?;
    let rows = statement.query_map([], |row| {
        let bytes = usize_from_i64(row.get(2)?, "work byte length")?;
        Ok(WorkSummary {
            work: row.get(0)?,
            sha256: row.get(1)?,
            size_bytes: bytes,
            created_at: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn work_view(work: &Work) -> WorkView {
    WorkView {
        summary: WorkSummary {
            work: work.label.clone(),
            sha256: work.sha256.clone(),
            size_bytes: work.text.len(),
            created_at: work.created_at.clone(),
        },
        headings: markdown_headings(&work.text)
            .into_iter()
            .map(|heading| crate::model::HeadingView { path: heading.path })
            .collect(),
    }
}

fn work_by_normalized_label(
    connection: &Connection,
    normalized: &str,
) -> Result<Option<Work>, AppError> {
    work_query(
        connection,
        "WHERE normalized_label = ?1",
        rusqlite::params![normalized],
    )
}

fn work_by_digest(connection: &Connection, digest: &str) -> Result<Option<Work>, AppError> {
    work_query(connection, "WHERE sha256 = ?1", rusqlite::params![digest])
}

fn work_query<P: rusqlite::Params>(
    connection: &Connection,
    clause: &str,
    arguments: P,
) -> Result<Option<Work>, AppError> {
    let sql = format!("SELECT id, label, text, sha256, created_at FROM works {clause} LIMIT 1");
    Ok(connection
        .query_row(&sql, arguments, |row| {
            Ok(Work {
                id: row.get(0)?,
                label: row.get(1)?,
                text: row.get(2)?,
                sha256: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .optional()?)
}

pub(crate) fn head_snapshot(connection: &Connection) -> Result<Snapshot, AppError> {
    load_materialized_snapshot(connection)
}

pub(crate) fn snapshot_at(connection: &Connection, requested: i64) -> Result<Snapshot, AppError> {
    if requested < 0 {
        return Err(revision_not_found(requested));
    }
    if requested == 0 {
        return Ok(Snapshot::empty());
    }
    let json = connection
        .query_row(
            "SELECT after_snapshot FROM commits WHERE revision = ?1",
            [requested],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| revision_not_found(requested))?;
    serde_json::from_str(&json).map_err(|error| {
        AppError::database(
            "invalid_history_snapshot",
            format!("revision {requested} contains an invalid snapshot: {error}"),
        )
    })
}

fn revision_not_found(requested: i64) -> AppError {
    AppError::not_found(
        "revision_not_found",
        format!("corpus revision {requested} was not found"),
    )
}

fn load_materialized_snapshot(connection: &Connection) -> Result<Snapshot, AppError> {
    let mut concepts_statement = connection.prepare(
        "SELECT id, parent_id, label, position, created_revision, updated_revision \
         FROM concepts ORDER BY id",
    )?;
    let concepts = concepts_statement
        .query_map([], |row| {
            Ok(SnapshotConcept {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                label: row.get(2)?,
                position: row.get(3)?,
                created_revision: row.get(4)?,
                updated_revision: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut evidence_statement = connection.prepare(
        "SELECT id, concept_id, work_id, start_byte, end_byte, created_at \
         FROM evidence ORDER BY id",
    )?;
    let evidence = evidence_statement
        .query_map([], |row| {
            Ok(SnapshotEvidence {
                id: row.get(0)?,
                concept_id: row.get(1)?,
                work_id: row.get(2)?,
                start_byte: usize_from_i64(row.get(3)?, "evidence start byte")?,
                end_byte: usize_from_i64(row.get(4)?, "evidence end byte")?,
                created_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Snapshot { concepts, evidence })
}

pub(crate) fn materialize_snapshot(
    transaction: &Transaction<'_>,
    snapshot: &Snapshot,
) -> Result<(), AppError> {
    validate_snapshot(transaction, snapshot)?;
    transaction.execute("DELETE FROM evidence", [])?;
    transaction.execute("DELETE FROM concepts", [])?;

    let concepts = snapshot
        .concepts
        .iter()
        .map(|concept| (concept.id, concept))
        .collect::<BTreeMap<_, _>>();
    for id in preorder(snapshot)? {
        let concept = concepts.get(&id).ok_or_else(|| {
            AppError::database(
                "snapshot_materialization_failed",
                "the validated corpus snapshot lost a concept during traversal",
            )
        })?;
        transaction.execute(
            "INSERT INTO concepts(\
                 id, parent_id, label, normalized_label, position, created_revision, \
                 updated_revision\
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                concept.id,
                concept.parent_id,
                concept.label,
                index::normalize(&concept.label),
                concept.position,
                concept.created_revision,
                concept.updated_revision
            ],
        )?;
    }
    for evidence in &snapshot.evidence {
        transaction.execute(
            "INSERT INTO evidence(\
                 id, concept_id, work_id, start_byte, end_byte, created_at\
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                evidence.id,
                evidence.concept_id,
                evidence.work_id,
                i64_from_usize(evidence.start_byte, "evidence start byte")?,
                i64_from_usize(evidence.end_byte, "evidence end byte")?,
                evidence.created_at
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn validate_snapshot(
    connection: &Connection,
    snapshot: &Snapshot,
) -> Result<(), AppError> {
    let concepts = snapshot
        .concepts
        .iter()
        .map(|concept| (concept.id, concept))
        .collect::<BTreeMap<_, _>>();
    if concepts.len() != snapshot.concepts.len() || concepts.keys().any(|id| *id <= 0) {
        return Err(invalid_change("the corpus contains duplicate concepts"));
    }

    let mut sibling_labels = HashSet::new();
    let mut sibling_positions = HashSet::new();
    for concept in &snapshot.concepts {
        validate_label(&concept.label, "concept label")?;
        if concept.created_revision <= 0 || concept.updated_revision < concept.created_revision {
            return Err(invalid_change(
                "concept revision metadata must be positive and monotonic",
            ));
        }
        if concept.position < 0 {
            return Err(invalid_change("concept positions cannot be negative"));
        }
        if let Some(parent) = concept.parent_id
            && (parent == concept.id || !concepts.contains_key(&parent))
        {
            return Err(invalid_change(format!(
                "concept {:?} has an invalid parent",
                concept.label
            )));
        }
        let label_key = (concept.parent_id, index::normalize(&concept.label));
        if !sibling_labels.insert(label_key) {
            return Err(AppError::conflict(
                "duplicate_sibling_label",
                format!(
                    "concept label {:?} duplicates one of its siblings",
                    concept.label
                ),
            ));
        }
        if !sibling_positions.insert((concept.parent_id, concept.position)) {
            return Err(invalid_change(
                "two siblings have the same internal ordering value",
            ));
        }
        let mut seen = BTreeSet::new();
        let mut current = Some(concept.id);
        while let Some(id) = current {
            if !seen.insert(id) {
                return Err(AppError::conflict(
                    "would_create_cycle",
                    format!("concept {:?} would belong to a cycle", concept.label),
                ));
            }
            current = concepts.get(&id).and_then(|item| item.parent_id);
        }
    }

    let work_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.work_id)
        .collect::<BTreeSet<_>>();
    let works = load_work_texts(connection, &work_ids)?;
    let mut evidence_keys = HashSet::new();
    let mut evidence_ids = HashSet::new();
    for evidence in &snapshot.evidence {
        if evidence.id <= 0 || !evidence_ids.insert(evidence.id) {
            return Err(invalid_change("the corpus contains duplicate evidence"));
        }
        if !concepts.contains_key(&evidence.concept_id) {
            return Err(invalid_change("evidence names a missing concept"));
        }
        let Some(work) = works.get(&evidence.work_id) else {
            return Err(invalid_change("evidence names a missing immutable work"));
        };
        if evidence.end_byte <= evidence.start_byte
            || evidence.end_byte > work.len()
            || !work.is_char_boundary(evidence.start_byte)
            || !work.is_char_boundary(evidence.end_byte)
        {
            return Err(invalid_change("evidence has an invalid UTF-8 source range"));
        }
        if !evidence_keys.insert((
            evidence.concept_id,
            evidence.work_id,
            evidence.start_byte,
            evidence.end_byte,
        )) {
            return Err(invalid_change(
                "the same evidence is attached more than once",
            ));
        }
    }

    for concept in &snapshot.concepts {
        let is_leaf = !snapshot
            .concepts
            .iter()
            .any(|candidate| candidate.parent_id == Some(concept.id));
        if is_leaf
            && !snapshot
                .evidence
                .iter()
                .any(|evidence| evidence.concept_id == concept.id)
        {
            return Err(AppError::conflict(
                "ungrounded_leaf",
                format!("leaf concept {:?} has no source evidence", concept.label),
            ));
        }
    }
    Ok(())
}

pub(crate) fn corpus_view(
    connection: &Connection,
    requested_revision: i64,
) -> Result<CorpusView, AppError> {
    let snapshot = snapshot_at(connection, requested_revision)?;
    snapshot_view(connection, requested_revision, &snapshot)
}

pub(crate) fn snapshot_view(
    connection: &Connection,
    requested_revision: i64,
    snapshot: &Snapshot,
) -> Result<CorpusView, AppError> {
    let paths = paths(snapshot)?;
    let work_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.work_id)
        .collect::<BTreeSet<_>>();
    let works = load_works(connection, &work_ids)?;
    let by_id = snapshot
        .concepts
        .iter()
        .map(|concept| (concept.id, concept))
        .collect::<BTreeMap<_, _>>();
    let ordered = preorder(snapshot)?;
    let concepts = ordered
        .into_iter()
        .map(|id| {
            let concept = by_id.get(&id).ok_or_else(|| {
                AppError::database("snapshot_concept_missing", "snapshot traversal failed")
            })?;
            let evidence = evidence_views(snapshot, id, &works)?;
            let mut children = snapshot
                .concepts
                .iter()
                .filter(|candidate| candidate.parent_id == Some(id))
                .collect::<Vec<_>>();
            children.sort_by_key(|child| (child.position, child.id));
            Ok(ConceptView {
                path: paths.get(&id).cloned().ok_or_else(|| {
                    AppError::database("snapshot_path_missing", "snapshot path is missing")
                })?,
                label: concept.label.clone(),
                parent: concept
                    .parent_id
                    .and_then(|parent| paths.get(&parent).cloned()),
                children: children.iter().map(|child| child.label.clone()).collect(),
                evidence,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(CorpusView {
        revision: requested_revision,
        concepts,
    })
}

fn evidence_views(
    snapshot: &Snapshot,
    concept_id: i64,
    works: &BTreeMap<i64, Work>,
) -> Result<Vec<EvidenceView>, AppError> {
    snapshot
        .evidence
        .iter()
        .filter(|evidence| evidence.concept_id == concept_id)
        .map(|evidence| {
            let work = works.get(&evidence.work_id).ok_or_else(|| {
                AppError::database("work_reference_missing", "evidence work is missing")
            })?;
            Ok(EvidenceView {
                work: work.label.clone(),
                quote: work.text[evidence.start_byte..evidence.end_byte].to_owned(),
            })
        })
        .collect()
}

pub(crate) fn paths(snapshot: &Snapshot) -> Result<BTreeMap<i64, Vec<String>>, AppError> {
    let by_id = snapshot
        .concepts
        .iter()
        .map(|concept| (concept.id, concept))
        .collect::<BTreeMap<_, _>>();
    let mut output = BTreeMap::new();
    for concept in &snapshot.concepts {
        let mut path = Vec::new();
        let mut current = Some(concept.id);
        let mut seen = BTreeSet::new();
        while let Some(id) = current {
            if !seen.insert(id) {
                return Err(AppError::database(
                    "concept_cycle",
                    "a historical concept belongs to a cycle",
                ));
            }
            let item = by_id.get(&id).ok_or_else(|| {
                AppError::database(
                    "concept_parent_missing",
                    "a historical concept parent is missing",
                )
            })?;
            path.push(item.label.clone());
            current = item.parent_id;
        }
        path.reverse();
        output.insert(concept.id, path);
    }
    Ok(output)
}

fn preorder(snapshot: &Snapshot) -> Result<Vec<i64>, AppError> {
    let mut children = BTreeMap::<Option<i64>, Vec<&SnapshotConcept>>::new();
    for concept in &snapshot.concepts {
        children.entry(concept.parent_id).or_default().push(concept);
    }
    for siblings in children.values_mut() {
        siblings.sort_by_key(|concept| (concept.position, concept.id));
    }
    let mut output = Vec::with_capacity(snapshot.concepts.len());
    let mut pending = children
        .get(&None)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    while let Some(concept) = pending.pop() {
        output.push(concept.id);
        if let Some(descendants) = children.get(&Some(concept.id)) {
            pending.extend(descendants.iter().rev().copied());
        }
    }
    if output.len() != snapshot.concepts.len() {
        return Err(AppError::database(
            "snapshot_not_forest",
            "the corpus snapshot is not one complete forest",
        ));
    }
    Ok(output)
}

pub(crate) fn search_current(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<SearchOutput, AppError> {
    index::require_current(connection)?;
    if query.trim().is_empty() {
        return Err(AppError::invalid(
            "empty_query",
            "a concept search query cannot be empty",
        ));
    }
    if limit == 0 {
        return Err(AppError::invalid(
            "invalid_limit",
            "a search result limit must be at least one",
        ));
    }
    let snapshot = head_snapshot(connection)?;
    let paths = paths(&snapshot)?;
    let normalized = index::normalize(query);
    let terms = normalized.split_whitespace().collect::<Vec<_>>();
    let mut candidates = snapshot
        .concepts
        .iter()
        .filter_map(|concept| {
            let path = paths.get(&concept.id)?;
            let haystack = index::normalize(&path.join(" "));
            let exact = usize::from(
                haystack == normalized || index::normalize(&concept.label) == normalized,
            );
            let matches = terms
                .iter()
                .filter(|term| haystack.contains(**term))
                .count();
            (exact > 0 || matches > 0).then_some((concept.id, exact, matches))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(id, exact, matches)| {
        (std::cmp::Reverse(*exact), std::cmp::Reverse(*matches), *id)
    });
    candidates.truncate(limit);

    let work_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.work_id)
        .collect::<BTreeSet<_>>();
    let works = load_works(connection, &work_ids)?;
    let by_id = snapshot
        .concepts
        .iter()
        .map(|concept| (concept.id, concept))
        .collect::<BTreeMap<_, _>>();
    let results = candidates
        .into_iter()
        .map(|(id, _, _)| {
            let concept = by_id.get(&id).ok_or_else(|| {
                AppError::database("search_concept_missing", "search concept is missing")
            })?;
            Ok(SearchResult {
                path: paths.get(&id).cloned().unwrap_or_default(),
                label: concept.label.clone(),
                evidence: evidence_views(&snapshot, id, &works)?,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(SearchOutput {
        query: query.to_owned(),
        results,
    })
}

pub(crate) fn list_commits(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<CommitView>, AppError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let sql_limit = i64::try_from(limit)
        .map_err(|_| AppError::invalid("invalid_limit", "commit limit is too large"))?;
    let mut statement = connection.prepare(
        "SELECT c.revision, c.parent_revision, c.kind, c.summary, w.label, c.actor, c.created_at \
         FROM commits AS c LEFT JOIN works AS w ON w.id = c.work_id \
         ORDER BY c.revision DESC LIMIT ?1",
    )?;
    let rows = statement.query_map([sql_limit], |row| {
        Ok(CommitView {
            revision: row.get(0)?,
            parent_revision: row.get(1)?,
            kind: row.get(2)?,
            summary: row.get(3)?,
            work: row.get(4)?,
            actor: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn recorded_change_at(
    connection: &Connection,
    requested_revision: i64,
) -> Result<RecordedChangeView, AppError> {
    let row = connection
        .query_row(
            "SELECT c.revision, c.parent_revision, c.base_revision, c.kind, c.summary, \
                    w.label, c.submitted_request, c.resolved_operations, c.metadata, \
                    c.actor, c.created_at \
             FROM commits AS c LEFT JOIN works AS w ON w.id = c.work_id \
             WHERE c.revision = ?1",
            [requested_revision],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                ))
            },
        )
        .optional()?;
    let Some((
        revision,
        parent_revision,
        base_revision,
        kind,
        summary,
        work,
        reconciliation,
        resolved_operations,
        metadata,
        actor,
        created_at,
    )) = row
    else {
        if requested_revision < 0
            || (requested_revision != 0
                && !connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM commits WHERE revision = ?1)",
                    [requested_revision],
                    |row| row.get::<_, bool>(0),
                )?)
        {
            return Err(revision_not_found(requested_revision));
        }
        return Err(AppError::not_found(
            "recorded_change_not_found",
            format!("revision {requested_revision} does not contain a recorded corpus change"),
        ));
    };
    Ok(RecordedChangeView {
        revision,
        parent_revision,
        base_revision,
        status: "applied".to_owned(),
        kind,
        summary,
        work,
        submitted_request: serde_json::from_str(&reconciliation).map_err(|error| {
            AppError::database(
                "invalid_commit_request",
                format!("revision {requested_revision} has an invalid submitted request: {error}"),
            )
        })?,
        resolved_operations: serde_json::from_str(&resolved_operations).map_err(|error| {
            AppError::database(
                "invalid_commit_operations",
                format!("revision {requested_revision} has invalid resolved operations: {error}"),
            )
        })?,
        metadata: serde_json::from_str(&metadata).map_err(|error| {
            AppError::database(
                "invalid_commit_metadata",
                format!("revision {requested_revision} has invalid metadata: {error}"),
            )
        })?,
        actor,
        created_at,
    })
}

pub(crate) fn diff(
    connection: &Connection,
    from_revision: i64,
    to_revision: i64,
) -> Result<DiffView, AppError> {
    let before = snapshot_at(connection, from_revision)?;
    let after = snapshot_at(connection, to_revision)?;
    let entries = diff_snapshot_entries(connection, &before, &after)?;
    Ok(DiffView {
        from_revision,
        to_revision,
        entries,
    })
}

pub(crate) fn diff_snapshot_entries(
    connection: &Connection,
    before: &Snapshot,
    after: &Snapshot,
) -> Result<Vec<DiffEntry>, AppError> {
    let before_paths = paths(before)?;
    let after_paths = paths(after)?;
    let before_concepts = before
        .concepts
        .iter()
        .map(|concept| (concept.id, concept))
        .collect::<BTreeMap<_, _>>();
    let after_concepts = after
        .concepts
        .iter()
        .map(|concept| (concept.id, concept))
        .collect::<BTreeMap<_, _>>();
    let mut entries = Vec::new();
    for id in before_concepts
        .keys()
        .chain(after_concepts.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (before_concepts.get(&id), after_concepts.get(&id)) {
            (None, Some(_)) => entries.push(DiffEntry {
                kind: DiffKind::Created,
                before: None,
                after: after_paths.get(&id).cloned(),
                work: None,
                quote: None,
            }),
            (Some(_), None) => entries.push(DiffEntry {
                kind: DiffKind::Retired,
                before: before_paths.get(&id).cloned(),
                after: None,
                work: None,
                quote: None,
            }),
            (Some(old), Some(new)) => {
                if old.parent_id != new.parent_id || sibling_order_changed(id, before, after) {
                    entries.push(DiffEntry {
                        kind: DiffKind::Moved,
                        before: before_paths.get(&id).cloned(),
                        after: after_paths.get(&id).cloned(),
                        work: None,
                        quote: None,
                    });
                }
                if old.label != new.label {
                    entries.push(DiffEntry {
                        kind: DiffKind::Reworded,
                        before: before_paths.get(&id).cloned(),
                        after: after_paths.get(&id).cloned(),
                        work: None,
                        quote: None,
                    });
                }
            }
            (None, None) => {}
        }
    }
    append_evidence_diff(
        connection,
        before,
        after,
        &before_paths,
        &after_paths,
        &mut entries,
    )?;
    Ok(entries)
}

fn sibling_order_changed(id: i64, before: &Snapshot, after: &Snapshot) -> bool {
    previous_sibling(before, id) != previous_sibling(after, id)
}

fn previous_sibling(snapshot: &Snapshot, id: i64) -> Option<Vec<String>> {
    let concept = snapshot.concepts.iter().find(|concept| concept.id == id)?;
    let all_paths = paths(snapshot).ok()?;
    let mut siblings = snapshot
        .concepts
        .iter()
        .filter(|candidate| candidate.parent_id == concept.parent_id)
        .collect::<Vec<_>>();
    siblings.sort_by_key(|candidate| (candidate.position, candidate.id));
    let index = siblings.iter().position(|candidate| candidate.id == id)?;
    index
        .checked_sub(1)
        .and_then(|previous| all_paths.get(&siblings[previous].id).cloned())
}

fn append_evidence_diff(
    connection: &Connection,
    before: &Snapshot,
    after: &Snapshot,
    before_paths: &BTreeMap<i64, Vec<String>>,
    after_paths: &BTreeMap<i64, Vec<String>>,
    entries: &mut Vec<DiffEntry>,
) -> Result<(), AppError> {
    let before_by_id = before
        .evidence
        .iter()
        .map(|evidence| (evidence.id, evidence))
        .collect::<BTreeMap<_, _>>();
    let after_by_id = after
        .evidence
        .iter()
        .map(|evidence| (evidence.id, evidence))
        .collect::<BTreeMap<_, _>>();
    for id in before_by_id
        .keys()
        .chain(after_by_id.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let (kind, evidence, path) = match (before_by_id.get(&id), after_by_id.get(&id)) {
            (None, Some(evidence)) => (
                DiffKind::EvidenceAdded,
                *evidence,
                after_paths.get(&evidence.concept_id).cloned(),
            ),
            (Some(evidence), None) => (
                DiffKind::EvidenceRemoved,
                *evidence,
                before_paths.get(&evidence.concept_id).cloned(),
            ),
            _ => continue,
        };
        let work = get_work_by_id(connection, evidence.work_id)?;
        let (before, after) = match &kind {
            DiffKind::EvidenceAdded => (None, path),
            DiffKind::EvidenceRemoved => (path, None),
            _ => {
                return Err(AppError::unexpected(
                    "invalid_evidence_diff",
                    "an evidence diff had a non-evidence kind",
                ));
            }
        };
        entries.push(DiffEntry {
            kind,
            before,
            after,
            work: Some(work.label),
            quote: Some(work.text[evidence.start_byte..evidence.end_byte].to_owned()),
        });
    }
    Ok(())
}

pub(crate) fn reconciliation_view(
    record: &ReconciliationRecord,
) -> Result<ReconciliationView, AppError> {
    let request: Reconciliation = serde_json::from_str(&record.submitted_request)?;
    Ok(ReconciliationView {
        work: record.work_label.clone(),
        base_revision: record.base_revision,
        status: record.status.clone(),
        summary: record.summary.clone(),
        request: serde_json::from_str(&record.submitted_request)?,
        annotations: request.annotations().to_vec(),
        created_at: record.created_at.clone(),
        applied_revision: record.applied_revision,
    })
}

pub(crate) fn select_reconciliation(
    connection: &Connection,
    work_label: Option<&str>,
    pending_only: bool,
) -> Result<ReconciliationRecord, AppError> {
    if let Some(label) = work_label {
        let work = get_work(connection, label)?;
        let status_clause = if pending_only {
            "AND r.status = 'pending'"
        } else {
            ""
        };
        let order_clause = if pending_only {
            "r.id DESC"
        } else {
            "CASE WHEN r.status = 'pending' THEN 0 ELSE 1 END, r.id DESC"
        };
        let sql = format!(
            "SELECT r.id, r.work_id, w.label, r.base_revision, r.status, r.summary, \
                    r.submitted_request, r.resolved_reconciliation, r.actor, r.created_at, \
                    r.applied_revision \
             FROM reconciliations AS r JOIN works AS w ON w.id = r.work_id \
             WHERE r.work_id = ?1 {status_clause} ORDER BY {order_clause} LIMIT 1"
        );
        return reconciliation_query(connection, &sql, [work.id])?.ok_or_else(|| {
            AppError::not_found(
                "pending_reconciliation_not_found",
                format!(
                    "no applicable reconciliation was found for work {:?}",
                    work.label
                ),
            )
        });
    }
    let pending_sql = "SELECT r.id, r.work_id, w.label, r.base_revision, r.status, r.summary, \
                r.submitted_request, r.resolved_reconciliation, r.actor, r.created_at, \
                r.applied_revision \
         FROM reconciliations AS r JOIN works AS w ON w.id = r.work_id \
         WHERE r.status = 'pending' ORDER BY r.id DESC";
    let mut records = reconciliation_rows(connection, pending_sql, [])?;
    if records.is_empty() && !pending_only {
        let latest_per_work_sql = "SELECT r.id, r.work_id, w.label, r.base_revision, r.status, \
                    r.summary, r.submitted_request, r.resolved_reconciliation, r.actor, \
                    r.created_at, r.applied_revision \
             FROM reconciliations AS r JOIN works AS w ON w.id = r.work_id \
             WHERE r.id = (SELECT MAX(latest.id) FROM reconciliations AS latest \
                           WHERE latest.work_id = r.work_id) \
             ORDER BY r.id DESC";
        records = reconciliation_rows(connection, latest_per_work_sql, [])?;
    }
    match records.as_slice() {
        [record] => Ok(record.clone()),
        [] => Err(AppError::not_found(
            "pending_reconciliation_not_found",
            "no applicable reconciliation was found",
        )),
        _ => Err(AppError::conflict(
            "reconciliation_selector_required",
            format!(
                "several reconciliations are available; select one with --work: {}",
                records
                    .iter()
                    .map(|record| format!("{:?}", record.work_label))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

pub(crate) fn list_reconciliations(
    connection: &Connection,
) -> Result<Vec<ReconciliationView>, AppError> {
    let sql = "SELECT r.id, r.work_id, w.label, r.base_revision, r.status, r.summary, \
                      r.submitted_request, r.resolved_reconciliation, r.actor, r.created_at, \
                      r.applied_revision \
               FROM reconciliations AS r JOIN works AS w ON w.id = r.work_id ORDER BY r.id DESC";
    reconciliation_rows(connection, sql, [])?
        .iter()
        .map(reconciliation_view)
        .collect()
}

pub(crate) fn reconciliation_query<P: rusqlite::Params>(
    connection: &Connection,
    sql: &str,
    parameters: P,
) -> Result<Option<ReconciliationRecord>, AppError> {
    Ok(connection
        .query_row(sql, parameters, reconciliation_from_row)
        .optional()?)
}

fn reconciliation_rows<P: rusqlite::Params>(
    connection: &Connection,
    sql: &str,
    parameters: P,
) -> Result<Vec<ReconciliationRecord>, AppError> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(parameters, reconciliation_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn reconciliation_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ReconciliationRecord> {
    Ok(ReconciliationRecord {
        id: row.get(0)?,
        work_id: row.get(1)?,
        work_label: row.get(2)?,
        base_revision: row.get(3)?,
        status: row.get(4)?,
        summary: row.get(5)?,
        submitted_request: row.get(6)?,
        resolved_reconciliation: row.get(7)?,
        actor: row.get(8)?,
        created_at: row.get(9)?,
        applied_revision: row.get(10)?,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_reconciliation(
    transaction: &Transaction<'_>,
    work_id: i64,
    base_revision: i64,
    model_run_id: Option<i64>,
    changes_corpus: bool,
    summary: &str,
    request_json: &str,
    resolved_json: &str,
    actor: &str,
) -> Result<ReconciliationRecord, AppError> {
    let pending_base = transaction
        .query_row(
            "SELECT base_revision FROM reconciliations \
             WHERE work_id = ?1 AND status = 'pending'",
            [work_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let replaces_pending = pending_base.is_none_or(|pending| base_revision >= pending);
    if replaces_pending {
        transaction.execute(
            "UPDATE reconciliations SET status = 'superseded' \
             WHERE work_id = ?1 AND status = 'pending'",
            [work_id],
        )?;
    }
    let status = match (changes_corpus, replaces_pending) {
        (false, _) => "recorded",
        (true, true) => "pending",
        (true, false) => "superseded",
    };
    let created_at = now()?;
    transaction.execute(
        "INSERT INTO reconciliations(\
             work_id, base_revision, model_run_id, status, summary, submitted_request, \
             resolved_reconciliation, actor, created_at\
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            work_id,
            base_revision,
            model_run_id,
            status,
            summary,
            request_json,
            resolved_json,
            actor,
            created_at
        ],
    )?;
    let id = transaction.last_insert_rowid();
    reconciliation_query(
        transaction,
        "SELECT r.id, r.work_id, w.label, r.base_revision, r.status, r.summary, \
                r.submitted_request, r.resolved_reconciliation, r.actor, r.created_at, \
                r.applied_revision \
         FROM reconciliations AS r JOIN works AS w ON w.id = r.work_id WHERE r.id = ?1",
        [id],
    )?
    .ok_or_else(|| {
        AppError::database(
            "reconciliation_insert_failed",
            "reconciliation was not stored",
        )
    })
}

pub(crate) fn sequence_next(connection: &Connection, table: &str) -> Result<i64, AppError> {
    let last = connection
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = ?1",
            [table],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    last.checked_add(1).ok_or_else(|| {
        AppError::database(
            "identity_overflow",
            format!("{table} identity space is exhausted"),
        )
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_commit(
    transaction: &Transaction<'_>,
    revision: i64,
    work_id: Option<i64>,
    reconciliation_id: Option<i64>,
    kind: &str,
    summary: &str,
    request: &Value,
    resolved: &Value,
    before: &Snapshot,
    after: &Snapshot,
    metadata: &Value,
    actor: &str,
) -> Result<(), AppError> {
    let parent = revision.checked_sub(1).ok_or_else(|| {
        AppError::database("revision_underflow", "a commit revision must be positive")
    })?;
    transaction.execute(
        "INSERT INTO commits(\
             revision, parent_revision, base_revision, work_id, reconciliation_id, kind, summary, \
             submitted_request, resolved_operations, before_snapshot, after_snapshot, metadata, \
             actor, created_at\
         ) VALUES(?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            revision,
            parent,
            work_id,
            reconciliation_id,
            kind,
            summary,
            serde_json::to_string(request)?,
            serde_json::to_string(resolved)?,
            serde_json::to_string(before)?,
            serde_json::to_string(after)?,
            serde_json::to_string(metadata)?,
            actor,
            now()?
        ],
    )?;
    transaction.execute(
        "UPDATE library_state SET revision = ?1 WHERE singleton = 1",
        [revision],
    )?;
    Ok(())
}

pub(crate) fn revert(transaction: &Transaction<'_>, target_revision: i64) -> Result<i64, AppError> {
    if target_revision <= 0 {
        return Err(revision_not_found(target_revision));
    }
    let (before_json, after_json, target_work): (String, String, Option<i64>) = transaction
        .query_row(
            "SELECT before_snapshot, after_snapshot, work_id FROM commits WHERE revision = ?1",
            [target_revision],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| revision_not_found(target_revision))?;
    let target_before: Snapshot = serde_json::from_str(&before_json)?;
    let target_after: Snapshot = serde_json::from_str(&after_json)?;
    let head = load_materialized_snapshot(transaction)?;
    validate_snapshot(transaction, &target_before).map_err(|error| {
        AppError::database(
            "invalid_history_snapshot",
            format!("revision {target_revision} has an invalid before-state: {error}"),
        )
    })?;
    validate_snapshot(transaction, &target_after).map_err(|error| {
        AppError::database(
            "invalid_history_snapshot",
            format!("revision {target_revision} has an invalid after-state: {error}"),
        )
    })?;
    validate_snapshot(transaction, &head)?;
    let current = revision(transaction)?;
    let new_revision = current.checked_add(1).ok_or_else(|| {
        AppError::database("revision_overflow", "the corpus revision is too large")
    })?;
    let inverse = invert_snapshot_change(
        target_revision,
        new_revision,
        &target_before,
        &target_after,
        &head,
    )?;
    validate_snapshot(transaction, &inverse).map_err(|error| {
        revert_conflict(
            target_revision,
            format!("the inverse would violate corpus invariants: {error}"),
        )
    })?;
    materialize_snapshot(transaction, &inverse)?;
    index::rebuild_all(transaction)?;
    let request = json!({ "revert_revision": target_revision });
    let resolved = serde_json::to_value(diff_snapshot_entries(transaction, &head, &inverse)?)?;
    insert_commit(
        transaction,
        new_revision,
        target_work,
        None,
        "revert",
        &format!("Revert revision {target_revision}"),
        &request,
        &resolved,
        &head,
        &inverse,
        &json!({ "reverted_revision": target_revision }),
        "human",
    )?;
    Ok(new_revision)
}

fn invert_snapshot_change(
    target_revision: i64,
    new_revision: i64,
    target_before: &Snapshot,
    target_after: &Snapshot,
    head: &Snapshot,
) -> Result<Snapshot, AppError> {
    let before_concepts = concept_map(target_before);
    let after_concepts = concept_map(target_after);
    let head_concepts = concept_map(head);
    let mut result_concepts = head_concepts
        .iter()
        .map(|(id, concept)| (*id, (*concept).clone()))
        .collect::<BTreeMap<_, _>>();
    let concept_ids = before_concepts
        .keys()
        .chain(after_concepts.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    for id in concept_ids {
        match (before_concepts.get(&id), after_concepts.get(&id)) {
            (None, Some(created)) => {
                require_revert_state(
                    target_revision,
                    result_concepts.get(&id) == Some(*created),
                    format!(
                        "created concept {} no longer matches the revision's after-state",
                        semantic_path(target_after, id)
                    ),
                )?;
                result_concepts.remove(&id);
            }
            (Some(retired), None) => {
                require_revert_state(
                    target_revision,
                    !result_concepts.contains_key(&id),
                    format!(
                        "retired concept {} has since been restored or reused",
                        semantic_path(target_before, id)
                    ),
                )?;
                let mut restored = (*retired).clone();
                restored.updated_revision = new_revision;
                result_concepts.insert(id, restored);
            }
            (Some(before), Some(after)) if before != after => {
                let current = result_concepts.get_mut(&id).ok_or_else(|| {
                    revert_conflict(
                        target_revision,
                        format!(
                            "changed concept {} no longer exists",
                            semantic_path(target_after, id)
                        ),
                    )
                })?;
                invert_concept_fields(target_revision, new_revision, before, after, current)?;
            }
            _ => {}
        }
    }

    let before_evidence = evidence_map(target_before);
    let after_evidence = evidence_map(target_after);
    let head_evidence = evidence_map(head);
    let mut result_evidence = head_evidence
        .iter()
        .map(|(id, evidence)| (*id, (*evidence).clone()))
        .collect::<BTreeMap<_, _>>();
    let semantically_changed_concepts = before_concepts
        .iter()
        .filter_map(|(id, before)| {
            after_concepts
                .get(id)
                .filter(|after| before.parent_id != after.parent_id || before.label != after.label)
                .map(|_| *id)
        })
        .collect::<BTreeSet<_>>();
    for concept_id in semantically_changed_concepts {
        let after_set = evidence_semantic_set(target_after, concept_id);
        let head_set = evidence_semantic_set(head, concept_id);
        require_revert_state(
            target_revision,
            after_set == head_set,
            format!(
                "source evidence for affected concept {} has changed since the target revision",
                semantic_path(target_after, concept_id)
            ),
        )?;
    }
    let evidence_ids = before_evidence
        .keys()
        .chain(after_evidence.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    for id in evidence_ids {
        match (before_evidence.get(&id), after_evidence.get(&id)) {
            (None, Some(added)) => {
                require_revert_state(
                    target_revision,
                    result_evidence.get(&id) == Some(*added),
                    "added source evidence no longer matches the revision's after-state".to_owned(),
                )?;
                result_evidence.remove(&id);
            }
            (Some(removed), None) => {
                require_revert_state(
                    target_revision,
                    !result_evidence.contains_key(&id),
                    "removed source evidence has since been restored or replaced".to_owned(),
                )?;
                result_evidence.insert(id, (*removed).clone());
            }
            (Some(before), Some(after)) if before != after => {
                require_revert_state(
                    target_revision,
                    result_evidence.get(&id) == Some(*after),
                    "changed source evidence no longer matches the revision's after-state"
                        .to_owned(),
                )?;
                result_evidence.insert(id, (*before).clone());
            }
            _ => {}
        }
    }

    Ok(Snapshot {
        concepts: result_concepts.into_values().collect(),
        evidence: result_evidence.into_values().collect(),
    })
}

fn concept_map(snapshot: &Snapshot) -> BTreeMap<i64, &SnapshotConcept> {
    snapshot
        .concepts
        .iter()
        .map(|concept| (concept.id, concept))
        .collect()
}

fn semantic_path(snapshot: &Snapshot, concept_id: i64) -> String {
    paths(snapshot)
        .ok()
        .and_then(|all_paths| all_paths.get(&concept_id).cloned())
        .map_or_else(
            || "at the recorded path".to_owned(),
            |path| display_path(&path),
        )
}

fn display_path(path: &[String]) -> String {
    path.iter()
        .map(|segment| format!("{segment:?}"))
        .collect::<Vec<_>>()
        .join(" › ")
}

fn evidence_map(snapshot: &Snapshot) -> BTreeMap<i64, &SnapshotEvidence> {
    snapshot
        .evidence
        .iter()
        .map(|evidence| (evidence.id, evidence))
        .collect()
}

fn evidence_semantic_set(snapshot: &Snapshot, concept_id: i64) -> BTreeSet<(i64, usize, usize)> {
    snapshot
        .evidence
        .iter()
        .filter(|evidence| evidence.concept_id == concept_id)
        .map(|evidence| (evidence.work_id, evidence.start_byte, evidence.end_byte))
        .collect()
}

fn invert_concept_fields(
    target_revision: i64,
    new_revision: i64,
    before: &SnapshotConcept,
    after: &SnapshotConcept,
    current: &mut SnapshotConcept,
) -> Result<(), AppError> {
    if before.created_revision != after.created_revision {
        return Err(AppError::database(
            "invalid_history_snapshot",
            format!("revision {target_revision} changes a concept's creation revision"),
        ));
    }
    let mut changed = invert_field(
        target_revision,
        before.parent_id,
        &after.parent_id,
        &mut current.parent_id,
        "parent",
    )?;
    changed |= invert_field(
        target_revision,
        before.label.clone(),
        &after.label,
        &mut current.label,
        "label",
    )?;
    changed |= invert_field(
        target_revision,
        before.position,
        &after.position,
        &mut current.position,
        "ordering",
    )?;
    if changed {
        current.updated_revision = new_revision;
    }
    Ok(())
}

fn invert_field<T: PartialEq + Clone>(
    target_revision: i64,
    before: T,
    after: &T,
    current: &mut T,
    field: &str,
) -> Result<bool, AppError> {
    if &before == after {
        return Ok(false);
    }
    require_revert_state(
        target_revision,
        current == after,
        format!("an affected concept's {field} has changed since the target revision"),
    )?;
    *current = before;
    Ok(true)
}

fn require_revert_state(
    target_revision: i64,
    condition: bool,
    reason: String,
) -> Result<(), AppError> {
    if condition {
        Ok(())
    } else {
        Err(revert_conflict(target_revision, reason))
    }
}

fn revert_conflict(target_revision: i64, reason: impl std::fmt::Display) -> AppError {
    AppError::conflict(
        "revert_conflict",
        format!("revision {target_revision} cannot be reverted: {reason}"),
    )
}

#[derive(Debug, Clone)]
struct Heading {
    path: Vec<String>,
    start_byte: usize,
}

fn markdown_headings(text: &str) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut stack = Vec::<String>::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        let hashes = bare.bytes().take_while(|byte| *byte == b'#').count();
        if (1..=6).contains(&hashes)
            && bare
                .as_bytes()
                .get(hashes)
                .is_some_and(u8::is_ascii_whitespace)
        {
            let label = bare[hashes..].trim().trim_end_matches('#').trim();
            if !label.is_empty() {
                stack.truncate(hashes.saturating_sub(1));
                stack.push(label.to_owned());
                headings.push(Heading {
                    path: stack.clone(),
                    start_byte: offset,
                });
            }
        }
        offset += line.len();
    }
    headings
}

pub(crate) fn heading_for_offset(text: &str, offset: usize) -> Option<Vec<String>> {
    markdown_headings(text)
        .into_iter()
        .rev()
        .find(|heading| heading.start_byte <= offset)
        .map(|heading| heading.path)
}

fn load_work_texts(
    connection: &Connection,
    ids: &BTreeSet<i64>,
) -> Result<BTreeMap<i64, String>, AppError> {
    ids.iter()
        .map(|id| Ok((*id, get_work_by_id(connection, *id)?.text)))
        .collect()
}

fn load_works(
    connection: &Connection,
    ids: &BTreeSet<i64>,
) -> Result<BTreeMap<i64, Work>, AppError> {
    ids.iter()
        .map(|id| Ok((*id, get_work_by_id(connection, *id)?)))
        .collect()
}

fn validate_label(label: &str, description: &str) -> Result<(), AppError> {
    if label.is_empty() || label.trim() != label {
        return Err(AppError::invalid(
            "invalid_label",
            format!("{description} must be nonempty and have no outer whitespace"),
        ));
    }
    if label.chars().any(char::is_control) {
        return Err(AppError::invalid(
            "invalid_label",
            format!("{description} cannot contain control characters"),
        ));
    }
    Ok(())
}

fn invalid_change(message: impl Into<String>) -> AppError {
    AppError::invalid("invalid_change", message)
}

fn i64_from_usize(value: usize, description: &str) -> Result<i64, AppError> {
    i64::try_from(value).map_err(|_| {
        AppError::database(
            "numeric_overflow",
            format!("{description} is too large for SQLite"),
        )
    })
}

fn usize_from_i64(value: i64, description: &str) -> rusqlite::Result<usize> {
    usize::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::other(format!(
                "invalid {description}: {error}"
            ))),
        )
    })
}

pub(crate) fn next_position(snapshot: &Snapshot, parent_id: Option<i64>) -> Result<i64, AppError> {
    snapshot
        .concepts
        .iter()
        .filter(|concept| concept.parent_id == parent_id)
        .map(|concept| concept.position)
        .max()
        .unwrap_or(-POSITION_STEP)
        .checked_add(POSITION_STEP)
        .ok_or_else(|| AppError::database("position_overflow", "concept order is too large"))
}

pub(crate) fn renumber_siblings(snapshot: &mut Snapshot, parent_id: Option<i64>) {
    let mut siblings = snapshot
        .concepts
        .iter_mut()
        .filter(|concept| concept.parent_id == parent_id)
        .collect::<Vec<_>>();
    siblings.sort_by_key(|concept| (concept.position, concept.id));
    for (index, concept) in siblings.into_iter().enumerate() {
        concept.position = i64::try_from(index)
            .unwrap_or(i64::MAX / POSITION_STEP)
            .saturating_mul(POSITION_STEP);
    }
}

pub(crate) fn path_lookup(snapshot: &Snapshot) -> Result<HashMap<Vec<String>, i64>, AppError> {
    Ok(paths(snapshot)?
        .into_iter()
        .map(|(id, path)| {
            (
                path.into_iter()
                    .map(|segment| index::normalize(&segment))
                    .collect(),
                id,
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{
        Snapshot, SnapshotConcept, SnapshotEvidence, diff_snapshot_entries, heading_for_offset,
        invert_snapshot_change, markdown_headings, sha256_hex, store_work,
    };
    use rusqlite::Connection;

    #[test]
    fn markdown_outline_tracks_nesting_and_offsets() {
        let text = "# Root\nintro\n## Child\nbody";
        let headings = markdown_headings(text);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[1].path, ["Root", "Child"]);
        assert_eq!(
            heading_for_offset(text, text.find("body").unwrap_or_default()),
            Some(vec!["Root".to_owned(), "Child".to_owned()])
        );
    }

    #[test]
    fn sha_is_stable() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn work_storage_uses_content_identity_before_labels() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut connection = test_connection()?;
        let retained = store_work(&mut connection, "Original", "same bytes")?;
        let repeated = store_work(&mut connection, "Another label", "same bytes")?;

        assert_eq!(repeated.id, retained.id);
        assert_eq!(repeated.label, "Original");
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM works", [], |row| row.get::<_, i64>(0))?,
            1
        );
        Ok(())
    }

    #[test]
    fn a_label_collision_still_rejects_different_content() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut connection = test_connection()?;
        store_work(&mut connection, "Paper", "first bytes")?;
        let Err(error) = store_work(&mut connection, "PAPER", "different bytes") else {
            return Err("different content unexpectedly reused an occupied label".into());
        };

        assert_eq!(error.code(), "work_name_exists");
        Ok(())
    }

    #[test]
    fn whitespace_only_work_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let mut connection = test_connection()?;
        let Err(error) = store_work(&mut connection, "Blank", " \n\t") else {
            return Err("whitespace-only source text was unexpectedly retained".into());
        };

        assert_eq!(error.code(), "empty_work");
        Ok(())
    }

    #[test]
    fn non_head_inverse_preserves_later_independent_fields() {
        let before = Snapshot {
            concepts: vec![concept(1, None, "Old wording", 0, 1)],
            evidence: Vec::new(),
        };
        let after = Snapshot {
            concepts: vec![concept(1, None, "New wording", 0, 2)],
            evidence: Vec::new(),
        };
        let head = Snapshot {
            concepts: vec![
                concept(1, Some(2), "New wording", 0, 3),
                concept(2, None, "Parent", 0, 3),
            ],
            evidence: Vec::new(),
        };
        let inverse = invert_snapshot_change(2, 4, &before, &after, &head)
            .unwrap_or_else(|error| panic!("inverse unexpectedly failed: {error}"));
        let restored = inverse
            .concepts
            .iter()
            .find(|item| item.id == 1)
            .unwrap_or_else(|| panic!("restored concept is missing"));
        assert_eq!(restored.label, "Old wording");
        assert_eq!(restored.parent_id, Some(2));
        assert_eq!(restored.updated_revision, 4);
    }

    #[test]
    fn non_head_inverse_rejects_a_changed_postcondition() {
        let before = Snapshot {
            concepts: vec![concept(1, None, "Old wording", 0, 1)],
            evidence: Vec::new(),
        };
        let after = Snapshot {
            concepts: vec![concept(1, None, "New wording", 0, 2)],
            evidence: Vec::new(),
        };
        let head = Snapshot {
            concepts: vec![concept(1, None, "Later wording", 0, 3)],
            evidence: Vec::new(),
        };
        let Err(error) = invert_snapshot_change(2, 4, &before, &after, &head) else {
            panic!("conflicting inverse unexpectedly succeeded");
        };
        assert_eq!(error.code(), "revert_conflict");
    }

    #[test]
    fn reword_inverse_rejects_later_evidence_without_exposing_ids() {
        let before = Snapshot {
            concepts: vec![concept(987_654, None, "Old meaning", 0, 1)],
            evidence: vec![evidence(765_432, 987_654, 1, 0, 3)],
        };
        let after = Snapshot {
            concepts: vec![concept(987_654, None, "New meaning", 0, 2)],
            evidence: before.evidence.clone(),
        };
        let mut head = after.clone();
        head.evidence.push(evidence(765_433, 987_654, 2, 4, 8));
        let Err(error) = invert_snapshot_change(2, 4, &before, &after, &head) else {
            panic!("reword with later evidence unexpectedly reverted");
        };
        assert_eq!(error.code(), "revert_conflict");
        let message = error.to_string();
        assert!(message.contains("New meaning"));
        assert!(!message.contains("987654"));
        assert!(!message.contains("765433"));
    }

    #[test]
    fn move_inverse_rejects_later_evidence() {
        let concepts_before = vec![
            concept(1, None, "Old parent", 0, 1),
            concept(2, None, "New parent", 1024, 1),
            concept(3, Some(1), "Moved", 0, 1),
        ];
        let before = Snapshot {
            concepts: concepts_before,
            evidence: vec![evidence(1, 3, 1, 0, 3)],
        };
        let after = Snapshot {
            concepts: vec![
                concept(1, None, "Old parent", 0, 1),
                concept(2, None, "New parent", 1024, 1),
                concept(3, Some(2), "Moved", 0, 2),
            ],
            evidence: before.evidence.clone(),
        };
        let mut head = after.clone();
        head.evidence.push(evidence(2, 3, 2, 4, 8));
        let Err(error) = invert_snapshot_change(2, 4, &before, &after, &head) else {
            panic!("move with later evidence unexpectedly reverted");
        };
        assert_eq!(error.code(), "revert_conflict");
        assert!(error.to_string().contains("New parent"));
    }

    #[test]
    fn sibling_reordering_appears_in_semantic_diff() {
        let before = Snapshot {
            concepts: vec![
                concept(1, None, "First", 0, 1),
                concept(2, None, "Second", 1024, 1),
            ],
            evidence: Vec::new(),
        };
        let after = Snapshot {
            concepts: vec![
                concept(1, None, "First", 1024, 2),
                concept(2, None, "Second", 0, 2),
            ],
            evidence: Vec::new(),
        };
        let connection = Connection::open_in_memory()
            .unwrap_or_else(|error| panic!("in-memory database failed: {error}"));
        let entries = diff_snapshot_entries(&connection, &before, &after)
            .unwrap_or_else(|error| panic!("semantic diff failed: {error}"));
        assert!(entries.iter().any(|entry| {
            entry.kind == crate::model::DiffKind::Moved
                && entry.before == Some(vec!["Second".to_owned()])
        }));
    }

    fn concept(
        id: i64,
        parent_id: Option<i64>,
        label: &str,
        position: i64,
        updated_revision: i64,
    ) -> SnapshotConcept {
        SnapshotConcept {
            id,
            parent_id,
            label: label.to_owned(),
            position,
            created_revision: 1,
            updated_revision,
        }
    }

    fn evidence(
        id: i64,
        concept_id: i64,
        work_id: i64,
        start_byte: usize,
        end_byte: usize,
    ) -> SnapshotEvidence {
        SnapshotEvidence {
            id,
            concept_id,
            work_id,
            start_byte,
            end_byte,
            created_at: "2026-08-12T00:00:00Z".to_owned(),
        }
    }

    fn test_connection() -> Result<Connection, Box<dyn std::error::Error>> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(include_str!("../schema.sql"))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        Ok(connection)
    }
}
