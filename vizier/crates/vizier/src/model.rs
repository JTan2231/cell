use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest as _, Sha256};

use crate::error::{AppError, AppResult};

pub const MAX_MARKDOWN_BYTES: usize = 1024 * 1024;
pub const MAX_INPUT_BUNDLE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TOOL_RESULT_BYTES: usize = 1024 * 1024;
pub const MAX_PACKETS: usize = 64;
pub const MAX_CONTRACT_UNITS: usize = 64;
pub const MAX_GATE_OUTPUT_BYTES: usize = 1024 * 1024;

/// Exact, nonempty UTF-8 Markdown. It is never normalized or structurally parsed.
#[derive(Clone, PartialEq, Eq)]
pub struct OpaqueMarkdown(String);

impl OpaqueMarkdown {
    pub fn new(bytes: Vec<u8>) -> AppResult<Self> {
        if bytes.is_empty() {
            return Err(AppError::new(
                "markdown_empty",
                "Markdown must not be empty",
            ));
        }
        if bytes.len() > MAX_MARKDOWN_BYTES {
            return Err(AppError::new(
                "markdown_too_large",
                format!("Markdown exceeds the {MAX_MARKDOWN_BYTES}-byte limit"),
            ));
        }
        String::from_utf8(bytes)
            .map(Self)
            .map_err(|_| AppError::new("markdown_invalid_utf8", "Markdown must be valid UTF-8"))
    }

    pub fn from_text(text: impl Into<String>) -> AppResult<Self> {
        Self::new(text.into().into_bytes())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    #[must_use]
    pub fn sha256(&self) -> String {
        sha256_hex(self.as_bytes())
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
        let value = String::deserialize(deserializer)?;
        Self::new(value.into_bytes()).map_err(de::Error::custom)
    }
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            #[must_use]
            pub fn parse(value: &str) -> Option<Self> {
                match value { $($text => Some(Self::$variant)),+, _ => None }
            }
        }
    };
}

string_enum!(RunState {
    Queued => "queued",
    Planning => "planning",
    Assembling => "assembling",
    PlanReview => "plan_review",
    Implementing => "implementing",
    PacketReview => "packet_review",
    Integrating => "integrating",
    Gates => "gates",
    FinalReview => "final_review",
    Succeeded => "succeeded",
    NeedsAttention => "needs_attention",
    Cancelled => "cancelled",
});

impl RunState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::NeedsAttention | Self::Cancelled
        )
    }
}

string_enum!(Role {
    Planner => "planner",
    Assembler => "assembler",
    PlanReviewer => "plan_reviewer",
    Implementor => "implementor",
    PacketReviewer => "packet_reviewer",
    Integrator => "integrator",
    IntegratedReviewer => "integrated_reviewer",
});

impl Role {
    #[must_use]
    pub const fn is_writer(self) -> bool {
        matches!(self, Self::Implementor | Self::Integrator)
    }
}

string_enum!(AttemptState {
    Prepared => "prepared",
    Admitted => "admitted",
    Running => "running",
    Completed => "completed",
    Failed => "failed",
    Cancelled => "cancelled",
    TimedOut => "timed_out",
    Lost => "lost",
});

impl AttemptState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::TimedOut | Self::Lost
        )
    }
}

string_enum!(Disposition {
    Accepted => "accepted",
    ChangesRequested => "changes_requested",
    Blocked => "blocked",
});

string_enum!(RecoveryCause {
    PlanReviewExhausted => "plan_review_exhausted",
    PacketReviewExhausted => "packet_review_exhausted",
    GateFailureExhausted => "gate_failure_exhausted",
    IntegratedReviewExhausted => "integrated_review_exhausted",
    Blocked => "blocked",
    OperationalError => "operational_error",
    AmbiguousEvidence => "ambiguous_evidence",
    Cancelled => "cancelled",
    UnsafeGit => "unsafe_git",
    MissingAuthorityOrEvidence => "missing_authority_or_evidence",
    ActiveOrResultlessAttempt => "active_or_resultless_attempt",
    MixedFrontier => "mixed_frontier",
});

string_enum!(RecoveryFrontier {
    AssembledPlan => "assembled_plan",
    Packets => "packets",
    IntegratedCandidate => "integrated_candidate",
});

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecoveryEnvelope {
    pub version: u32,
    pub run_id: String,
    pub checkpoint_id: String,
    pub continuable: bool,
    pub cause: RecoveryCause,
    pub frontier: Option<RecoveryFrontier>,
    pub responsible_role: Option<Role>,
    pub subject_id: Option<String>,
    #[serde(default)]
    pub failed_packet_keys: Vec<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub permitted_scopes: Vec<PathScope>,
    #[serde(default)]
    pub invalidated_checks: Vec<String>,
    pub candidate_id: Option<String>,
    pub reviewed_candidate_id: Option<String>,
    pub predecessor_candidate_id: Option<String>,
    pub review_attempt_id: Option<String>,
    pub gate_result_ids: Vec<String>,
    pub canonical_basis_digest: String,
}

impl RecoveryEnvelope {
    pub fn validate(&self) -> AppResult<()> {
        let supported = matches!(
            self.cause,
            RecoveryCause::PlanReviewExhausted
                | RecoveryCause::PacketReviewExhausted
                | RecoveryCause::GateFailureExhausted
                | RecoveryCause::IntegratedReviewExhausted
        );
        if self.version != 1
            || self.checkpoint_id.is_empty()
            || self.canonical_basis_digest.len() != 64
            || !self
                .canonical_basis_digest
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(AppError::new(
                "recovery_envelope_invalid",
                "recovery envelope is not a complete versioned mechanical checkpoint",
            ));
        }
        if self.continuable != supported {
            return Err(AppError::new(
                "recovery_envelope_invalid",
                "continuability must exactly match a supported exhausted frontier",
            ));
        }
        if self.continuable
            && (self.frontier.is_none()
                || self.responsible_role.is_none()
                || self.subject_id.as_deref().unwrap_or_default().is_empty()
                || self.evidence_ids.is_empty())
        {
            return Err(AppError::new(
                "recovery_envelope_incomplete",
                "a continuable recovery envelope needs exact frontier, responsibility, subject, and evidence",
            ));
        }
        if self.cause == RecoveryCause::PacketReviewExhausted && self.failed_packet_keys.is_empty()
        {
            return Err(AppError::new(
                "recovery_envelope_incomplete",
                "packet recovery requires exact failed packets",
            ));
        }
        if self.continuable {
            let expected = match self.cause {
                RecoveryCause::PlanReviewExhausted => {
                    (RecoveryFrontier::AssembledPlan, Role::Assembler)
                }
                RecoveryCause::PacketReviewExhausted => {
                    (RecoveryFrontier::Packets, Role::Implementor)
                }
                RecoveryCause::GateFailureExhausted | RecoveryCause::IntegratedReviewExhausted => {
                    (RecoveryFrontier::IntegratedCandidate, Role::Integrator)
                }
                _ => unreachable!(),
            };
            if self.frontier != Some(expected.0) || self.responsible_role != Some(expected.1) {
                return Err(AppError::new(
                    "recovery_envelope_incomplete",
                    "recovery cause has an invalid frontier or responsible role",
                ));
            }
            if matches!(
                self.cause,
                RecoveryCause::PacketReviewExhausted
                    | RecoveryCause::GateFailureExhausted
                    | RecoveryCause::IntegratedReviewExhausted
            ) && self.candidate_id.is_none()
            {
                return Err(AppError::new(
                    "recovery_envelope_incomplete",
                    "candidate frontier recovery requires its exact candidate",
                ));
            }
            if matches!(
                self.cause,
                RecoveryCause::PlanReviewExhausted
                    | RecoveryCause::PacketReviewExhausted
                    | RecoveryCause::IntegratedReviewExhausted
            ) && self.review_attempt_id.is_none()
            {
                return Err(AppError::new(
                    "recovery_envelope_incomplete",
                    "review recovery requires its exact review attempt",
                ));
            }
            if self.cause == RecoveryCause::GateFailureExhausted && self.gate_result_ids.is_empty()
            {
                return Err(AppError::new(
                    "recovery_envelope_incomplete",
                    "gate recovery requires complete exact gate evidence",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContinuationRequest {
    pub parent_run_id: String,
    pub request_key: String,
    pub remediation_rounds: u32,
}

string_enum!(PacketState {
    Planned => "planned",
    Implementing => "implementing",
    Reviewing => "reviewing",
    Accepted => "accepted",
    Blocked => "blocked",
});

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunView {
    pub id: String,
    pub repository: String,
    pub source_commit: String,
    pub state: RunState,
    pub contract_set_sha256: String,
    pub input_bundle_sha256: String,
    pub remediation_limit: u32,
    pub parent_run_id: Option<String>,
    pub recovery_checkpoint_id: Option<String>,
    pub final_candidate_id: Option<String>,
    pub final_ref: Option<String>,
    pub cancel_requested: bool,
    pub detail: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentView {
    pub id: String,
    pub run_id: String,
    pub kind: String,
    pub subject_id: Option<String>,
    pub ordinal: u32,
    pub markdown: OpaqueMarkdown,
    pub sha256: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct PathScope {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PacketView {
    pub run_id: String,
    pub key: String,
    pub ordinal: u32,
    pub state: PacketState,
    pub contract_unit_ids: Vec<String>,
    pub depends_on: Vec<String>,
    pub path_scopes: Vec<PathScope>,
    pub plan_document_id: String,
    pub current_candidate_id: Option<String>,
    pub remediation_round: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttemptView {
    pub id: String,
    pub run_id: String,
    pub role: Role,
    pub subject_id: String,
    pub round: u32,
    pub targeted: bool,
    pub state: AttemptState,
    pub nucleus_job_id: String,
    pub request_bytes: Vec<u8>,
    pub request_sha256: String,
    pub toolset_name: String,
    pub workspace_path: String,
    pub base_commit: Option<String>,
    pub allowed_scopes: Vec<PathScope>,
    pub admitted: bool,
    pub tool_after: u64,
    pub domain_document_id: Option<String>,
    pub disposition: Option<Disposition>,
    pub predecessor_attempt_id: Option<String>,
    pub detail: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandidateView {
    pub id: String,
    pub run_id: String,
    pub subject_id: String,
    pub kind: String,
    pub round: u32,
    pub base_commit: String,
    pub commit_oid: String,
    pub ref_name: String,
    pub handoff_document_id: String,
    pub attempt_id: String,
    pub predecessor_candidate_id: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateSpec {
    pub id: String,
    pub run_id: String,
    pub ordinal: u32,
    pub name: String,
    pub command: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateResult {
    pub id: String,
    pub gate_id: String,
    pub candidate_id: String,
    pub round: u32,
    pub exit_code: i32,
    pub output: String,
    pub output_truncated: bool,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewRun {
    pub id: String,
    pub request_key: Option<String>,
    pub repository: String,
    pub source_commit: String,
    pub brief: OpaqueMarkdown,
    pub terminology: OpaqueMarkdown,
    pub contracts: Vec<(String, OpaqueMarkdown)>,
    pub gates: Vec<(String, String)>,
    pub remediation_limit: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PacketSubmission {
    pub packet_key: String,
    pub contract_unit_ids: Vec<String>,
    pub depends_on: Vec<String>,
    pub path_scopes: Vec<PathScope>,
    pub plan_markdown: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationSubmission {
    pub overview_markdown: String,
    pub packets: Vec<PacketSubmission>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewSubmission {
    pub disposition: Disposition,
    #[serde(default)]
    pub affected_packet_keys: Vec<String>,
    #[serde(default)]
    pub contract_unit_ids: Vec<String>,
    pub markdown: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewScopeView {
    pub review_attempt_id: String,
    pub review_document_id: String,
    pub affected_packet_keys: Vec<String>,
    pub contract_unit_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HandoffSubmission {
    pub outcome: HandoffOutcome,
    pub markdown: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffOutcome {
    Ready,
    Blocked,
}

#[cfg(test)]
mod tests {
    use super::OpaqueMarkdown;

    #[test]
    fn opaque_markdown_preserves_exact_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let text = "# Heading\r\n\r\nTrailing spaces.  \r\nλ\n";
        let markdown = OpaqueMarkdown::new(text.as_bytes().to_vec())?;
        assert_eq!(markdown.as_bytes(), text.as_bytes());
        assert_eq!(markdown.sha256().len(), 64);
        Ok(())
    }
}
