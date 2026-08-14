//! Bounded, database-backed graph projections.
//!
//! This module deliberately keeps the graph in `SQLite`. [`GraphReader`] selects a
//! revision and returns a cheap [`GraphView`]; each view operation materializes
//! only the bounded [`GraphProjection`] needed by its caller.
//!
//! The queries assume revision-addressable relational tables with this shape:
//!
//! - `revision_concepts(revision, concept_id, label, normalized_label, ...counts)`
//! - `revision_edges(revision, parent_id, child_id)`
//! - `revision_evidence(revision, concept_id, work_id, start_byte, end_byte)`

use std::collections::BTreeMap;
use std::num::NonZeroU16;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::index;
use crate::model::{
    ConceptDetail, ConceptId, ConceptReference, ConceptSummary, CorpusOverview, EvidenceView,
    FrontierEntry, GraphDirection, GraphEdge, GraphNode as OutputGraphNode,
    GraphView as OutputGraphView, Page, PageInfo, SearchOutput, SearchResult,
};
use crate::revision_store;

const MAX_PAGE_SIZE: u16 = 200;
const MAX_WALK_DEPTH: u8 = 10;
const MAX_WALK_NODES: u16 = 1_000;
const MAX_WALK_EDGES: u16 = 10_000;

/// A checked page size shared by all graph selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageLimit(NonZeroU16);

impl PageLimit {
    pub(crate) fn new(value: usize) -> Result<Self, AppError> {
        let value = u16::try_from(value).map_err(|_| invalid_page_limit())?;
        if value > MAX_PAGE_SIZE {
            return Err(invalid_page_limit());
        }
        NonZeroU16::new(value)
            .map(Self)
            .ok_or_else(invalid_page_limit)
    }

    const fn get(self) -> u16 {
        self.0.get()
    }
}

/// A checked zero-based page offset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PageOffset(u32);

impl PageOffset {
    pub(crate) fn new(value: usize) -> Result<Self, AppError> {
        u32::try_from(value)
            .map(Self)
            .map_err(|_| AppError::invalid("invalid_cursor", "the graph page offset is too large"))
    }

    const fn get(self) -> u32 {
        self.0
    }
}

/// A bounded graph page request. Cursor decoding belongs above this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageRequest {
    offset: PageOffset,
    limit: PageLimit,
}

impl PageRequest {
    pub(crate) const fn new(offset: PageOffset, limit: PageLimit) -> Self {
        Self { offset, limit }
    }
}

/// Parent or child selection for a one-hop query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NeighborDirection {
    Parents,
    Children,
}

/// Checked limits for a bounded graph walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WalkBounds {
    depth: u8,
    max_nodes: NonZeroU16,
    max_edges: NonZeroU16,
}

impl WalkBounds {
    pub(crate) fn new(depth: usize, max_nodes: usize, max_edges: usize) -> Result<Self, AppError> {
        let depth = u8::try_from(depth).map_err(|_| invalid_walk_bounds())?;
        let max_nodes = u16::try_from(max_nodes).map_err(|_| invalid_walk_bounds())?;
        let max_edges = u16::try_from(max_edges).map_err(|_| invalid_walk_bounds())?;
        if depth > MAX_WALK_DEPTH || max_nodes > MAX_WALK_NODES || max_edges > MAX_WALK_EDGES {
            return Err(invalid_walk_bounds());
        }
        let max_nodes = NonZeroU16::new(max_nodes).ok_or_else(invalid_walk_bounds)?;
        let max_edges = NonZeroU16::new(max_edges).ok_or_else(invalid_walk_bounds)?;
        Ok(Self {
            depth,
            max_nodes,
            max_edges,
        })
    }
}

/// Lightweight database entry point. It owns no graph rows.
#[derive(Clone, Copy)]
pub(crate) struct GraphReader<'db> {
    db: &'db Connection,
}

impl<'db> GraphReader<'db> {
    #[must_use]
    pub(crate) const fn new(db: &'db Connection) -> Self {
        Self { db }
    }

    /// Select the current revision without loading graph content.
    pub(crate) fn head(self) -> Result<GraphView<'db>, AppError> {
        let (library_id, revision) = self.db.query_row(
            "SELECT library_id, revision FROM library_state WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if !revision_store::revision_exists(self.db, revision)? {
            return Err(revision_not_found(revision));
        }
        Ok(GraphView {
            db: self.db,
            library_id,
            revision,
        })
    }

    /// Select an immutable revision without loading graph content.
    pub(crate) fn at(self, revision: i64) -> Result<GraphView<'db>, AppError> {
        if !revision_store::revision_exists(self.db, revision)? {
            return Err(revision_not_found(revision));
        }
        let library_id = self.db.query_row(
            "SELECT library_id FROM library_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(GraphView {
            db: self.db,
            library_id,
            revision,
        })
    }

    /// Resolve an explicit revision or a cursor-pinned revision without loading graph rows.
    pub(crate) fn paged_at(
        self,
        requested: Option<i64>,
        cursor: Option<&str>,
    ) -> Result<GraphView<'db>, AppError> {
        let decoded = cursor.map(decode_cursor_value).transpose()?;
        let library_id = self.db.query_row(
            "SELECT library_id FROM library_state WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )?;
        if decoded
            .as_ref()
            .is_some_and(|cursor| cursor.library_id != library_id)
        {
            return Err(AppError::invalid(
                "invalid_cursor",
                "the pagination cursor belongs to a different library",
            ));
        }
        let cursor_revision = decoded.as_ref().map(|cursor| cursor.revision);
        let revision = match (requested, cursor_revision) {
            (Some(requested), Some(cursor_revision)) if requested != cursor_revision => {
                return Err(AppError::invalid(
                    "invalid_cursor",
                    "the pagination cursor belongs to a different revision",
                ));
            }
            (Some(requested), _) => requested,
            (None, Some(cursor_revision)) => cursor_revision,
            (None, None) => self.db.query_row(
                "SELECT revision FROM library_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )?,
        };
        if !revision_store::revision_exists(self.db, revision)? {
            return Err(revision_not_found(revision));
        }
        Ok(GraphView {
            db: self.db,
            library_id,
            revision,
        })
    }
}

/// Cheap, revision-scoped selector facade. It owns no graph rows.
pub(crate) struct GraphView<'db> {
    db: &'db Connection,
    library_id: String,
    revision: i64,
}

impl GraphView<'_> {
    #[must_use]
    pub(crate) const fn revision(&self) -> i64 {
        self.revision
    }

    pub(crate) fn overview(&self) -> Result<CorpusOverview, AppError> {
        let stats = revision_store::revision_stats(self.db, self.revision)?
            .ok_or_else(|| revision_not_found(self.revision))?;
        let (root_count, leaf_count, shared_count) = self.db.query_row(
            "SELECT \
                 COALESCE(SUM(parent_count = 0), 0), \
                 COALESCE(SUM(child_count = 0), 0), \
                 COALESCE(SUM(parent_count > 1), 0) \
             FROM revision_concepts WHERE revision = ?1",
            [self.revision],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        Ok(CorpusOverview {
            revision: self.revision,
            concept_count: u64_from_i64(stats.concepts, "concept count")?,
            edge_count: u64_from_i64(stats.edges, "edge count")?,
            root_count: u64_from_i64(root_count, "root count")?,
            leaf_count: u64_from_i64(leaf_count, "leaf count")?,
            shared_concept_count: u64_from_i64(shared_count, "shared concept count")?,
            evidence_count: u64_from_i64(stats.evidence, "evidence count")?,
        })
    }

    pub(crate) fn reference(&self, id: ConceptId) -> Result<ConceptReference, AppError> {
        let reference = self
            .db
            .query_row(
                "SELECT concept_id, label FROM revision_concepts \
                 WHERE revision = ?1 AND concept_id = ?2",
                params![self.revision, id.storage_id()],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let (stored_id, label) = reference.ok_or_else(|| concept_not_found(id, self.revision))?;
        Ok(ConceptReference {
            id: concept_id(stored_id)?,
            label,
        })
    }

    pub(crate) fn roots_page(
        &self,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<Page<ConceptSummary>, AppError> {
        let request = self.page_request(limit, cursor, "roots")?;
        let projection = self.roots(request)?;
        let page = self.output_page(projection.page, "roots")?;
        Ok(Page {
            items: projection
                .nodes
                .into_iter()
                .map(GraphNode::into_summary)
                .collect(),
            page,
        })
    }

    pub(crate) fn neighbor_page(
        &self,
        id: ConceptId,
        direction: NeighborDirection,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<Page<ConceptReference>, AppError> {
        let kind = match direction {
            NeighborDirection::Parents => "parents",
            NeighborDirection::Children => "children",
        };
        let context = format!("{kind}:{id}");
        let request = self.page_request(limit, cursor, &context)?;
        let projection = self.neighbors(id, direction, request)?;
        let page = self.output_page(projection.page, &context)?;
        Ok(Page {
            items: projection
                .nodes
                .into_iter()
                .skip(1)
                .map(GraphNode::into_reference)
                .collect(),
            page,
        })
    }

    pub(crate) fn evidence_page(
        &self,
        id: ConceptId,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<Page<EvidenceView>, AppError> {
        let context = format!("evidence:{id}");
        let request = self.page_request(limit, cursor, &context)?;
        let projection = self.evidence(id, request)?;
        let items = self.materialize_evidence(&projection.evidence)?;
        let page = self.output_page(projection.page, &context)?;
        Ok(Page { items, page })
    }

    pub(crate) fn concept_detail(
        &self,
        id: ConceptId,
        preview_limit: usize,
    ) -> Result<ConceptDetail, AppError> {
        if preview_limit > 20 {
            return Err(AppError::invalid(
                "invalid_limit",
                "a concept preview limit cannot exceed 20",
            ));
        }
        let summary = self
            .concept(id)?
            .nodes
            .into_iter()
            .next()
            .ok_or_else(|| concept_not_found(id, self.revision))?
            .into_summary();
        if preview_limit == 0 {
            return Ok(ConceptDetail {
                parents: empty_preview(summary.parent_count)?,
                children: empty_preview(summary.child_count)?,
                evidence: empty_preview(summary.evidence_count)?,
                summary,
            });
        }
        let parents = self.neighbor_page(id, NeighborDirection::Parents, preview_limit, None)?;
        let children = self.neighbor_page(id, NeighborDirection::Children, preview_limit, None)?;
        let evidence = self.evidence_page(id, preview_limit, None)?;
        Ok(ConceptDetail {
            summary,
            parents,
            children,
            evidence,
        })
    }

    pub(crate) fn graph_view(
        &self,
        seed: ConceptId,
        direction: GraphDirection,
        depth: usize,
        max_nodes: usize,
    ) -> Result<OutputGraphView, AppError> {
        let projection = self.walk(
            seed,
            direction,
            WalkBounds::new(depth, max_nodes, usize::from(MAX_WALK_EDGES))?,
        )?;
        if projection.edge_limit_reached {
            return Err(AppError::invalid(
                "graph_edge_limit",
                format!(
                    "the bounded graph contains more than {MAX_WALK_EDGES} edges; reduce depth or max_nodes"
                ),
            ));
        }
        let frontier = Self::frontier(&projection.nodes, &projection.edges, direction)?;
        let nodes = projection
            .nodes
            .into_iter()
            .map(|node| {
                let distance = usize::from(node.distance.unwrap_or(0));
                OutputGraphNode {
                    summary: node.into_summary(),
                    distance,
                }
            })
            .collect();
        Ok(OutputGraphView {
            revision: self.revision,
            seed,
            direction,
            depth,
            max_nodes,
            nodes,
            edges: projection.edges,
            complete_within_depth: !projection.node_limit_reached,
            node_limit_reached: projection.node_limit_reached,
            frontier,
        })
    }

    pub(crate) fn search(
        &self,
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
        if query.len() > 512 {
            return Err(AppError::invalid(
                "invalid_query",
                "a concept search query cannot exceed 512 bytes",
            ));
        }
        let normalized_query = index::normalize(query);
        let mut terms = normalized_query.split_whitespace().collect::<Vec<_>>();
        if terms.is_empty() || terms.len() > 16 {
            return Err(AppError::invalid(
                "invalid_query",
                "a concept search query must contain between 1 and 16 normalized terms",
            ));
        }
        terms.sort_unstable();
        terms.dedup();
        let within_reference = within.map(|id| self.reference(id)).transpose()?;
        let context = format!(
            "search:{}:{}",
            normalized_query,
            within.map_or_else(|| "all".to_owned(), |id| id.to_string())
        );
        let request = self.page_request(limit, cursor, &context)?;
        let encoded_terms = serde_json::to_string(&terms)?;
        let within_id = within.map(ConceptId::storage_id);
        let mut statement = self.db.prepare_cached(SEARCH_ROWS_SQL)?;
        let rows = statement.query_map(
            params![
                self.revision,
                encoded_terms,
                within_id,
                normalized_query,
                i64::from(request.limit.get()),
                i64::from(request.offset.get())
            ],
            |row| Ok((raw_node(row)?, row.get::<_, i64>(6)?)),
        )?;
        let mut items = Vec::with_capacity(usize::from(request.limit.get()));
        let mut total = None;
        for row in rows {
            let (node, row_total) = row?;
            if total.is_some_and(|total| total != row_total) {
                return Err(AppError::database(
                    "inconsistent_search_count",
                    "a search page returned inconsistent total counts",
                ));
            }
            total = Some(row_total);
            items.push(SearchResult {
                concept: GraphNode::try_from(node)?.into_summary(),
            });
        }
        let total = match total {
            Some(total) => usize_from_i64(total, "search result count")?,
            None if request.offset.get() == 0 => 0,
            None => {
                let total = self.db.query_row(
                    SEARCH_COUNT_SQL,
                    params![self.revision, encoded_terms, within_id, normalized_query],
                    |row| row.get::<_, i64>(0),
                )?;
                usize_from_i64(total, "search result count")?
            }
        };
        let projection_page = page_info(request, total, items.len())?;
        let page = self.output_page(Some(projection_page), &context)?;
        Ok(SearchOutput {
            revision: self.revision,
            query: query.to_owned(),
            within: within_reference,
            results: Page { items, page },
        })
    }

    /// Select one concept and its aggregate graph counts.
    pub(crate) fn concept(&self, id: ConceptId) -> Result<GraphProjection, AppError> {
        let node = self.require_node(id)?;
        Ok(GraphProjection::from_node(self.revision, node))
    }

    /// Select one bounded page of root concepts.
    pub(crate) fn roots(&self, page: PageRequest) -> Result<GraphProjection, AppError> {
        let total = self.db.query_row(
            "SELECT COUNT(*) \
             FROM revision_concepts AS c \
             WHERE c.revision = ?1 AND c.parent_count = 0",
            [self.revision],
            |row| row.get::<_, i64>(0),
        )?;
        let total = usize_from_i64(total, "root count")?;
        let mut statement = self.db.prepare_cached(
            "SELECT c.concept_id, c.label, c.parent_count, \
                    c.child_count, c.evidence_count, \
                    NULL \
             FROM revision_concepts AS c \
             WHERE c.revision = ?1 AND c.parent_count = 0 \
             ORDER BY c.normalized_label, c.concept_id \
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows = statement.query_map(
            params![
                self.revision,
                i64::from(page.limit.get()),
                i64::from(page.offset.get())
            ],
            raw_node,
        )?;
        let nodes = rows
            .map(|row| row.map_err(AppError::from).and_then(GraphNode::try_from))
            .collect::<Result<Vec<_>, _>>()?;
        let page = page_info(page, total, nodes.len())?;
        Ok(GraphProjection::paged(self.revision, nodes, page))
    }

    /// Select a bounded page of direct parents or children.
    pub(crate) fn neighbors(
        &self,
        id: ConceptId,
        direction: NeighborDirection,
        page: PageRequest,
    ) -> Result<GraphProjection, AppError> {
        let seed = self.require_node(id)?;
        let (count_sql, rows_sql) = match direction {
            NeighborDirection::Parents => (PARENT_COUNT_SQL, PARENT_ROWS_SQL),
            NeighborDirection::Children => (CHILD_COUNT_SQL, CHILD_ROWS_SQL),
        };
        let total =
            self.db
                .query_row(count_sql, params![self.revision, id.storage_id()], |row| {
                    row.get::<_, i64>(0)
                })?;
        let total = usize_from_i64(total, "neighbor count")?;
        let mut statement = self.db.prepare_cached(rows_sql)?;
        let rows = statement.query_map(
            params![
                self.revision,
                id.storage_id(),
                i64::from(page.limit.get()),
                i64::from(page.offset.get())
            ],
            raw_node,
        )?;
        let neighbors = rows
            .map(|row| row.map_err(AppError::from).and_then(GraphNode::try_from))
            .collect::<Result<Vec<_>, _>>()?;
        let mut nodes = Vec::with_capacity(neighbors.len().saturating_add(1));
        nodes.push(seed);
        let mut edges = Vec::with_capacity(neighbors.len());
        for neighbor in neighbors {
            let edge = match direction {
                NeighborDirection::Parents => GraphEdge {
                    parent_id: neighbor.id,
                    child_id: id,
                },
                NeighborDirection::Children => GraphEdge {
                    parent_id: id,
                    child_id: neighbor.id,
                },
            };
            nodes.push(neighbor);
            edges.push(edge);
        }
        let page = page_info(page, total, edges.len())?;
        Ok(GraphProjection {
            revision: self.revision,
            nodes,
            edges,
            evidence: Vec::new(),
            page: Some(page),
            node_limit_reached: false,
            edge_limit_reached: false,
        })
    }

    /// Select bounded evidence coordinates without loading work text.
    pub(crate) fn evidence(
        &self,
        id: ConceptId,
        page: PageRequest,
    ) -> Result<GraphProjection, AppError> {
        let seed = self.require_node(id)?;
        let total = self.db.query_row(
            "SELECT COUNT(*) FROM revision_evidence \
             WHERE revision = ?1 AND concept_id = ?2",
            params![self.revision, id.storage_id()],
            |row| row.get::<_, i64>(0),
        )?;
        let total = usize_from_i64(total, "evidence count")?;
        let mut statement = self.db.prepare_cached(EVIDENCE_ROWS_SQL)?;
        let rows = statement.query_map(
            params![
                self.revision,
                id.storage_id(),
                i64::from(page.limit.get()),
                i64::from(page.offset.get())
            ],
            |row| {
                Ok(RawEvidence {
                    concept_id: row.get(0)?,
                    work_id: row.get(1)?,
                    start_byte: row.get(2)?,
                    end_byte: row.get(3)?,
                })
            },
        )?;
        let evidence = rows
            .map(|row| row.map_err(AppError::from).and_then(EvidenceRef::try_from))
            .collect::<Result<Vec<_>, _>>()?;
        let page = page_info(page, total, evidence.len())?;
        Ok(GraphProjection {
            revision: self.revision,
            nodes: vec![seed],
            edges: Vec::new(),
            evidence,
            page: Some(page),
            node_limit_reached: false,
            edge_limit_reached: false,
        })
    }

    /// Select a bounded breadth-first projection around one seed concept.
    pub(crate) fn walk(
        &self,
        seed: ConceptId,
        direction: GraphDirection,
        bounds: WalkBounds,
    ) -> Result<GraphProjection, AppError> {
        let expansion_limit = walk_expansion_limit(bounds)?;
        let row_limit = i64::from(bounds.max_nodes.get())
            .checked_add(1)
            .ok_or_else(|| {
                AppError::database("numeric_overflow", "the graph walk row limit is too large")
            })?;
        let sql = match direction {
            GraphDirection::Parents => WALK_PARENTS_SQL,
            GraphDirection::Children => WALK_CHILDREN_SQL,
            GraphDirection::Both => WALK_BOTH_SQL,
        };
        let mut statement = self.db.prepare_cached(sql)?;
        let rows = statement.query_map(
            params![
                self.revision,
                seed.storage_id(),
                i64::from(bounds.depth),
                expansion_limit,
                row_limit
            ],
            raw_node,
        )?;
        let mut nodes = rows
            .map(|row| row.map_err(AppError::from).and_then(GraphNode::try_from))
            .collect::<Result<Vec<_>, _>>()?;
        if nodes.is_empty() {
            return Err(concept_not_found(seed, self.revision));
        }
        let node_limit = usize::from(bounds.max_nodes.get());
        let node_limit_reached = nodes.len() > node_limit;
        nodes.truncate(node_limit);

        let selected_ids = nodes
            .iter()
            .map(|node| node.id.storage_id())
            .collect::<Vec<_>>();
        let selected_json = serde_json::to_string(&selected_ids)?;
        let edge_row_limit = i64::from(bounds.max_edges.get())
            .checked_add(1)
            .ok_or_else(|| {
                AppError::database("numeric_overflow", "the graph walk edge limit is too large")
            })?;
        let mut edge_statement = self.db.prepare_cached(
            "WITH selected(concept_id) AS MATERIALIZED (\
                 SELECT CAST(value AS INTEGER) FROM json_each(?2)\
             ) \
             SELECT e.parent_id, e.child_id \
             FROM selected AS child \
             CROSS JOIN revision_edges AS e \
             CROSS JOIN selected AS parent \
             WHERE e.revision = ?1 \
               AND e.child_id = child.concept_id \
               AND e.parent_id = parent.concept_id \
             LIMIT ?3",
        )?;
        let edge_rows = edge_statement.query_map(
            params![self.revision, selected_json, edge_row_limit],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let mut edges = edge_rows
            .map(|row| {
                let (parent, child) = row?;
                Ok(GraphEdge {
                    parent_id: concept_id(parent)?,
                    child_id: concept_id(child)?,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        edges.sort_unstable_by_key(|edge| (edge.parent_id, edge.child_id));
        let edge_limit = usize::from(bounds.max_edges.get());
        let edge_limit_reached = edges.len() > edge_limit;
        edges.truncate(edge_limit);

        Ok(GraphProjection {
            revision: self.revision,
            nodes,
            edges,
            evidence: Vec::new(),
            page: None,
            node_limit_reached,
            edge_limit_reached,
        })
    }

    fn require_node(&self, id: ConceptId) -> Result<GraphNode, AppError> {
        let mut statement = self.db.prepare_cached(NODE_BY_ID_SQL)?;
        let raw = statement
            .query_row(params![self.revision, id.storage_id()], raw_node)
            .optional()?;
        raw.map(GraphNode::try_from)
            .transpose()?
            .ok_or_else(|| concept_not_found(id, self.revision))
    }

    fn page_request(
        &self,
        limit: usize,
        cursor: Option<&str>,
        context: &str,
    ) -> Result<PageRequest, AppError> {
        let limit = PageLimit::new(limit)?;
        let offset = decode_cursor(cursor, &self.library_id, self.revision, context)?;
        Ok(PageRequest::new(PageOffset::new(offset)?, limit))
    }

    fn output_page(
        &self,
        page: Option<ProjectionPage>,
        context: &str,
    ) -> Result<PageInfo, AppError> {
        let page = page.ok_or_else(|| {
            AppError::unexpected(
                "missing_page",
                "a paged graph projection has no page metadata",
            )
        })?;
        let next_cursor = page
            .next_offset
            .map(|offset| {
                encode_cursor(
                    &self.library_id,
                    self.revision,
                    context,
                    usize::try_from(offset.get()).map_err(|_| {
                        AppError::database("numeric_overflow", "the graph page offset is too large")
                    })?,
                )
            })
            .transpose()?;
        Ok(PageInfo {
            limit: usize::from(page.limit),
            returned: page.returned,
            total: page.total,
            next_cursor,
        })
    }

    fn materialize_evidence(
        &self,
        evidence: &[EvidenceRef],
    ) -> Result<Vec<EvidenceView>, AppError> {
        if evidence.is_empty() {
            return Ok(Vec::new());
        }
        let coordinates = evidence
            .iter()
            .map(|item| {
                Ok([
                    item.work_id,
                    i64::try_from(item.start_byte).map_err(|_| {
                        AppError::database(
                            "numeric_overflow",
                            "an evidence start byte is too large",
                        )
                    })?,
                    i64::try_from(item.end_byte).map_err(|_| {
                        AppError::database("numeric_overflow", "an evidence end byte is too large")
                    })?,
                ])
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        let encoded = serde_json::to_string(&coordinates)?;
        let mut statement = self.db.prepare_cached(
            "WITH requested(position, work_id, start_byte, end_byte) AS (\
                 SELECT CAST(key AS INTEGER), \
                        CAST(json_extract(value, '$[0]') AS INTEGER), \
                        CAST(json_extract(value, '$[1]') AS INTEGER), \
                        CAST(json_extract(value, '$[2]') AS INTEGER) \
                 FROM json_each(?1)\
             ) \
             SELECT works.label, \
                    CAST(substr(\
                        CAST(works.text AS BLOB), \
                        requested.start_byte + 1, \
                        requested.end_byte - requested.start_byte\
                    ) AS TEXT) \
             FROM requested JOIN works ON works.id = requested.work_id \
             ORDER BY requested.position",
        )?;
        let rows = statement.query_map([encoded], |row| {
            Ok(EvidenceView {
                work: row.get(0)?,
                quote: row.get(1)?,
            })
        })?;
        let views = rows.collect::<Result<Vec<_>, _>>()?;
        if views.len() != evidence.len() {
            return Err(AppError::database(
                "evidence_work_missing",
                "a stored evidence reference names a missing work",
            ));
        }
        Ok(views)
    }

    fn frontier(
        nodes: &[GraphNode],
        edges: &[GraphEdge],
        direction: GraphDirection,
    ) -> Result<Vec<FrontierEntry>, AppError> {
        let mut returned = nodes
            .iter()
            .map(|node| (node.id, (0_u64, 0_u64)))
            .collect::<BTreeMap<_, _>>();
        for edge in edges {
            let parent = returned.get_mut(&edge.parent_id).ok_or_else(|| {
                AppError::database(
                    "invalid_graph_projection",
                    "a projected edge has a missing parent node",
                )
            })?;
            parent.1 = parent.1.checked_add(1).ok_or_else(frontier_overflow)?;
            let child = returned.get_mut(&edge.child_id).ok_or_else(|| {
                AppError::database(
                    "invalid_graph_projection",
                    "a projected edge has a missing child node",
                )
            })?;
            child.0 = child.0.checked_add(1).ok_or_else(frontier_overflow)?;
        }

        let include_parents = matches!(direction, GraphDirection::Parents | GraphDirection::Both);
        let include_children = matches!(direction, GraphDirection::Children | GraphDirection::Both);
        let mut frontier = Vec::new();
        for node in nodes {
            let &(returned_parents, returned_children) =
                returned.get(&node.id).ok_or_else(|| {
                    AppError::database(
                        "invalid_graph_projection",
                        "a projected node is missing its returned-degree entry",
                    )
                })?;
            let parents = if include_parents {
                node.parent_count
                    .checked_sub(returned_parents)
                    .ok_or_else(frontier_overflow)?
            } else {
                0
            };
            let children = if include_children {
                node.child_count
                    .checked_sub(returned_children)
                    .ok_or_else(frontier_overflow)?
            } else {
                0
            };
            if parents > 0 || children > 0 {
                frontier.push(FrontierEntry {
                    id: node.id,
                    unreturned_parent_count: parents,
                    unreturned_child_count: children,
                });
            }
        }
        Ok(frontier)
    }
}

/// An owned, bounded graph subset ready for conversion to presentation DTOs.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct GraphProjection {
    pub(crate) revision: i64,
    pub(crate) nodes: Vec<GraphNode>,
    pub(crate) edges: Vec<GraphEdge>,
    pub(crate) evidence: Vec<EvidenceRef>,
    pub(crate) page: Option<ProjectionPage>,
    pub(crate) node_limit_reached: bool,
    pub(crate) edge_limit_reached: bool,
}

impl GraphProjection {
    fn from_node(revision: i64, node: GraphNode) -> Self {
        Self {
            revision,
            nodes: vec![node],
            edges: Vec::new(),
            evidence: Vec::new(),
            page: None,
            node_limit_reached: false,
            edge_limit_reached: false,
        }
    }

    fn paged(revision: i64, nodes: Vec<GraphNode>, page: ProjectionPage) -> Self {
        Self {
            revision,
            nodes,
            edges: Vec::new(),
            evidence: Vec::new(),
            page: Some(page),
            node_limit_reached: false,
            edge_limit_reached: false,
        }
    }
}

/// One concept in a projection. `distance` is populated only by `walk`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct GraphNode {
    pub(crate) id: ConceptId,
    pub(crate) label: String,
    pub(crate) parent_count: u64,
    pub(crate) child_count: u64,
    pub(crate) evidence_count: u64,
    pub(crate) distance: Option<u8>,
}

/// An evidence attachment intentionally contains no work label or source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvidenceRef {
    pub(crate) concept_id: ConceptId,
    pub(crate) work_id: i64,
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
}

/// Page information independent of cursor encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProjectionPage {
    pub(crate) limit: u16,
    pub(crate) returned: usize,
    pub(crate) total: usize,
    pub(crate) next_offset: Option<PageOffset>,
}

#[derive(Serialize)]
struct CursorRef<'a> {
    version: u8,
    library_id: &'a str,
    revision: i64,
    context: &'a str,
    offset: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    library_id: String,
    revision: i64,
    context: String,
    offset: usize,
}

#[derive(Debug)]
struct RawNode {
    id: i64,
    label: String,
    parent_count: i64,
    child_count: i64,
    evidence_count: i64,
    distance: Option<i64>,
}

impl TryFrom<RawNode> for GraphNode {
    type Error = AppError;

    fn try_from(raw: RawNode) -> Result<Self, Self::Error> {
        let distance = raw
            .distance
            .map(|distance| {
                u8::try_from(distance).map_err(|_| {
                    AppError::database(
                        "invalid_graph_distance",
                        "a stored graph distance is invalid",
                    )
                })
            })
            .transpose()?;
        Ok(Self {
            id: concept_id(raw.id)?,
            label: raw.label,
            parent_count: u64_from_i64(raw.parent_count, "parent count")?,
            child_count: u64_from_i64(raw.child_count, "child count")?,
            evidence_count: u64_from_i64(raw.evidence_count, "evidence count")?,
            distance,
        })
    }
}

impl GraphNode {
    fn into_reference(self) -> ConceptReference {
        ConceptReference {
            id: self.id,
            label: self.label,
        }
    }

    fn into_summary(self) -> ConceptSummary {
        ConceptSummary {
            id: self.id,
            label: self.label,
            parent_count: self.parent_count,
            child_count: self.child_count,
            evidence_count: self.evidence_count,
            root: self.parent_count == 0,
            leaf: self.child_count == 0,
            shared: self.parent_count > 1,
        }
    }
}

#[derive(Debug)]
struct RawEvidence {
    concept_id: i64,
    work_id: i64,
    start_byte: i64,
    end_byte: i64,
}

impl TryFrom<RawEvidence> for EvidenceRef {
    type Error = AppError;

    fn try_from(raw: RawEvidence) -> Result<Self, Self::Error> {
        if raw.work_id <= 0 {
            return Err(AppError::database(
                "invalid_work_id",
                "stored evidence has an invalid work ID",
            ));
        }
        let start_byte = usize_from_i64(raw.start_byte, "evidence start byte")?;
        let end_byte = usize_from_i64(raw.end_byte, "evidence end byte")?;
        if end_byte <= start_byte {
            return Err(AppError::database(
                "invalid_evidence_range",
                "stored evidence has an invalid byte range",
            ));
        }
        Ok(Self {
            concept_id: concept_id(raw.concept_id)?,
            work_id: raw.work_id,
            start_byte,
            end_byte,
        })
    }
}

fn raw_node(row: &Row<'_>) -> rusqlite::Result<RawNode> {
    Ok(RawNode {
        id: row.get(0)?,
        label: row.get(1)?,
        parent_count: row.get(2)?,
        child_count: row.get(3)?,
        evidence_count: row.get(4)?,
        distance: row.get(5)?,
    })
}

fn page_info(
    request: PageRequest,
    total: usize,
    returned: usize,
) -> Result<ProjectionPage, AppError> {
    let offset = usize::try_from(request.offset.get()).map_err(|_| {
        AppError::database("numeric_overflow", "the graph page offset is too large")
    })?;
    if offset > total {
        return Err(AppError::invalid(
            "invalid_cursor",
            "the graph page offset is beyond the result",
        ));
    }
    let end = offset
        .checked_add(returned)
        .ok_or_else(|| AppError::database("numeric_overflow", "the graph page end is too large"))?;
    let next_offset = (end < total).then(|| PageOffset::new(end)).transpose()?;
    Ok(ProjectionPage {
        limit: request.limit.get(),
        returned,
        total,
        next_offset,
    })
}

fn encode_cursor(
    library_id: &str,
    revision: i64,
    context: &str,
    offset: usize,
) -> Result<String, AppError> {
    let bytes = serde_json::to_vec(&CursorRef {
        version: 2,
        library_id,
        revision,
        context,
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

fn empty_preview<T>(total: u64) -> Result<Page<T>, AppError> {
    Ok(Page {
        items: Vec::new(),
        page: PageInfo {
            limit: 0,
            returned: 0,
            total: usize::try_from(total).map_err(|_| {
                AppError::database("numeric_overflow", "a graph preview count is too large")
            })?,
            next_cursor: None,
        },
    })
}

fn walk_expansion_limit(bounds: WalkBounds) -> Result<i64, AppError> {
    let nodes = i64::from(bounds.max_nodes.get())
        .checked_add(1)
        .ok_or_else(|| AppError::database("numeric_overflow", "walk node limit overflow"))?;
    let levels = i64::from(bounds.depth)
        .checked_add(1)
        .ok_or_else(|| AppError::database("numeric_overflow", "walk depth overflow"))?;
    nodes
        .checked_mul(levels)
        .ok_or_else(|| AppError::database("numeric_overflow", "walk expansion limit overflow"))
}

fn concept_id(value: i64) -> Result<ConceptId, AppError> {
    ConceptId::from_storage(value).map_err(|error| {
        AppError::database(
            "invalid_concept_id",
            format!("stored concept ID {value}: {error}"),
        )
    })
}

fn usize_from_i64(value: i64, description: &str) -> Result<usize, AppError> {
    usize::try_from(value).map_err(|_| {
        AppError::database(
            "numeric_overflow",
            format!("stored {description} is invalid"),
        )
    })
}

fn u64_from_i64(value: i64, description: &str) -> Result<u64, AppError> {
    u64::try_from(value).map_err(|_| {
        AppError::database(
            "numeric_overflow",
            format!("stored {description} is invalid"),
        )
    })
}

fn invalid_page_limit() -> AppError {
    AppError::invalid(
        "invalid_limit",
        format!("a graph page limit must be between 1 and {MAX_PAGE_SIZE}"),
    )
}

fn invalid_walk_bounds() -> AppError {
    AppError::invalid(
        "invalid_graph_bounds",
        format!(
            "graph depth must be at most {MAX_WALK_DEPTH}, max_nodes must be between 1 and \
             {MAX_WALK_NODES}, and max_edges must be between 1 and {MAX_WALK_EDGES}"
        ),
    )
}

fn frontier_overflow() -> AppError {
    AppError::database(
        "invalid_graph_projection",
        "a projected node's stored degree is smaller than its returned degree",
    )
}

fn revision_not_found(revision: i64) -> AppError {
    AppError::not_found(
        "revision_not_found",
        format!("corpus revision {revision} was not found"),
    )
}

fn concept_not_found(id: ConceptId, revision: i64) -> AppError {
    AppError::not_found(
        "concept_not_found",
        format!("concept {id} was not found at revision {revision}"),
    )
}

const NODE_BY_ID_SQL: &str = "SELECT c.concept_id, c.label, \
            c.parent_count, c.child_count, c.evidence_count, \
            NULL \
     FROM revision_concepts AS c \
     WHERE c.revision = ?1 AND c.concept_id = ?2";

const PARENT_COUNT_SQL: &str = "SELECT COUNT(*) FROM revision_edges \
     WHERE revision = ?1 AND child_id = ?2";

const CHILD_COUNT_SQL: &str = "SELECT COUNT(*) FROM revision_edges \
     WHERE revision = ?1 AND parent_id = ?2";

const PARENT_ROWS_SQL: &str = "SELECT c.concept_id, c.label, \
            c.parent_count, c.child_count, c.evidence_count, \
            NULL \
     FROM revision_edges AS selected \
     JOIN revision_concepts AS c \
       ON c.revision = selected.revision AND c.concept_id = selected.parent_id \
     WHERE selected.revision = ?1 AND selected.child_id = ?2 \
     ORDER BY selected.parent_id \
     LIMIT ?3 OFFSET ?4";

const CHILD_ROWS_SQL: &str = "SELECT c.concept_id, c.label, \
            c.parent_count, c.child_count, c.evidence_count, \
            NULL \
     FROM revision_edges AS selected \
     JOIN revision_concepts AS c \
       ON c.revision = selected.revision AND c.concept_id = selected.child_id \
     WHERE selected.revision = ?1 AND selected.parent_id = ?2 \
     ORDER BY selected.child_id \
     LIMIT ?3 OFFSET ?4";

const EVIDENCE_ROWS_SQL: &str = "SELECT evidence.concept_id, evidence.work_id, \
            evidence.start_byte, evidence.end_byte \
     FROM revision_evidence AS evidence \
     WHERE evidence.revision = ?1 AND evidence.concept_id = ?2 \
     ORDER BY evidence.work_id, evidence.start_byte, evidence.end_byte \
     LIMIT ?3 OFFSET ?4";

macro_rules! search_query {
    ($tail:literal) => {
        concat!(
            "WITH RECURSIVE \
             terms(term) AS (\
                 SELECT CAST(value AS TEXT) FROM json_each(?2)\
             ), \
             direct(term, concept_id) AS (\
                 SELECT terms.term, concepts.concept_id \
                 FROM terms \
                 JOIN revision_concepts AS concepts \
                   ON concepts.revision = ?1 \
                  AND instr(concepts.normalized_label, terms.term) > 0\
             ), \
             matching(term, concept_id) AS (\
                 SELECT term, concept_id FROM direct \
                 UNION \
                 SELECT matching.term, edges.child_id \
                 FROM matching \
                 JOIN revision_edges AS edges \
                   ON edges.revision = ?1 \
                  AND edges.parent_id = matching.concept_id\
             ), \
             scope(concept_id) AS (\
                 SELECT concepts.concept_id \
                 FROM revision_concepts AS concepts \
                 WHERE concepts.revision = ?1 AND concepts.concept_id = ?3 \
                 UNION \
                 SELECT edges.child_id \
                 FROM scope \
                 JOIN revision_edges AS edges \
                   ON edges.revision = ?1 AND edges.parent_id = scope.concept_id\
             ), \
             satisfied(concept_id, term_count) AS (\
                 SELECT concept_id, COUNT(*) FROM matching GROUP BY concept_id\
             ), \
             candidates(\
                 concept_id, label, normalized_label, parent_count, child_count, \
                 evidence_count, exact, prefix, label_matches\
             ) AS (\
                 SELECT concepts.concept_id, concepts.label, concepts.normalized_label, \
                        concepts.parent_count, concepts.child_count, concepts.evidence_count, \
                        concepts.normalized_label = ?4, \
                        substr(concepts.normalized_label, 1, length(?4)) = ?4, \
                        (SELECT COUNT(*) FROM terms \
                         WHERE instr(concepts.normalized_label, terms.term) > 0) \
                 FROM revision_concepts AS concepts \
                 JOIN satisfied ON satisfied.concept_id = concepts.concept_id \
                 WHERE concepts.revision = ?1 \
                   AND satisfied.term_count = (SELECT COUNT(*) FROM terms) \
                   AND (?3 IS NULL OR EXISTS(\
                       SELECT 1 FROM scope WHERE scope.concept_id = concepts.concept_id\
                   ))\
             ) ",
            $tail
        )
    };
}

const SEARCH_COUNT_SQL: &str = search_query!("SELECT COUNT(*) FROM candidates");

const SEARCH_ROWS_SQL: &str = search_query!(
    "SELECT candidates.concept_id, candidates.label, \
            candidates.parent_count, candidates.child_count, candidates.evidence_count, \
            NULL, COUNT(*) OVER() \
     FROM candidates \
     ORDER BY exact DESC, prefix DESC, label_matches DESC, concept_id \
     LIMIT ?5 OFFSET ?6"
);

const WALK_PARENTS_SQL: &str = "WITH RECURSIVE reached(concept_id, distance) AS (\
         SELECT ?2, 0 \
         UNION \
         SELECT edge.parent_id, reached.distance + 1 \
         FROM reached \
         JOIN revision_edges AS edge \
           ON edge.revision = ?1 AND edge.child_id = reached.concept_id \
         WHERE reached.distance < ?3 \
         ORDER BY 2, 1 \
         LIMIT ?4\
     ), selected(concept_id, distance) AS (\
         SELECT concept_id, MIN(distance) \
         FROM reached \
         GROUP BY concept_id \
         ORDER BY MIN(distance), concept_id \
         LIMIT ?5\
     ) \
     SELECT c.concept_id, c.label, \
            c.parent_count, c.child_count, c.evidence_count, \
            selected.distance \
     FROM selected \
     JOIN revision_concepts AS c \
       ON c.revision = ?1 AND c.concept_id = selected.concept_id \
     ORDER BY selected.distance, c.concept_id";

const WALK_CHILDREN_SQL: &str = "WITH RECURSIVE reached(concept_id, distance) AS (\
         SELECT ?2, 0 \
         UNION \
         SELECT edge.child_id, reached.distance + 1 \
         FROM reached \
         JOIN revision_edges AS edge \
           ON edge.revision = ?1 AND edge.parent_id = reached.concept_id \
         WHERE reached.distance < ?3 \
         ORDER BY 2, 1 \
         LIMIT ?4\
     ), selected(concept_id, distance) AS (\
         SELECT concept_id, MIN(distance) \
         FROM reached \
         GROUP BY concept_id \
         ORDER BY MIN(distance), concept_id \
         LIMIT ?5\
     ) \
     SELECT c.concept_id, c.label, \
            c.parent_count, c.child_count, c.evidence_count, \
            selected.distance \
     FROM selected \
     JOIN revision_concepts AS c \
       ON c.revision = ?1 AND c.concept_id = selected.concept_id \
     ORDER BY selected.distance, c.concept_id";

const WALK_BOTH_SQL: &str = "WITH RECURSIVE reached(concept_id, distance) AS (\
         SELECT ?2, 0 \
         UNION \
         SELECT edge.parent_id, reached.distance + 1 \
         FROM reached \
         JOIN revision_edges AS edge \
           ON edge.revision = ?1 AND edge.child_id = reached.concept_id \
         WHERE reached.distance < ?3 \
         UNION \
         SELECT edge.child_id, reached.distance + 1 \
         FROM reached \
         JOIN revision_edges AS edge \
           ON edge.revision = ?1 AND edge.parent_id = reached.concept_id \
         WHERE reached.distance < ?3 \
         ORDER BY 2, 1 \
         LIMIT ?4\
     ), selected(concept_id, distance) AS (\
         SELECT concept_id, MIN(distance) \
         FROM reached \
         GROUP BY concept_id \
         ORDER BY MIN(distance), concept_id \
         LIMIT ?5\
     ) \
     SELECT c.concept_id, c.label, \
            c.parent_count, c.child_count, c.evidence_count, \
            selected.distance \
     FROM selected \
     JOIN revision_concepts AS c \
       ON c.revision = ?1 AND c.concept_id = selected.concept_id \
     ORDER BY selected.distance, c.concept_id";

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{
        CHILD_ROWS_SQL, EVIDENCE_ROWS_SQL, GraphReader, NeighborDirection, PARENT_ROWS_SQL,
        PageLimit, PageOffset, PageRequest, WalkBounds,
    };
    use crate::model::{ConceptId, GraphDirection};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn request_bounds_are_checked() -> TestResult {
        let Err(zero_page) = PageLimit::new(0) else {
            return Err("zero page limit was accepted".into());
        };
        assert_eq!(zero_page.code(), "invalid_limit");
        let Err(large_page) = PageLimit::new(201) else {
            return Err("large page limit was accepted".into());
        };
        assert_eq!(large_page.code(), "invalid_limit");
        let Err(deep_walk) = WalkBounds::new(11, 10, 10) else {
            return Err("deep graph walk was accepted".into());
        };
        assert_eq!(deep_walk.code(), "invalid_graph_bounds");
        let Err(empty_walk) = WalkBounds::new(2, 0, 10) else {
            return Err("empty graph walk was accepted".into());
        };
        assert_eq!(empty_walk.code(), "invalid_graph_bounds");
        Ok(())
    }

    #[test]
    fn selectors_materialize_only_the_requested_projection() -> TestResult {
        let connection = fixture()?;
        let graph = GraphReader::new(&connection).head()?;
        assert_eq!(graph.revision(), 2);

        let one = PageRequest::new(PageOffset::default(), PageLimit::new(1)?);
        let roots = graph.roots(one)?;
        assert_eq!(roots.nodes.len(), 1);
        assert_eq!(roots.page.ok_or("missing root page")?.total, 2);

        let leaf = ConceptId::from_storage(4)?;
        let concept = graph.concept(leaf)?;
        assert_eq!(concept.nodes[0].parent_count, 2);
        assert_eq!(concept.nodes[0].evidence_count, 1);

        let parents = graph.neighbors(leaf, NeighborDirection::Parents, one)?;
        assert_eq!(parents.nodes.len(), 2); // seed plus one selected parent
        assert_eq!(parents.edges.len(), 1);
        assert_eq!(parents.page.ok_or("missing neighbor page")?.total, 2);

        let evidence = graph.evidence(leaf, one)?;
        assert_eq!(evidence.evidence.len(), 1);
        assert_eq!(evidence.evidence[0].work_id, 7);
        assert_eq!(
            (
                evidence.evidence[0].start_byte,
                evidence.evidence[0].end_byte
            ),
            (4, 9)
        );
        Ok(())
    }

    #[test]
    fn revision_selection_and_walk_are_bounded() -> TestResult {
        let connection = fixture()?;
        let reader = GraphReader::new(&connection);
        let old = reader.at(1)?;
        assert!(
            old.concept(ConceptId::from_storage(4)?).is_err(),
            "revision one unexpectedly exposed a later concept"
        );
        let first_old_roots = old.roots_page(1, None)?;
        let old_cursor = first_old_roots
            .page
            .next_cursor
            .as_deref()
            .ok_or("revision one did not produce a continuation cursor")?;
        let resumed = reader.paged_at(None, Some(old_cursor))?;
        assert_eq!(resumed.revision(), 1);
        let second_old_roots = resumed.roots_page(1, Some(old_cursor))?;
        assert_eq!(second_old_roots.items.len(), 1);
        assert_eq!(second_old_roots.items[0].id.to_string(), "c1");

        let current = reader.head()?;
        let bounds = WalkBounds::new(2, 2, 1)?;
        let projection = current.walk(
            ConceptId::from_storage(1)?,
            GraphDirection::Children,
            bounds,
        )?;
        assert_eq!(projection.nodes.len(), 2);
        assert!(projection.node_limit_reached);
        assert_eq!(projection.edges.len(), 1);
        Ok(())
    }

    #[test]
    fn search_and_evidence_hydration_stay_revision_scoped_and_bounded() -> TestResult {
        let connection = fixture()?;
        let graph = GraphReader::new(&connection).at(2)?;

        let first = graph.search("Root", None, 1, None)?;
        assert_eq!(first.results.page.total, 3);
        assert_eq!(first.results.items.len(), 1);
        assert_eq!(first.results.items[0].concept.id.to_string(), "c1");
        let cursor = first
            .results
            .page
            .next_cursor
            .as_deref()
            .ok_or("search did not return a continuation cursor")?;
        let second = graph.search("Root", None, 1, Some(cursor))?;
        assert_eq!(second.results.items.len(), 1);
        assert_eq!(second.results.items[0].concept.id.to_string(), "c2");
        let repeated_term = graph.search("Root Root", None, 10, None)?;
        assert_eq!(repeated_term.results.page.total, 3);

        let child = ConceptId::from_storage(2)?;
        let scoped = graph.search("Root", Some(child), 10, None)?;
        assert_eq!(scoped.results.page.total, 2);
        assert_eq!(scoped.within.ok_or("missing search scope")?.id, child);

        let evidence = graph.evidence_page(ConceptId::from_storage(4)?, 1, None)?;
        assert_eq!(evidence.items.len(), 1);
        assert_eq!(evidence.items[0].work, "Work");
        assert_eq!(evidence.items[0].quote, "45678");
        Ok(())
    }

    #[test]
    fn local_pages_use_revision_leading_indexes_without_temporary_sorts() -> TestResult {
        let connection = fixture()?;
        let parents = query_plan(&connection, PARENT_ROWS_SQL)?;
        let children = query_plan(&connection, CHILD_ROWS_SQL)?;
        let evidence = query_plan(&connection, EVIDENCE_ROWS_SQL)?;

        assert!(
            parents
                .iter()
                .any(|detail| detail.contains("revision_edges_by_child"))
        );
        assert!(
            children
                .iter()
                .any(|detail| detail.contains("SEARCH selected USING")),
            "{children:?}"
        );
        assert!(
            evidence
                .iter()
                .any(|detail| detail.contains("SEARCH evidence USING")),
            "{evidence:?}"
        );
        for detail in parents.iter().chain(&children).chain(&evidence) {
            assert!(!detail.contains("USE TEMP B-TREE"), "{detail}");
            assert!(!detail.starts_with("SCAN selected"), "{detail}");
            assert!(!detail.starts_with("SCAN evidence"), "{detail}");
        }
        Ok(())
    }

    fn query_plan(connection: &Connection, sql: &str) -> TestResult<Vec<String>> {
        let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        Ok(statement
            .query_map([2_i64, 4, 1, 0], |row| row.get::<_, String>(3))?
            .collect::<Result<Vec<_>, _>>()?)
    }

    fn fixture() -> Result<Connection, rusqlite::Error> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(
            "CREATE TABLE library_state(\
                 singleton INTEGER PRIMARY KEY, revision INTEGER NOT NULL, library_id TEXT NOT NULL\
             ); \
             INSERT INTO library_state VALUES(1, 2, '0123456789abcdef0123456789abcdef'); \
             CREATE TABLE commits(revision INTEGER PRIMARY KEY); \
             INSERT INTO commits VALUES(1), (2); \
             CREATE TABLE revision_snapshots(\
                 revision INTEGER PRIMARY KEY, concept_count INTEGER NOT NULL, \
                 edge_count INTEGER NOT NULL, evidence_count INTEGER NOT NULL\
             ); \
             INSERT INTO revision_snapshots VALUES(1, 3, 1, 1), (2, 4, 3, 1); \
             CREATE TABLE revision_concepts(\
                 revision INTEGER NOT NULL, concept_id INTEGER NOT NULL, \
                 label TEXT NOT NULL, normalized_label TEXT NOT NULL, \
                 parent_count INTEGER NOT NULL, child_count INTEGER NOT NULL, \
                 evidence_count INTEGER NOT NULL, \
                 PRIMARY KEY(revision, concept_id)\
             ); \
             CREATE INDEX revision_concepts_by_label \
                 ON revision_concepts(revision, normalized_label, concept_id); \
             CREATE TABLE revision_edges(\
                 revision INTEGER NOT NULL, parent_id INTEGER NOT NULL, child_id INTEGER NOT NULL, \
                 PRIMARY KEY(revision, parent_id, child_id)\
             ); \
             CREATE INDEX revision_edges_by_child \
                 ON revision_edges(revision, child_id, parent_id); \
             CREATE TABLE revision_evidence(\
                 revision INTEGER NOT NULL, concept_id INTEGER NOT NULL, work_id INTEGER NOT NULL, \
                 start_byte INTEGER NOT NULL, end_byte INTEGER NOT NULL, \
                 PRIMARY KEY(revision, concept_id, work_id, start_byte, end_byte)\
             ); \
             CREATE TABLE works(\
                 id INTEGER PRIMARY KEY, label TEXT NOT NULL, normalized_label TEXT NOT NULL, \
                 text TEXT NOT NULL\
             ); \
             INSERT INTO works VALUES(7, 'Work', 'work', '0123456789'); \
             INSERT INTO revision_concepts VALUES \
                 (1, 1, 'Root', 'root', 0, 1, 0), \
                 (1, 2, 'Child', 'child', 1, 0, 1), \
                 (1, 3, 'Other', 'other', 0, 0, 0), \
                 (2, 1, 'Root', 'root', 0, 1, 0), \
                 (2, 2, 'Child', 'child', 1, 1, 0), \
                 (2, 3, 'Other', 'other', 0, 1, 0), \
                 (2, 4, 'Leaf', 'leaf', 2, 0, 1); \
             INSERT INTO revision_edges VALUES \
                 (1, 1, 2), \
                 (2, 1, 2), \
                 (2, 2, 4), \
                 (2, 3, 4); \
             INSERT INTO revision_evidence VALUES \
                 (1, 2, 7, 0, 3), \
                 (2, 4, 7, 4, 9);",
        )?;
        Ok(connection)
    }
}
