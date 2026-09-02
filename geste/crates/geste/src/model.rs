use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub schema_version: u32,
    pub title: String,
    pub shape: String,
    pub basis_cutoff_at: String,
    pub recorded_by: String,
    pub situation: String,
    pub response: String,
    pub outcome: Outcome,
    pub applicability: String,
    pub actions: Vec<String>,
    pub lessons: Vec<String>,
    pub settlements: Vec<Settlement>,
    pub tags: Vec<String>,
    pub gaps: Vec<String>,
    pub sources: Vec<SourceAnchor>,
    pub related_episodes: Vec<RelatedEpisode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub status: OutcomeStatus,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeStatus {
    Solved,
    Partial,
    Failed,
    Unknown,
}

impl OutcomeStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solved => "solved",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "solved" => Some(Self::Solved),
            "partial" => Some(Self::Partial),
            "failed" => Some(Self::Failed),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Settlement {
    pub id: String,
    pub statement: String,
    pub status: SettlementStatus,
    #[serde(deserialize_with = "required_nullable")]
    pub gap: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SettlementStatus {
    Verified,
    Unverified,
}

impl SettlementStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Unverified => "unverified",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "verified" => Some(Self::Verified),
            "unverified" => Some(Self::Unverified),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceAnchor {
    pub id: String,
    pub system: String,
    pub kind: String,
    pub reference: String,
    #[serde(deserialize_with = "required_nullable")]
    pub revision: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub digest: Option<String>,
    pub observed_at: String,
    pub role: SourceRole,
    pub label: String,
    pub supports: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceRole {
    Authority,
    Context,
    Evidence,
    Effect,
    Procedure,
    Outcome,
}

impl SourceRole {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authority => "authority",
            Self::Context => "context",
            Self::Evidence => "evidence",
            Self::Effect => "effect",
            Self::Procedure => "procedure",
            Self::Outcome => "outcome",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "authority" => Some(Self::Authority),
            "context" => Some(Self::Context),
            "evidence" => Some(Self::Evidence),
            "effect" => Some(Self::Effect),
            "procedure" => Some(Self::Procedure),
            "outcome" => Some(Self::Outcome),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RelatedEpisode {
    pub episode: String,
    pub revision: u32,
    pub relation: EpisodeRelation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeRelation {
    BuildsOn,
    SimilarTo,
    ContrastsWith,
    Supersedes,
}

impl EpisodeRelation {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BuildsOn => "builds_on",
            Self::SimilarTo => "similar_to",
            Self::ContrastsWith => "contrasts_with",
            Self::Supersedes => "supersedes",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "builds_on" => Some(Self::BuildsOn),
            "similar_to" => Some(Self::SimilarTo),
            "contrasts_with" => Some(Self::ContrastsWith),
            "supersedes" => Some(Self::Supersedes),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RevisionView {
    pub episode: String,
    pub revision: u32,
    pub submitted_sha256: String,
    pub recorded_at: String,
    #[serde(flatten)]
    pub capture: Capture,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EpisodeListItem {
    pub episode: String,
    pub revision: u32,
    pub title: String,
    pub shape: String,
    pub outcome_status: OutcomeStatus,
    pub basis_cutoff_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SearchResult {
    pub episode: String,
    pub revision: u32,
    pub title: String,
    pub shape: String,
    pub outcome_status: OutcomeStatus,
    pub score: u32,
    pub matched_terms: Vec<String>,
    pub matched_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Report {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub episode: RevisionView,
    pub interpretation_label: &'static str,
    pub source_boundary: &'static str,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Graph {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub episode: String,
    pub revision: u32,
    pub interpretation_label: &'static str,
    pub source_boundary: &'static str,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphNode {
    pub id: String,
    pub kind: &'static str,
    pub origin: &'static str,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<GraphSource>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphSource {
    pub system: String,
    pub kind: String,
    pub reference: String,
    pub revision: Option<String>,
    pub digest: Option<String>,
    pub observed_at: String,
    pub role: SourceRole,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
