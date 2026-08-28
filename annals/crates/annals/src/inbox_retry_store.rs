use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use serde::Serialize;

use crate::corpus::now;
use crate::error::AppError;

const MAX_REASON_CHARACTERS: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RetrySelection {
    pub from_job_id: String,
    pub through_job_id: String,
    pub items: Vec<RetrySelectionItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RetrySelectionItem {
    pub ordinal: i64,
    pub original_job_id: String,
    pub original_sequence: u64,
    pub original_delivery_id: i64,
    pub original_completed_at: String,
    pub original_error_code: String,
    pub original_error_message: String,
    #[serde(skip_serializing)]
    pub original_work_id: Option<i64>,
    pub already_selected_by: Option<i64>,
    pub already_selected_child_job_id: Option<String>,
    pub already_selected_child_delivery_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RetryHalt {
    pub halted_at: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RetryEvent {
    pub id: i64,
    pub from_job_id: String,
    pub through_job_id: String,
    pub reason: Option<String>,
    pub state: String,
    pub created_at: String,
    pub ready_at: Option<String>,
    pub completed_at: Option<String>,
    pub last_halt: Option<RetryHalt>,
    pub member_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RetryItem {
    pub ordinal: i64,
    pub original_job_id: String,
    pub original_sequence: u64,
    pub original_delivery_id: i64,
    pub original_completed_at: String,
    pub original_error_code: String,
    pub original_error_message: String,
    #[serde(skip_serializing)]
    pub original_work_id: Option<i64>,
    pub child_job_id: Option<String>,
    pub child_sequence: Option<u64>,
    pub child_delivery_id: Option<i64>,
    pub child_delivery_status: Option<String>,
    pub child_result: Option<String>,
    pub child_result_revision: Option<i64>,
    pub child_completed_at: Option<String>,
    pub child_error_code: Option<String>,
    pub child_error_message: Option<String>,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RetrySummary {
    pub selected: usize,
    pub attempted: usize,
    pub succeeded: usize,
    pub unsuccessful: usize,
    pub remaining: usize,
    pub not_attempted: usize,
    pub processing: usize,
    pub applied: usize,
    pub recorded: usize,
    pub failed: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RetryEventReport {
    pub event: RetryEvent,
    pub summary: RetrySummary,
    pub items: Vec<RetryItem>,
}

pub(crate) fn preview(
    connection: &Connection,
    from_job_id: &str,
    through_job_id: &str,
) -> Result<RetrySelection, AppError> {
    validate_job_id(from_job_id)?;
    validate_job_id(through_job_id)?;
    let from = failed_anchor(connection, from_job_id)?;
    let through = failed_anchor(connection, through_job_id)?;
    if from > through {
        return Err(AppError::invalid(
            "invalid_inbox_retry_range",
            format!(
                "retry range begins after it ends in delivery completion order: {from_job_id} through {through_job_id}"
            ),
        ));
    }

    let mut statement = connection.prepare(
        "SELECT original.id, original.delivery_key, original.completed_at,
                original.error_code, original.error_message, original.work_id,
                item.event_id, item.child_job_id, item.child_ingestion_id
         FROM ingestions AS original
         LEFT JOIN inbox_retry_items AS item
           ON item.original_ingestion_id = original.id
         WHERE original.channel = 'inbox' AND original.status = 'failed'
           AND original.error_code <> 'inbox_job_skipped'
           AND (original.completed_at > ?1
                OR (original.completed_at = ?1 AND original.id >= ?2))
           AND (original.completed_at < ?3
                OR (original.completed_at = ?3 AND original.id <= ?4))
         ORDER BY original.completed_at, original.id",
    )?;
    let rows = statement.query_map(params![from.0, from.1, through.0, through.1], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<i64>>(8)?,
        ))
    })?;
    let mut items = Vec::new();
    for (ordinal, row) in rows.enumerate() {
        let (
            original_delivery_id,
            delivery_key,
            original_completed_at,
            original_error_code,
            original_error_message,
            original_work_id,
            already_selected_by,
            already_selected_child_job_id,
            already_selected_child_delivery_id,
        ) = row?;
        let original_job_id = job_id_from_delivery_key(&delivery_key)?;
        let original_sequence = validate_job_id(&original_job_id)?;
        items.push(RetrySelectionItem {
            ordinal: i64::try_from(ordinal).map_err(|_| {
                AppError::database(
                    "inbox_retry_range_too_large",
                    "retry range contains too many failed source deliveries",
                )
            })?,
            original_job_id,
            original_sequence,
            original_delivery_id,
            original_completed_at,
            original_error_code,
            original_error_message,
            original_work_id,
            already_selected_by,
            already_selected_child_job_id,
            already_selected_child_delivery_id,
        });
    }
    if items.is_empty() {
        return Err(AppError::not_found(
            "inbox_retry_range_empty",
            "retry range contains no failed inbox source deliveries",
        ));
    }
    Ok(RetrySelection {
        from_job_id: from_job_id.to_owned(),
        through_job_id: through_job_id.to_owned(),
        items,
    })
}

pub(crate) fn create_event(
    connection: &mut Connection,
    selection: &RetrySelection,
    reason: Option<&str>,
) -> Result<RetryEventReport, AppError> {
    validate_selection(selection)?;
    validate_reason(reason)?;
    if let Some(item) = selection
        .items
        .iter()
        .find(|item| item.already_selected_by.is_some())
    {
        return Err(already_selected_error(item));
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(event) = active_event(&transaction)? {
        return Err(AppError::conflict(
            "inbox_retry_event_active",
            format!(
                "retry event {} is {}; finish it before creating another",
                event.id, event.state
            ),
        ));
    }
    for item in &selection.items {
        let selected_by = transaction
            .query_row(
                "SELECT event_id FROM inbox_retry_items WHERE original_ingestion_id = ?1",
                [item.original_delivery_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if let Some(event_id) = selected_by {
            let mut stale = item.clone();
            stale.already_selected_by = Some(event_id);
            let prior_child = transaction.query_row(
                "SELECT child_job_id, child_ingestion_id
                     FROM inbox_retry_items WHERE original_ingestion_id = ?1",
                [item.original_delivery_id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                    ))
                },
            )?;
            stale.already_selected_child_job_id = prior_child.0;
            stale.already_selected_child_delivery_id = prior_child.1;
            return Err(already_selected_error(&stale));
        }
    }

    transaction.execute(
        "INSERT INTO inbox_retry_events(
             from_job_id, through_job_id, reason, state, created_at
         ) VALUES(?1, ?2, ?3, 'preparing', ?4)",
        params![
            selection.from_job_id,
            selection.through_job_id,
            reason,
            now()?
        ],
    )?;
    let event_id = transaction.last_insert_rowid();
    for item in &selection.items {
        transaction.execute(
            "INSERT INTO inbox_retry_items(
                 event_id, ordinal, original_job_id, original_sequence,
                 original_ingestion_id, original_completed_at, original_error_code,
                 original_error_message, original_work_id
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                event_id,
                item.ordinal,
                item.original_job_id,
                sqlite_sequence(item.original_sequence)?,
                item.original_delivery_id,
                item.original_completed_at,
                item.original_error_code,
                item.original_error_message,
                item.original_work_id,
            ],
        )?;
    }
    transaction.commit()?;
    event_report(connection, event_id)
}

pub(crate) fn list_events(connection: &Connection) -> Result<Vec<RetryEvent>, AppError> {
    let mut statement = connection.prepare(EVENT_QUERY)?;
    let rows = statement.query_map([], event_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub(crate) fn active_event(connection: &Connection) -> Result<Option<RetryEvent>, AppError> {
    connection
        .query_row(
            "SELECT event.id, event.from_job_id, event.through_job_id, event.reason,
                    event.state, event.created_at, event.ready_at, event.completed_at,
                    event.last_halted_at, event.last_halt_code, event.last_halt_message,
                    COUNT(item.ordinal)
             FROM inbox_retry_events AS event
             LEFT JOIN inbox_retry_items AS item ON item.event_id = event.id
             WHERE event.state <> 'completed'
             GROUP BY event.id
             ORDER BY event.id DESC
             LIMIT 1",
            [],
            event_from_row,
        )
        .optional()
        .map_err(AppError::from)
}

pub(crate) fn event_report(
    connection: &Connection,
    event_id: i64,
) -> Result<RetryEventReport, AppError> {
    let event = event(connection, event_id)?;
    let mut statement = connection.prepare(
        "SELECT item.ordinal, item.original_job_id, item.original_sequence,
                item.original_ingestion_id, item.original_completed_at,
                item.original_error_code, item.original_error_message,
                item.original_work_id, item.child_job_id, item.child_sequence,
                item.child_ingestion_id, child.status, child.result,
                child.result_revision, child.completed_at, child.error_code,
                child.error_message
         FROM inbox_retry_items AS item
         LEFT JOIN ingestions AS child ON child.id = item.child_ingestion_id
         WHERE item.event_id = ?1
         ORDER BY item.ordinal",
    )?;
    let rows = statement.query_map([event_id], item_from_row)?;
    let mut items = rows.collect::<Result<Vec<_>, _>>()?;
    for item in &mut items {
        item.outcome = derive_outcome(item)?;
    }
    let summary = summarize(&items);
    Ok(RetryEventReport {
        event,
        summary,
        items,
    })
}

pub(crate) fn link_child_job(
    connection: &Connection,
    event_id: i64,
    ordinal: i64,
    child_job_id: &str,
    child_sequence: u64,
) -> Result<(), AppError> {
    let parsed_sequence = validate_job_id(child_job_id)?;
    if parsed_sequence != child_sequence {
        return Err(AppError::invalid(
            "invalid_inbox_job_id",
            format!(
                "inbox job {child_job_id} encodes sequence {parsed_sequence}, not {child_sequence}"
            ),
        ));
    }
    let existing = connection
        .query_row(
            "SELECT item.child_job_id, item.child_sequence, event.state
             FROM inbox_retry_items AS item
             JOIN inbox_retry_events AS event ON event.id = item.event_id
             WHERE item.event_id = ?1 AND item.ordinal = ?2",
            params![event_id, ordinal],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| retry_item_not_found(event_id, ordinal))?;
    let child_sequence_sql = sqlite_sequence(child_sequence)?;
    if existing.0.as_deref() == Some(child_job_id) && existing.1 == Some(child_sequence_sql) {
        return Ok(());
    }
    if existing.0.is_some() {
        return Err(AppError::conflict(
            "inbox_retry_child_already_linked",
            format!("retry event {event_id} item {ordinal} already has a child job"),
        ));
    }
    if existing.2 != "preparing" {
        return Err(state_conflict(event_id, &existing.2, "link child jobs"));
    }
    connection.execute(
        "UPDATE inbox_retry_items SET child_job_id = ?1, child_sequence = ?2
         WHERE event_id = ?3 AND ordinal = ?4 AND child_job_id IS NULL",
        params![child_job_id, child_sequence_sql, event_id, ordinal],
    )?;
    Ok(())
}

pub(crate) fn link_child_delivery(
    connection: &Connection,
    event_id: i64,
    child_job_id: &str,
    child_delivery_id: i64,
) -> Result<(), AppError> {
    if child_delivery_id <= 0 {
        return Err(AppError::invalid(
            "invalid_ingestion",
            "retry child delivery identifier must be positive",
        ));
    }
    let (existing_delivery_id, state) = connection
        .query_row(
            "SELECT item.child_ingestion_id, event.state
             FROM inbox_retry_items AS item
             JOIN inbox_retry_events AS event ON event.id = item.event_id
             WHERE item.event_id = ?1 AND item.child_job_id = ?2",
            params![event_id, child_job_id],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found(
                "inbox_retry_child_not_found",
                format!("inbox job {child_job_id} is not a child of retry event {event_id}"),
            )
        })?;
    if existing_delivery_id == Some(child_delivery_id) {
        return Ok(());
    }
    if existing_delivery_id.is_some() {
        return Err(AppError::conflict(
            "inbox_retry_child_delivery_already_linked",
            format!("inbox job {child_job_id} already has a child delivery"),
        ));
    }
    if !matches!(state.as_str(), "running" | "halted") {
        return Err(state_conflict(event_id, &state, "link a child delivery"));
    }
    let (channel, delivery_key) = connection
        .query_row(
            "SELECT channel, delivery_key FROM ingestions WHERE id = ?1",
            [child_delivery_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found(
                "ingestion_not_found",
                format!("source delivery {child_delivery_id} was not found"),
            )
        })?;
    if channel != "inbox"
        || delivery_key
            .as_deref()
            .map(job_id_from_delivery_key)
            .transpose()?
            .as_deref()
            != Some(child_job_id)
    {
        return Err(AppError::conflict(
            "inbox_retry_child_delivery_mismatch",
            format!(
                "source delivery {child_delivery_id} does not belong to inbox job {child_job_id}"
            ),
        ));
    }
    connection.execute(
        "UPDATE inbox_retry_items SET child_ingestion_id = ?1
         WHERE event_id = ?2 AND child_job_id = ?3 AND child_ingestion_id IS NULL",
        params![child_delivery_id, event_id, child_job_id],
    )?;
    Ok(())
}

pub(crate) fn mark_running(connection: &Connection, event_id: i64) -> Result<(), AppError> {
    let current = event(connection, event_id)?;
    if current.state == "running" {
        return Ok(());
    }
    if current.state != "preparing" {
        return Err(state_conflict(event_id, &current.state, "mark it running"));
    }
    let unpublished = connection.query_row(
        "SELECT COUNT(*) FROM inbox_retry_items
         WHERE event_id = ?1 AND child_job_id IS NULL",
        [event_id],
        |row| row.get::<_, i64>(0),
    )?;
    if unpublished != 0 {
        return Err(AppError::conflict(
            "inbox_retry_publication_incomplete",
            format!("retry event {event_id} still has {unpublished} child jobs to publish"),
        ));
    }
    connection.execute(
        "UPDATE inbox_retry_events SET state = 'running', ready_at = ?1
         WHERE id = ?2 AND state = 'preparing'",
        params![now()?, event_id],
    )?;
    Ok(())
}

pub(crate) fn halt(
    connection: &Connection,
    event_id: i64,
    code: &str,
    message: &str,
) -> Result<(), AppError> {
    if code.trim().is_empty() || message.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_inbox_retry_halt",
            "retry halt code and message must be nonempty",
        ));
    }
    let current = event(connection, event_id)?;
    if current.state == "halted"
        && current
            .last_halt
            .as_ref()
            .is_some_and(|halt| halt.code == code && halt.message == message)
    {
        return Ok(());
    }
    if current.state != "running" {
        return Err(state_conflict(event_id, &current.state, "halt it"));
    }
    connection.execute(
        "UPDATE inbox_retry_events
         SET state = 'halted', last_halted_at = ?1,
             last_halt_code = ?2, last_halt_message = ?3
         WHERE id = ?4 AND state = 'running'",
        params![now()?, code, message, event_id],
    )?;
    Ok(())
}

pub(crate) fn resume_running(connection: &Connection, event_id: i64) -> Result<(), AppError> {
    let current = event(connection, event_id)?;
    if current.state == "running" {
        return Ok(());
    }
    if current.state != "halted" {
        return Err(state_conflict(event_id, &current.state, "continue it"));
    }
    connection.execute(
        "UPDATE inbox_retry_events SET state = 'running'
         WHERE id = ?1 AND state = 'halted'",
        [event_id],
    )?;
    Ok(())
}

pub(crate) fn complete(connection: &Connection, event_id: i64) -> Result<(), AppError> {
    let current = event_report(connection, event_id)?;
    if current.event.state == "completed" {
        return Ok(());
    }
    if current.event.state != "running" {
        return Err(state_conflict(
            event_id,
            &current.event.state,
            "complete it",
        ));
    }
    if current.summary.remaining != 0 {
        return Err(AppError::conflict(
            "inbox_retry_event_incomplete",
            format!(
                "retry event {event_id} still has {} child deliveries without terminal outcomes",
                current.summary.remaining
            ),
        ));
    }
    connection.execute(
        "UPDATE inbox_retry_events SET state = 'completed', completed_at = ?1
         WHERE id = ?2 AND state = 'running'",
        params![now()?, event_id],
    )?;
    Ok(())
}

const EVENT_QUERY: &str = "SELECT event.id, event.from_job_id, event.through_job_id, event.reason,
            event.state, event.created_at, event.ready_at, event.completed_at,
            event.last_halted_at, event.last_halt_code, event.last_halt_message,
            COUNT(item.ordinal)
     FROM inbox_retry_events AS event
     LEFT JOIN inbox_retry_items AS item ON item.event_id = event.id
     WHERE event.state <> 'completed'
        OR event.id IN (
            SELECT recent.id FROM inbox_retry_events AS recent
            WHERE recent.state = 'completed'
            ORDER BY recent.id DESC LIMIT 20
        )
     GROUP BY event.id
     ORDER BY event.id DESC";

fn event(connection: &Connection, event_id: i64) -> Result<RetryEvent, AppError> {
    connection
        .query_row(
            "SELECT event.id, event.from_job_id, event.through_job_id, event.reason,
                    event.state, event.created_at, event.ready_at, event.completed_at,
                    event.last_halted_at, event.last_halt_code, event.last_halt_message,
                    COUNT(item.ordinal)
             FROM inbox_retry_events AS event
             LEFT JOIN inbox_retry_items AS item ON item.event_id = event.id
             WHERE event.id = ?1
             GROUP BY event.id",
            [event_id],
            event_from_row,
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found(
                "inbox_retry_event_not_found",
                format!("retry event {event_id} was not found"),
            )
        })
}

fn event_from_row(row: &Row<'_>) -> rusqlite::Result<RetryEvent> {
    let halted_at = row.get::<_, Option<String>>(8)?;
    let halt_code = row.get::<_, Option<String>>(9)?;
    let halt_message = row.get::<_, Option<String>>(10)?;
    let last_halt = match (halted_at, halt_code, halt_message) {
        (Some(halted_at), Some(code), Some(message)) => Some(RetryHalt {
            halted_at,
            code,
            message,
        }),
        (None, None, None) => None,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let member_count = row.get::<_, i64>(11)?;
    Ok(RetryEvent {
        id: row.get(0)?,
        from_job_id: row.get(1)?,
        through_job_id: row.get(2)?,
        reason: row.get(3)?,
        state: row.get(4)?,
        created_at: row.get(5)?,
        ready_at: row.get(6)?,
        completed_at: row.get(7)?,
        last_halt,
        member_count: usize::try_from(member_count)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(11, member_count))?,
    })
}

fn item_from_row(row: &Row<'_>) -> rusqlite::Result<RetryItem> {
    let original_sequence = row.get::<_, i64>(2)?;
    let child_sequence = row
        .get::<_, Option<i64>>(9)?
        .map(|sequence| {
            u64::try_from(sequence)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(9, sequence))
        })
        .transpose()?;
    Ok(RetryItem {
        ordinal: row.get(0)?,
        original_job_id: row.get(1)?,
        original_sequence: u64::try_from(original_sequence)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, original_sequence))?,
        original_delivery_id: row.get(3)?,
        original_completed_at: row.get(4)?,
        original_error_code: row.get(5)?,
        original_error_message: row.get(6)?,
        original_work_id: row.get(7)?,
        child_job_id: row.get(8)?,
        child_sequence,
        child_delivery_id: row.get(10)?,
        child_delivery_status: row.get(11)?,
        child_result: row.get(12)?,
        child_result_revision: row.get(13)?,
        child_completed_at: row.get(14)?,
        child_error_code: row.get(15)?,
        child_error_message: row.get(16)?,
        outcome: String::new(),
    })
}

fn derive_outcome(item: &RetryItem) -> Result<String, AppError> {
    let outcome = match item.child_delivery_status.as_deref() {
        None => "not_attempted",
        Some("processing") => "processing",
        Some("completed") => match item.child_result.as_deref() {
            Some("applied") => "applied",
            Some("recorded") => "recorded",
            Some(result) => {
                return Err(AppError::database(
                    "invalid_inbox_retry_child_outcome",
                    format!(
                        "retry child job {} completed with invalid result {result:?}",
                        item.child_job_id.as_deref().unwrap_or("unknown")
                    ),
                ));
            }
            None => {
                return Err(AppError::database(
                    "invalid_inbox_retry_child_outcome",
                    format!(
                        "retry child job {} completed without a result",
                        item.child_job_id.as_deref().unwrap_or("unknown")
                    ),
                ));
            }
        },
        Some("failed") if item.child_error_code.as_deref() == Some("inbox_job_skipped") => {
            "skipped"
        }
        Some("failed") => "failed",
        Some(status) => {
            return Err(AppError::database(
                "invalid_inbox_retry_child_outcome",
                format!(
                    "retry child job {} has invalid delivery status {status:?}",
                    item.child_job_id.as_deref().unwrap_or("unknown")
                ),
            ));
        }
    };
    Ok(outcome.to_owned())
}

fn summarize(items: &[RetryItem]) -> RetrySummary {
    let count = |outcome: &str| items.iter().filter(|item| item.outcome == outcome).count();
    let not_attempted = count("not_attempted");
    let processing = count("processing");
    let applied = count("applied");
    let recorded = count("recorded");
    let failed = count("failed");
    let skipped = count("skipped");
    RetrySummary {
        selected: items.len(),
        attempted: items.len() - not_attempted,
        succeeded: applied + recorded,
        unsuccessful: failed + skipped,
        remaining: not_attempted + processing,
        not_attempted,
        processing,
        applied,
        recorded,
        failed,
        skipped,
    }
}

fn failed_anchor(connection: &Connection, job_id: &str) -> Result<(String, i64), AppError> {
    let mut statement = connection.prepare(
        "SELECT completed_at, id, delivery_key
         FROM ingestions
         WHERE channel = 'inbox' AND status = 'failed'
           AND error_code <> 'inbox_job_skipped'
           AND delivery_key GLOB ('inbox:' || ?1 || ':*')
         ORDER BY id",
    )?;
    let rows = statement.query_map([job_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let matches = rows.collect::<Result<Vec<_>, _>>()?;
    match matches.as_slice() {
        [(completed_at, id, delivery_key)] if job_id_from_delivery_key(delivery_key)? == job_id => {
            Ok((completed_at.clone(), *id))
        }
        [] => Err(AppError::not_found(
            "inbox_retry_anchor_not_found",
            format!("failed inbox job {job_id} was not found"),
        )),
        _ => Err(AppError::database(
            "invalid_ingestion",
            format!("inbox job {job_id} has more than one source delivery"),
        )),
    }
}

fn validate_selection(selection: &RetrySelection) -> Result<(), AppError> {
    validate_job_id(&selection.from_job_id)?;
    validate_job_id(&selection.through_job_id)?;
    if selection.items.is_empty() {
        return Err(AppError::invalid(
            "inbox_retry_range_empty",
            "retry event must contain at least one failed inbox source delivery",
        ));
    }
    for (ordinal, item) in selection.items.iter().enumerate() {
        let ordinal = i64::try_from(ordinal).map_err(|_| {
            AppError::invalid(
                "inbox_retry_range_too_large",
                "retry range contains too many failed source deliveries",
            )
        })?;
        if item.original_work_id.is_none() {
            return Err(AppError::invalid(
                "inbox_retry_original_not_retained",
                format!(
                    "failed inbox job {} has no retained work identity",
                    item.original_job_id
                ),
            ));
        }
        if item.ordinal != ordinal
            || validate_job_id(&item.original_job_id)? != item.original_sequence
            || item.original_delivery_id <= 0
            || item.original_completed_at.trim().is_empty()
            || item.original_error_code.trim().is_empty()
            || item.original_error_message.trim().is_empty()
            || item.original_work_id.is_some_and(|id| id <= 0)
        {
            return Err(AppError::invalid(
                "invalid_inbox_retry_selection",
                "retry selection is not a valid ordered failure snapshot",
            ));
        }
    }
    if selection
        .items
        .first()
        .map(|item| item.original_job_id.as_str())
        != Some(selection.from_job_id.as_str())
        || selection
            .items
            .last()
            .map(|item| item.original_job_id.as_str())
            != Some(selection.through_job_id.as_str())
    {
        return Err(AppError::invalid(
            "invalid_inbox_retry_selection",
            "retry selection anchors do not match its first and last members",
        ));
    }
    Ok(())
}

fn validate_reason(reason: Option<&str>) -> Result<(), AppError> {
    if reason.is_some_and(|reason| {
        reason.trim().is_empty()
            || reason != reason.trim()
            || reason.chars().count() > MAX_REASON_CHARACTERS
    }) {
        return Err(AppError::invalid(
            "invalid_inbox_retry_reason",
            format!(
                "retry reason must be trimmed, nonempty, and at most {MAX_REASON_CHARACTERS} characters"
            ),
        ));
    }
    Ok(())
}

fn validate_job_id(id: &str) -> Result<u64, AppError> {
    let base = id.get(..21).filter(|base| {
        base.starts_with('j') && base[1..].bytes().all(|byte| byte.is_ascii_digit())
    });
    let suffix_is_valid = id.get(21..).is_some_and(|suffix| {
        suffix.is_empty()
            || suffix.strip_prefix('-').is_some_and(|digits| {
                !digits.is_empty()
                    && digits.bytes().all(|byte| byte.is_ascii_digit())
                    && digits.parse::<u64>().is_ok_and(|value| value > 0)
            })
    });
    let sequence = base
        .and_then(|base| base[1..].parse::<u64>().ok())
        .filter(|sequence| *sequence > 0);
    if !suffix_is_valid || sequence.is_none() {
        return Err(AppError::invalid(
            "invalid_inbox_job_id",
            format!("inbox job identifier is invalid: {id}"),
        ));
    }
    Ok(sequence.unwrap_or_default())
}

fn job_id_from_delivery_key(delivery_key: &str) -> Result<String, AppError> {
    let job_id = delivery_key
        .strip_prefix("inbox:")
        .and_then(|rest| rest.split_once(':').map(|(job_id, _)| job_id))
        .ok_or_else(|| {
            AppError::database(
                "invalid_ingestion",
                format!("inbox source delivery has invalid key {delivery_key:?}"),
            )
        })?;
    validate_job_id(job_id).map_err(|_| {
        AppError::database(
            "invalid_ingestion",
            format!("inbox source delivery has invalid key {delivery_key:?}"),
        )
    })?;
    Ok(job_id.to_owned())
}

fn sqlite_sequence(sequence: u64) -> Result<i64, AppError> {
    i64::try_from(sequence).map_err(|_| {
        AppError::invalid(
            "invalid_inbox_job_id",
            "inbox job sequence exceeds the supported library range",
        )
    })
}

fn already_selected_error(item: &RetrySelectionItem) -> AppError {
    let event_id = item.already_selected_by.unwrap_or_default();
    let child = match (
        item.already_selected_child_job_id.as_deref(),
        item.already_selected_child_delivery_id,
    ) {
        (Some(job_id), Some(delivery_id)) => {
            format!("; its child is job {job_id}, source delivery {delivery_id}")
        }
        (Some(job_id), None) => format!("; its child is job {job_id}"),
        (None, _) => String::new(),
    };
    AppError::conflict(
        "inbox_retry_original_already_selected",
        format!(
            "failed inbox job {} was already selected by retry event {event_id}{child}; select its failed child for another retry",
            item.original_job_id
        ),
    )
}

fn retry_item_not_found(event_id: i64, ordinal: i64) -> AppError {
    AppError::not_found(
        "inbox_retry_item_not_found",
        format!("retry event {event_id} has no item {ordinal}"),
    )
}

fn state_conflict(event_id: i64, state: &str, action: &str) -> AppError {
    AppError::conflict(
        "inbox_retry_state_conflict",
        format!("retry event {event_id} is {state}; cannot {action}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn retained_work(connection: &Connection) -> Result<i64, rusqlite::Error> {
        connection.execute(
            "INSERT INTO works(label, normalized_label, text, sha256, created_at)
             VALUES('work', 'work', 'source', ?1, 'now')",
            ["0".repeat(64)],
        )?;
        Ok(connection.last_insert_rowid())
    }

    fn failed_delivery(
        connection: &Connection,
        job_sequence: u64,
        completed_at: &str,
        requested_work_id: Option<i64>,
    ) -> TestResult {
        let job_id = format!("j{job_sequence:020}");
        let work_id = if let Some(work_id) = requested_work_id {
            work_id
        } else {
            connection.execute(
                "INSERT INTO works(label, normalized_label, text, sha256, created_at)
                 VALUES(?1, ?1, ?2, ?3, 'now')",
                params![
                    format!("work-{job_sequence}"),
                    format!("source-{job_sequence}"),
                    format!("{job_sequence:064x}"),
                ],
            )?;
            connection.last_insert_rowid()
        };
        connection.execute(
            "INSERT INTO ingestions(
                 delivery_key, source_name, channel, first_seen_at, ingested_at,
                 completed_at, status, work_id, new_work, error_code, error_message
             ) VALUES(?1, ?2, 'inbox', ?3, ?3, ?4, 'failed', ?5, 1,
                      'model_runner_failed', 'source delivery failed')",
            params![
                format!("inbox:{job_id}:{completed_at}"),
                format!("source-{job_sequence}.txt"),
                format!("seen-{job_sequence}"),
                completed_at,
                work_id,
            ],
        )?;
        Ok(())
    }

    fn skipped_delivery(
        connection: &Connection,
        job_sequence: u64,
        completed_at: &str,
    ) -> TestResult {
        let job_id = format!("j{job_sequence:020}");
        connection.execute(
            "INSERT INTO ingestions(
                 delivery_key, source_name, channel, first_seen_at, completed_at,
                 status, error_code, error_message
             ) VALUES(?1, 'skipped.txt', 'inbox', 'seen', ?2, 'failed',
                      'inbox_job_skipped', 'source delivery failed')",
            params![format!("inbox:{job_id}:{completed_at}"), completed_at],
        )?;
        Ok(())
    }

    fn child_delivery(
        connection: &Connection,
        job_id: &str,
        status: &str,
        completed_at: Option<&str>,
        result: Option<&str>,
        error_code: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        let retained = result.map(|_| 1_i64);
        connection.execute(
            "INSERT INTO ingestions(
                 delivery_key, source_name, channel, first_seen_at, ingested_at,
                 completed_at, status, work_id, new_work, result, error_code,
                 error_message
             ) VALUES(?1, 'retry.txt', 'inbox', 'retry-seen',
                      CASE WHEN ?4 IS NULL THEN NULL ELSE 'retry-ingested' END,
                      ?2, ?3, ?6, CASE WHEN ?6 IS NULL THEN NULL ELSE 0 END,
                      ?4, ?5,
                      CASE WHEN ?5 IS NULL THEN NULL ELSE 'source delivery failed' END)",
            params![
                format!("inbox:{job_id}:retry-seen"),
                completed_at,
                status,
                result,
                error_code,
                retained,
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }

    #[test]
    fn preview_uses_delivery_completion_order_and_inclusive_anchors() -> TestResult {
        let directory = tempfile::tempdir()?;
        let connection = db::init(&directory.path().join("annals.db"))?;
        failed_delivery(&connection, 12, "2026-08-27T00:00:02.000000Z", None)?;
        failed_delivery(&connection, 99, "2026-08-27T00:00:01.000000Z", None)?;
        failed_delivery(&connection, 3, "2026-08-27T00:00:02.000000Z", None)?;
        skipped_delivery(&connection, 77, "2026-08-27T00:00:01.500000Z")?;

        let selection = preview(
            &connection,
            "j00000000000000000099",
            "j00000000000000000003",
        )?;
        assert_eq!(
            selection
                .items
                .iter()
                .map(|item| item.original_sequence)
                .collect::<Vec<_>>(),
            vec![99, 12, 3]
        );
        let Err(error) = preview(
            &connection,
            "j00000000000000000003",
            "j00000000000000000099",
        ) else {
            return Err("reversed range unexpectedly succeeded".into());
        };
        assert_eq!(error.code(), "invalid_inbox_retry_range");
        let Err(error) = preview(
            &connection,
            "j00000000000000000077",
            "j00000000000000000003",
        ) else {
            return Err("operator-skipped job unexpectedly anchored a retry".into());
        };
        assert_eq!(error.code(), "inbox_retry_anchor_not_found");
        Ok(())
    }

    #[test]
    fn event_creation_rejects_a_pre_retention_failure() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut connection = db::init(&directory.path().join("annals.db"))?;
        let job_id = "j00000000000000000001";
        connection.execute(
            "INSERT INTO ingestions(
                 delivery_key, source_name, channel, first_seen_at, completed_at,
                 status, error_code, error_message
             ) VALUES(?1, 'invalid.txt', 'inbox', 'seen', 'completed', 'failed',
                      'input_not_utf8', 'source delivery failed')",
            [format!("inbox:{job_id}:seen")],
        )?;
        let selection = preview(&connection, job_id, job_id)?;
        let Err(error) = create_event(&mut connection, &selection, None) else {
            return Err("pre-retention failure unexpectedly created an event".into());
        };
        assert_eq!(error.code(), "inbox_retry_original_not_retained");
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM inbox_retry_events", [], |row| {
                row.get::<_, i64>(0)
            })?,
            0
        );
        Ok(())
    }

    #[test]
    fn event_membership_is_frozen_and_an_original_has_one_direct_child() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut connection = db::init(&directory.path().join("annals.db"))?;
        failed_delivery(&connection, 1, "2026-08-27T00:00:01Z", None)?;
        failed_delivery(&connection, 2, "2026-08-27T00:00:02Z", None)?;
        let selection = preview(
            &connection,
            "j00000000000000000001",
            "j00000000000000000002",
        )?;
        let json = serde_json::to_value(&selection)?;
        assert!(json["items"][0].get("original_work_id").is_none());
        let report = create_event(&mut connection, &selection, Some("auth repair"))?;
        assert_eq!(report.event.state, "preparing");
        assert_eq!(report.event.member_count, 2);
        assert!(
            connection
                .execute(
                    "UPDATE inbox_retry_items SET original_error_code = 'changed'
                 WHERE event_id = 1 AND ordinal = 0",
                    []
                )
                .is_err()
        );
        let Err(error) = create_event(&mut connection, &selection, None) else {
            return Err("another event unexpectedly overlapped the active event".into());
        };
        assert_eq!(error.code(), "inbox_retry_event_active");
        Ok(())
    }

    #[test]
    fn event_list_is_recent_and_always_includes_the_open_event() -> TestResult {
        let directory = tempfile::tempdir()?;
        let connection = db::init(&directory.path().join("annals.db"))?;
        connection.execute(
            "INSERT INTO inbox_retry_events(
                 from_job_id, through_job_id, state, created_at
             ) VALUES('j00000000000000000001', 'j00000000000000000001',
                      'preparing', 'open')",
            [],
        )?;
        for sequence in 2..=26 {
            let job_id = format!("j{sequence:020}");
            connection.execute(
                "INSERT INTO inbox_retry_events(
                     from_job_id, through_job_id, state, created_at, ready_at, completed_at
                 ) VALUES(?1, ?1, 'completed', ?2, ?2, ?2)",
                params![job_id, format!("completed-{sequence}")],
            )?;
        }

        let events = list_events(&connection)?;
        assert_eq!(events.len(), 21);
        assert_eq!(events.first().map(|event| event.id), Some(26));
        assert!(
            events
                .iter()
                .any(|event| event.id == 1 && event.state == "preparing")
        );
        Ok(())
    }

    #[test]
    fn lifecycle_links_children_and_derives_terminal_outcomes() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut connection = db::init(&directory.path().join("annals.db"))?;
        retained_work(&connection)?;
        failed_delivery(&connection, 1, "2026-08-27T00:00:01Z", None)?;
        failed_delivery(&connection, 2, "2026-08-27T00:00:02Z", None)?;
        let selection = preview(
            &connection,
            "j00000000000000000001",
            "j00000000000000000002",
        )?;
        let event_id = create_event(&mut connection, &selection, None)?.event.id;
        link_child_job(&connection, event_id, 0, "j00000000000000000003", 3)?;
        let Err(error) = mark_running(&connection, event_id) else {
            return Err("event started with one unpublished child".into());
        };
        assert_eq!(error.code(), "inbox_retry_publication_incomplete");
        link_child_job(&connection, event_id, 1, "j00000000000000000004", 4)?;
        mark_running(&connection, event_id)?;

        let applied_id = child_delivery(
            &connection,
            "j00000000000000000003",
            "completed",
            Some("2026-08-27T00:00:03Z"),
            Some("recorded"),
            None,
        )?;
        link_child_delivery(&connection, event_id, "j00000000000000000003", applied_id)?;
        let failed_id = child_delivery(
            &connection,
            "j00000000000000000004",
            "failed",
            Some("2026-08-27T00:00:04Z"),
            None,
            Some("model_runner_failed"),
        )?;
        link_child_delivery(&connection, event_id, "j00000000000000000004", failed_id)?;
        halt(
            &connection,
            event_id,
            "model_runner_failed",
            "runner stopped",
        )?;
        resume_running(&connection, event_id)?;
        complete(&connection, event_id)?;

        let report = event_report(&connection, event_id)?;
        assert_eq!(report.event.state, "completed");
        assert_eq!(report.summary.selected, 2);
        assert_eq!(report.summary.attempted, 2);
        assert_eq!(report.summary.succeeded, 1);
        assert_eq!(report.summary.unsuccessful, 1);
        assert_eq!(report.summary.remaining, 0);
        assert_eq!(report.items[0].child_result.as_deref(), Some("recorded"));
        assert_eq!(report.items[0].outcome, "recorded");
        assert_eq!(
            report.items[1].child_error_code.as_deref(),
            Some("model_runner_failed")
        );
        assert_eq!(report.items[1].outcome, "failed");
        assert_eq!(
            report
                .event
                .last_halt
                .as_ref()
                .map(|halt| halt.code.as_str()),
            Some("model_runner_failed")
        );
        assert!(
            serde_json::to_value(&report)?["items"][0]
                .get("original_work_id")
                .is_none()
        );
        assert!(active_event(&connection)?.is_none());

        let prior = preview(
            &connection,
            "j00000000000000000001",
            "j00000000000000000001",
        )?;
        assert_eq!(prior.items[0].already_selected_by, Some(event_id));
        assert_eq!(
            prior.items[0].already_selected_child_job_id.as_deref(),
            Some("j00000000000000000003")
        );
        assert_eq!(
            prior.items[0].already_selected_child_delivery_id,
            Some(applied_id)
        );
        let Err(error) = create_event(&mut connection, &prior, None) else {
            return Err("original source delivery unexpectedly got a second child".into());
        };
        assert_eq!(error.code(), "inbox_retry_original_already_selected");
        Ok(())
    }

    #[test]
    fn retained_child_is_not_a_successful_retry_outcome() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut connection = db::init(&directory.path().join("annals.db"))?;
        retained_work(&connection)?;
        failed_delivery(&connection, 1, "2026-08-27T00:00:01Z", None)?;
        let selection = preview(
            &connection,
            "j00000000000000000001",
            "j00000000000000000001",
        )?;
        let event_id = create_event(&mut connection, &selection, None)?.event.id;
        link_child_job(&connection, event_id, 0, "j00000000000000000002", 2)?;
        mark_running(&connection, event_id)?;
        let delivery_id = child_delivery(
            &connection,
            "j00000000000000000002",
            "completed",
            Some("2026-08-27T00:00:02Z"),
            Some("retained"),
            None,
        )?;
        link_child_delivery(&connection, event_id, "j00000000000000000002", delivery_id)?;
        let Err(error) = complete(&connection, event_id) else {
            return Err("retained child unexpectedly completed a retry event".into());
        };
        assert_eq!(error.code(), "invalid_inbox_retry_child_outcome");
        assert_eq!(
            connection.query_row(
                "SELECT state FROM inbox_retry_events WHERE id = ?1",
                [event_id],
                |row| row.get::<_, String>(0)
            )?,
            "running"
        );
        Ok(())
    }
}
