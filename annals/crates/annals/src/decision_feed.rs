use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::cli::DecisionFeedPageArgs;
use crate::config::Config;
use crate::db;
use crate::error::AppError;
use crate::render::CommandOutput;

pub(crate) use annals_api::CONTRACT_VERSION;
use annals_api::{
    AcceptedDocumentEvent, MAX_PAGE_DOCUMENT_BYTES, MAX_PAGE_SIZE, Page as PageOutput,
    Watermark as WatermarkOutput,
};
const CURSOR_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(crate) struct AcceptanceRecord {
    pub source_sha256: String,
    pub job_id: String,
    pub accepted_at: String,
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

pub(crate) fn valid_producer_key(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 512
        && !value.chars().any(char::is_control)
        && value == value.trim()
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
) -> Result<(), AppError> {
    transaction.execute(
        "INSERT INTO decision_account_acceptances(
            event_id, producer, producer_key, source_sha256, job_id, accepted_at
         ) VALUES('dae_' || lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5)",
        params![producer, key, source_sha256, job_id, accepted_at],
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
        "SELECT sequence, event_id, producer_key, source_sha256, job_id, accepted_at
         FROM decision_account_acceptances
         WHERE sequence > ?1 AND sequence <= ?2 ORDER BY sequence ASC LIMIT ?3",
    )?;
    let rows = statement.query_map(
        params![
            i64::try_from(after_sequence).map_err(|_| invalid_cursor())?,
            i64::try_from(watermark.sequence).map_err(|_| invalid_cursor())?,
            i64::try_from(args.limit).map_err(|_| invalid_cursor())?
        ],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        },
    )?;
    let mut events = Vec::new();
    let mut document_bytes = 0;
    for row in rows {
        let (sequence, event_id, document_id, source_sha256, job_id, accepted_at) = row?;
        let (source_name, document) = crate::inbox::accepted_document(
            config,
            &library_id,
            &document_id,
            &source_sha256,
            &job_id,
            &accepted_at,
        )?;
        if !events.is_empty() && document_bytes + document.len() > MAX_PAGE_DOCUMENT_BYTES {
            break;
        }
        document_bytes += document.len();
        events.push(AcceptedDocumentEvent {
            cursor: encode_cursor(&CursorPayload {
                version: CURSOR_VERSION,
                kind: CursorKind::Item,
                library_id: library_id.clone(),
                sequence: u64::try_from(sequence).map_err(|_| invalid_feed_state())?,
            })?,
            event_id,
            document_id,
            source_name,
            source_sha256,
            accepted_at,
            document,
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
