use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use conversations::{AppServerClient, ClientConfig, StderrPolicy, ThreadRef};
use serde::Deserialize;

use crate::domain::{DecisionAnchor, DecisionEvent};
use crate::{Error, Result};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DecisionEventPage {
    pub after_cursor: String,
    pub next_cursor: String,
    pub watermark_cursor: String,
    pub has_more: bool,
    pub events: Vec<DecisionEvent>,
}

pub trait DecisionEventSource {
    fn watermark(&mut self) -> Result<String>;
    fn read_after(&mut self, cursor: &str, limit: u16) -> Result<DecisionEventPage>;
}

pub trait ConversationLocator {
    fn exact_cwd(&mut self, anchor: &DecisionAnchor) -> Result<Option<PathBuf>>;
}

#[derive(Debug, Clone)]
pub struct DecisionsCli {
    binary: PathBuf,
}

impl DecisionsCli {
    #[must_use]
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    #[must_use]
    pub fn for_current_user() -> Self {
        Self::new(
            std::env::var_os("SEMANTICS_DECISIONS")
                .map_or_else(|| PathBuf::from("decisions"), PathBuf::from),
        )
    }

    fn json(&self, arguments: &[OsString]) -> Result<Vec<u8>> {
        let output = Command::new(&self.binary)
            .args(arguments)
            .output()
            .map_err(|source| crate::error::io(&self.binary, source))?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            return Err(Error::domain(
                "decisions_events_failed",
                format!(
                    "Decisions event command exited with {}: {}",
                    output.status,
                    detail.trim()
                ),
            ));
        }
        Ok(output.stdout)
    }
}

impl Default for DecisionsCli {
    fn default() -> Self {
        Self::for_current_user()
    }
}

impl DecisionEventSource for DecisionsCli {
    fn watermark(&mut self) -> Result<String> {
        let output = self.json(&[
            OsString::from("events"),
            OsString::from("watermark"),
            OsString::from("--json"),
        ])?;
        let response: WatermarkResponse = serde_json::from_slice(&output)?;
        validate_stream(&response.stream, response.envelope_version)?;
        Ok(response.cursor)
    }

    fn read_after(&mut self, cursor: &str, limit: u16) -> Result<DecisionEventPage> {
        if !(1..=1_000).contains(&limit) {
            return Err(Error::domain(
                "event_limit_invalid",
                "Decisions event limit must be between 1 and 1000",
            ));
        }
        let output = self.json(&[
            OsString::from("events"),
            OsString::from("read"),
            OsString::from("--after"),
            OsString::from(cursor),
            OsString::from("--limit"),
            OsString::from(limit.to_string()),
            OsString::from("--json"),
        ])?;
        let response: EventPageResponse = serde_json::from_slice(&output)?;
        validate_stream(&response.stream, response.envelope_version)?;
        let events = response
            .events
            .into_iter()
            .map(EventItem::normalize)
            .collect::<Result<Vec<_>>>()?;
        Ok(DecisionEventPage {
            after_cursor: response.after_cursor,
            next_cursor: response.next_cursor,
            watermark_cursor: response.watermark_cursor,
            has_more: response.has_more,
            events,
        })
    }
}

pub struct AppServerConversationLocator {
    client: Option<AppServerClient>,
}

impl AppServerConversationLocator {
    pub fn for_current_user() -> Result<Self> {
        Ok(Self { client: None })
    }

    fn client(&mut self) -> Result<&mut AppServerClient> {
        if self.client.is_none() {
            self.client = Some(AppServerClient::spawn(ClientConfig {
                stderr_policy: StderrPolicy::Suppress,
                ..ClientConfig::default()
            })?);
        }
        self.client.as_mut().ok_or_else(|| {
            Error::domain(
                "conversations_client_missing",
                "Conversations client was not retained after startup",
            )
        })
    }
}

impl ConversationLocator for AppServerConversationLocator {
    fn exact_cwd(&mut self, anchor: &DecisionAnchor) -> Result<Option<PathBuf>> {
        let summary = self.client()?.read_thread_summary(&ThreadRef {
            host_id: anchor.host_id.clone(),
            thread_id: anchor.thread_id.clone(),
        })?;
        let Some(cwd) = summary.cwd else {
            return Ok(None);
        };
        let path = PathBuf::from(cwd);
        if !path.is_absolute() {
            return Err(Error::domain(
                "conversation_cwd_relative",
                format!(
                    "Conversations returned a relative cwd for thread {}",
                    anchor.thread_id
                ),
            ));
        }
        Ok(Some(path))
    }
}

pub fn canonical_directory(path: &Path) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .map_err(|source| crate::error::io(path, source))?;
    let metadata = std::fs::symlink_metadata(&canonical)
        .map_err(|source| crate::error::io(&canonical, source))?;
    if !metadata.is_dir() {
        return Err(Error::domain(
            "project_root_not_directory",
            format!("project root is not a directory: {}", path.display()),
        ));
    }
    Ok(canonical)
}

pub fn require_participation_marker(root: &Path, project_id: &str) -> Result<()> {
    let path = root.join("AGENTS.md");
    let metadata =
        std::fs::symlink_metadata(&path).map_err(|source| crate::error::io(&path, source))?;
    if !metadata.file_type().is_file() {
        return Err(Error::domain(
            "participation_marker_missing",
            format!("{} must be a regular file", path.display()),
        ));
    }
    let contents =
        std::fs::read_to_string(&path).map_err(|source| crate::error::io(&path, source))?;
    let marker = crate::domain::marker_for(project_id);
    if !contents.lines().any(|line| line == marker) {
        return Err(Error::domain(
            "participation_marker_missing",
            format!("exact-root AGENTS.md must contain the exact line {marker:?}"),
        ));
    }
    Ok(())
}

fn validate_stream(stream: &str, version: u32) -> Result<()> {
    if stream != "decisions.lifecycle" || version != 1 {
        return Err(Error::domain(
            "decisions_stream_incompatible",
            format!("unsupported Decisions stream {stream:?} envelope version {version}"),
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct WatermarkResponse {
    stream: String,
    envelope_version: u32,
    cursor: String,
}

#[derive(Debug, Deserialize)]
struct EventPageResponse {
    stream: String,
    envelope_version: u32,
    after_cursor: String,
    next_cursor: String,
    watermark_cursor: String,
    has_more: bool,
    events: Vec<EventItem>,
}

#[derive(Debug, Deserialize)]
struct EventItem {
    cursor: String,
    event: EventEnvelope,
}

impl EventItem {
    fn normalize(self) -> Result<DecisionEvent> {
        if self.event.event_version != 1 {
            return Err(Error::domain(
                "decision_event_incompatible",
                format!(
                    "unsupported decision event version {}",
                    self.event.event_version
                ),
            ));
        }
        let anchors = self
            .event
            .decision
            .sources
            .into_iter()
            .map(|source| DecisionAnchor {
                source_role: source.source_role,
                host_id: source.host_id,
                thread_id: source.thread_id,
                turn_id: source.turn_id,
                item_id: source.item_id,
                message_role: source.message_role,
                occurred_at: source.occurred_at,
                timestamp_precision: source.timestamp_precision,
            })
            .collect();
        let (review_id, review_action, reviewed_at, review_source) = match self.event.review {
            Some(review) => (
                Some(review.review_id),
                Some(review.action),
                Some(review.reviewed_at),
                Some(review.review_source),
            ),
            None => (None, None, None, None),
        };
        Ok(DecisionEvent {
            event_id: self.event.event_id,
            event_version: self.event.event_version,
            cursor: self.cursor,
            event_kind: self.event.event_kind,
            occurred_at: self.event.occurred_at,
            decision_id: self.event.decision.decision_id,
            decided_at: self.event.decision.decided_at,
            timestamp_precision: self.event.decision.timestamp_precision,
            statement: self.event.decision.statement,
            disposition: self.event.decision.disposition,
            confidence: self.event.decision.confidence,
            rationale: self.event.decision.rationale,
            supersedes_decision_id: self.event.decision.supersedes_decision_id,
            authority_start: self.event.decision.authority_span.start,
            authority_end: self.event.decision.authority_span.end,
            review_state: self.event.decision.review_state,
            review_id,
            review_action,
            reviewed_at,
            review_source,
            anchors,
        })
    }
}

#[derive(Debug, Deserialize)]
struct EventEnvelope {
    event_id: String,
    event_version: u32,
    event_kind: String,
    occurred_at: i64,
    decision: EventDecision,
    review: Option<EventReview>,
}

#[derive(Debug, Deserialize)]
struct EventDecision {
    decision_id: String,
    decided_at: i64,
    timestamp_precision: String,
    statement: String,
    disposition: String,
    confidence: String,
    rationale: Option<String>,
    supersedes_decision_id: Option<String>,
    review_state: String,
    authority_span: EventAuthoritySpan,
    sources: Vec<EventSource>,
}

#[derive(Debug, Deserialize)]
struct EventAuthoritySpan {
    start: i64,
    end: i64,
}

#[derive(Debug, Deserialize)]
struct EventSource {
    source_role: String,
    host_id: String,
    thread_id: String,
    turn_id: String,
    item_id: String,
    message_role: String,
    occurred_at: i64,
    timestamp_precision: String,
}

#[derive(Debug, Deserialize)]
struct EventReview {
    review_id: String,
    action: String,
    reviewed_at: i64,
    review_source: String,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::require_participation_marker;

    #[test]
    fn marker_must_be_an_exact_line_in_exact_root() {
        let temporary = TempDir::new().expect("temporary directory");
        fs::write(
            temporary.path().join("AGENTS.md"),
            "# Agent instructions\nSemantics-Project: cell\n",
        )
        .expect("marker fixture");
        require_participation_marker(temporary.path(), "cell").expect("exact marker");
        assert!(require_participation_marker(temporary.path(), "other").is_err());
    }
}
