//! Frozen Decisions version-one lifecycle envelopes and read-only CLI access.
//! No new event publishing, cursor manufacturing, or consumer acknowledgement.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::PathBuf;
use std::process::Command;

pub const STREAM: &str = "decisions.lifecycle";
pub const ENVELOPE_VERSION: i64 = 1;
pub const MAX_PAGE_SIZE: u16 = 1_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEventWatermark {
    pub stream: String,
    pub envelope_version: i64,
    pub cursor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEventPage<T = DecisionEventEnvelope> {
    pub stream: String,
    pub envelope_version: i64,
    pub after_cursor: String,
    pub next_cursor: String,
    pub watermark_cursor: String,
    pub has_more: bool,
    pub events: Vec<DecisionEventItem<T>>,
}

/// The provider preserves retained JSON as-is; readers use the typed default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEventItem<T = DecisionEventEnvelope> {
    pub cursor: String,
    pub event: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEventSource {
    pub source_role: String,
    pub host_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub message_role: String,
    pub occurred_at: i64,
    pub timestamp_precision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEventEnvelope {
    pub event_id: String,
    pub event_version: i64,
    pub event_kind: String,
    pub occurred_at: i64,
    pub decision: DecisionEventDecision,
    pub review: Option<DecisionEventReview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEventDecision {
    pub decision_id: String,
    pub decided_at: i64,
    pub timestamp_precision: String,
    pub statement: String,
    pub disposition: String,
    pub confidence: String,
    pub rationale: Option<String>,
    pub supersedes_decision_id: Option<String>,
    pub review_state: String,
    pub authority_span: DecisionEventAuthoritySpan,
    pub sources: Vec<DecisionEventSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEventAuthoritySpan {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEventReview {
    pub review_id: String,
    pub action: String,
    pub reviewed_at: i64,
    pub review_source: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unable to run the lifecycle command")]
    Io(#[from] std::io::Error),
    #[error("Decisions event command exited with {status}: {detail}")]
    Failed {
        status: std::process::ExitStatus,
        detail: String,
    },
    #[error("invalid lifecycle response JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{message}")]
    Invalid { code: &'static str, message: String },
}

#[derive(Debug, Clone)]
pub struct Client {
    binary: PathBuf,
    database: Option<PathBuf>,
}

impl Client {
    #[must_use]
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            database: None,
        }
    }

    #[must_use]
    pub fn with_database(mut self, database: impl Into<PathBuf>) -> Self {
        self.database = Some(database.into());
        self
    }

    fn json<T: DeserializeOwned>(&self, arguments: &[&str]) -> Result<T, Error> {
        let mut command = Command::new(&self.binary);
        if let Some(database) = &self.database {
            command.arg("--database").arg(database);
        }
        let output = command.args(arguments).output()?;
        if !output.status.success() {
            return Err(Error::Failed {
                status: output.status,
                detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    /// Read the end watermark of retained legacy events.
    ///
    /// # Errors
    /// Returns an error for an unavailable command, malformed result, or incompatible stream.
    pub fn watermark(&self) -> Result<DecisionEventWatermark, Error> {
        let response: DecisionEventWatermark = self.json(&["events", "watermark", "--json"])?;
        validate_stream(&response.stream, response.envelope_version)?;
        Ok(response)
    }

    /// Resume strictly after a caller-owned opaque legacy cursor.
    ///
    /// # Errors
    /// Returns an error for invalid bounds, failed transport, or incompatible results.
    pub fn read_after(&self, cursor: &str, limit: u16) -> Result<DecisionEventPage, Error> {
        if !(1..=MAX_PAGE_SIZE).contains(&limit) {
            return Err(Error::Invalid {
                code: "event_limit_invalid",
                message: "Decisions event limit must be between 1 and 1000".to_owned(),
            });
        }
        let response: DecisionEventPage = self.json(&[
            "events",
            "read",
            "--after",
            cursor,
            "--limit",
            &limit.to_string(),
            "--json",
        ])?;
        validate_stream(&response.stream, response.envelope_version)?;
        for item in &response.events {
            if item.event.event_version != ENVELOPE_VERSION {
                return Err(Error::Invalid {
                    code: "decision_event_incompatible",
                    message: format!(
                        "unsupported decision event version {}",
                        item.event.event_version
                    ),
                });
            }
        }
        Ok(response)
    }
}

fn validate_stream(stream: &str, version: i64) -> Result<(), Error> {
    if stream != STREAM || version != ENVELOPE_VERSION {
        return Err(Error::Invalid {
            code: "decisions_stream_incompatible",
            message: format!("unsupported Decisions stream {stream:?} envelope version {version}"),
        });
    }
    Ok(())
}
