//! Annals-owned corpus, retention, integration, and inbox interfaces.
//!
//! Public views are shared with the CLI. Database connections, stored rows,
//! workers, and mutation recovery remain private to Annals.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub use crate::change::{
    ChangeOperation, ConceptSelector, EvidenceDisposition, EvidenceSelector, Reconciliation,
    ReconciliationContractError, parse_reconciliation,
};
pub use crate::cli::{
    AtArgs, BackupArgs, ChangeCommand, ChangeSelectArgs, ChangeShowArgs, ChangeSubmitArgs,
    CliGraphDirection, CliLibraryKind, Command as Request, ConceptCommand, ConceptPageArgs,
    ConceptShowArgs, DecisionFeedCommand, DecisionFeedPageArgs, DiffArgs, GraphArgs,
    InboxAcceptArgs, InboxCommand, InboxEnqueueArgs, InboxImportArgs, InboxInterruptArgs,
    InboxInterruptDisposition, InboxPriorityArgs, InboxRetryCommand, InboxRetryContinueArgs,
    InboxRetryStartArgs, InboxRetryStatusArgs, InboxRetryWindowArgs, InboxRunArgs,
    IngestionChannel, IngestionStatus, InitArgs, IntegrateArgs, LatelyArgs, LatelyTime, LogArgs,
    PagedAtArgs, RevertArgs, SearchArgs, ShakeArgs, WorkAddArgs, WorkCommand, WorkShowArgs,
};
pub use crate::client::{CliClient, ClientError, Response};
pub use crate::corpus::ShakeEdge;
pub use crate::error::AppError;
pub use crate::inbox::{
    ActiveJob, BacklogImportSummary, EnqueueSummary, InboxStatus, InterruptSummary, JobPriority,
    PauseSummary, PrioritySummary, RegisteredJob, RegistrationSummary, RunSummary,
    StorageLocationStatus, StorageStatus,
};
pub use crate::inbox_retry_store::{
    RetryEvent, RetryEventReport, RetryHalt, RetryItem, RetrySelection, RetrySelectionItem,
    RetrySummary,
};
pub use crate::ingestion::{IngestionErrorView, IngestionView, LatelyReport};
pub use crate::model::{
    CommitView, ConceptDetail, ConceptId, ConceptReference, ConceptSummary, CorpusOverview,
    DiffEntry, DiffView, EvidenceView, FrontierEntry, GraphDirection, GraphEdge, GraphNode,
    GraphView, HeadingView, InvalidConceptId, LibraryStats, Page, PageInfo, ReconciliationView,
    RecordedChangeView, SearchOutput, SearchResult, WorkSummary, WorkView,
};
pub use crate::model_runner::ModelQuality;
pub use crate::resolver::{ResolvedEvidence, ResolvedOperation};
pub use annals_api::SuccessEnvelope;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ErrorOutput {
    pub ok: bool,
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InitializedLibrary {
    pub library: String,
    pub library_id: String,
    pub kind: String,
    pub revision: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MigratedLibrary {
    pub library: String,
    pub from_version: i64,
    pub to_version: i64,
    pub migrated: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BackupResult {
    pub output: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetentionResult {
    pub work: String,
    pub size_bytes: usize,
    pub sha256: String,
    pub first_retained_at: String,
    pub retention: String,
    pub corpus_revision: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkContent {
    #[serde(flatten)]
    pub view: WorkView,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReconciliationResult {
    pub work: String,
    pub base_revision: i64,
    pub status: String,
    pub summary: String,
    pub operation_count: usize,
    pub annotations: Vec<String>,
    pub reconciliation: Reconciliation,
    pub created_at: String,
    pub applied_revision: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ValidatedReconciliation {
    pub work: String,
    pub base_revision: i64,
    pub status: String,
    pub summary: String,
    pub operations: Vec<ResolvedOperation>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppliedReconciliation {
    pub work: String,
    pub base_revision: i64,
    pub revision: i64,
    pub status: String,
    pub summary: String,
    pub annotations: Vec<String>,
    pub reconciliation: Reconciliation,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RootsResult {
    pub revision: i64,
    pub roots: Page<ConceptSummary>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConceptResult {
    pub revision: i64,
    pub concept: ConceptDetail,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ParentsResult {
    pub revision: i64,
    pub concept: ConceptReference,
    pub parents: Page<ConceptReference>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChildrenResult {
    pub revision: i64,
    pub concept: ConceptReference,
    pub children: Page<ConceptReference>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EvidenceResult {
    pub revision: i64,
    pub concept: ConceptReference,
    pub evidence: Page<EvidenceView>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShakeResult {
    pub status: String,
    pub base_revision: i64,
    pub revision: i64,
    pub edge_count_before: usize,
    pub removed_edge_count: usize,
    pub edge_count_after: usize,
    pub removed_edges: Vec<ShakeEdge>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LogResult {
    pub head_revision: i64,
    pub commits: Vec<CommitView>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RevertResult {
    pub revision: i64,
    pub reverted_revision: i64,
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetryEventsResult {
    pub events: Vec<RetryEvent>,
}

/// A read-only library handle. It exposes owned views, never a database connection.
pub struct LibraryReader {
    connection: rusqlite::Connection,
}

impl LibraryReader {
    /// Read direct parents with the same cursor and revision checks as the CLI.
    ///
    /// # Errors
    /// Returns an Annals input, cursor, or library-read error.
    pub fn parents(
        &self,
        revision: Option<i64>,
        id: ConceptId,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<ParentsResult, AppError> {
        let graph = self.graph(revision, cursor)?;
        Ok(ParentsResult {
            revision: graph.revision(),
            concept: graph.reference(id)?,
            parents: graph.neighbor_page(
                id,
                crate::graph::NeighborDirection::Parents,
                limit,
                cursor,
            )?,
        })
    }

    /// Read direct children with the same cursor and revision checks as the CLI.
    ///
    /// # Errors
    /// Returns an Annals input, cursor, or library-read error.
    pub fn children(
        &self,
        revision: Option<i64>,
        id: ConceptId,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<ChildrenResult, AppError> {
        let graph = self.graph(revision, cursor)?;
        Ok(ChildrenResult {
            revision: graph.revision(),
            concept: graph.reference(id)?,
            children: graph.neighbor_page(
                id,
                crate::graph::NeighborDirection::Children,
                limit,
                cursor,
            )?,
        })
    }

    /// Expand the existing bounded graph view around one concept.
    ///
    /// # Errors
    /// Returns an Annals input, graph-bound, or library-read error.
    pub fn walk(
        &self,
        revision: Option<i64>,
        seed: ConceptId,
        direction: GraphDirection,
        depth: usize,
        max_nodes: usize,
    ) -> Result<GraphView, AppError> {
        self.graph(revision, None)?
            .graph_view(seed, direction, depth, max_nodes)
    }
    /// Open a readable, compatible Annals library.
    ///
    /// # Errors
    /// Returns the existing Annals error for invalid input or unreadable library state.
    pub fn open(path: &Path) -> Result<Self, AppError> {
        Ok(Self {
            connection: crate::db::open_read(path)?,
        })
    }

    /// List retained work summaries.
    ///
    /// # Errors
    /// Returns the existing Annals error for invalid input or unreadable library state.
    pub fn works(&self) -> Result<Vec<WorkSummary>, AppError> {
        crate::corpus::list_works(&self.connection)
    }

    /// Read one retained work and its exact text.
    ///
    /// # Errors
    /// Returns the existing Annals error for invalid input or unreadable library state.
    pub fn work(&self, label: &str) -> Result<WorkContent, AppError> {
        let work = crate::corpus::get_work(&self.connection, label)?;
        Ok(WorkContent {
            view: crate::corpus::work_view(&work),
            text: work.text,
        })
    }

    /// Read the counts for one corpus revision.
    ///
    /// # Errors
    /// Returns the existing Annals error for invalid input or unreadable library state.
    pub fn overview(&self, revision: Option<i64>) -> Result<CorpusOverview, AppError> {
        self.graph(revision, None)?.overview()
    }

    /// Read a page of root concepts, preserving the cursor revision.
    ///
    /// # Errors
    /// Returns the existing Annals error for invalid input or unreadable library state.
    pub fn roots(
        &self,
        revision: Option<i64>,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<RootsResult, AppError> {
        let graph = self.graph(revision, cursor)?;
        Ok(RootsResult {
            revision: graph.revision(),
            roots: graph.roots_page(limit, cursor)?,
        })
    }

    /// Read one concept with bounded neighboring and evidence previews.
    ///
    /// # Errors
    /// Returns the existing Annals error for invalid input or unreadable library state.
    pub fn concept(
        &self,
        revision: Option<i64>,
        id: ConceptId,
        preview_limit: usize,
    ) -> Result<ConceptResult, AppError> {
        let graph = self.graph(revision, None)?;
        Ok(ConceptResult {
            revision: graph.revision(),
            concept: graph.concept_detail(id, preview_limit)?,
        })
    }

    /// Read a page of evidence for one concept.
    ///
    /// # Errors
    /// Returns the existing Annals error for invalid input or unreadable library state.
    pub fn evidence(
        &self,
        revision: Option<i64>,
        id: ConceptId,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<EvidenceResult, AppError> {
        let graph = self.graph(revision, cursor)?;
        Ok(EvidenceResult {
            revision: graph.revision(),
            concept: graph.reference(id)?,
            evidence: graph.evidence_page(id, limit, cursor)?,
        })
    }

    /// Search concept wording using the existing bounded lexical search.
    ///
    /// # Errors
    /// Returns the existing Annals error for invalid input or unreadable library state.
    pub fn search(
        &self,
        revision: Option<i64>,
        query: &str,
        within: Option<ConceptId>,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<SearchOutput, AppError> {
        self.graph(revision, cursor)?
            .search(query, within, limit, cursor)
    }

    /// Read the newest corpus commits and current revision.
    ///
    /// # Errors
    /// Returns the existing Annals error for invalid input or unreadable library state.
    pub fn log(&self, limit: usize) -> Result<LogResult, AppError> {
        Ok(LogResult {
            head_revision: crate::corpus::revision(&self.connection)?,
            commits: crate::corpus::list_commits(&self.connection, limit)?,
        })
    }

    /// Read the recorded effects of one corpus commit.
    ///
    /// # Errors
    /// Returns the existing Annals error for invalid input or unreadable library state.
    pub fn change(&self, revision: i64) -> Result<RecordedChangeView, AppError> {
        crate::corpus::recorded_change_at(&self.connection, revision)
    }

    /// List retained reconciliations.
    ///
    /// # Errors
    /// Returns the existing Annals error for invalid input or unreadable library state.
    pub fn reconciliations(&self) -> Result<Vec<ReconciliationView>, AppError> {
        crate::corpus::list_reconciliations(&self.connection)
    }

    /// Compare two corpus revisions.
    ///
    /// # Errors
    /// Returns the existing Annals error for invalid input or unreadable library state.
    pub fn diff(&self, from: i64, to: i64) -> Result<DiffView, AppError> {
        crate::corpus::diff(&self.connection, from, to)
    }

    fn graph(
        &self,
        revision: Option<i64>,
        cursor: Option<&str>,
    ) -> Result<crate::graph::GraphView<'_>, AppError> {
        crate::graph::GraphReader::new(&self.connection).paged_at(revision, cursor)
    }
}
