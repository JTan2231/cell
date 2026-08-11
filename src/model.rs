use std::{fmt, str::FromStr};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Stable identifier for a node within one library.
pub type NodeId = i64;

/// The structural role of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Topic,
    Source,
}

impl NodeKind {
    /// Return the value stored in `SQLite` and exposed through JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Topic => "topic",
            Self::Source => "source",
        }
    }
}

impl fmt::Display for NodeKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for NodeKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "topic" => Ok(Self::Topic),
            "source" => Ok(Self::Source),
            _ => Err(format!("invalid node kind: {value}")),
        }
    }
}

/// Hard node-kind filter accepted by search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum SearchKind {
    #[default]
    All,
    Topic,
    Source,
}

impl SearchKind {
    /// Return the value exposed through JSON and the command line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Topic => "topic",
            Self::Source => "source",
        }
    }

    /// Convert a concrete filter into its corresponding node kind.
    #[must_use]
    pub const fn node_kind(self) -> Option<NodeKind> {
        match self {
            Self::All => None,
            Self::Topic => Some(NodeKind::Topic),
            Self::Source => Some(NodeKind::Source),
        }
    }
}

impl fmt::Display for SearchKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Tree-aware search presentation preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum Detail {
    Overview,
    #[default]
    Balanced,
    Source,
}

impl Detail {
    /// Return the value exposed through JSON and the command line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Balanced => "balanced",
            Self::Source => "source",
        }
    }
}

impl fmt::Display for Detail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Optional provenance attached to a source node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub node_id: NodeId,
    pub locator: Option<String>,
    pub media_type: Option<String>,
    pub checksum: Option<String>,
    pub captured_at: Option<String>,
}

/// One canonical topic or source node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub parent_id: Option<NodeId>,
    pub kind: NodeKind,
    pub title: String,
    pub body: String,
    #[serde(skip_serializing)]
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
    pub source: Option<Source>,
}

/// A node title in a root-to-result breadcrumb.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreadcrumbItem {
    #[serde(rename = "node_id")]
    pub id: NodeId,
    pub title: String,
}

/// One root returned by `tree list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeSummary {
    pub root_id: NodeId,
    pub title: String,
    pub node_count: u64,
}

/// One row in a depth-first tree rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    pub node: Node,
    pub depth: u64,
}

/// Counts and index state returned by `stats`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryStats {
    pub root_count: u64,
    pub node_count: u64,
    pub source_count: u64,
    pub indexed_unit_count: u64,
    pub schema_version: i64,
    pub database_size_bytes: u64,
    pub index_current: bool,
}

/// Severity of a validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Warning,
    Error,
}

/// One structural, integrity, or derived-index validation finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub code: String,
    pub message: String,
    pub node_id: Option<NodeId>,
}

/// Result of validating a library without modifying it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
}

/// Byte range of a passage in the canonical node body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyRange {
    pub start_byte: usize,
    pub end_byte: usize,
}

/// Stable, user-facing explanation for why a search result matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchReason {
    ExactId,
    ExactPath,
    ExactTitle,
    Phrase,
    Lexical,
    Prefix,
    Typo,
    DescendantSupport,
}

/// Unstable per-result ranking and grouping diagnostics for `search --explain`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultExplanation {
    pub primary_unit_id: Option<i64>,
    pub raw_bm25: Option<f64>,
    pub lexical_rank: Option<usize>,
    pub retrieval_pass: Option<String>,
    pub exact_class: String,
    pub direct_score: f64,
    pub support_score: f64,
    pub support_source_node_id: Option<NodeId>,
    pub chain_group_node_id: NodeId,
    pub grouping_reason: String,
    pub branch_key: NodeId,
    pub diversity_reason: String,
    pub final_position: Option<usize>,
}

/// Unstable query-level diagnostics emitted only for `search --explain`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchExplanation {
    pub exact_candidates: usize,
    pub after_and_candidates: usize,
    pub or_fallback_used: bool,
    pub after_or_candidates: usize,
    pub prefix_fallback_used: bool,
    pub after_prefix_candidates: usize,
    pub groups_after_collapse: usize,
    pub returned_results: usize,
}

/// A secondary direct hit attached to a primary result on the same chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelatedHit {
    pub node_id: NodeId,
    pub kind: NodeKind,
    pub title: String,
    pub breadcrumb: Vec<BreadcrumbItem>,
    pub body_range: Option<BodyRange>,
    pub snippet: Option<String>,
    pub match_reasons: Vec<MatchReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<ResultExplanation>,
}

/// One ranked, node-level search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub rank: usize,
    pub node_id: NodeId,
    pub kind: NodeKind,
    pub title: String,
    pub breadcrumb: Vec<BreadcrumbItem>,
    pub body_range: Option<BodyRange>,
    pub snippet: Option<String>,
    pub match_reasons: Vec<MatchReason>,
    pub related_hits: Vec<RelatedHit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<ResultExplanation>,
}

/// Search response payload shared by human and JSON renderers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchOutput {
    pub query: String,
    pub results: Vec<SearchResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<SearchExplanation>,
}

/// IDs changed by a successful mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationOutput {
    pub node_ids: Vec<NodeId>,
}

#[cfg(test)]
mod tests {
    use super::{NodeKind, SearchKind};
    use std::str::FromStr;

    #[test]
    fn node_kind_uses_database_spelling() {
        assert_eq!(NodeKind::from_str("topic"), Ok(NodeKind::Topic));
        assert_eq!(NodeKind::Source.as_str(), "source");
        assert!(NodeKind::from_str("other").is_err());
    }

    #[test]
    fn all_search_kind_has_no_database_filter() {
        assert_eq!(SearchKind::All.node_kind(), None);
        assert_eq!(SearchKind::Source.node_kind(), Some(NodeKind::Source));
    }
}
