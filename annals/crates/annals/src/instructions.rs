//! Library-owned interpretation instructions and their immutable history.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::corpus::{now, sha256_hex};
use crate::error::AppError;

/// The initial interpretation for a library. Updates to Annals do not replace
/// a library's stored selection.
pub const DEFAULT_LIBRARY_INSTRUCTIONS: &str = "Organize the retained sources into an evidence-grounded map of ideas. Inspect each work broadly, using multiple access paths when bounded or repetitive source structure prevents sequential traversal. Choose a coherent granularity relative to the work and corpus. Group related source material into concepts while preserving distinctions. Represent assertions, qualifications, exceptions, examples, limitations, relationships, contradictions, and reported states and results without mechanically creating one concept per sentence. Do not omit material because it appears familiar, minor, speculative, redundant, obvious, low-signal, or unlikely to be useful. Consolidate equivalent meanings, but preserve distinctions in modality and source stance. Associate represented meaning with an existing concept and exact evidence, or create an appropriately scoped grounded concept. Parent edges express broader conceptual scope to narrower conceptual scope. Several parents are symmetric; none is primary. Do not invent a canonical path or sibling ordering. Express each mapping even when its effect appears already satisfied; Annals determines corpus effects mechanically.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionRevision {
    pub library_id: String,
    pub revision: i64,
    pub content: String,
    pub sha256: String,
    /// When Annals recorded this instruction selection, not a graph revision.
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionSetResult {
    pub instructions: InstructionRevision,
    pub changed: bool,
}

/// Read the library's selected instruction document.
pub fn current(connection: &Connection) -> Result<InstructionRevision, AppError> {
    let revision = connection
        .query_row(
            "SELECT current_revision FROM library_instruction_selection WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| {
            AppError::database(
                "library_instructions_missing",
                format!("the library has no instruction selection: {error}"),
            )
        })?;
    at_revision(connection, revision)
}

/// Read one exact, immutable instruction revision.
pub fn at_revision(
    connection: &Connection,
    revision: i64,
) -> Result<InstructionRevision, AppError> {
    let sql = format!("{SELECT_INSTRUCTIONS} WHERE instructions.revision = ?1");
    connection
        .query_row(&sql, [revision], from_row)
        .optional()?
        .ok_or_else(|| {
            AppError::not_found(
                "instruction_revision_not_found",
                format!("library instruction revision {revision} was not found"),
            )
        })
}

/// Read a bounded page newest first. `before` excludes that revision and all
/// newer selections, so later instruction changes do not shift continuation.
pub fn history_page(
    connection: &Connection,
    before: Option<i64>,
    limit: usize,
) -> Result<Vec<InstructionRevision>, AppError> {
    if !(1..=200).contains(&limit) || before.is_some_and(|revision| revision <= 0) {
        return Err(AppError::invalid(
            "invalid_instruction_history_page",
            "instruction history limit must be 1 through 200 and before must be positive",
        ));
    }
    let sql_limit = i64::try_from(limit).map_err(|_| {
        AppError::invalid(
            "invalid_instruction_history_page",
            "instruction history limit is too large",
        )
    })?;
    let sql = format!(
        "{SELECT_INSTRUCTIONS} WHERE (?1 IS NULL OR instructions.revision < ?1) \
         ORDER BY instructions.revision DESC LIMIT ?2"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![before, sql_limit], from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Preserve exact instruction text and atomically select its new revision.
/// Selecting the current bytes is a no-op; returning to earlier bytes appends.
pub fn set(connection: &mut Connection, content: &str) -> Result<InstructionSetResult, AppError> {
    if content.trim().is_empty() {
        return Err(AppError::invalid(
            "empty_library_instructions",
            "library instructions must contain nonblank UTF-8 text",
        ));
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let selected = current(&transaction)?;
    if selected.content == content {
        transaction.commit()?;
        return Ok(InstructionSetResult {
            instructions: selected,
            changed: false,
        });
    }
    let revision = selected.revision.checked_add(1).ok_or_else(|| {
        AppError::database(
            "instruction_revision_overflow",
            "the library instruction revision is too large",
        )
    })?;
    insert_revision(&transaction, revision, content)?;
    transaction.execute(
        "UPDATE library_instruction_selection SET current_revision = ?1 WHERE singleton = 1",
        [revision],
    )?;
    let instructions = at_revision(&transaction, revision)?;
    transaction.commit()?;
    Ok(InstructionSetResult {
        instructions,
        changed: true,
    })
}

pub(crate) fn initialize(connection: &Connection) -> Result<(), AppError> {
    insert_revision(connection, 1, DEFAULT_LIBRARY_INSTRUCTIONS)?;
    connection.execute(
        "INSERT INTO library_instruction_selection(singleton, current_revision) VALUES(1, 1)",
        [],
    )?;
    Ok(())
}

/// Read the frozen interpretation basis for a model request. Direct manual
/// submissions select current instructions in their existing transaction.
pub(crate) fn request_revision(
    connection: &Connection,
    model_run_id: Option<i64>,
) -> Result<Option<i64>, AppError> {
    if let Some(run_id) = model_run_id {
        Ok(connection.query_row(
            "SELECT instruction_revision FROM model_runs WHERE id = ?1",
            [run_id],
            |row| row.get(0),
        )?)
    } else {
        Ok(Some(current(connection)?.revision))
    }
}

pub(crate) fn require_current(
    connection: &Connection,
    instruction_revision: Option<i64>,
) -> Result<(), AppError> {
    let selected = current(connection)?.revision;
    if instruction_revision != Some(selected) {
        let examined = instruction_revision.map_or_else(
            || "an unknown legacy instruction revision".to_owned(),
            |revision| format!("instruction revision {revision}"),
        );
        return Err(AppError::conflict(
            "stale_instructions",
            format!(
                "the reconciliation used {examined}, but the library selects instruction revision {selected}"
            ),
        ));
    }
    Ok(())
}

fn insert_revision(connection: &Connection, revision: i64, content: &str) -> Result<(), AppError> {
    connection.execute(
        "INSERT INTO library_instruction_revisions(revision, content, sha256, recorded_at)
         VALUES(?1, ?2, ?3, ?4)",
        params![revision, content, sha256_hex(content.as_bytes()), now()?],
    )?;
    Ok(())
}

const SELECT_INSTRUCTIONS: &str = "SELECT identity.library_id, instructions.revision, \
    instructions.content, instructions.sha256, instructions.recorded_at \
    FROM library_instruction_revisions AS instructions \
    JOIN library_identity AS identity ON identity.singleton = 1";

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InstructionRevision> {
    Ok(InstructionRevision {
        library_id: row.get(0)?,
        revision: row.get(1)?,
        content: row.get(2)?,
        sha256: row.get(3)?,
        recorded_at: row.get(4)?,
    })
}
