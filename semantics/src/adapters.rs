use std::path::{Path, PathBuf};

use conversations::{AppServerClient, ClientConfig, StderrPolicy, ThreadRef};

use crate::domain::{DecisionAccountAnchor, DecisionAccountEvent, DecisionAnchor, DecisionEvent};
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
    client: annals_api::Client,
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
            client: annals_api::Client::new(binary, config),
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
        let response = self.client.watermark().map_err(annals_error)?;
        self.require_library(&response.library_id)?;
        Ok((response.library_id, response.watermark))
    }

    fn read_page(
        &mut self,
        cursor: &str,
        watermark: &str,
        limit: u16,
    ) -> Result<DecisionAccountPage> {
        let response = self
            .client
            .read_page(cursor, watermark, limit)
            .map_err(annals_error)?;
        self.require_library(&response.library_id)?;
        let events = response
            .events
            .into_iter()
            .map(|event| normalize_account(event, &response.library_id))
            .collect();
        Ok(DecisionAccountPage {
            library_id: response.library_id,
            request_cursor: response.request_cursor,
            next_cursor: response.next_cursor,
            watermark: response.watermark,
            events,
        })
    }
}

fn annals_error(error: annals_api::Error) -> Error {
    let (code, message) = match error.code {
        "annals_command_unavailable" => (
            "annals_feed_unavailable",
            "unable to run the configured Annals decision-feed command",
        ),
        "annals_command_failed" => (
            "annals_feed_failed",
            "Annals decision-feed command did not complete successfully",
        ),
        "annals_response_invalid" => ("annals_feed_invalid", error.message),
        _ => (error.code, error.message),
    };
    Error::domain(code, message)
}

#[derive(Debug, Clone)]
pub struct DecisionsCli {
    binary: PathBuf,
    client: krisis_api::lifecycle::Client,
}

impl DecisionsCli {
    #[must_use]
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        let binary = binary.into();
        Self {
            client: krisis_api::lifecycle::Client::new(&binary),
            binary,
        }
    }

    #[must_use]
    pub fn for_current_user() -> Self {
        Self::new(
            std::env::var_os("SEMANTICS_DECISIONS")
                .map_or_else(|| PathBuf::from("decisions"), PathBuf::from),
        )
    }

    fn error(&self, error: krisis_api::lifecycle::Error) -> Error {
        match error {
            krisis_api::lifecycle::Error::Io(source) => crate::error::io(&self.binary, source),
            krisis_api::lifecycle::Error::Json(source) => Error::Json(source),
            krisis_api::lifecycle::Error::Failed { .. } => {
                Error::domain("decisions_events_failed", error.to_string())
            }
            krisis_api::lifecycle::Error::Invalid { code, message } => Error::domain(code, message),
        }
    }
}

impl Default for DecisionsCli {
    fn default() -> Self {
        Self::for_current_user()
    }
}

impl DecisionEventSource for DecisionsCli {
    fn watermark(&mut self) -> Result<String> {
        let response = self.client.watermark().map_err(|error| self.error(error))?;
        Ok(response.cursor)
    }

    fn read_after(&mut self, cursor: &str, limit: u16) -> Result<DecisionEventPage> {
        let response = self
            .client
            .read_after(cursor, limit)
            .map_err(|error| self.error(error))?;
        let events = response
            .events
            .into_iter()
            .map(normalize_legacy_event)
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

fn normalize_legacy_event(item: krisis_api::lifecycle::DecisionEventItem) -> Result<DecisionEvent> {
    let anchors = item
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
    let (review_id, review_action, reviewed_at, review_source) = match item.event.review {
        Some(review) => (
            Some(review.review_id),
            Some(review.action),
            Some(review.reviewed_at),
            Some(review.review_source),
        ),
        None => (None, None, None, None),
    };
    Ok(DecisionEvent {
        event_id: item.event.event_id,
        event_version: u32::try_from(item.event.event_version).map_err(|_| {
            Error::domain(
                "decision_event_incompatible",
                "unsupported decision event version",
            )
        })?,
        cursor: item.cursor,
        event_kind: item.event.event_kind,
        occurred_at: item.event.occurred_at,
        decision_id: item.event.decision.decision_id,
        decided_at: item.event.decision.decided_at,
        timestamp_precision: item.event.decision.timestamp_precision,
        statement: item.event.decision.statement,
        disposition: item.event.decision.disposition,
        confidence: item.event.decision.confidence,
        rationale: item.event.decision.rationale,
        supersedes_decision_id: item.event.decision.supersedes_decision_id,
        authority_start: item.event.decision.authority_span.start,
        authority_end: item.event.decision.authority_span.end,
        review_state: item.event.decision.review_state,
        review_id,
        review_action,
        reviewed_at,
        review_source,
        anchors,
    })
}

fn normalize_account(
    event: annals_api::AcceptedAccountEvent,
    library_id: &str,
) -> DecisionAccountEvent {
    DecisionAccountEvent {
        library_id: library_id.to_owned(),
        cursor: event.cursor,
        event_id: event.event_id,
        account_id: event.account_id,
        account_schema_version: event.account_schema_version,
        statement: event.statement,
        context: event.context,
        action: event.action,
        result: event.result,
        occurred_at: event.occurred_at,
        occurred_at_precision: event.occurred_at_precision,
        authority: DecisionAccountAnchor {
            host_id: event.authority.host_id,
            thread_id: event.authority.thread_id,
            turn_id: event.authority.turn_id,
            item_id: event.authority.item_id,
            span_start: event.authority.span.start,
            span_end: event.authority.span.end,
        },
    }
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
