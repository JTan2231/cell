//! Version-one immutable decision-account content, maintained by Krisis.
use serde::{Deserialize, Serialize};

pub const ACCOUNT_SCHEMA_VERSION: u32 = 1;
pub const CAPTURE_RULE_VERSION: &str = "krisis/decision-account-classification/1";
pub const MAX_ACCOUNT_BYTES: usize = 1_048_576;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("unable to render decision source")]
    Json(#[from] serde_json::Error),
    #[error("rendered decision account exceeds one MiB")]
    TooLarge,
}

/// The ordered Markdown account sections. `authority_quote` is the complete
/// Markdown block quotation, including its `>` prefixes.
#[derive(Debug, Clone)]
pub struct Account {
    pub statement: String,
    pub authority_quote: String,
    pub context: String,
    pub action: String,
    pub result: String,
    pub source: SourceMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityAnchor {
    pub host_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub span: AuthoritySpan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoritySpan {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceMetadata {
    pub schema_version: u32,
    pub decision_id: String,
    pub occurred_at: i64,
    pub occurred_at_precision: String,
    pub capture_rule_version: String,
    pub authority: AuthorityAnchor,
}

/// Render the canonical schema-one bytes used for producer idempotency.
///
/// # Errors
/// Returns an error if metadata cannot serialize or the account exceeds one MiB.
pub fn render(account: &Account) -> Result<String, Error> {
    // Preserve the original JSON-object key ordering in already retained bytes.
    let source = serde_json::to_string_pretty(&serde_json::to_value(&account.source)?)?;
    let markdown = format!(
        "# Decision\n\n{}\n\n## Authority\n\n{}\n\n## Context\n\n{}\n\n## Action\n\n{}\n\n## Result\n\n{}\n\n## Source\n\n```json\n{}\n```\n",
        account.statement,
        account.authority_quote,
        account.context,
        account.action,
        account.result,
        source
    );
    if markdown.len() > MAX_ACCOUNT_BYTES {
        return Err(Error::TooLarge);
    }
    Ok(markdown)
}

/// Decode an account and require its source identity to match the producer key.
///
/// # Errors
/// Returns an error for an invalid section, metadata, schema, identity, or span.
pub fn parse(text: &str, expected_key: &str) -> Result<Account, Error> {
    let normalized = text.strip_suffix('\n').unwrap_or(text);
    let after_title = normalized.strip_prefix("# Decision\n").ok_or_else(|| {
        invalid_account("decision account must begin with the exact heading # Decision")
    })?;
    let (statement, rest) = split_section(after_title, "Authority")?;
    let (authority_quote, rest) = split_section(rest, "Context")?;
    let (context, rest) = split_section(rest, "Action")?;
    let (action, rest) = split_section(rest, "Result")?;
    let (result, source) = split_section(rest, "Source")?;
    for section in [statement, authority_quote, context, action, result] {
        if section.trim().is_empty() {
            return Err(invalid_account(
                "decision account sections must not be blank",
            ));
        }
        if section.lines().any(|line| line.starts_with('#')) {
            return Err(invalid_account(
                "decision account contains an unexpected heading",
            ));
        }
    }
    if authority_quote
        .lines()
        .filter(|line| !line.trim().is_empty())
        .any(|line| line != ">" && !line.starts_with("> "))
        || authority_quote
            .lines()
            .all(|line| line.trim_matches(['>', ' ']).is_empty())
    {
        return Err(invalid_account(
            "the Authority section must contain exactly one Markdown block quotation",
        ));
    }
    let source = source.trim();
    let json = source
        .strip_prefix("```json\n")
        .and_then(|value| value.strip_suffix("\n```"))
        .ok_or_else(|| {
            invalid_account("the Source section must be exactly one fenced json object")
        })?;
    let metadata: SourceMetadata = serde_json::from_str(json)
        .map_err(|error| invalid_account(format!("invalid Source metadata: {error}")))?;
    validate_source_metadata(&metadata, expected_key)?;
    Ok(Account {
        statement: statement.trim().to_owned(),
        context: context.trim().to_owned(),
        action: action.trim().to_owned(),
        result: result.trim().to_owned(),
        authority_quote: authority_quote.trim().to_owned(),
        source: metadata,
    })
}

fn split_section<'a>(input: &'a str, heading: &str) -> Result<(&'a str, &'a str), Error> {
    input
        .split_once(&format!("\n## {heading}\n"))
        .ok_or_else(|| invalid_account(format!("decision account is missing ## {heading}")))
}

fn validate_source_metadata(metadata: &SourceMetadata, expected_key: &str) -> Result<(), Error> {
    if metadata.schema_version != ACCOUNT_SCHEMA_VERSION {
        return Err(invalid_account(
            "unsupported decision account schema_version",
        ));
    }
    if metadata.decision_id != expected_key {
        return Err(invalid_account(
            "Source decision_id does not match the producer key",
        ));
    }
    for (name, value) in [
        ("decision_id", metadata.decision_id.as_str()),
        (
            "occurred_at_precision",
            metadata.occurred_at_precision.as_str(),
        ),
        (
            "capture_rule_version",
            metadata.capture_rule_version.as_str(),
        ),
        ("authority.host_id", metadata.authority.host_id.as_str()),
        ("authority.thread_id", metadata.authority.thread_id.as_str()),
        ("authority.turn_id", metadata.authority.turn_id.as_str()),
        ("authority.item_id", metadata.authority.item_id.as_str()),
    ] {
        if !bounded_identifier(value) {
            return Err(invalid_account(format!(
                "Source {name} must be a nonblank single-line value of at most 512 bytes"
            )));
        }
    }
    if metadata.authority.span.end <= metadata.authority.span.start
        || metadata.authority.span.end > i64::MAX as u64
    {
        return Err(invalid_account(
            "Source authority span must be a nonempty signed-64-bit byte range",
        ));
    }
    Ok(())
}

#[must_use]
pub fn valid_producer_key(value: &str) -> bool {
    bounded_identifier(value)
}

fn bounded_identifier(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 512
        && !value.chars().any(char::is_control)
        && value == value.trim()
}

fn invalid_account(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
