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
