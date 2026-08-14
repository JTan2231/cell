#![allow(clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::change::Reconciliation;
use crate::error::AppError;
use crate::index;
use crate::model::{
    CommitView, ConceptDetail, ConceptId, ConceptReference, ConceptSummary, CorpusOverview,
    DiffEntry, DiffView, EvidenceView, FrontierEntry, GraphDirection, GraphEdge, GraphNode,
    GraphView, Page, PageInfo, ReconciliationView, RecordedChangeView, SearchOutput, SearchResult,
    WorkSummary, WorkView,
};

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
    evidence_count: BTreeMap<i64, u64>,
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
        let mut evidence_count = BTreeMap::new();
        for evidence in &snapshot.evidence {
            *evidence_count.entry(evidence.concept_id).or_insert(0) += 1;
        }
        Self {
            concepts,
            parents,
            children,
            evidence_count,
        }
    }

    fn require(&self, id: ConceptId) -> Result<&'a SnapshotConcept, AppError> {
        self.concepts.get(&id.storage_id()).copied().ok_or_else(|| {
            AppError::not_found(
                "concept_not_found",
                format!("concept {id} was not found in the requested revision"),
            )
        })
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

    fn summary(&self, id: i64) -> Result<ConceptSummary, AppError> {
        let reference = self.reference(id)?;
        let parent_count = count_u64(self.parents.get(&id).map_or(0, BTreeSet::len))?;
        let child_count = count_u64(self.children.get(&id).map_or(0, BTreeSet::len))?;
        Ok(ConceptSummary {
            id: reference.id,
            label: reference.label,
            parent_count,
            child_count,
            evidence_count: self.evidence_count.get(&id).copied().unwrap_or(0),
            root: parent_count == 0,
            leaf: child_count == 0,
            shared: parent_count > 1,
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

fn count_u64(value: usize) -> Result<u64, AppError> {
    u64::try_from(value)
        .map_err(|_| AppError::database("numeric_overflow", "a corpus count is too large"))
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

pub(crate) fn corpus_overview(
    connection: &Connection,
    requested_revision: i64,
) -> Result<CorpusOverview, AppError> {
    let snapshot = read_snapshot(connection, requested_revision)?;
    let index = SnapshotIndex::new(&snapshot);
    Ok(CorpusOverview {
        revision: requested_revision,
        concept_count: count_u64(snapshot.concepts.len())?,
        edge_count: count_u64(snapshot.edges.len())?,
        root_count: count_u64(
            snapshot
                .concepts
                .iter()
                .filter(|concept| !index.parents.contains_key(&concept.id))
                .count(),
        )?,
        leaf_count: count_u64(
            snapshot
                .concepts
                .iter()
                .filter(|concept| !index.children.contains_key(&concept.id))
                .count(),
        )?,
        shared_concept_count: count_u64(
            snapshot
                .concepts
                .iter()
                .filter(|concept| {
                    index
                        .parents
                        .get(&concept.id)
                        .is_some_and(|set| set.len() > 1)
                })
                .count(),
        )?,
        evidence_count: count_u64(snapshot.evidence.len())?,
    })
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    library_id: String,
    revision: i64,
    context: String,
    offset: usize,
}

fn encode_cursor(
    library_id: &str,
    revision: i64,
    context: &str,
    offset: usize,
) -> Result<String, AppError> {
    let bytes = serde_json::to_vec(&Cursor {
        version: 2,
        library_id: library_id.to_owned(),
        revision,
        context: context.to_owned(),
        offset,
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn decode_cursor_value(encoded: &str) -> Result<Cursor, AppError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| AppError::invalid("invalid_cursor", "the pagination cursor is not valid"))?;
    let cursor: Cursor = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::invalid("invalid_cursor", "the pagination cursor is not valid"))?;
    if cursor.version != 2 {
        return Err(AppError::invalid(
            "invalid_cursor",
            "the pagination cursor version is not supported",
        ));
    }
    Ok(cursor)
}

fn decode_cursor(
    encoded: Option<&str>,
    library_id: &str,
    revision: i64,
    context: &str,
) -> Result<usize, AppError> {
    let Some(encoded) = encoded else {
        return Ok(0);
    };
    let cursor = decode_cursor_value(encoded)?;
    if cursor.library_id != library_id || cursor.revision != revision || cursor.context != context {
        return Err(AppError::invalid(
            "invalid_cursor",
            "the pagination cursor belongs to a different library, revision, or request",
        ));
    }
    Ok(cursor.offset)
}

pub(crate) fn page_revision(
    connection: &Connection,
    requested: Option<i64>,
    cursor: Option<&str>,
) -> Result<i64, AppError> {
    let expected_library_id = library_id(connection)?;
    let cursor_revision = cursor
        .map(decode_cursor_value)
        .transpose()?
        .map(|cursor| {
            if cursor.library_id == expected_library_id {
                Ok(cursor.revision)
            } else {
                Err(AppError::invalid(
                    "invalid_cursor",
                    "the pagination cursor belongs to a different library",
                ))
            }
        })
        .transpose()?;
    let resolved = match (requested, cursor_revision) {
        (Some(requested), Some(from_cursor)) if requested != from_cursor => {
            return Err(AppError::invalid(
                "invalid_cursor",
                "the pagination cursor belongs to a different revision",
            ));
        }
        (Some(requested), _) => requested,
        (None, Some(from_cursor)) => from_cursor,
        (None, None) => revision(connection)?,
    };
    snapshot_at(connection, resolved)?;
    Ok(resolved)
}

fn page<T>(
    connection: &Connection,
    items: Vec<T>,
    revision: i64,
    context: &str,
    limit: usize,
    cursor: Option<&str>,
    allow_zero: bool,
) -> Result<Page<T>, AppError> {
    if limit == 0 && !allow_zero {
        return Err(AppError::invalid(
            "invalid_limit",
            "a page limit must be at least one",
        ));
    }
    if limit > 200 {
        return Err(AppError::invalid(
            "invalid_limit",
            "a page limit cannot exceed 200",
        ));
    }
    let total = items.len();
    if limit == 0 {
        return Ok(Page {
            items: Vec::new(),
            page: PageInfo {
                limit,
                returned: 0,
                total,
                next_cursor: None,
            },
        });
    }
    let expected_library_id = library_id(connection)?;
    let offset = decode_cursor(cursor, &expected_library_id, revision, context)?;
    if offset > total {
        return Err(AppError::invalid(
            "invalid_cursor",
            "the pagination cursor is beyond the end of this result",
        ));
    }
    let end = offset.saturating_add(limit).min(total);
    let returned = end - offset;
    let next_cursor = (end < total)
        .then(|| encode_cursor(&expected_library_id, revision, context, end))
        .transpose()?;
    Ok(Page {
        items: items.into_iter().skip(offset).take(returned).collect(),
        page: PageInfo {
            limit,
            returned,
            total,
            next_cursor,
        },
    })
}

pub(crate) fn roots_page(
    connection: &Connection,
    requested_revision: i64,
    limit: usize,
    cursor: Option<&str>,
) -> Result<Page<ConceptSummary>, AppError> {
    let snapshot = read_snapshot(connection, requested_revision)?;
    let index = SnapshotIndex::new(&snapshot);
    let mut items = snapshot
        .concepts
        .iter()
        .filter(|concept| !index.parents.contains_key(&concept.id))
        .map(|concept| index.summary(concept.id))
        .collect::<Result<Vec<_>, _>>()?;
    items.sort_by_key(|item| (index::normalize(&item.label), item.id));
    page(
        connection,
        items,
        requested_revision,
        "roots",
        limit,
        cursor,
        false,
    )
}

fn references(
    ids: Option<&BTreeSet<i64>>,
    index: &SnapshotIndex<'_>,
) -> Result<Vec<ConceptReference>, AppError> {
    let mut references = ids
        .into_iter()
        .flatten()
        .map(|id| index.reference(*id))
        .collect::<Result<Vec<_>, _>>()?;
    references.sort_by_key(|item| (index::normalize(&item.label), item.id));
    Ok(references)
}

fn evidence_items(
    connection: &Connection,
    snapshot: &Snapshot,
    concept_id: i64,
) -> Result<Vec<EvidenceView>, AppError> {
    let mut items = snapshot
        .evidence
        .iter()
        .filter(|evidence| evidence.concept_id == concept_id)
        .map(|evidence| {
            let work = get_work_by_id(connection, evidence.work_id)?;
            Ok((
                index::normalize(&work.label),
                evidence.work_id,
                evidence.start_byte,
                evidence.end_byte,
                EvidenceView {
                    work: work.label,
                    quote: work.text[evidence.start_byte..evidence.end_byte].to_owned(),
                },
            ))
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    items.sort_by(|left, right| {
        (&left.0, left.1, left.2, left.3).cmp(&(&right.0, right.1, right.2, right.3))
    });
    Ok(items.into_iter().map(|(_, _, _, _, view)| view).collect())
}

pub(crate) fn neighbor_page(
    connection: &Connection,
    requested_revision: i64,
    id: ConceptId,
    direction: GraphDirection,
    limit: usize,
    cursor: Option<&str>,
) -> Result<Page<ConceptReference>, AppError> {
    let snapshot = read_snapshot(connection, requested_revision)?;
    let index = SnapshotIndex::new(&snapshot);
    index.require(id)?;
    let (kind, ids) = match direction {
        GraphDirection::Parents => ("parents", index.parents.get(&id.storage_id())),
        GraphDirection::Children => ("children", index.children.get(&id.storage_id())),
        GraphDirection::Both => {
            return Err(AppError::invalid(
                "invalid_direction",
                "a neighbor page direction must be parents or children",
            ));
        }
    };
    let items = references(ids, &index)?;
    page(
        connection,
        items,
        requested_revision,
        &format!("{kind}:{id}"),
        limit,
        cursor,
        false,
    )
}

pub(crate) fn evidence_page(
    connection: &Connection,
    requested_revision: i64,
    id: ConceptId,
    limit: usize,
    cursor: Option<&str>,
) -> Result<Page<EvidenceView>, AppError> {
    let snapshot = read_snapshot(connection, requested_revision)?;
    let index = SnapshotIndex::new(&snapshot);
    index.require(id)?;
    let items = evidence_items(connection, &snapshot, id.storage_id())?;
    page(
        connection,
        items,
        requested_revision,
        &format!("evidence:{id}"),
        limit,
        cursor,
        false,
    )
}

pub(crate) fn concept_detail(
    connection: &Connection,
    requested_revision: i64,
    id: ConceptId,
    preview_limit: usize,
) -> Result<ConceptDetail, AppError> {
    if preview_limit > 20 {
        return Err(AppError::invalid(
            "invalid_limit",
            "a concept preview limit cannot exceed 20",
        ));
    }
    let snapshot = read_snapshot(connection, requested_revision)?;
    let index = SnapshotIndex::new(&snapshot);
    index.require(id)?;
    let parents = page(
        connection,
        references(index.parents.get(&id.storage_id()), &index)?,
        requested_revision,
        &format!("parents:{id}"),
        preview_limit,
        None,
        true,
    )?;
    let children = page(
        connection,
        references(index.children.get(&id.storage_id()), &index)?,
        requested_revision,
        &format!("children:{id}"),
        preview_limit,
        None,
        true,
    )?;
    let evidence = page(
        connection,
        evidence_items(connection, &snapshot, id.storage_id())?,
        requested_revision,
        &format!("evidence:{id}"),
        preview_limit,
        None,
        true,
    )?;
    Ok(ConceptDetail {
        summary: index.summary(id.storage_id())?,
        parents,
        children,
        evidence,
    })
}

pub(crate) fn graph_view(
    connection: &Connection,
    requested_revision: i64,
    seed: ConceptId,
    direction: GraphDirection,
    depth: usize,
    max_nodes: usize,
) -> Result<GraphView, AppError> {
    if depth > 10 {
        return Err(AppError::invalid(
            "invalid_depth",
            "graph depth cannot exceed 10",
        ));
    }
    if !(1..=1_000).contains(&max_nodes) {
        return Err(AppError::invalid(
            "invalid_limit",
            "graph max_nodes must be between 1 and 1000",
        ));
    }
    let snapshot = read_snapshot(connection, requested_revision)?;
    let index = SnapshotIndex::new(&snapshot);
    let seed_concept = index.require(seed)?;
    let mut distances = BTreeMap::from([(seed.storage_id(), 0_usize)]);
    let mut queue = VecDeque::from([seed.storage_id()]);
    let mut node_limit_reached = false;
    while let Some(id) = queue.pop_front() {
        let distance = distances[&id];
        if distance == depth {
            continue;
        }
        let mut neighbors = BTreeSet::new();
        if matches!(direction, GraphDirection::Parents | GraphDirection::Both) {
            neighbors.extend(index.parents.get(&id).into_iter().flatten().copied());
        }
        if matches!(direction, GraphDirection::Children | GraphDirection::Both) {
            neighbors.extend(index.children.get(&id).into_iter().flatten().copied());
        }
        for neighbor in neighbors {
            if distances.contains_key(&neighbor) {
                continue;
            }
            if distances.len() == max_nodes {
                node_limit_reached = true;
                continue;
            }
            distances.insert(neighbor, distance + 1);
            queue.push_back(neighbor);
        }
    }
    let returned = distances.keys().copied().collect::<BTreeSet<_>>();
    let mut nodes = distances
        .iter()
        .map(|(id, distance)| {
            Ok(GraphNode {
                summary: index.summary(*id)?,
                distance: *distance,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    nodes.sort_by_key(|node| (node.distance, node.summary.id));
    let edges = snapshot
        .edges
        .iter()
        .filter(|edge| returned.contains(&edge.parent_id) && returned.contains(&edge.child_id))
        .map(|edge| {
            Ok(GraphEdge {
                parent: index.reference(edge.parent_id)?,
                child: index.reference(edge.child_id)?,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let frontier = returned
        .iter()
        .filter_map(|id| {
            let unreturned_parent_count =
                if matches!(direction, GraphDirection::Parents | GraphDirection::Both) {
                    index
                        .parents
                        .get(id)
                        .map_or(0, |parents| parents.difference(&returned).count())
                } else {
                    0
                };
            let unreturned_child_count =
                if matches!(direction, GraphDirection::Children | GraphDirection::Both) {
                    index
                        .children
                        .get(id)
                        .map_or(0, |children| children.difference(&returned).count())
                } else {
                    0
                };
            (unreturned_parent_count > 0 || unreturned_child_count > 0).then_some((
                *id,
                unreturned_parent_count,
                unreturned_child_count,
            ))
        })
        .map(|(id, parent_count, child_count)| {
            Ok(FrontierEntry {
                id: public_id(id)?,
                unreturned_parent_count: count_u64(parent_count)?,
                unreturned_child_count: count_u64(child_count)?,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(GraphView {
        revision: requested_revision,
        seed: ConceptReference {
            id: seed,
            label: seed_concept.label.clone(),
        },
        direction: match direction {
            GraphDirection::Parents => "parents",
            GraphDirection::Children => "children",
            GraphDirection::Both => "both",
        }
        .to_owned(),
        depth,
        max_nodes,
        nodes,
        edges,
        complete_within_depth: !node_limit_reached,
        node_limit_reached,
        frontier,
    })
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

fn descendants(index: &SnapshotIndex<'_>, seed: i64) -> BTreeSet<i64> {
    let mut output = BTreeSet::from([seed]);
    let mut pending = VecDeque::from([seed]);
    while let Some(id) = pending.pop_front() {
        for child in index.children.get(&id).into_iter().flatten() {
            if output.insert(*child) {
                pending.push_back(*child);
            }
        }
    }
    output
}

pub(crate) fn search_at(
    connection: &Connection,
    requested_revision: i64,
    query: &str,
    within: Option<ConceptId>,
    limit: usize,
    cursor: Option<&str>,
) -> Result<SearchOutput, AppError> {
    if query.trim().is_empty() {
        return Err(AppError::invalid(
            "empty_query",
            "a concept search query cannot be empty",
        ));
    }
    if limit > 200 {
        return Err(AppError::invalid(
            "invalid_limit",
            "a search result limit cannot exceed 200",
        ));
    }
    let snapshot = read_snapshot(connection, requested_revision)?;
    let snapshot_index = SnapshotIndex::new(&snapshot);
    let eligible = if let Some(within) = within {
        snapshot_index.require(within)?;
        descendants(&snapshot_index, within.storage_id())
    } else {
        snapshot.concepts.iter().map(|concept| concept.id).collect()
    };
    let normalized_query = index::normalize(query);
    let terms = normalized_query.split_whitespace().collect::<Vec<_>>();
    // Search from the selected immutable snapshot even at HEAD. Reading the mutable derived
    // index here could mix that snapshot with a concurrent commit's ancestry projection.
    let ancestors = ancestor_sets(&snapshot)?;
    let contexts = snapshot
        .concepts
        .iter()
        .map(|concept| {
            let context = ancestors[&concept.id]
                .iter()
                .filter_map(|id| snapshot_index.concepts.get(id))
                .map(|ancestor| ancestor.label.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            (
                concept.id,
                (index::normalize(&concept.label), index::normalize(&context)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut candidates = snapshot
        .concepts
        .iter()
        .filter(|concept| eligible.contains(&concept.id))
        .filter_map(|concept| {
            let (label, ancestor_context) = contexts.get(&concept.id)?;
            let matches_all = terms
                .iter()
                .all(|term| label.contains(*term) || ancestor_context.contains(*term));
            matches_all.then(|| {
                let exact = label == &normalized_query;
                let prefix = label.starts_with(&normalized_query);
                let label_matches = terms.iter().filter(|term| label.contains(**term)).count();
                (concept.id, exact, prefix, label_matches)
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(id, exact, prefix, label_matches)| {
        (
            std::cmp::Reverse(*exact),
            std::cmp::Reverse(*prefix),
            std::cmp::Reverse(*label_matches),
            *id,
        )
    });
    let items = candidates
        .into_iter()
        .map(|(id, _, _, _)| {
            Ok(SearchResult {
                concept: snapshot_index.summary(id)?,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let context = format!(
        "search:{}:{}",
        normalized_query,
        within.map_or_else(|| "all".to_owned(), |id| id.to_string())
    );
    let results = page(
        connection,
        items,
        requested_revision,
        &context,
        limit,
        cursor,
        false,
    )?;
    Ok(SearchOutput {
        revision: requested_revision,
        query: query.to_owned(),
        within: within
            .map(|id| snapshot_index.reference(id.storage_id()))
            .transpose()?,
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
    metadata: &Value,
    actor: &str,
) -> Result<(), AppError> {
    let parent = revision.checked_sub(1).ok_or_else(|| {
        AppError::database("revision_underflow", "a commit revision must be positive")
    })?;
    transaction.execute(
        "INSERT INTO commits(\
             revision, parent_revision, base_revision, work_id, reconciliation_id, kind, summary, \
             submitted_request, resolved_operations, after_snapshot, metadata, actor, created_at\
         ) VALUES(?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            revision,
            parent,
            work_id,
            reconciliation_id,
            kind,
            summary,
            serde_json::to_string(request)?,
            serde_json::to_string(resolved)?,
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
        &inverse,
        &json!({ "reverted_revision": target_revision }),
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
    use rusqlite::Connection;

    use super::{
        Snapshot, SnapshotConcept, SnapshotEdge, SnapshotEvidence, diff_snapshot_entries,
        heading_for_offset, invert_snapshot_change, markdown_headings, page, sha256_hex,
        store_work, validate_snapshot,
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
    fn page_cursors_are_revision_and_context_bound() -> Result<(), Box<dyn std::error::Error>> {
        let connection = test_connection()?;
        let first = page(&connection, vec![1, 2, 3], 7, "roots", 2, None, false)?;
        assert_eq!(first.items, [1, 2]);
        let cursor = first.page.next_cursor.as_deref().ok_or("missing cursor")?;
        let second = page(
            &connection,
            vec![1, 2, 3],
            7,
            "roots",
            2,
            Some(cursor),
            false,
        )?;
        assert_eq!(second.items, [3]);
        let resized = page(
            &connection,
            vec![1, 2, 3],
            7,
            "roots",
            1,
            Some(cursor),
            false,
        )?;
        assert_eq!(resized.items, [3]);
        assert!(
            page(
                &connection,
                vec![1, 2, 3],
                8,
                "roots",
                2,
                Some(cursor),
                false
            )
            .is_err()
        );
        assert!(
            page(
                &connection,
                vec![1, 2, 3],
                7,
                "parents:c1",
                2,
                Some(cursor),
                false
            )
            .is_err()
        );
        let another_library = test_connection()?;
        assert!(
            page(
                &another_library,
                vec![1, 2, 3],
                7,
                "roots",
                2,
                Some(cursor),
                false
            )
            .is_err()
        );
        Ok(())
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

    fn test_connection() -> Result<Connection, Box<dyn std::error::Error>> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(include_str!("../schema.sql"))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        Ok(connection)
    }
}
