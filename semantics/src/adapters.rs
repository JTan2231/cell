use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use conversations::{AppServerClient, ClientConfig, StderrPolicy, ThreadRef};
use serde::Deserialize;

use crate::domain::{
    DecisionAccountAnchor, DecisionAccountEvent, DecisionAnchor, DecisionEvent,
    validate_annals_library_id,
};
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

pub trait AccountConversationLocator {
    fn exact_account_cwd(&mut self, anchor: &DecisionAccountAnchor) -> Result<Option<PathBuf>>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DecisionAccountPage {
    pub library_id: String,
    pub request_cursor: String,
    pub next_cursor: String,
    pub watermark: String,
    pub events: Vec<DecisionAccountEvent>,
}

pub trait DecisionAccountSource {
    fn watermark(&mut self) -> Result<(String, String)>;
    fn read_page(
        &mut self,
        cursor: &str,
        watermark: &str,
        limit: u16,
    ) -> Result<DecisionAccountPage>;
}

#[derive(Debug, Clone)]
pub struct AnnalsDecisionFeedCli {
    binary: PathBuf,
    config: PathBuf,
    expected_library_id: Option<String>,
}

impl AnnalsDecisionFeedCli {
    #[must_use]
    pub fn new(
        binary: impl Into<PathBuf>,
        config: impl Into<PathBuf>,
        expected_library_id: Option<impl Into<String>>,
    ) -> Self {
        Self {
            binary: binary.into(),
            config: config.into(),
            expected_library_id: expected_library_id.map(Into::into),
        }
    }

    pub fn for_current_user(expected_library_id: Option<&str>) -> Result<Self> {
        let config = match std::env::var_os("SEMANTICS_ANNALS_CONFIG") {
            Some(value) => PathBuf::from(value),
            None => {
                let home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .ok_or_else(|| {
                        Error::domain(
                            "annals_config_unavailable",
                            "HOME or SEMANTICS_ANNALS_CONFIG must identify the decisions-library config",
                        )
                    })?;
                home.join("Library/Application Support/Annals/decisions/config.toml")
            }
        };
        if !config.is_absolute() {
            return Err(Error::domain(
                "annals_config_unavailable",
                "SEMANTICS_ANNALS_CONFIG must be absolute",
            ));
        }
        Ok(Self::new(
            std::env::var_os("SEMANTICS_ANNALS")
                .map_or_else(|| PathBuf::from("annals"), PathBuf::from),
            config,
            expected_library_id,
        ))
    }

    fn json(&self, arguments: &[OsString]) -> Result<Vec<u8>> {
        let output = Command::new(&self.binary)
            .arg("--config")
            .arg(&self.config)
            .arg("--json")
            .args(arguments)
            .output()
            .map_err(|_| {
                Error::domain(
                    "annals_feed_unavailable",
                    "unable to run the configured Annals decision-feed command",
                )
            })?;
        if !output.status.success() {
            return Err(Error::domain(
                "annals_feed_failed",
                "Annals decision-feed command did not complete successfully",
            ));
        }
        Ok(output.stdout)
    }

    fn require_library(&self, library_id: &str) -> Result<()> {
        if self
            .expected_library_id
            .as_deref()
            .is_some_and(|expected| library_id != expected)
        {
            return Err(Error::domain(
                "annals_library_mismatch",
                format!(
                    "Annals returned library {library_id:?}, expected {:?}",
                    self.expected_library_id.as_deref().unwrap_or_default()
                ),
            ));
        }
        Ok(())
    }
}

impl DecisionAccountSource for AnnalsDecisionFeedCli {
    fn watermark(&mut self) -> Result<(String, String)> {
        let output = self.json(&[OsString::from("decision-feed"), OsString::from("watermark")])?;
        let response: CliSuccess<AccountWatermarkResponse> = serde_json::from_slice(&output)
            .map_err(|_| {
                Error::domain(
                    "annals_feed_invalid",
                    "Annals returned an invalid decision-feed watermark envelope",
                )
            })?;
        if !response.ok {
            return Err(Error::domain(
                "annals_feed_failed",
                "Annals returned a non-success JSON envelope",
            ));
        }
        let response = response.data;
        if response.contract_version != 1 {
            return Err(Error::domain(
                "annals_feed_incompatible",
                format!(
                    "unsupported Annals decision-feed contract {}",
                    response.contract_version
                ),
            ));
        }
        validate_annals_library_id(&response.library_id)?;
        require_account_text("watermark", &response.watermark, 1_024)?;
        self.require_library(&response.library_id)?;
        Ok((response.library_id, response.watermark))
    }

    fn read_page(
        &mut self,
        cursor: &str,
        watermark: &str,
        limit: u16,
    ) -> Result<DecisionAccountPage> {
        if !(1..=200).contains(&limit) {
            return Err(Error::domain(
                "account_event_limit_invalid",
                "Annals decision-feed limit must be between 1 and 200",
            ));
        }
        if cursor.trim().is_empty() || watermark.trim().is_empty() {
            return Err(Error::domain(
                "annals_cursor_invalid",
                "Annals decision-feed cursor and watermark must not be blank",
            ));
        }
        let arguments = vec![
            OsString::from("decision-feed"),
            OsString::from("page"),
            OsString::from("--watermark"),
            OsString::from(watermark),
            OsString::from("--after"),
            OsString::from(cursor),
            OsString::from("--limit"),
            OsString::from(limit.to_string()),
        ];
        let output = self.json(&arguments)?;
        let response: CliSuccess<AccountPageResponse> =
            serde_json::from_slice(&output).map_err(|_| {
                Error::domain(
                    "annals_feed_invalid",
                    "Annals returned an invalid decision-feed page envelope",
                )
            })?;
        if !response.ok {
            return Err(Error::domain(
                "annals_feed_failed",
                "Annals returned a non-success JSON envelope",
            ));
        }
        let response = response.data;
        if response.contract_version != 1 {
            return Err(Error::domain(
                "annals_feed_incompatible",
                format!(
                    "unsupported Annals decision-feed contract {}",
                    response.contract_version
                ),
            ));
        }
        validate_annals_library_id(&response.library_id)?;
        require_account_text("watermark", &response.watermark, 1_024)?;
        require_account_text("request_cursor", &response.request_cursor, 1_024)?;
        require_account_text("next_cursor", &response.next_cursor, 1_024)?;
        self.require_library(&response.library_id)?;
        if response.watermark != watermark {
            return Err(Error::domain(
                "annals_watermark_mismatch",
                "Annals did not keep the page fixed to the requested watermark",
            ));
        }
        let events = response
            .events
            .into_iter()
            .map(|event| event.normalize(&response.library_id))
            .collect::<Result<Vec<_>>>()?;
        Ok(DecisionAccountPage {
            library_id: response.library_id,
            request_cursor: response.request_cursor,
            next_cursor: response.next_cursor,
            watermark: response.watermark,
            events,
        })
    }
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

impl AccountConversationLocator for AppServerConversationLocator {
    fn exact_account_cwd(&mut self, anchor: &DecisionAccountAnchor) -> Result<Option<PathBuf>> {
        let summary = self.client()?.read_thread_summary(&ThreadRef {
            host_id: anchor.host_id.clone(),
            thread_id: anchor.thread_id.clone(),
        })?;
        bounded_account_cwd(summary.cwd)
    }
}

fn bounded_account_cwd(cwd: Option<String>) -> Result<Option<PathBuf>> {
    let Some(cwd) = cwd else {
        return Ok(None);
    };
    let path = PathBuf::from(cwd);
    if !path.is_absolute() {
        return Err(Error::domain(
            "conversation_cwd_relative",
            "Conversations returned a relative cwd for an account authority thread",
        ));
    }
    Ok(Some(path))
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CliSuccess<T> {
    ok: bool,
    data: T,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountWatermarkResponse {
    contract_version: u32,
    library_id: String,
    watermark: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountPageResponse {
    contract_version: u32,
    library_id: String,
    watermark: String,
    request_cursor: String,
    next_cursor: String,
    events: Vec<AccountEventItem>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountEventItem {
    cursor: String,
    event_id: String,
    account_id: String,
    account_schema_version: u32,
    statement: String,
    context: String,
    action: String,
    result: String,
    occurred_at: i64,
    occurred_at_precision: String,
    authority: AccountAuthority,
}

impl AccountEventItem {
    fn normalize(self, library_id: &str) -> Result<DecisionAccountEvent> {
        if self.account_schema_version != 1 {
            return Err(Error::domain(
                "decision_account_incompatible",
                format!(
                    "unsupported decision account schema {}",
                    self.account_schema_version
                ),
            ));
        }
        if self.authority.span.end <= self.authority.span.start {
            return Err(Error::domain(
                "decision_account_anchor_invalid",
                "decision account authority span ends before it starts",
            ));
        }
        for (field, value, maximum) in [
            ("cursor", self.cursor.as_str(), 1_024),
            ("event_id", self.event_id.as_str(), 1_024),
            ("account_id", self.account_id.as_str(), 1_024),
            ("statement", self.statement.as_str(), 16_384),
            ("context", self.context.as_str(), 16_384),
            ("action", self.action.as_str(), 16_384),
            ("result", self.result.as_str(), 16_384),
            (
                "occurred_at_precision",
                self.occurred_at_precision.as_str(),
                128,
            ),
            ("authority.host_id", self.authority.host_id.as_str(), 1_024),
            (
                "authority.thread_id",
                self.authority.thread_id.as_str(),
                1_024,
            ),
            ("authority.turn_id", self.authority.turn_id.as_str(), 1_024),
            ("authority.item_id", self.authority.item_id.as_str(), 1_024),
        ] {
            require_account_text(field, value, maximum)?;
        }
        Ok(DecisionAccountEvent {
            library_id: library_id.to_owned(),
            cursor: self.cursor,
            event_id: self.event_id,
            account_id: self.account_id,
            account_schema_version: self.account_schema_version,
            statement: self.statement,
            context: self.context,
            action: self.action,
            result: self.result,
            occurred_at: self.occurred_at,
            occurred_at_precision: self.occurred_at_precision,
            authority: DecisionAccountAnchor {
                host_id: self.authority.host_id,
                thread_id: self.authority.thread_id,
                turn_id: self.authority.turn_id,
                item_id: self.authority.item_id,
                span_start: self.authority.span.start,
                span_end: self.authority.span.end,
            },
        })
    }
}

fn require_account_text(field: &str, value: &str, maximum: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > maximum {
        return Err(Error::domain(
            "decision_account_invalid",
            format!("{field} must contain 1..={maximum} bytes"),
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountAuthority {
    host_id: String,
    thread_id: String,
    turn_id: String,
    item_id: String,
    span: AccountSpan,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountSpan {
    start: u64,
    end: u64,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    use tempfile::TempDir;

    use super::{
        AnnalsDecisionFeedCli, DecisionAccountSource, bounded_account_cwd,
        require_participation_marker,
    };

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

    #[test]
    fn annals_activation_watermark_is_reused_as_an_opaque_page_cursor() {
        let temporary = TempDir::new().expect("temporary directory");
        let binary = temporary.path().join("annals");
        let config = temporary.path().join("decisions.toml");
        fs::write(&config, "synthetic = true\n").expect("config fixture");
        fs::write(
            &binary,
            r##"#!/bin/sh
set -eu
[ "$1" = --config ]
[ -f "$2" ]
[ "$3" = --json ]
[ "$4" = decision-feed ]
if [ "$5" = watermark ]; then
  printf '%s\n' '{"ok":true,"data":{"contract_version":1,"library_id":"0123456789abcdef0123456789abcdef","watermark":"afe1_0000"}}'
  exit 0
fi
[ "$5" = page ]
case " $* " in
  *' --after afe1_0000 '*)
    printf '%s\n' '{"ok":true,"data":{"contract_version":1,"library_id":"0123456789abcdef0123456789abcdef","watermark":"afe1_0001","request_cursor":"afe1_0000","next_cursor":"afe1_0001","events":[{"cursor":"afe1_0001","event_id":"event-1","account_id":"account-1","account_schema_version":1,"statement":"Use stable identities.","context":"A durable boundary is needed.","action":"Applied the boundary.","result":"The identity is stable.","occurred_at":1,"occurred_at_precision":"second","authority":{"host_id":"host","thread_id":"thread","turn_id":"turn","item_id":"item","span":{"start":0,"end":1}}}]}}'
    ;;
  *)
    exit 2
    ;;
esac
"##,
        )
        .expect("fake Annals");
        let mut permissions = fs::metadata(&binary).expect("metadata").permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&binary, permissions).expect("executable fake");
        let mut annals =
            AnnalsDecisionFeedCli::new(&binary, &config, Some("0123456789abcdef0123456789abcdef"));
        let (library, activation) = annals.watermark().expect("watermark");
        assert_eq!(library, "0123456789abcdef0123456789abcdef");
        assert_eq!(activation, "afe1_0000");
        let page = annals
            .read_page(&activation, "afe1_0001", 100)
            .expect("later page from activation watermark");
        assert_eq!(page.request_cursor, activation);
        assert_eq!(page.next_cursor, "afe1_0001");
        assert_eq!(page.events[0].account_id, "account-1");
    }

    #[test]
    fn annals_stderr_and_relative_account_cwd_are_not_disclosed() {
        let temporary = TempDir::new().expect("temporary directory");
        let binary = temporary.path().join("annals");
        let config = temporary.path().join("decisions.toml");
        fs::write(&config, "synthetic = true\n").expect("config fixture");
        fs::write(
            &binary,
            "#!/bin/sh\nprintf '%s\\n' 'PRIVATE account body /private/project thread-secret' >&2\nexit 1\n",
        )
        .expect("fake Annals");
        let mut permissions = fs::metadata(&binary).expect("metadata").permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&binary, permissions).expect("executable fake");
        let error =
            AnnalsDecisionFeedCli::new(&binary, &config, Some("0123456789abcdef0123456789abcdef"))
                .watermark()
                .expect_err("failed command");
        let relative = bounded_account_cwd(Some("PRIVATE/relative/project".to_owned()))
            .expect_err("relative cwd");
        fs::write(
            &binary,
            "#!/bin/sh\nprintf '%s\\n' '{\"PRIVATE-account-body-at-/private/project\":true}'\n",
        )
        .expect("invalid Annals JSON");
        let invalid =
            AnnalsDecisionFeedCli::new(&binary, &config, Some("0123456789abcdef0123456789abcdef"))
                .watermark()
                .expect_err("invalid response");
        for rendered in [error.to_string(), invalid.to_string(), relative.to_string()] {
            for private in ["PRIVATE", "/private/project", "thread-secret"] {
                assert!(!rendered.contains(private));
            }
            assert!(rendered.len() < 200);
        }
    }
}
