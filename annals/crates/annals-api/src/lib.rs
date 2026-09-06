//! Annals-owned typed interfaces for account acceptance and feed reads.
//! The client invokes the configured Annals CLI and never reads library state.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use krisis_api::account::{AuthorityAnchor, AuthoritySpan};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub mod usage;

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuccessEnvelope<T> {
    pub ok: bool,
    pub data: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceReceipt {
    pub contract_version: u32,
    pub library_id: String,
    pub producer: String,
    #[serde(rename = "key")]
    pub producer_key: String,
    pub source_sha256: String,
    pub job_id: String,
    pub accepted_at: String,
    pub acceptance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Watermark {
    pub contract_version: u32,
    pub library_id: String,
    pub watermark: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub contract_version: u32,
    pub library_id: String,
    pub watermark: String,
    pub request_cursor: String,
    pub next_cursor: String,
    pub events: Vec<AcceptedAccountEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedAccountEvent {
    pub cursor: String,
    pub event_id: String,
    pub account_id: String,
    pub account_schema_version: u32,
    pub statement: String,
    pub context: String,
    pub action: String,
    pub result: String,
    pub occurred_at: i64,
    pub occurred_at_precision: String,
    pub authority: AuthorityAnchor,
}

/// A bounded failure that never includes provider output, paths, or account text.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    pub code: &'static str,
    pub message: &'static str,
}

fn failure(code: &'static str, message: &'static str) -> Error {
    Error { code, message }
}

#[derive(Debug, Clone)]
pub struct Client {
    binary: PathBuf,
    config: PathBuf,
}

impl Client {
    #[must_use]
    pub fn new(binary: impl Into<PathBuf>, config: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            config: config.into(),
        }
    }

    fn json<T: DeserializeOwned>(&self, arguments: &[OsString]) -> Result<T, Error> {
        let output = Command::new(&self.binary)
            .arg("--config")
            .arg(&self.config)
            .arg("--json")
            .args(arguments)
            .output()
            .map_err(|_| {
                failure(
                    "annals_command_unavailable",
                    "unable to run the configured Annals command",
                )
            })?;
        if !output.status.success() {
            return Err(failure(
                "annals_command_failed",
                "Annals command did not complete successfully",
            ));
        }
        let envelope: SuccessEnvelope<T> =
            serde_json::from_slice(&output.stdout).map_err(|_| {
                failure(
                    "annals_response_invalid",
                    "Annals returned an invalid response envelope",
                )
            })?;
        if !envelope.ok {
            return Err(failure(
                "annals_response_invalid",
                "Annals returned a non-success JSON envelope",
            ));
        }
        Ok(envelope.data)
    }

    /// Accept one exact producer file without dispatching it.
    ///
    /// # Errors
    /// Returns a bounded transport, rejection, or incompatible-response failure.
    pub fn accept(&self, key: &str, file: &Path) -> Result<AcceptanceReceipt, Error> {
        let receipt: AcceptanceReceipt = self.json(&[
            "inbox".into(),
            "accept".into(),
            "--producer".into(),
            "krisis".into(),
            "--key".into(),
            key.into(),
            file.as_os_str().to_owned(),
        ])?;
        receipt.validate()?;
        if receipt.producer_key != key {
            return Err(failure(
                "annals_receipt_invalid",
                "Annals returned a receipt for a different account",
            ));
        }
        Ok(receipt)
    }

    /// Freeze the current committed acceptance prefix.
    ///
    /// # Errors
    /// Returns a bounded transport, rejection, or incompatible-response failure.
    pub fn watermark(&self) -> Result<Watermark, Error> {
        let watermark: Watermark = self.json(&["decision-feed".into(), "watermark".into()])?;
        require_version(watermark.contract_version)?;
        require_library_id(&watermark.library_id)?;
        require_text(&watermark.watermark, 1_024)?;
        Ok(watermark)
    }

    /// Read the immutable page strictly after a cursor at one fixed watermark.
    ///
    /// # Errors
    /// Returns a bounded input, transport, or invalid-response failure.
    pub fn read_page(&self, cursor: &str, watermark: &str, limit: u16) -> Result<Page, Error> {
        if !(1..=MAX_PAGE_SIZE).contains(&usize::from(limit)) {
            return Err(failure(
                "account_event_limit_invalid",
                "Annals decision-feed limit must be between 1 and 200",
            ));
        }
        if cursor.trim().is_empty() || watermark.trim().is_empty() {
            return Err(failure(
                "annals_cursor_invalid",
                "Annals decision-feed cursor and watermark must not be blank",
            ));
        }
        let page: Page = self.json(&[
            "decision-feed".into(),
            "page".into(),
            "--watermark".into(),
            watermark.into(),
            "--after".into(),
            cursor.into(),
            "--limit".into(),
            limit.to_string().into(),
        ])?;
        require_version(page.contract_version)?;
        require_library_id(&page.library_id)?;
        require_text(&page.watermark, 1_024)?;
        require_text(&page.request_cursor, 1_024)?;
        require_text(&page.next_cursor, 1_024)?;
        if page.watermark != watermark {
            return Err(failure(
                "annals_watermark_mismatch",
                "Annals did not keep the page fixed to the requested watermark",
            ));
        }
        for event in &page.events {
            event.validate()?;
        }
        Ok(page)
    }
}

fn require_version(version: u32) -> Result<(), Error> {
    if version != CONTRACT_VERSION {
        return Err(failure(
            "annals_feed_incompatible",
            "Annals returned an unsupported exchange contract",
        ));
    }
    Ok(())
}

/// Validate the public persistent-library identity without inspecting storage.
///
/// # Errors
/// Returns an error unless the identity is exactly 32 lowercase hex characters.
pub fn require_library_id(value: &str) -> Result<(), Error> {
    if value.len() != 32
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(failure(
            "annals_library_id_invalid",
            "Annals library identity must be exactly 32 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

fn require_text(value: &str, maximum: usize) -> Result<(), Error> {
    if value.trim().is_empty() || value.len() > maximum {
        return Err(failure(
            "decision_account_invalid",
            "Annals returned a blank or oversized account field",
        ));
    }
    Ok(())
}

impl AcceptedAccountEvent {
    /// Validate the version-one bounded feed projection.
    ///
    /// # Errors
    /// Returns a bounded error for an incompatible schema, span, or field.
    pub fn validate(&self) -> Result<(), Error> {
        if self.account_schema_version != krisis_api::account::ACCOUNT_SCHEMA_VERSION {
            return Err(failure(
                "decision_account_incompatible",
                "Annals returned an unsupported decision account schema",
            ));
        }
        if self.authority.span.end <= self.authority.span.start {
            return Err(failure(
                "decision_account_anchor_invalid",
                "decision account authority span ends before it starts",
            ));
        }
        for value in [
            &self.cursor,
            &self.event_id,
            &self.account_id,
            &self.authority.host_id,
            &self.authority.thread_id,
            &self.authority.turn_id,
            &self.authority.item_id,
        ] {
            require_text(value, 1_024)?;
        }
        for value in [&self.statement, &self.context, &self.action, &self.result] {
            require_text(value, 16_384)?;
        }
        require_text(&self.occurred_at_precision, 128)
    }
}

impl AcceptanceReceipt {
    /// Validate the version-one acceptance receipt independently of caller state.
    ///
    /// # Errors
    /// Returns a bounded error for invalid identity, version, or receipt fields.
    pub fn validate(&self) -> Result<(), Error> {
        require_version(self.contract_version)?;
        require_library_id(&self.library_id)?;
        if self.producer != "krisis"
            || self.producer_key.trim().is_empty()
            || self.source_sha256.len() != 64
            || !self
                .source_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || self.job_id.trim().is_empty()
            || self.accepted_at.trim().is_empty()
            || !matches!(self.acceptance.as_str(), "created" | "replayed")
        {
            return Err(failure(
                "annals_receipt_invalid",
                "Annals returned an invalid acceptance receipt",
            ));
        }
        Ok(())
    }
}
