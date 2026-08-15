use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConceptId(i64);

impl ConceptId {
    pub fn from_storage(value: i64) -> Result<Self, InvalidConceptId> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(InvalidConceptId)
        }
    }

    #[must_use]
    pub const fn storage_id(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for ConceptId {
    type Error = InvalidConceptId;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::from_storage(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidConceptId;

impl fmt::Display for InvalidConceptId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a concept ID must be c followed by a positive decimal integer")
    }
}

impl std::error::Error for InvalidConceptId {}

impl fmt::Display for ConceptId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "c{}", self.0)
    }
}

impl FromStr for ConceptId {
    type Err = InvalidConceptId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let digits = value.strip_prefix('c').ok_or(InvalidConceptId)?;
        if digits.is_empty()
            || digits.starts_with('0')
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(InvalidConceptId);
        }
        let storage_id = digits.parse::<i64>().map_err(|_| InvalidConceptId)?;
        Self::from_storage(storage_id)
    }
}

impl Serialize for ConceptId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ConceptId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ConceptIdVisitor;

        impl Visitor<'_> for ConceptIdVisitor {
            type Value = ConceptId;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a concept ID such as \"c42\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                value.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(ConceptIdVisitor)
    }
}

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
pub struct ConceptReference {
    pub id: ConceptId,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptSummary {
    pub id: ConceptId,
    pub label: String,
    pub parent_count: u64,
    pub child_count: u64,
    pub evidence_count: u64,
    pub root: bool,
    pub leaf: bool,
    pub shared: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PageInfo {
    pub limit: usize,
    pub returned: usize,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: PageInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptDetail {
    #[serde(flatten)]
    pub summary: ConceptSummary,
    pub parents: Page<ConceptReference>,
    pub children: Page<ConceptReference>,
    pub evidence: Page<EvidenceView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusOverview {
    pub revision: i64,
    pub concept_count: u64,
    pub edge_count: u64,
    pub root_count: u64,
    pub leaf_count: u64,
    pub shared_concept_count: u64,
    pub evidence_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphDirection {
    Parents,
    Children,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    #[serde(flatten)]
    pub summary: ConceptSummary,
    pub distance: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub parent_id: ConceptId,
    pub child_id: ConceptId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontierEntry {
    pub id: ConceptId,
    pub unreturned_parent_count: u64,
    pub unreturned_child_count: u64,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphView {
    pub revision: i64,
    pub seed: ConceptId,
    pub direction: GraphDirection,
    pub depth: usize,
    pub max_nodes: usize,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub complete_within_depth: bool,
    pub node_limit_reached: bool,
    pub frontier: Vec<FrontierEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciliationView {
    pub work: String,
    pub base_revision: i64,
    pub status: String,
    pub summary: String,
    pub request: serde_json::Value,
    pub annotations: Vec<String>,
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
    pub effects: Vec<DiffEntry>,
    pub metadata: serde_json::Value,
    pub actor: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiffEntry {
    Created {
        concept: ConceptReference,
    },
    Retired {
        concept: ConceptReference,
    },
    Reworded {
        id: ConceptId,
        before: String,
        after: String,
    },
    ParentAdded {
        parent: ConceptReference,
        child: ConceptReference,
    },
    ParentRemoved {
        parent: ConceptReference,
        child: ConceptReference,
    },
    EvidenceAdded {
        concept: ConceptReference,
        work: String,
        quote: String,
    },
    EvidenceRemoved {
        concept: ConceptReference,
        work: String,
        quote: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffView {
    pub from_revision: i64,
    pub to_revision: i64,
    pub entries: Vec<DiffEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    pub concept: ConceptSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchOutput {
    pub revision: i64,
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within: Option<ConceptReference>,
    pub results: Page<SearchResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryStats {
    pub revision: i64,
    pub concept_count: u64,
    pub edge_count: u64,
    pub work_count: u64,
    pub evidence_count: u64,
    pub pending_reconciliation_count: u64,
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

#[cfg(test)]
mod tests {
    use super::{ConceptId, ConceptReference, DiffEntry};

    #[test]
    fn concept_ids_round_trip_as_prefixed_strings() {
        for storage_id in [1, 42, i64::MAX] {
            let id = ConceptId::from_storage(storage_id)
                .unwrap_or_else(|error| panic!("valid storage ID was rejected: {error}"));
            let encoded = serde_json::to_string(&id)
                .unwrap_or_else(|error| panic!("concept ID serialization failed: {error}"));
            assert_eq!(encoded, format!("\"c{storage_id}\""));
            let decoded: ConceptId = serde_json::from_str(&encoded)
                .unwrap_or_else(|error| panic!("concept ID deserialization failed: {error}"));
            assert_eq!(decoded, id);
            assert_eq!(decoded.storage_id(), storage_id);
        }
    }

    #[test]
    fn concept_ids_reject_noncanonical_spellings() {
        for invalid in [
            "",
            "c",
            "c0",
            "c01",
            "C1",
            "+1",
            "1",
            "c-1",
            " c1",
            "c1 ",
            "c١",
            "c9223372036854775808",
        ] {
            assert!(
                invalid.parse::<ConceptId>().is_err(),
                "accepted {invalid:?}"
            );
        }
        assert!(ConceptId::from_storage(0).is_err());
        assert!(ConceptId::from_storage(-1).is_err());
        assert!(serde_json::from_str::<ConceptId>("42").is_err());
    }

    #[test]
    fn graph_diff_uses_public_ids() {
        let entry = DiffEntry::ParentAdded {
            parent: ConceptReference {
                id: ConceptId::from_storage(1).unwrap_or_else(|error| panic!("{error}")),
                label: "Parent".to_owned(),
            },
            child: ConceptReference {
                id: ConceptId::from_storage(2).unwrap_or_else(|error| panic!("{error}")),
                label: "Child".to_owned(),
            },
        };
        let encoded = serde_json::to_value(&entry)
            .unwrap_or_else(|error| panic!("diff serialization failed: {error}"));
        assert_eq!(encoded["kind"], "parent_added");
        assert_eq!(encoded["parent"]["id"], "c1");
        assert_eq!(encoded["child"]["id"], "c2");
        let decoded: DiffEntry = serde_json::from_value(encoded)
            .unwrap_or_else(|error| panic!("diff deserialization failed: {error}"));
        assert_eq!(decoded, entry);
    }
}
