#![allow(clippy::too_many_lines)]

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::change::Reconciliation;
use crate::error::AppError;
use crate::index;
use crate::model::{
    CommitView, ConceptId, ConceptReference, DiffEntry, DiffView, ReconciliationView,
    RecordedChangeView, WorkSummary, WorkView,
};

pub(crate) const MAX_EVIDENCE_BYTES: usize = 8 * 1024;

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
    pub edges: Vec<SnapshotEdge>,
    pub evidence: Vec<SnapshotEvidence>,
}

impl Snapshot {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            concepts: Vec::new(),
            edges: Vec::new(),
            evidence: Vec::new(),
        }
    }

    pub fn canonicalize(&mut self) {
        self.concepts.sort_by_key(|concept| concept.id);
        self.edges
            .sort_by_key(|edge| (edge.parent_id, edge.child_id));
        self.evidence.sort_by_key(|evidence| {
            (
                evidence.concept_id,
                evidence.work_id,
                evidence.start_byte,
                evidence.end_byte,
            )
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotConcept {
    pub id: i64,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotEdge {
    pub parent_id: i64,
    pub child_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotEvidence {
    pub concept_id: i64,
    pub work_id: i64,
    pub start_byte: usize,
    pub end_byte: usize,
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

#[derive(Debug, Clone)]
pub(crate) struct ShakePlan {
    pub base_revision: i64,
    pub edge_count_before: usize,
    pub edge_count_after: usize,
    pub removed_edges: Vec<ShakeEdge>,
    library_id: String,
    before: Snapshot,
    after: Snapshot,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ShakeEdge {
    pub parent: ConceptReference,
    pub child: ConceptReference,
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

fn library_id(connection: &Connection) -> Result<String, AppError> {
    Ok(connection.query_row(
        "SELECT library_id FROM library_state WHERE singleton = 1",
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

pub(crate) fn plan_shake(connection: &Connection) -> Result<ShakePlan, AppError> {
    let transaction = connection.unchecked_transaction()?;
    let base_revision = revision(&transaction)?;
    let library_id = library_id(&transaction)?;
    let before = head_snapshot(&transaction)?;
    validate_snapshot(&transaction, &before)?;
    transaction.commit()?;
    let (after, removed) = transitive_reduction(&before)?;
    let index = SnapshotIndex::new(&before);
    let removed_edges = removed
        .iter()
        .map(|edge| {
            Ok(ShakeEdge {
                parent: index.reference(edge.parent_id)?,
                child: index.reference(edge.child_id)?,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(ShakePlan {
        base_revision,
        edge_count_before: before.edges.len(),
        edge_count_after: after.edges.len(),
        removed_edges,
        library_id,
        before,
        after,
    })
}

pub(crate) fn apply_shake(connection: &mut Connection, plan: &ShakePlan) -> Result<i64, AppError> {
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let head_revision = revision(&transaction)?;
    let head_library_id = library_id(&transaction)?;
    let head = head_snapshot(&transaction)?;
    if head_library_id != plan.library_id
        || head_revision != plan.base_revision
        || head != plan.before
    {
        return Err(AppError::conflict(
            "shake_stale",
            format!(
                "the shake examined revision {}, but the library identity, HEAD, or its graph changed; run shake again",
                plan.base_revision
            ),
        ));
    }
    if plan.removed_edges.is_empty() {
        return Err(AppError::conflict(
            "nothing_to_shake",
            "the concept graph has no transitively implied parent edges",
        ));
    }
    let (after, removed) = transitive_reduction(&head)?;
    if after != plan.after || removed.len() != plan.removed_edges.len() {
        return Err(AppError::conflict(
            "shake_stale",
            "the confirmed shake plan no longer matches HEAD; run shake again",
        ));
    }
    validate_snapshot(&transaction, &after)?;
    let new_revision = head_revision.checked_add(1).ok_or_else(|| {
        AppError::database("revision_overflow", "the corpus revision is too large")
    })?;
    let resolved = diff_snapshot_entries(&transaction, &head, &after)?;
    materialize_snapshot(&transaction, &after)?;
    insert_commit(
        &transaction,
        new_revision,
        None,
        None,
        "shake",
        &shake_summary(removed.len()),
        &json!({ "operation": "transitive_reduction" }),
        &serde_json::to_value(&resolved)?,
        &after,
        "human",
    )?;
    transaction.commit()?;
    Ok(new_revision)
}

pub(crate) fn shake_summary(removed_edge_count: usize) -> String {
    let noun = if removed_edge_count == 1 {
        "edge"
    } else {
        "edges"
    };
    format!("Shake {removed_edge_count} transitively implied parent {noun}")
}

pub(crate) fn transitive_reduction(
    snapshot: &Snapshot,
) -> Result<(Snapshot, Vec<SnapshotEdge>), AppError> {
    let before_ancestors = ancestor_sets(snapshot)?;
    let index = SnapshotIndex::new(snapshot);
    let removed = snapshot
        .edges
        .iter()
        .filter(|edge| {
            index
                .parents
                .get(&edge.child_id)
                .into_iter()
                .flatten()
                .any(|candidate| {
                    *candidate != edge.parent_id
                        && before_ancestors
                            .get(candidate)
                            .is_some_and(|ancestors| ancestors.contains(&edge.parent_id))
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    let removed_set = removed.iter().collect::<BTreeSet<_>>();
    let mut after = snapshot.clone();
    after.edges.retain(|edge| !removed_set.contains(edge));
    if ancestor_sets(&after)? != before_ancestors {
        return Err(AppError::unexpected(
            "transitive_reduction_failed",
            "transitive reduction changed concept reachability",
        ));
    }
    Ok((after, removed))
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
    let mut concepts_statement =
        connection.prepare("SELECT id, label FROM concepts ORDER BY id")?;
    let concepts = concepts_statement
        .query_map([], |row| {
            Ok(SnapshotConcept {
                id: row.get(0)?,
                label: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut edges_statement = connection
        .prepare("SELECT parent_id, child_id FROM concept_edges ORDER BY parent_id, child_id")?;
    let edges = edges_statement
        .query_map([], |row| {
            Ok(SnapshotEdge {
                parent_id: row.get(0)?,
                child_id: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut evidence_statement = connection.prepare(
        "SELECT concept_id, work_id, start_byte, end_byte FROM evidence \
         ORDER BY concept_id, work_id, start_byte, end_byte",
    )?;
    let evidence = evidence_statement
        .query_map([], |row| {
            Ok(SnapshotEvidence {
                concept_id: row.get(0)?,
                work_id: row.get(1)?,
                start_byte: usize_from_i64(row.get(2)?, "evidence start byte")?,
                end_byte: usize_from_i64(row.get(3)?, "evidence end byte")?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Snapshot {
        concepts,
        edges,
        evidence,
    })
}

pub(crate) fn materialize_snapshot(
    transaction: &Transaction<'_>,
    snapshot: &Snapshot,
) -> Result<(), AppError> {
    validate_snapshot(transaction, snapshot)?;
    transaction.execute("DELETE FROM evidence", [])?;
    transaction.execute("DELETE FROM concept_edges", [])?;
    transaction.execute("DELETE FROM concepts", [])?;

    for concept in &snapshot.concepts {
        transaction.execute(
            "INSERT INTO concepts(id, label) VALUES(?1, ?2)",
            params![concept.id, concept.label],
        )?;
    }
    for edge in &snapshot.edges {
        transaction.execute(
            "INSERT INTO concept_edges(parent_id, child_id) VALUES(?1, ?2)",
            params![edge.parent_id, edge.child_id],
        )?;
    }
    for evidence in &snapshot.evidence {
        transaction.execute(
            "INSERT INTO evidence(concept_id, work_id, start_byte, end_byte) \
             VALUES(?1, ?2, ?3, ?4)",
            params![
                evidence.concept_id,
                evidence.work_id,
                i64_from_usize(evidence.start_byte, "evidence start byte")?,
                i64_from_usize(evidence.end_byte, "evidence end byte")?
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn validate_snapshot(
    connection: &Connection,
    snapshot: &Snapshot,
) -> Result<(), AppError> {
    if !strictly_sorted_by(&snapshot.concepts, |concept| concept.id) {
        return Err(invalid_change(
            "snapshot concepts must be strictly ordered by positive ID",
        ));
    }
    let concepts = snapshot
        .concepts
        .iter()
        .map(|concept| (concept.id, concept))
        .collect::<BTreeMap<_, _>>();
    for concept in &snapshot.concepts {
        if concept.id <= 0 {
            return Err(invalid_change("concept IDs must be positive"));
        }
        validate_label(&concept.label, "concept label")?;
    }

    if !strictly_sorted_by(&snapshot.edges, |edge| (edge.parent_id, edge.child_id)) {
        return Err(invalid_change(
            "snapshot edges must be strictly ordered by parent and child ID",
        ));
    }
    let mut indegree = concepts
        .keys()
        .map(|id| (*id, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut children = BTreeMap::<i64, BTreeSet<i64>>::new();
    for edge in &snapshot.edges {
        if edge.parent_id == edge.child_id {
            return Err(AppError::conflict(
                "would_create_cycle",
                format!("concept c{} cannot be its own parent", edge.parent_id),
            ));
        }
        if !concepts.contains_key(&edge.parent_id) || !concepts.contains_key(&edge.child_id) {
            return Err(invalid_change("an edge names a missing concept"));
        }
        children
            .entry(edge.parent_id)
            .or_default()
            .insert(edge.child_id);
        *indegree
            .get_mut(&edge.child_id)
            .ok_or_else(|| invalid_change("an edge names a missing child concept"))? += 1;
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect::<BTreeSet<_>>();
    let mut visited = 0_usize;
    while let Some(id) = ready.pop_first() {
        visited += 1;
        for child in children.get(&id).into_iter().flatten() {
            let degree = indegree
                .get_mut(child)
                .ok_or_else(|| invalid_change("an edge names a missing child concept"))?;
            *degree -= 1;
            if *degree == 0 {
                ready.insert(*child);
            }
        }
    }
    if visited != concepts.len() {
        return Err(AppError::conflict(
            "would_create_cycle",
            "the concept graph contains a directed cycle",
        ));
    }

    if !strictly_sorted_by(&snapshot.evidence, |evidence| {
        (
            evidence.concept_id,
            evidence.work_id,
            evidence.start_byte,
            evidence.end_byte,
        )
    }) {
        return Err(invalid_change(
            "snapshot evidence must be strictly ordered by its composite identity",
        ));
    }
    let work_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.work_id)
        .collect::<BTreeSet<_>>();
    let works = load_work_texts(connection, &work_ids)?;
    for evidence in &snapshot.evidence {
        if !concepts.contains_key(&evidence.concept_id) {
            return Err(invalid_change("evidence names a missing concept"));
        }
        let Some(work) = works.get(&evidence.work_id) else {
            return Err(invalid_change("evidence names a missing immutable work"));
        };
        if evidence.end_byte.saturating_sub(evidence.start_byte) > MAX_EVIDENCE_BYTES {
            return Err(invalid_change(format!(
                "evidence cannot exceed {MAX_EVIDENCE_BYTES} UTF-8 bytes"
            )));
        }
        if evidence.end_byte <= evidence.start_byte
            || evidence.end_byte > work.len()
            || !work.is_char_boundary(evidence.start_byte)
            || !work.is_char_boundary(evidence.end_byte)
        {
            return Err(invalid_change("evidence has an invalid UTF-8 source range"));
        }
    }
    for concept in &snapshot.concepts {
        if !children.contains_key(&concept.id)
            && !snapshot
                .evidence
                .iter()
                .any(|evidence| evidence.concept_id == concept.id)
        {
            return Err(AppError::conflict(
                "ungrounded_leaf",
                format!("leaf concept c{} has no source evidence", concept.id),
            ));
        }
    }
    Ok(())
}

fn strictly_sorted_by<T, K: Ord>(items: &[T], key: impl Fn(&T) -> K) -> bool {
    items.windows(2).all(|pair| key(&pair[0]) < key(&pair[1]))
}

struct SnapshotIndex<'a> {
    concepts: BTreeMap<i64, &'a SnapshotConcept>,
    parents: BTreeMap<i64, BTreeSet<i64>>,
    children: BTreeMap<i64, BTreeSet<i64>>,
}

impl<'a> SnapshotIndex<'a> {
    fn new(snapshot: &'a Snapshot) -> Self {
        let concepts = snapshot
            .concepts
            .iter()
            .map(|concept| (concept.id, concept))
            .collect();
        let mut parents = BTreeMap::<i64, BTreeSet<i64>>::new();
        let mut children = BTreeMap::<i64, BTreeSet<i64>>::new();
        for edge in &snapshot.edges {
            parents
                .entry(edge.child_id)
                .or_default()
                .insert(edge.parent_id);
            children
                .entry(edge.parent_id)
                .or_default()
                .insert(edge.child_id);
        }
        Self {
            concepts,
            parents,
            children,
        }
    }

    fn reference(&self, id: i64) -> Result<ConceptReference, AppError> {
        let concept = self.concepts.get(&id).copied().ok_or_else(|| {
            AppError::database(
                "concept_reference_missing",
                format!("concept c{id} is missing"),
            )
        })?;
        Ok(ConceptReference {
            id: public_id(id)?,
            label: concept.label.clone(),
        })
    }
}

fn public_id(id: i64) -> Result<ConceptId, AppError> {
    ConceptId::from_storage(id).map_err(|error| {
        AppError::database(
            "invalid_concept_id",
            format!("stored concept ID {id}: {error}"),
        )
    })
}

fn read_snapshot(connection: &Connection, requested_revision: i64) -> Result<Snapshot, AppError> {
    let snapshot = snapshot_at(connection, requested_revision)?;
    validate_snapshot(connection, &snapshot).map_err(|error| {
        AppError::database(
            "invalid_history_snapshot",
            format!("revision {requested_revision} is invalid: {error}"),
        )
    })?;
    Ok(snapshot)
}

fn ancestor_sets(snapshot: &Snapshot) -> Result<BTreeMap<i64, BTreeSet<i64>>, AppError> {
    let index = SnapshotIndex::new(snapshot);
    let mut indegree = snapshot
        .concepts
        .iter()
        .map(|concept| {
            (
                concept.id,
                index.parents.get(&concept.id).map_or(0, BTreeSet::len),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut output = snapshot
        .concepts
        .iter()
        .map(|concept| (concept.id, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut ready = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect::<BTreeSet<_>>();
    while let Some(parent) = ready.pop_first() {
        let mut inherited = output[&parent].clone();
        inherited.insert(parent);
        for child in index.children.get(&parent).into_iter().flatten() {
            output
                .get_mut(child)
                .ok_or_else(|| {
                    AppError::database(
                        "concept_reference_missing",
                        format!("concept c{child} has no ancestor set"),
                    )
                })?
                .extend(inherited.iter().copied());
            let degree = indegree.get_mut(child).ok_or_else(|| {
                AppError::database(
                    "concept_reference_missing",
                    format!("concept c{child} has no indegree"),
                )
            })?;
            *degree -= 1;
            if *degree == 0 {
                ready.insert(*child);
            }
        }
    }
    Ok(output)
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
        "SELECT c.revision, c.kind, c.summary, w.label, c.actor, c.created_at \
         FROM commits AS c LEFT JOIN works AS w ON w.id = c.work_id \
         ORDER BY c.revision DESC LIMIT ?1",
    )?;
    let rows = statement.query_map([sql_limit], |row| {
        Ok(CommitView {
            revision: row.get(0)?,
            kind: row.get(1)?,
            summary: row.get(2)?,
            work: row.get(3)?,
            actor: row.get(4)?,
            created_at: row.get(5)?,
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
            "SELECT c.revision, c.kind, c.summary, w.label, c.submitted_request, \
                    c.resolved_operations, c.actor, c.created_at \
             FROM commits AS c LEFT JOIN works AS w ON w.id = c.work_id \
             WHERE c.revision = ?1",
            [requested_revision],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((
        revision,
        kind,
        summary,
        work,
        reconciliation,
        resolved_operations,
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
    let effects = diff(connection, revision - 1, revision)?.entries;
    Ok(RecordedChangeView {
        revision,
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
        effects,
        actor,
        created_at,
    })
}

pub(crate) fn diff(
    connection: &Connection,
    from_revision: i64,
    to_revision: i64,
) -> Result<DiffView, AppError> {
    let before = read_snapshot(connection, from_revision)?;
    let after = read_snapshot(connection, to_revision)?;
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
            (None, Some(concept)) => entries.push(DiffEntry::Created {
                concept: concept_reference(concept)?,
            }),
            (Some(concept), None) => entries.push(DiffEntry::Retired {
                concept: concept_reference(concept)?,
            }),
            (Some(old), Some(new)) if old.label != new.label => {
                entries.push(DiffEntry::Reworded {
                    id: public_id(id)?,
                    before: old.label.clone(),
                    after: new.label.clone(),
                });
            }
            (None, None) | (Some(_), Some(_)) => {}
        }
    }

    let before_edges = before.edges.iter().cloned().collect::<BTreeSet<_>>();
    let after_edges = after.edges.iter().cloned().collect::<BTreeSet<_>>();
    for edge in before_edges.difference(&after_edges) {
        entries.push(DiffEntry::ParentRemoved {
            parent: snapshot_reference(before, edge.parent_id)?,
            child: snapshot_reference(before, edge.child_id)?,
        });
    }
    for edge in after_edges.difference(&before_edges) {
        entries.push(DiffEntry::ParentAdded {
            parent: snapshot_reference(after, edge.parent_id)?,
            child: snapshot_reference(after, edge.child_id)?,
        });
    }

    let before_evidence = before.evidence.iter().cloned().collect::<BTreeSet<_>>();
    let after_evidence = after.evidence.iter().cloned().collect::<BTreeSet<_>>();
    for evidence in before_evidence.difference(&after_evidence) {
        let work = get_work_by_id(connection, evidence.work_id)?;
        entries.push(DiffEntry::EvidenceRemoved {
            concept: snapshot_reference(before, evidence.concept_id)?,
            work: work.label,
            quote: work.text[evidence.start_byte..evidence.end_byte].to_owned(),
        });
    }
    for evidence in after_evidence.difference(&before_evidence) {
        let work = get_work_by_id(connection, evidence.work_id)?;
        entries.push(DiffEntry::EvidenceAdded {
            concept: snapshot_reference(after, evidence.concept_id)?,
            work: work.label,
            quote: work.text[evidence.start_byte..evidence.end_byte].to_owned(),
        });
    }
    Ok(entries)
}

fn concept_reference(concept: &SnapshotConcept) -> Result<ConceptReference, AppError> {
    Ok(ConceptReference {
        id: public_id(concept.id)?,
        label: concept.label.clone(),
    })
}

fn snapshot_reference(snapshot: &Snapshot, id: i64) -> Result<ConceptReference, AppError> {
    snapshot
        .concepts
        .binary_search_by_key(&id, |concept| concept.id)
        .ok()
        .and_then(|index| snapshot.concepts.get(index))
        .ok_or_else(|| {
            AppError::database(
                "concept_reference_missing",
                format!("snapshot concept c{id} is missing"),
            )
        })
        .and_then(concept_reference)
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
    after: &Snapshot,
    actor: &str,
) -> Result<(), AppError> {
    let parent = revision.checked_sub(1).ok_or_else(|| {
        AppError::database("revision_underflow", "a commit revision must be positive")
    })?;
    let current = self::revision(transaction)?;
    if current != parent {
        return Err(AppError::conflict(
            "commit_parent_mismatch",
            format!("cannot append revision {revision} while canonical HEAD is revision {current}"),
        ));
    }
    if parent > 0 {
        let parent_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM commits WHERE revision = ?1)",
            [parent],
            |row| row.get::<_, bool>(0),
        )?;
        if !parent_exists {
            return Err(AppError::database(
                "commit_parent_missing",
                format!("cannot append revision {revision} because revision {parent} is missing"),
            ));
        }
    }
    transaction.execute(
        "INSERT INTO commits(\
             revision, work_id, reconciliation_id, kind, summary, submitted_request, \
             resolved_operations, after_snapshot, actor, created_at\
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            revision,
            work_id,
            reconciliation_id,
            kind,
            summary,
            serde_json::to_string(request)?,
            serde_json::to_string(resolved)?,
            serde_json::to_string(after)?,
            actor,
            now()?
        ],
    )?;
    let advanced = transaction.execute(
        "UPDATE library_state SET revision = ?1 WHERE singleton = 1 AND revision = ?2",
        params![revision, parent],
    )?;
    if advanced != 1 {
        return Err(AppError::conflict(
            "commit_parent_mismatch",
            "canonical HEAD changed while the commit was being appended",
        ));
    }
    crate::revision_store::insert_canonical_revision(transaction, revision, after)?;
    Ok(())
}

pub(crate) fn revert(transaction: &Transaction<'_>, target_revision: i64) -> Result<i64, AppError> {
    if target_revision <= 0 {
        return Err(revision_not_found(target_revision));
    }
    let target_work = transaction
        .query_row(
            "SELECT work_id FROM commits WHERE revision = ?1",
            [target_revision],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()?
        .ok_or_else(|| revision_not_found(target_revision))?;
    let target_before = snapshot_at(transaction, target_revision - 1)?;
    let target_after = snapshot_at(transaction, target_revision)?;
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
    let inverse = invert_snapshot_change(target_revision, &target_before, &target_after, &head)?;
    validate_snapshot(transaction, &inverse).map_err(|error| {
        revert_conflict(
            target_revision,
            format!("the inverse would violate corpus invariants: {error}"),
        )
    })?;
    materialize_snapshot(transaction, &inverse)?;
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
        &inverse,
        "human",
    )?;
    Ok(new_revision)
}

pub(crate) fn invert_snapshot_change(
    target_revision: i64,
    target_before: &Snapshot,
    target_after: &Snapshot,
    head: &Snapshot,
) -> Result<Snapshot, AppError> {
    let before_concepts = owned_concept_map(target_before);
    let after_concepts = owned_concept_map(target_after);
    let head_concepts = owned_concept_map(head);
    let concept_ids = before_concepts
        .keys()
        .chain(after_concepts.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    for id in &concept_ids {
        match (before_concepts.get(id), after_concepts.get(id)) {
            (None, Some(created)) => require_revert_state(
                target_revision,
                head_concepts.get(id) == Some(created),
                format!("created concept c{id} no longer matches its after-state"),
            )?,
            (Some(_), None) => require_revert_state(
                target_revision,
                !head_concepts.contains_key(id),
                format!("retired concept c{id} has since been restored or reused"),
            )?,
            (Some(before), Some(after)) if before != after => require_revert_state(
                target_revision,
                head_concepts.get(id) == Some(after),
                format!("changed concept c{id} no longer matches its after-state"),
            )?,
            _ => {}
        }
    }

    let before_edges = target_before.edges.iter().cloned().collect::<BTreeSet<_>>();
    let after_edges = target_after.edges.iter().cloned().collect::<BTreeSet<_>>();
    let head_edges = head.edges.iter().cloned().collect::<BTreeSet<_>>();
    for edge in after_edges.difference(&before_edges) {
        require_revert_state(
            target_revision,
            head_edges.contains(edge),
            format!(
                "added parent edge c{} -> c{} is no longer present",
                edge.parent_id, edge.child_id
            ),
        )?;
    }
    for edge in before_edges.difference(&after_edges) {
        require_revert_state(
            target_revision,
            !head_edges.contains(edge),
            format!(
                "removed parent edge c{} -> c{} has since been restored",
                edge.parent_id, edge.child_id
            ),
        )?;
    }

    let before_evidence = target_before
        .evidence
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let after_evidence = target_after
        .evidence
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let head_evidence = head.evidence.iter().cloned().collect::<BTreeSet<_>>();
    for evidence in after_evidence.difference(&before_evidence) {
        require_revert_state(
            target_revision,
            head_evidence.contains(evidence),
            "added source evidence is no longer present".to_owned(),
        )?;
    }
    for evidence in before_evidence.difference(&after_evidence) {
        require_revert_state(
            target_revision,
            !head_evidence.contains(evidence),
            "removed source evidence has since been restored".to_owned(),
        )?;
    }

    for id in concept_ids
        .iter()
        .filter(|id| !before_concepts.contains_key(id) && after_concepts.contains_key(id))
    {
        let target_incident_edges = after_edges
            .iter()
            .filter(|edge| edge.parent_id == *id || edge.child_id == *id)
            .collect::<BTreeSet<_>>();
        let head_incident_edges = head_edges
            .iter()
            .filter(|edge| edge.parent_id == *id || edge.child_id == *id)
            .collect::<BTreeSet<_>>();
        let target_incident_evidence = after_evidence
            .iter()
            .filter(|evidence| evidence.concept_id == *id)
            .collect::<BTreeSet<_>>();
        let head_incident_evidence = head_evidence
            .iter()
            .filter(|evidence| evidence.concept_id == *id)
            .collect::<BTreeSet<_>>();
        require_revert_state(
            target_revision,
            target_incident_edges == head_incident_edges
                && target_incident_evidence == head_incident_evidence,
            format!("created concept c{id} has later incident edges or evidence"),
        )?;
    }

    let mut result_concepts = head_concepts;
    for id in concept_ids {
        match (before_concepts.get(&id), after_concepts.get(&id)) {
            (None, Some(_)) => {
                result_concepts.remove(&id);
            }
            (Some(before), None | Some(_)) => {
                result_concepts.insert(id, before.clone());
            }
            (None, None) => {}
        }
    }
    let mut result_edges = head_edges;
    for edge in after_edges.difference(&before_edges) {
        result_edges.remove(edge);
    }
    result_edges.extend(before_edges.difference(&after_edges).cloned());
    let mut result_evidence = head_evidence;
    for evidence in after_evidence.difference(&before_evidence) {
        result_evidence.remove(evidence);
    }
    result_evidence.extend(before_evidence.difference(&after_evidence).cloned());
    Ok(Snapshot {
        concepts: result_concepts.into_values().collect(),
        edges: result_edges.into_iter().collect(),
        evidence: result_evidence.into_iter().collect(),
    })
}

fn owned_concept_map(snapshot: &Snapshot) -> BTreeMap<i64, SnapshotConcept> {
    snapshot
        .concepts
        .iter()
        .cloned()
        .map(|concept| (concept.id, concept))
        .collect()
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use rusqlite::Connection;

    use super::{
        MAX_EVIDENCE_BYTES, Snapshot, SnapshotConcept, SnapshotEdge, SnapshotEvidence, apply_shake,
        diff_snapshot_entries, heading_for_offset, invert_snapshot_change, markdown_headings,
        materialize_snapshot, plan_shake, sha256_hex, store_work, transitive_reduction,
        validate_snapshot,
    };
    use crate::model::DiffEntry;

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
        assert!(
            connection
                .execute(
                    "UPDATE works SET label = 'Changed' WHERE id = ?1",
                    [retained.id]
                )
                .is_err(),
            "an immutable work was updated"
        );
        Ok(())
    }

    #[test]
    fn evidence_ranges_have_a_hard_byte_ceiling() -> Result<(), Box<dyn std::error::Error>> {
        let mut connection = test_connection()?;
        let text = "x".repeat(MAX_EVIDENCE_BYTES + 1);
        let work = store_work(&mut connection, "Source", &text)?;
        let snapshot = Snapshot {
            concepts: vec![concept(1, "Leaf")],
            edges: Vec::new(),
            evidence: vec![SnapshotEvidence {
                concept_id: 1,
                work_id: work.id,
                start_byte: 0,
                end_byte: text.len(),
            }],
        };
        let Err(error) = validate_snapshot(&connection, &snapshot) else {
            return Err("an oversized evidence range was accepted".into());
        };
        assert!(error.to_string().contains("8192"));
        Ok(())
    }

    #[test]
    fn dag_validation_allows_duplicate_labels_multiple_parents_and_redundant_edges()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Source", "grounded")?;
        let snapshot = Snapshot {
            concepts: vec![
                concept(1, "Same"),
                concept(2, "Same"),
                concept(3, "Shared"),
                concept(4, "Leaf"),
            ],
            edges: vec![edge(1, 3), edge(1, 4), edge(2, 3), edge(3, 4)],
            evidence: vec![SnapshotEvidence {
                concept_id: 4,
                work_id: work.id,
                start_byte: 0,
                end_byte: 8,
            }],
        };
        validate_snapshot(&connection, &snapshot)?;
        Ok(())
    }

    #[test]
    fn dag_validation_rejects_cycles_and_noncanonical_sets()
    -> Result<(), Box<dyn std::error::Error>> {
        let connection = test_connection()?;
        let cycle = Snapshot {
            concepts: vec![concept(1, "A"), concept(2, "B")],
            edges: vec![edge(1, 2), edge(2, 1)],
            evidence: Vec::new(),
        };
        let Err(cycle_error) = validate_snapshot(&connection, &cycle) else {
            panic!("cycle was accepted");
        };
        assert_eq!(cycle_error.code(), "would_create_cycle");

        let unordered = Snapshot {
            concepts: vec![concept(2, "B"), concept(1, "A")],
            edges: Vec::new(),
            evidence: Vec::new(),
        };
        let Err(order_error) = validate_snapshot(&connection, &unordered) else {
            panic!("unordered concepts were accepted");
        };
        assert_eq!(order_error.code(), "invalid_change");
        Ok(())
    }

    #[test]
    fn transitive_reduction_keeps_both_sides_of_a_diamond() {
        let snapshot = graph_snapshot(&[1, 2, 3, 4], &[(1, 2), (1, 3), (2, 4), (3, 4)]);
        let (reduced, removed) = transitive_reduction(&snapshot)
            .unwrap_or_else(|error| panic!("reduction failed: {error}"));
        assert!(removed.is_empty());
        assert_eq!(reduced, snapshot);
    }

    #[test]
    fn one_reduction_removes_interacting_shortcuts_from_a_complete_order() {
        let snapshot = graph_snapshot(
            &[1, 2, 3, 4],
            &[(1, 2), (1, 3), (1, 4), (2, 3), (2, 4), (3, 4)],
        );
        let (reduced, removed) = transitive_reduction(&snapshot)
            .unwrap_or_else(|error| panic!("reduction failed: {error}"));
        assert_eq!(removed, [edge(1, 3), edge(1, 4), edge(2, 4)]);
        assert_eq!(reduced.edges, [edge(1, 2), edge(2, 3), edge(3, 4)]);
        assert_eq!(reachable_pairs(&reduced), reachable_pairs(&snapshot));
    }

    #[test]
    fn confirmed_shake_rejects_a_changed_library_or_materialized_graph()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Source", "grounded")?;
        let snapshot = Snapshot {
            concepts: vec![concept(1, "A"), concept(2, "B"), concept(3, "C")],
            edges: vec![edge(1, 2), edge(1, 3), edge(2, 3)],
            evidence: vec![SnapshotEvidence {
                concept_id: 3,
                work_id: work.id,
                start_byte: 0,
                end_byte: 8,
            }],
        };
        let transaction = connection.transaction()?;
        materialize_snapshot(&transaction, &snapshot)?;
        transaction.commit()?;
        let plan = plan_shake(&connection)?;
        let mut other_library_id = plan.library_id.clone();
        let replacement = if other_library_id.starts_with('0') {
            "1"
        } else {
            "0"
        };
        other_library_id.replace_range(..1, replacement);
        connection.execute(
            "UPDATE library_state SET library_id = ?1",
            [&other_library_id],
        )?;

        let Err(error) = apply_shake(&mut connection, &plan) else {
            panic!("shake was applied to a different library identity");
        };
        assert_eq!(error.code(), "shake_stale");
        connection.execute(
            "UPDATE library_state SET library_id = ?1",
            [&plan.library_id],
        )?;
        connection.execute(
            "DELETE FROM concept_edges WHERE parent_id = 1 AND child_id = 3",
            [],
        )?;

        let Err(error) = apply_shake(&mut connection, &plan) else {
            panic!("stale shake was applied");
        };
        assert_eq!(error.code(), "shake_stale");
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM commits", [], |row| row
                .get::<_, i64>(0))?,
            0
        );
        Ok(())
    }

    #[test]
    fn transitive_reduction_is_exhaustively_minimal_for_five_node_dags() {
        let ids = [40, 7, 90, 2, 55];
        let possible_edges = (0..ids.len())
            .flat_map(|parent| {
                ((parent + 1)..ids.len()).map(move |child| (ids[parent], ids[child]))
            })
            .collect::<Vec<_>>();
        for mask in 0..(1_usize << possible_edges.len()) {
            let selected = possible_edges
                .iter()
                .enumerate()
                .filter_map(|(bit, edge)| ((mask & (1 << bit)) != 0).then_some(*edge))
                .collect::<Vec<_>>();
            let snapshot = graph_snapshot(&ids, &selected);
            let original_reachability = reachable_pairs(&snapshot);
            let (reduced, removed) = transitive_reduction(&snapshot)
                .unwrap_or_else(|error| panic!("reduction failed for mask {mask}: {error}"));
            let repeated = transitive_reduction(&snapshot).unwrap_or_else(|error| {
                panic!("repeated reduction failed for mask {mask}: {error}")
            });

            assert_eq!(
                reachable_pairs(&reduced),
                original_reachability,
                "mask {mask}"
            );
            assert_eq!(repeated, (reduced.clone(), removed.clone()), "mask {mask}");
            assert_eq!(roots_and_leaves(&reduced), roots_and_leaves(&snapshot));
            assert!(
                reduced.edges.windows(2).all(|pair| pair[0] < pair[1]),
                "mask {mask}"
            );
            for removed_edge in &removed {
                let mut without_edge = snapshot.clone();
                without_edge.edges.retain(|edge| edge != removed_edge);
                assert_eq!(
                    reachable_pairs(&without_edge),
                    original_reachability,
                    "reported edge was not redundant: {removed_edge:?} for mask {mask}"
                );
            }
            for retained_edge in &reduced.edges {
                let mut without_edge = reduced.clone();
                without_edge.edges.retain(|edge| edge != retained_edge);
                assert_ne!(
                    reachable_pairs(&without_edge),
                    original_reachability,
                    "retained redundant edge {retained_edge:?} for mask {mask}"
                );
            }
            let (second, removed_again) = transitive_reduction(&reduced)
                .unwrap_or_else(|error| panic!("second reduction failed for mask {mask}: {error}"));
            assert_eq!(second, reduced, "mask {mask}");
            assert!(removed_again.is_empty(), "mask {mask}");
        }
    }

    #[test]
    fn set_wise_inverse_preserves_unrelated_later_graph_changes() {
        let before = Snapshot {
            concepts: vec![concept(1, "Old"), concept(2, "Other")],
            edges: Vec::new(),
            evidence: Vec::new(),
        };
        let after = Snapshot {
            concepts: vec![concept(1, "New"), concept(2, "Other")],
            edges: Vec::new(),
            evidence: Vec::new(),
        };
        let head = Snapshot {
            concepts: after.concepts.clone(),
            edges: vec![edge(2, 1)],
            evidence: Vec::new(),
        };
        let inverse = invert_snapshot_change(1, &before, &after, &head)
            .unwrap_or_else(|error| panic!("inverse failed: {error}"));
        assert_eq!(inverse.concepts[0].label, "Old");
        assert_eq!(inverse.edges, [edge(2, 1)]);
    }

    #[test]
    fn set_wise_inverse_conflicts_if_a_created_concept_gained_a_later_edge() {
        let before = Snapshot {
            concepts: vec![concept(1, "Parent"), concept(3, "Later parent")],
            edges: Vec::new(),
            evidence: Vec::new(),
        };
        let after = Snapshot {
            concepts: vec![
                concept(1, "Parent"),
                concept(2, "Created"),
                concept(3, "Later parent"),
            ],
            edges: vec![edge(1, 2)],
            evidence: Vec::new(),
        };
        let head = Snapshot {
            concepts: after.concepts.clone(),
            edges: vec![edge(1, 2), edge(3, 2)],
            evidence: Vec::new(),
        };
        let Err(error) = invert_snapshot_change(1, &before, &after, &head) else {
            panic!("destructive inverse succeeded");
        };
        assert_eq!(error.code(), "revert_conflict");
    }

    #[test]
    fn edge_diff_is_graph_native() {
        let before = Snapshot {
            concepts: vec![concept(1, "Parent"), concept(2, "Child")],
            edges: Vec::new(),
            evidence: Vec::new(),
        };
        let after = Snapshot {
            concepts: before.concepts.clone(),
            edges: vec![edge(1, 2)],
            evidence: Vec::new(),
        };
        let connection = Connection::open_in_memory()
            .unwrap_or_else(|error| panic!("in-memory database failed: {error}"));
        let entries = diff_snapshot_entries(&connection, &before, &after)
            .unwrap_or_else(|error| panic!("diff failed: {error}"));
        assert!(matches!(
            entries.as_slice(),
            [DiffEntry::ParentAdded { parent, child }]
                if parent.id.to_string() == "c1" && child.id.to_string() == "c2"
        ));
    }

    fn concept(id: i64, label: &str) -> SnapshotConcept {
        SnapshotConcept {
            id,
            label: label.to_owned(),
        }
    }

    fn edge(parent_id: i64, child_id: i64) -> SnapshotEdge {
        SnapshotEdge {
            parent_id,
            child_id,
        }
    }

    fn graph_snapshot(ids: &[i64], edges: &[(i64, i64)]) -> Snapshot {
        let mut snapshot = Snapshot {
            concepts: ids
                .iter()
                .map(|id| concept(*id, &format!("Concept {id}")))
                .collect(),
            edges: edges
                .iter()
                .map(|(parent, child)| edge(*parent, *child))
                .collect(),
            evidence: Vec::new(),
        };
        snapshot.canonicalize();
        snapshot
    }

    fn reachable_pairs(snapshot: &Snapshot) -> BTreeSet<(i64, i64)> {
        let mut children = BTreeMap::<i64, Vec<i64>>::new();
        for edge in &snapshot.edges {
            children
                .entry(edge.parent_id)
                .or_default()
                .push(edge.child_id);
        }
        let mut pairs = BTreeSet::new();
        for concept in &snapshot.concepts {
            let mut frontier = children
                .get(&concept.id)
                .into_iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            while let Some(descendant) = frontier.pop() {
                if pairs.insert((concept.id, descendant)) {
                    frontier.extend(children.get(&descendant).into_iter().flatten().copied());
                }
            }
        }
        pairs
    }

    fn roots_and_leaves(snapshot: &Snapshot) -> (BTreeSet<i64>, BTreeSet<i64>) {
        let parents = snapshot
            .edges
            .iter()
            .map(|edge| edge.child_id)
            .collect::<BTreeSet<_>>();
        let children = snapshot
            .edges
            .iter()
            .map(|edge| edge.parent_id)
            .collect::<BTreeSet<_>>();
        let ids = snapshot
            .concepts
            .iter()
            .map(|concept| concept.id)
            .collect::<BTreeSet<_>>();
        (
            ids.difference(&parents).copied().collect(),
            ids.difference(&children).copied().collect(),
        )
    }

    fn test_connection() -> Result<Connection, Box<dyn std::error::Error>> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(include_str!("../schema.sql"))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        Ok(connection)
    }
}
