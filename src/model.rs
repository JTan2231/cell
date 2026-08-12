use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadingView {
    pub path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSummary {
    pub work: String,
    pub sha256: String,
    pub size_bytes: usize,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkView {
    #[serde(flatten)]
    pub summary: WorkSummary,
    pub headings: Vec<HeadingView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceView {
    pub work: String,
    pub quote: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptView {
    pub path: Vec<String>,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<Vec<String>>,
    pub children: Vec<String>,
    pub evidence: Vec<EvidenceView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusView {
    pub revision: i64,
    pub concepts: Vec<ConceptView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalView {
    pub work: String,
    pub base_revision: i64,
    pub status: String,
    pub outcome: String,
    pub summary: String,
    pub request: serde_json::Value,
    pub uncertainties: Vec<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_revision: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitView {
    pub revision: i64,
    pub parent_revision: i64,
    pub kind: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work: Option<String>,
    pub actor: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedChangeView {
    pub revision: i64,
    pub parent_revision: i64,
    pub base_revision: i64,
    pub status: String,
    pub kind: String,
    pub summary: String,
    pub work: Option<String>,
    pub submitted_request: serde_json::Value,
    pub resolved_operations: serde_json::Value,
    pub metadata: serde_json::Value,
    pub actor: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    Created,
    Retired,
    Moved,
    Reworded,
    EvidenceAdded,
    EvidenceRemoved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffEntry {
    pub kind: DiffKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffView {
    pub from_revision: i64,
    pub to_revision: i64,
    pub entries: Vec<DiffEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    pub path: Vec<String>,
    pub label: String,
    pub evidence: Vec<EvidenceView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchOutput {
    pub query: String,
    pub results: Vec<SearchResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryStats {
    pub revision: i64,
    pub concept_count: u64,
    pub work_count: u64,
    pub evidence_count: u64,
    pub pending_change_count: u64,
    pub commit_count: u64,
    pub model_run_count: u64,
    pub database_size_bytes: u64,
    pub index_current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
}
