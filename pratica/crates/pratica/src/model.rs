#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest as _, Sha256};
use std::fmt;

pub const MAX_MARKDOWN_BYTES: usize = 1024 * 1024;
pub const MAX_FROZEN_SOURCE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_FROZEN_CATALOG_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_FROZEN_SOURCES: usize = 4_000;
pub const MAX_AGENT_REQUEST_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TOOL_RESULT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MarkdownError {
    #[error("markdown must not be empty")]
    Empty,
    #[error("markdown exceeds the {MAX_MARKDOWN_BYTES}-byte limit")]
    TooLarge,
    #[error("markdown must be valid UTF-8")]
    InvalidUtf8,
}

/// Exact, nonempty UTF-8 Markdown. No normalization or structural parsing occurs.
#[derive(Clone, PartialEq, Eq)]
pub struct OpaqueMarkdown(String);

impl OpaqueMarkdown {
    pub fn new(bytes: Vec<u8>) -> Result<Self, MarkdownError> {
        if bytes.is_empty() {
            return Err(MarkdownError::Empty);
        }
        if bytes.len() > MAX_MARKDOWN_BYTES {
            return Err(MarkdownError::TooLarge);
        }
        String::from_utf8(bytes)
            .map(Self)
            .map_err(|_| MarkdownError::InvalidUtf8)
    }

    pub fn from_text(text: impl Into<String>) -> Result<Self, MarkdownError> {
        Self::new(text.into().into_bytes())
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn sha256(&self) -> String {
        sha256_hex(self.0.as_bytes())
    }
}

impl fmt::Debug for OpaqueMarkdown {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpaqueMarkdown")
            .field("byte_len", &self.0.len())
            .field("sha256", &self.sha256())
            .finish()
    }
}

impl Serialize for OpaqueMarkdown {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for OpaqueMarkdown {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        Self::new(text.into_bytes()).map_err(de::Error::custom)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }

            #[allow(dead_code)]
            pub fn parse(value: &str) -> Option<Self> {
                match value {
                    $($value => Some(Self::$variant)),+,
                    _ => None,
                }
            }
        }
    };
}

string_enum!(PartyRole {
    Entrant => "entrant",
    Steward => "steward",
});

string_enum!(NegotiationKind {
    Initial => "initial",
    Amendment => "amendment",
});

string_enum!(NegotiationStatus {
    Open => "open",
    Sealed => "sealed",
    Cancelled => "cancelled",
});

string_enum!(AssentStatus {
    None => "none",
    Current => "current",
    StaleTerms => "stale_terms",
    StaleBasis => "stale_basis",
    UnknownBasis => "unknown_basis",
    Withdrawn => "withdrawn",
    Blocked => "blocked",
});

string_enum!(BasisFreshness {
    Fresh => "fresh",
    Stale => "stale",
    Unknown => "unknown",
});

string_enum!(BasisKind {
    Steward => "steward",
    Candidate => "candidate",
});

string_enum!(NegotiationEventKind {
    Opened => "opened",
    OfferSubmitted => "offer_submitted",
    Assent => "assent",
    AssentWithdrawn => "assent_withdrawn",
    StewardBlocked => "steward_blocked",
    Cancelled => "cancelled",
    AgreementSealed => "agreement_sealed",
});

string_enum!(AttemptKind {
    StewardResponse => "steward_response",
    CompositionReview => "composition_review",
    ConformanceReview => "conformance_review",
});

string_enum!(RuntimeState {
    Prepared => "prepared",
    Admitted => "admitted",
    Running => "running",
    Completed => "completed",
    Failed => "failed",
    Cancelled => "cancelled",
    Lost => "lost",
    TimedOut => "timed_out",
});

impl RuntimeState {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Lost | Self::TimedOut
        )
    }
}

string_enum!(CompositionOutcome {
    Compatible => "compatible",
    Conflicts => "conflicts",
    Blocked => "blocked",
});

string_enum!(ConformanceOutcome {
    Conforms => "conforms",
    DoesNotConform => "does_not_conform",
    Blocked => "blocked",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewStewardScope {
    pub scope_id: String,
    pub version: u32,
    pub steward_party: String,
    pub title: String,
    pub charter_markdown: OpaqueMarkdown,
    /// Digest of the complete versioned scope descriptor, including policy not stored here.
    pub descriptor_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StewardScopeView {
    pub scope_id: String,
    pub version: u32,
    pub steward_party: String,
    pub title: String,
    pub charter_markdown: OpaqueMarkdown,
    pub charter_sha256: String,
    pub descriptor_sha256: String,
    pub recorded_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewFrozenSource {
    pub source_id: String,
    pub kind: String,
    pub locator: String,
    pub origin_path: Option<String>,
    pub revision: Option<String>,
    pub content: Vec<u8>,
    pub observed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrozenSourceView {
    pub ordinal: u32,
    pub source_id: String,
    pub kind: String,
    pub locator: String,
    pub origin_path: Option<String>,
    pub revision: Option<String>,
    pub content: Vec<u8>,
    pub content_sha256: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewFrozenBasis {
    pub kind: BasisKind,
    pub label: String,
    pub scope_id: Option<String>,
    pub scope_version: Option<u32>,
    pub verifier_version: String,
    pub observed_at: i64,
    pub sources: Vec<NewFrozenSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrozenBasisView {
    pub basis_id: String,
    pub kind: BasisKind,
    pub label: String,
    pub scope_id: Option<String>,
    pub scope_version: Option<u32>,
    pub verifier_version: String,
    pub manifest_sha256: String,
    pub observed_at: i64,
    pub recorded_at: i64,
    pub sources: Vec<FrozenSourceView>,
    pub freshness: BasisFreshness,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BasisGuard {
    pub basis_id: String,
    pub observed_manifest_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BasisVerificationView {
    pub verification_id: String,
    pub basis_id: String,
    pub outcome: BasisFreshness,
    pub observed_manifest_sha256: Option<String>,
    pub detail_markdown: Option<OpaqueMarkdown>,
    pub checked_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrationView {
    pub integration_id: String,
    pub entrant_party: String,
    pub title: String,
    pub context_markdown: Option<OpaqueMarkdown>,
    pub context_sha256: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackView {
    pub track_id: String,
    pub integration_id: String,
    pub scope_id: String,
    pub scope_version: u32,
    pub steward_party: String,
    pub active: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterView {
    pub integration_id: String,
    pub revision: u32,
    pub digest: String,
    pub tracks: Vec<TrackView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfferView {
    pub offer_id: String,
    pub negotiation_id: String,
    pub author_role: PartyRole,
    pub terms_markdown: OpaqueMarkdown,
    pub terms_sha256: String,
    pub basis_id: Option<String>,
    pub recorded_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgreementView {
    pub agreement_id: String,
    pub negotiation_id: String,
    pub track_id: String,
    pub entrant_party: String,
    pub steward_party: String,
    pub offer: OfferView,
    pub basis_id: String,
    pub basis_freshness: BasisFreshness,
    pub entrant_assent_event_ordinal: u32,
    pub steward_assent_event_ordinal: u32,
    pub predecessor_agreement_id: Option<String>,
    pub successor_agreement_id: Option<String>,
    pub sealed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartyAssentView {
    pub role: PartyRole,
    pub party: String,
    pub status: AssentStatus,
    pub offer_id: Option<String>,
    pub basis_id: Option<String>,
    pub event_ordinal: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NegotiationView {
    pub negotiation_id: String,
    pub track_id: String,
    pub kind: NegotiationKind,
    pub predecessor_agreement_id: Option<String>,
    pub status: NegotiationStatus,
    pub entrant: PartyAssentView,
    pub steward: PartyAssentView,
    pub head: Option<OfferView>,
    pub agreement: Option<AgreementView>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NegotiationEventView {
    pub event_id: i64,
    pub negotiation_id: String,
    pub ordinal: u32,
    pub kind: NegotiationEventKind,
    pub party_role: Option<PartyRole>,
    pub offer_id: Option<String>,
    pub basis_id: Option<String>,
    pub review_markdown: Option<OpaqueMarkdown>,
    pub reason: Option<String>,
    pub attempt_id: Option<String>,
    pub recorded_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum StewardResponse {
    Assent {
        review_markdown: OpaqueMarkdown,
    },
    Counterproposal {
        terms_markdown: OpaqueMarkdown,
        review_markdown: OpaqueMarkdown,
    },
    Blocked {
        review_markdown: OpaqueMarkdown,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationResult {
    pub negotiation: NegotiationView,
    pub offer_id: Option<String>,
    pub agreement_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttemptSourceInput {
    pub source_id: String,
    pub kind: String,
    pub locator: String,
    pub origin_path: String,
    pub revision: Option<String>,
    pub content: Vec<u8>,
    pub content_sha256: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewAgentAttempt {
    pub predecessor_attempt_id: Option<String>,
    pub kind: AttemptKind,
    pub subject_id: String,
    pub requester_id: String,
    pub nucleus_job_id: String,
    pub request_bytes: Vec<u8>,
    pub request_sha256: String,
    pub toolset_name: String,
    pub toolset_version: u32,
    pub expected_offer_id: Option<String>,
    pub expected_roster_digest: Option<String>,
    pub basis_id: Option<String>,
    pub basis_digest: String,
    pub catalog_scope: String,
    pub catalog_version: u32,
    pub catalog_verifier_version: String,
    pub catalog_observed_at: i64,
    pub catalog_party: String,
    pub catalog_title: String,
    pub catalog_charter_markdown: OpaqueMarkdown,
    pub catalog_charter_sha256: String,
    pub catalog_sha256: String,
    pub sources: Vec<AttemptSourceInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentAttemptView {
    pub attempt_id: String,
    pub predecessor_attempt_id: Option<String>,
    pub kind: AttemptKind,
    pub subject_id: String,
    pub requester_id: String,
    pub nucleus_job_id: String,
    pub request_bytes: Vec<u8>,
    pub request_sha256: String,
    pub toolset_name: String,
    pub toolset_version: u32,
    pub expected_offer_id: Option<String>,
    pub expected_roster_digest: Option<String>,
    pub basis_id: Option<String>,
    pub basis_digest: String,
    pub catalog_scope: String,
    pub catalog_version: u32,
    pub catalog_verifier_version: String,
    pub catalog_observed_at: i64,
    pub catalog_party: String,
    pub catalog_title: String,
    pub catalog_charter_markdown: OpaqueMarkdown,
    pub catalog_charter_sha256: String,
    pub catalog_sha256: String,
    pub sources: Vec<AttemptSourceInput>,
    pub tool_after: u64,
    pub admitted: bool,
    pub accepted_job_id: Option<String>,
    pub accepted_request_sha256: Option<String>,
    pub active: bool,
    pub runtime_state: RuntimeState,
    pub runtime_detail: Option<String>,
    pub domain_result_kind: Option<String>,
    pub domain_result_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolReceiptView {
    pub receipt_id: String,
    pub attempt_id: String,
    pub nucleus_job_id: String,
    pub call_id: String,
    pub arguments_sha256: String,
    pub result_json: Vec<u8>,
    pub is_error: bool,
    pub domain_result_kind: Option<String>,
    pub domain_result_id: Option<String>,
    pub emitted_source_refs: Vec<String>,
    pub recorded_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompositionAgreementRef {
    pub ordinal: u32,
    pub track_id: String,
    pub agreement_id: String,
    pub terms_sha256: String,
    pub basis_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompositionReviewView {
    pub review_id: String,
    pub integration_id: String,
    pub roster_revision: u32,
    pub roster_digest: String,
    pub outcome: CompositionOutcome,
    pub review_markdown: OpaqueMarkdown,
    pub review_sha256: String,
    pub attempt_id: Option<String>,
    pub agreements: Vec<CompositionAgreementRef>,
    pub stale: bool,
    pub recorded_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConformanceReviewView {
    pub review_id: String,
    pub agreement_id: String,
    pub candidate_basis_id: String,
    pub outcome: ConformanceOutcome,
    pub review_markdown: OpaqueMarkdown,
    pub review_sha256: String,
    pub attempt_id: Option<String>,
    pub candidate_freshness: BasisFreshness,
    pub recorded_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackStatusView {
    pub track: TrackView,
    pub negotiation: Option<NegotiationView>,
    pub active_agreement: Option<AgreementView>,
    pub renegotiating: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrationStatusView {
    pub integration: IntegrationView,
    pub roster: RosterView,
    pub tracks: Vec<TrackStatusView>,
    pub latest_composition_review: Option<CompositionReviewView>,
    pub ready: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_markdown_preserves_exact_bytes() {
        let bytes = b"No headings\r\nUnicode: \xF0\x9F\xA6\x80  \n".to_vec();
        let markdown = OpaqueMarkdown::new(bytes.clone()).expect("valid markdown");
        assert_eq!(markdown.as_bytes(), bytes);
        assert_eq!(markdown.sha256(), sha256_hex(&bytes));

        let encoded = serde_json::to_vec(&markdown).expect("serialize");
        let decoded: OpaqueMarkdown = serde_json::from_slice(&encoded).expect("deserialize");
        assert_eq!(decoded.as_bytes(), bytes);
    }

    #[test]
    fn opaque_markdown_rejects_only_mechanical_invalidity() {
        assert_eq!(OpaqueMarkdown::new(Vec::new()), Err(MarkdownError::Empty));
        assert_eq!(
            OpaqueMarkdown::new(vec![0xff]),
            Err(MarkdownError::InvalidUtf8)
        );
        assert_eq!(
            OpaqueMarkdown::new(vec![b'x'; MAX_MARKDOWN_BYTES + 1]),
            Err(MarkdownError::TooLarge)
        );
    }
}
