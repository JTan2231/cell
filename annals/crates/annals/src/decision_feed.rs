use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::cli::DecisionFeedPageArgs;
use crate::config::Config;
use crate::db;
use crate::error::AppError;
use crate::render::CommandOutput;

pub(crate) const CONTRACT_VERSION: u32 = 1;
pub(crate) const ACCOUNT_SCHEMA_VERSION: u32 = 1;
const MAX_PAGE_SIZE: usize = 200;
const CURSOR_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(crate) struct AccountProjection {
    pub schema_version: u32,
    pub statement: String,
    pub context: String,
    pub action: String,
    pub result: String,
    pub occurred_at: i64,
    pub occurred_at_precision: String,
    pub capture_rule_version: String,
    pub authority: AuthorityAnchor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuthorityAnchor {
    pub host_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub span: AuthoritySpan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuthoritySpan {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceMetadata {
    schema_version: u32,
    decision_id: String,
    occurred_at: i64,
    occurred_at_precision: String,
    capture_rule_version: String,
    authority: AuthorityAnchor,
}

#[derive(Debug, Clone)]
pub(crate) struct AcceptanceRecord {
    pub source_sha256: String,
    pub job_id: String,
    pub accepted_at: String,
}

#[derive(Debug, Serialize)]
struct WatermarkOutput {
    contract_version: u32,
    library_id: String,
    watermark: String,
}

#[derive(Debug, Serialize)]
struct PageOutput {
    contract_version: u32,
    library_id: String,
    watermark: String,
    request_cursor: String,
    next_cursor: String,
    events: Vec<AcceptedAccountEvent>,
}

#[derive(Debug, Serialize)]
struct AcceptedAccountEvent {
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
    authority: AuthorityAnchor,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CursorPayload {
    version: u32,
    kind: CursorKind,
    library_id: String,
    sequence: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CursorKind {
    Watermark,
    Item,
}

pub(crate) fn library_id(connection: &Connection) -> Result<String, AppError> {
    connection
        .query_row(
            "SELECT library_id FROM library_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(AppError::from)
}

pub(crate) fn require_expected_library(
    connection: &Connection,
    config: &Config,
) -> Result<String, AppError> {
    db::require_library_kind(connection, db::LibraryKind::Decisions)?;
    let expected = &config.decision_feed()?.expected_library_id;
    let actual = library_id(connection)?;
    if actual != *expected {
        return Err(AppError::conflict(
            "unexpected_decision_feed_library",
            "the selected library does not match decision_feed.expected_library_id",
        ));
    }
    Ok(actual)
}

pub(crate) fn parse_account(text: &str, expected_key: &str) -> Result<AccountProjection, AppError> {
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
    Ok(AccountProjection {
        schema_version: metadata.schema_version,
        statement: statement.trim().to_owned(),
        context: context.trim().to_owned(),
        action: action.trim().to_owned(),
        result: result.trim().to_owned(),
        occurred_at: metadata.occurred_at,
        occurred_at_precision: metadata.occurred_at_precision,
        capture_rule_version: metadata.capture_rule_version,
        authority: metadata.authority,
    })
}

fn split_section<'a>(input: &'a str, heading: &str) -> Result<(&'a str, &'a str), AppError> {
    input
        .split_once(&format!("\n## {heading}\n"))
        .ok_or_else(|| invalid_account(format!("decision account is missing ## {heading}")))
}

fn validate_source_metadata(metadata: &SourceMetadata, expected_key: &str) -> Result<(), AppError> {
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

pub(crate) fn valid_producer_key(value: &str) -> bool {
    bounded_identifier(value)
}

fn bounded_identifier(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 512
        && !value.chars().any(char::is_control)
        && value == value.trim()
}

fn invalid_account(message: impl Into<String>) -> AppError {
    AppError::invalid("invalid_decision_account", message)
}

pub(crate) fn find_acceptance(
    connection: &Connection,
    producer: &str,
    key: &str,
) -> Result<Option<AcceptanceRecord>, AppError> {
    connection
        .query_row(
            "SELECT source_sha256, job_id, accepted_at
             FROM decision_account_acceptances
             WHERE producer = ?1 AND producer_key = ?2",
            params![producer, key],
            |row| {
                Ok(AcceptanceRecord {
                    source_sha256: row.get(0)?,
                    job_id: row.get(1)?,
                    accepted_at: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(AppError::from)
}

pub(crate) fn insert_acceptance(
    transaction: &Transaction<'_>,
    producer: &str,
    key: &str,
    source_sha256: &str,
    job_id: &str,
    accepted_at: &str,
    account: &AccountProjection,
) -> Result<(), AppError> {
    let event_id =
        transaction.query_row("SELECT 'dae_' || lower(hex(randomblob(16)))", [], |row| {
            row.get::<_, String>(0)
        })?;
    transaction.execute(
        "INSERT INTO decision_account_acceptances(
             event_id, producer, producer_key, source_sha256, job_id, accepted_at,
             account_schema_version, statement, context, action, result, occurred_at,
             occurred_at_precision, capture_rule_version, authority_host_id,
             authority_thread_id, authority_turn_id, authority_item_id,
             authority_span_start, authority_span_end
         ) VALUES(
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
             ?15, ?16, ?17, ?18, ?19, ?20
         )",
        params![
            event_id,
            producer,
            key,
            source_sha256,
            job_id,
            accepted_at,
            account.schema_version,
            account.statement,
            account.context,
            account.action,
            account.result,
            account.occurred_at,
            account.occurred_at_precision,
            account.capture_rule_version,
            account.authority.host_id,
            account.authority.thread_id,
            account.authority.turn_id,
            account.authority.item_id,
            i64::try_from(account.authority.span.start).map_err(|_| {
                invalid_account("Source authority span start exceeds signed 64-bit storage")
            })?,
            i64::try_from(account.authority.span.end).map_err(|_| {
                invalid_account("Source authority span end exceeds signed 64-bit storage")
            })?,
        ],
    )?;
    Ok(())
}

pub(crate) fn watermark(
    path: &std::path::Path,
    config: &Config,
) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let library_id = require_expected_library(&connection, config)?;
    let sequence = maximum_sequence(&connection)?;
    let token = encode_cursor(&CursorPayload {
        version: CURSOR_VERSION,
        kind: CursorKind::Watermark,
        library_id: library_id.clone(),
        sequence,
    })?;
    let output = WatermarkOutput {
        contract_version: CONTRACT_VERSION,
        library_id,
        watermark: token,
    };
    Ok(CommandOutput::new(
        serde_json::to_value(&output)?,
        format!("Decision-account watermark: {}", output.watermark),
    ))
}

#[allow(clippy::too_many_lines)]
pub(crate) fn page(
    path: &std::path::Path,
    config: &Config,
    args: &DecisionFeedPageArgs,
) -> Result<CommandOutput, AppError> {
    if args.limit == 0 || args.limit > MAX_PAGE_SIZE {
        return Err(AppError::invalid(
            "invalid_decision_feed_limit",
            format!("decision-feed page limit must be between 1 and {MAX_PAGE_SIZE}"),
        ));
    }
    let connection = db::open_read(path)?;
    let library_id = require_expected_library(&connection, config)?;
    let watermark = decode_cursor(&args.watermark, CursorKind::Watermark, &library_id)?;
    if watermark.sequence > maximum_sequence(&connection)? {
        return Err(AppError::conflict(
            "decision_feed_watermark_unavailable",
            "the requested watermark is not a committed prefix of this library",
        ));
    }
    let after = decode_page_cursor(&args.after, &library_id)?;
    let after_sequence = after.sequence;
    if after_sequence > watermark.sequence {
        return Err(AppError::invalid(
            "invalid_decision_feed_cursor",
            "the item cursor is after the requested watermark",
        ));
    }
    let mut statement = connection.prepare(
        "SELECT sequence, event_id, producer_key, account_schema_version, statement,
                context, action, result, occurred_at, occurred_at_precision,
                authority_host_id, authority_thread_id, authority_turn_id,
                authority_item_id, authority_span_start, authority_span_end
         FROM decision_account_acceptances
         WHERE sequence > ?1 AND sequence <= ?2
         ORDER BY sequence ASC
         LIMIT ?3",
    )?;
    let rows = statement.query_map(
        params![
            i64::try_from(after_sequence).map_err(|_| invalid_cursor())?,
            i64::try_from(watermark.sequence).map_err(|_| invalid_cursor())?,
            i64::try_from(args.limit).map_err(|_| invalid_cursor())?,
        ],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, String>(12)?,
                row.get::<_, String>(13)?,
                row.get::<_, i64>(14)?,
                row.get::<_, i64>(15)?,
            ))
        },
    )?;
    let mut events = Vec::new();
    for row in rows {
        let (
            sequence,
            event_id,
            account_id,
            account_schema_version,
            statement,
            context,
            action,
            result,
            occurred_at,
            occurred_at_precision,
            host_id,
            thread_id,
            turn_id,
            item_id,
            span_start,
            span_end,
        ) = row?;
        let sequence = u64::try_from(sequence).map_err(|_| invalid_feed_state())?;
        let span_start = u64::try_from(span_start).map_err(|_| invalid_feed_state())?;
        let span_end = u64::try_from(span_end).map_err(|_| invalid_feed_state())?;
        events.push(AcceptedAccountEvent {
            cursor: encode_cursor(&CursorPayload {
                version: CURSOR_VERSION,
                kind: CursorKind::Item,
                library_id: library_id.clone(),
                sequence,
            })?,
            event_id,
            account_id,
            account_schema_version,
            statement,
            context,
            action,
            result,
            occurred_at,
            occurred_at_precision,
            authority: AuthorityAnchor {
                host_id,
                thread_id,
                turn_id,
                item_id,
                span: AuthoritySpan {
                    start: span_start,
                    end: span_end,
                },
            },
        });
    }
    let request_cursor = args.after.clone();
    let next_cursor = events
        .last()
        .map_or_else(|| request_cursor.clone(), |event| event.cursor.clone());
    let output = PageOutput {
        contract_version: CONTRACT_VERSION,
        library_id,
        watermark: args.watermark.clone(),
        request_cursor,
        next_cursor,
        events,
    };
    let human = format!(
        "{} accepted decision {}",
        output.events.len(),
        if output.events.len() == 1 {
            "account"
        } else {
            "accounts"
        }
    );
    Ok(CommandOutput::new(serde_json::to_value(output)?, human))
}

fn maximum_sequence(connection: &Connection) -> Result<u64, AppError> {
    let value = connection.query_row(
        "SELECT COALESCE(MAX(sequence), 0) FROM decision_account_acceptances",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    u64::try_from(value).map_err(|_| invalid_feed_state())
}

fn encode_cursor(cursor: &CursorPayload) -> Result<String, AppError> {
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(cursor)?))
}

fn decode_cursor(
    token: &str,
    kind: CursorKind,
    library_id: &str,
) -> Result<CursorPayload, AppError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| invalid_cursor())?;
    let cursor: CursorPayload = serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())?;
    if cursor.version != CURSOR_VERSION
        || cursor.kind != kind
        || cursor.library_id != library_id
        || cursor.sequence > i64::MAX as u64
    {
        return Err(invalid_cursor());
    }
    Ok(cursor)
}

fn decode_page_cursor(token: &str, library_id: &str) -> Result<CursorPayload, AppError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| invalid_cursor())?;
    let cursor: CursorPayload = serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())?;
    if cursor.version != CURSOR_VERSION
        || !matches!(cursor.kind, CursorKind::Watermark | CursorKind::Item)
        || cursor.library_id != library_id
        || cursor.sequence > i64::MAX as u64
    {
        return Err(invalid_cursor());
    }
    Ok(cursor)
}

fn invalid_cursor() -> AppError {
    AppError::invalid(
        "invalid_decision_feed_cursor",
        "the decision-feed cursor is invalid for this library or request",
    )
}

fn invalid_feed_state() -> AppError {
    AppError::database(
        "invalid_decision_feed_state",
        "the decision-account feed contains invalid stored state",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_bounded_account_shape() -> Result<(), AppError> {
        let text = concat!(
            "# Decision\n\nUse one library.\n\n",
            "## Authority\n\n> use Annals\n\n",
            "## Context\n\nThe boundary is local.\n\n",
            "## Action\n\nCreate a feed.\n\n",
            "## Result\n\nUnknown.\n\n",
            "## Source\n\n```json\n",
            "{\"schema_version\":1,\"decision_id\":\"d1\",",
            "\"occurred_at\":1788436800,",
            "\"occurred_at_precision\":\"second\",",
            "\"capture_rule_version\":\"krisis/1\",",
            "\"authority\":{\"host_id\":\"h\",\"thread_id\":\"t\",",
            "\"turn_id\":\"u\",\"item_id\":\"i\",",
            "\"span\":{\"start\":4,\"end\":14}}}\n```\n",
        );
        let parsed = parse_account(text, "d1")?;
        assert_eq!(parsed.statement, "Use one library.");
        assert_eq!(parsed.authority.span.end, 14);
        Ok(())
    }
}
