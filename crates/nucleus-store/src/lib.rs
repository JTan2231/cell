//! Durable storage for Nucleus runtime state and schema-bound raw records.
//!
//! The relational schema intentionally projects only facts Nucleus owns. Job
//! requests, harness messages, tool arguments, tool results, and external
//! schemas are retained as opaque bytes with SHA-256 digests.

use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use rusqlite::{
    Connection, OptionalExtension, Row, Transaction, TransactionBehavior, params, types::Type,
};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

const SCHEMA_VERSION: i64 = 1;
const DEFAULT_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub type Digest = [u8; 32];

#[derive(Debug, Error)]
pub enum StoreError {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),

    #[error("database schema version {found} is newer than supported version {supported}")]
    UnsupportedSchemaVersion { found: i64, supported: i64 },

    #[error("job `{0}` already exists with a different request")]
    JobConflict(String),

    #[error("log schema `{0}` already exists with different contents")]
    LogSchemaConflict(String),

    #[error("toolset `{provider}/{name}@{version}` already exists with different contents")]
    ToolsetConflict {
        provider: String,
        name: String,
        version: u32,
    },

    #[error("tool call `{call_id}` in job `{job_id}` already exists with different contents")]
    ToolCallConflict { job_id: String, call_id: String },

    #[error("tool call `{call_id}` in job `{job_id}` was already answered differently")]
    ToolResultConflict { job_id: String, call_id: String },

    #[error("{entity} `{id}` was not found")]
    NotFound { entity: &'static str, id: String },

    #[error("invalid {entity} state transition from `{from}` to `{to}`")]
    InvalidStateTransition {
        entity: &'static str,
        from: String,
        to: String,
    },

    #[error("attempt `{attempt_id}` does not belong to job `{job_id}`")]
    AttemptJobMismatch { job_id: String, attempt_id: String },

    #[error("tool-call log record does not match job `{job_id}` and attempt `{attempt_id}`")]
    ToolCallLogMismatch { job_id: String, attempt_id: String },
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Accepted,
    Running,
    WaitingOnRequester,
    Completed,
    Failed,
    Cancelled,
}

impl JobState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Running => "running",
            Self::WaitingOnRequester => "waiting_on_requester",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

impl fmt::Display for JobState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for JobState {
    type Err = InvalidStoredValue;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "accepted" => Ok(Self::Accepted),
            "running" => Ok(Self::Running),
            "waiting_on_requester" => Ok(Self::WaitingOnRequester),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(InvalidStoredValue(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptState {
    Pending,
    Starting,
    Running,
    WaitingOnRequester,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Lost,
}

impl AttemptState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::WaitingOnRequester => "waiting_on_requester",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Lost => "lost",
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::TimedOut | Self::Lost
        )
    }

    const fn job_state(self) -> JobState {
        match self {
            Self::Pending | Self::Starting => JobState::Accepted,
            Self::Running => JobState::Running,
            Self::WaitingOnRequester => JobState::WaitingOnRequester,
            Self::Completed => JobState::Completed,
            Self::Cancelled => JobState::Cancelled,
            Self::Failed | Self::TimedOut | Self::Lost => JobState::Failed,
        }
    }
}

impl fmt::Display for AttemptState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for AttemptState {
    type Err = InvalidStoredValue;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "starting" => Ok(Self::Starting),
            "running" => Ok(Self::Running),
            "waiting_on_requester" => Ok(Self::WaitingOnRequester),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "timed_out" => Ok(Self::TimedOut),
            "lost" => Ok(Self::Lost),
            _ => Err(InvalidStoredValue(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallState {
    Pending,
    Answered,
}

impl FromStr for ToolCallState {
    type Err = InvalidStoredValue;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "answered" => Ok(Self::Answered),
            _ => Err(InvalidStoredValue(value.to_owned())),
        }
    }
}

#[derive(Debug, Error)]
#[error("invalid value stored in database: {0}")]
pub struct InvalidStoredValue(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJob {
    pub id: String,
    pub label: String,
    pub requester_program: String,
    pub requester_id: String,
    pub parent_job_id: Option<String>,
    pub request_schema_id: String,
    pub request_bytes: Vec<u8>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    pub id: String,
    pub label: String,
    pub requester_program: String,
    pub requester_id: String,
    pub parent_job_id: Option<String>,
    pub request_schema_id: String,
    pub request_bytes: Vec<u8>,
    pub request_digest: Digest,
    pub state: JobState,
    pub current_attempt_id: Option<String>,
    pub cancellation_requested_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub terminal_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission<T> {
    Created(T),
    Existing(T),
}

impl<T> Admission<T> {
    #[must_use]
    pub fn into_inner(self) -> T {
        match self {
            Self::Created(value) | Self::Existing(value) => value,
        }
    }

    #[must_use]
    pub const fn was_created(&self) -> bool {
        matches!(self, Self::Created(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAttempt {
    pub id: String,
    pub job_id: String,
    pub ordinal: u32,
    pub harness: String,
    pub harness_version: String,
    pub adapter_version: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptRecord {
    pub id: String,
    pub job_id: String,
    pub ordinal: u32,
    pub harness: String,
    pub harness_version: String,
    pub adapter_version: String,
    pub state: AttemptState,
    pub process_id: Option<u32>,
    pub process_group_id: Option<i32>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub terminal_reason: Option<String>,
    pub terminal_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLogSchema {
    pub id: String,
    pub name: String,
    pub version: String,
    pub media_type: String,
    pub producer: String,
    pub producer_version: Option<String>,
    pub schema_bytes: Vec<u8>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSchemaRecord {
    pub id: String,
    pub name: String,
    pub version: String,
    pub media_type: String,
    pub producer: String,
    pub producer_version: Option<String>,
    pub schema_bytes: Vec<u8>,
    pub schema_digest: Digest,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLogRecord {
    pub job_id: String,
    pub attempt_id: Option<String>,
    pub observed_at: String,
    pub emitted_at: Option<String>,
    pub stream: String,
    pub schema_id: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    pub job_id: String,
    pub attempt_id: Option<String>,
    pub sequence: u64,
    pub observed_at: String,
    pub emitted_at: Option<String>,
    pub stream: String,
    pub schema_id: String,
    pub payload: Vec<u8>,
    pub payload_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewToolset {
    pub provider: String,
    pub name: String,
    pub version: u32,
    pub definitions_schema_id: String,
    pub definitions_bytes: Vec<u8>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsetRecord {
    pub provider: String,
    pub name: String,
    pub version: u32,
    pub definitions_schema_id: String,
    pub definitions_bytes: Vec<u8>,
    pub definitions_digest: Digest,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPendingToolCall {
    pub id: String,
    pub job_id: String,
    pub attempt_id: String,
    pub tool_name: String,
    pub arguments_schema_id: String,
    pub arguments_bytes: Vec<u8>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewToolResult {
    pub schema_id: String,
    pub result_bytes: Vec<u8>,
    pub is_error: bool,
    pub answered_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingToolCallRecord {
    pub id: String,
    pub job_id: String,
    pub attempt_id: String,
    pub state: ToolCallState,
    pub tool_name: String,
    pub arguments_schema_id: String,
    pub arguments_bytes: Vec<u8>,
    pub arguments_digest: Digest,
    pub request_sequence: u64,
    pub result_schema_id: Option<String>,
    pub result_bytes: Option<Vec<u8>>,
    pub result_digest: Option<Digest>,
    pub result_is_error: Option<bool>,
    pub result_sequence: Option<u64>,
    pub created_at: String,
    pub answered_at: Option<String>,
}

pub struct Store {
    connection: Connection,
}

#[allow(clippy::missing_errors_doc, clippy::needless_pass_by_value)]
impl Store {
    /// Opens an existing store, creating and migrating it when necessary.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        let mut store = Self { connection };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    /// Creates a store. This is intentionally equivalent to [`Store::open`].
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Self::open(path)
    }

    /// Opens an isolated in-memory store, primarily for callers and tests that
    /// need the same constraints without filesystem state.
    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        let mut store = Self { connection };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    fn configure(&self) -> Result<()> {
        self.connection.pragma_update(None, "foreign_keys", "ON")?;
        self.connection.busy_timeout(DEFAULT_BUSY_TIMEOUT)?;
        self.connection.pragma_update(None, "journal_mode", "WAL")?;
        self.connection
            .pragma_update(None, "synchronous", "NORMAL")?;
        Ok(())
    }

    fn migrate(&mut self) -> Result<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let found = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if found > SCHEMA_VERSION {
            return Err(StoreError::UnsupportedSchemaVersion {
                found,
                supported: SCHEMA_VERSION,
            });
        }
        if found == 0 {
            transaction.execute_batch(include_str!("schema.sql"))?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn admit_job(&mut self, new: NewJob) -> Result<Admission<JobRecord>> {
        let request_digest = digest(&new.request_bytes);
        let transaction = self.immediate_transaction()?;
        if let Some(existing) = query_job(&transaction, &new.id)? {
            if same_job_request(&existing, &new, request_digest) {
                transaction.commit()?;
                return Ok(Admission::Existing(existing));
            }
            return Err(StoreError::JobConflict(new.id));
        }

        transaction.execute(
            "INSERT INTO jobs (
                id, label, requester_program, requester_id, parent_job_id,
                request_schema_id, request_bytes, request_digest, state,
                created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'accepted', ?9, ?9)",
            params![
                new.id,
                new.label,
                new.requester_program,
                new.requester_id,
                new.parent_job_id,
                new.request_schema_id,
                new.request_bytes,
                request_digest.as_slice(),
                new.created_at,
            ],
        )?;
        let record = require_job(&transaction, &new.id)?;
        transaction.commit()?;
        Ok(Admission::Created(record))
    }

    pub fn get_job(&self, id: &str) -> Result<Option<JobRecord>> {
        query_job(&self.connection, id)
    }

    pub fn list_jobs_by_requester(
        &self,
        requester_program: &str,
        requester_id: &str,
    ) -> Result<Vec<JobRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT
                id, label, requester_program, requester_id, parent_job_id,
                request_schema_id, request_bytes, request_digest, state,
                current_attempt_id, cancellation_requested_at, created_at,
                updated_at, completed_at, terminal_reason
             FROM jobs
             WHERE requester_program = ?1 AND requester_id = ?2
             ORDER BY created_at, id",
        )?;
        let records = statement
            .query_map(params![requester_program, requester_id], job_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn list_jobs(&self) -> Result<Vec<JobRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT
                id, label, requester_program, requester_id, parent_job_id,
                request_schema_id, request_bytes, request_digest, state,
                current_attempt_id, cancellation_requested_at, created_at,
                updated_at, completed_at, terminal_reason
             FROM jobs
             ORDER BY created_at, id",
        )?;
        let records = statement
            .query_map([], job_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(records)
    }

    /// Records a cancellation request without claiming that cancellation has
    /// completed. Repeated requests preserve the first observation time.
    pub fn request_cancellation(&mut self, job_id: &str, requested_at: &str) -> Result<JobRecord> {
        let transaction = self.immediate_transaction()?;
        let current = require_job(&transaction, job_id)?;
        if !current.state.is_terminal() {
            transaction.execute(
                "UPDATE jobs
                 SET cancellation_requested_at = COALESCE(cancellation_requested_at, ?2),
                     updated_at = CASE
                         WHEN cancellation_requested_at IS NULL THEN ?2
                         ELSE updated_at
                     END
                 WHERE id = ?1",
                params![job_id, requested_at],
            )?;
        }
        let updated = require_job(&transaction, job_id)?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn finish_job(
        &mut self,
        job_id: &str,
        state: JobState,
        completed_at: &str,
        terminal_reason: Option<&str>,
    ) -> Result<JobRecord> {
        if !state.is_terminal() {
            return Err(StoreError::InvalidStateTransition {
                entity: "job",
                from: "nonterminal".to_owned(),
                to: state.to_string(),
            });
        }
        let transaction = self.immediate_transaction()?;
        let current = require_job(&transaction, job_id)?;
        if current.state.is_terminal() && current.state != state {
            return Err(StoreError::InvalidStateTransition {
                entity: "job",
                from: current.state.to_string(),
                to: state.to_string(),
            });
        }
        if !current.state.is_terminal() {
            transaction.execute(
                "UPDATE jobs
                 SET state = ?2, completed_at = ?3, terminal_reason = ?4,
                     updated_at = ?3
                 WHERE id = ?1",
                params![job_id, state.as_str(), completed_at, terminal_reason],
            )?;
        }
        let updated = require_job(&transaction, job_id)?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn create_attempt(&mut self, new: NewAttempt) -> Result<AttemptRecord> {
        let transaction = self.immediate_transaction()?;
        let job = require_job(&transaction, &new.job_id)?;
        if job.state.is_terminal() {
            return Err(StoreError::InvalidStateTransition {
                entity: "job",
                from: job.state.to_string(),
                to: JobState::Accepted.to_string(),
            });
        }
        transaction.execute(
            "INSERT INTO attempts (
                id, job_id, ordinal, harness, harness_version, adapter_version,
                state, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)",
            params![
                new.id,
                new.job_id,
                new.ordinal,
                new.harness,
                new.harness_version,
                new.adapter_version,
                new.created_at,
            ],
        )?;
        transaction.execute(
            "UPDATE jobs SET current_attempt_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![new.job_id, new.id, new.created_at],
        )?;
        let record = require_attempt(&transaction, &new.id)?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn get_attempt(&self, id: &str) -> Result<Option<AttemptRecord>> {
        query_attempt(&self.connection, id)
    }

    pub fn list_attempts(&self, job_id: &str) -> Result<Vec<AttemptRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT
                id, job_id, ordinal, harness, harness_version, adapter_version,
                state, process_id, process_group_id, created_at, started_at,
                completed_at, terminal_reason, terminal_message
             FROM attempts
             WHERE job_id = ?1
             ORDER BY ordinal",
        )?;
        let records = statement
            .query_map([job_id], attempt_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(records)
    }

    /// Returns attempts that could still own a process after daemon recovery.
    pub fn running_attempts(&self) -> Result<Vec<AttemptRecord>> {
        self.attempts_in_states(&[
            AttemptState::Starting,
            AttemptState::Running,
            AttemptState::WaitingOnRequester,
        ])
    }

    /// Includes pending attempts as well as attempts that could own a process.
    pub fn unfinished_attempts(&self) -> Result<Vec<AttemptRecord>> {
        self.attempts_in_states(&[
            AttemptState::Pending,
            AttemptState::Starting,
            AttemptState::Running,
            AttemptState::WaitingOnRequester,
        ])
    }

    fn attempts_in_states(&self, states: &[AttemptState]) -> Result<Vec<AttemptRecord>> {
        let wanted = states
            .iter()
            .map(|state| state.as_str())
            .collect::<Vec<_>>();
        let mut statement = self.connection.prepare(
            "SELECT
                id, job_id, ordinal, harness, harness_version, adapter_version,
                state, process_id, process_group_id, created_at, started_at,
                completed_at, terminal_reason, terminal_message
             FROM attempts
             WHERE state IN (?1, ?2, ?3, ?4)
             ORDER BY created_at, id",
        )?;
        let value = |index: usize| wanted.get(index).copied().unwrap_or("");
        let records = statement
            .query_map(
                params![value(0), value(1), value(2), value(3)],
                attempt_from_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn bind_attempt_process(
        &mut self,
        attempt_id: &str,
        process_id: u32,
        process_group_id: i32,
    ) -> Result<AttemptRecord> {
        let transaction = self.immediate_transaction()?;
        let current = require_attempt(&transaction, attempt_id)?;
        if current.state.is_terminal() {
            return Err(StoreError::InvalidStateTransition {
                entity: "attempt",
                from: current.state.to_string(),
                to: current.state.to_string(),
            });
        }
        transaction.execute(
            "UPDATE attempts SET process_id = ?2, process_group_id = ?3 WHERE id = ?1",
            params![attempt_id, process_id, process_group_id],
        )?;
        let updated = require_attempt(&transaction, attempt_id)?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn transition_attempt(
        &mut self,
        attempt_id: &str,
        state: AttemptState,
        transitioned_at: &str,
        terminal_reason: Option<&str>,
    ) -> Result<AttemptRecord> {
        self.transition_attempt_with_message(
            attempt_id,
            state,
            transitioned_at,
            terminal_reason,
            None,
        )
    }

    pub fn transition_attempt_with_message(
        &mut self,
        attempt_id: &str,
        state: AttemptState,
        transitioned_at: &str,
        terminal_reason: Option<&str>,
        terminal_message: Option<&str>,
    ) -> Result<AttemptRecord> {
        let transaction = self.immediate_transaction()?;
        let current = require_attempt(&transaction, attempt_id)?;
        if current.state == state {
            transaction.commit()?;
            return Ok(current);
        }
        if !valid_attempt_transition(current.state, state) {
            return Err(StoreError::InvalidStateTransition {
                entity: "attempt",
                from: current.state.to_string(),
                to: state.to_string(),
            });
        }

        let started_at = matches!(state, AttemptState::Running).then_some(transitioned_at);
        let completed_at = state.is_terminal().then_some(transitioned_at);
        transaction.execute(
            "UPDATE attempts
             SET state = ?2,
                 started_at = COALESCE(started_at, ?3),
                 completed_at = ?4,
                 terminal_reason = ?5,
                 terminal_message = ?6
             WHERE id = ?1",
            params![
                attempt_id,
                state.as_str(),
                started_at,
                completed_at,
                terminal_reason,
                terminal_message,
            ],
        )?;

        let job_state = state.job_state();
        transaction.execute(
            "UPDATE jobs
             SET state = ?2,
                 completed_at = CASE WHEN ?3 THEN ?4 ELSE NULL END,
                 terminal_reason = CASE WHEN ?3 THEN ?5 ELSE NULL END,
                 updated_at = ?6
             WHERE id = ?1 AND current_attempt_id = ?7",
            params![
                current.job_id,
                job_state.as_str(),
                job_state.is_terminal(),
                completed_at,
                terminal_reason,
                transitioned_at,
                attempt_id,
            ],
        )?;
        let updated = require_attempt(&transaction, attempt_id)?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn put_log_schema(&mut self, new: NewLogSchema) -> Result<Admission<LogSchemaRecord>> {
        let schema_digest = digest(&new.schema_bytes);
        let transaction = self.immediate_transaction()?;
        if let Some(existing) = query_log_schema(&transaction, &new.id)? {
            if same_log_schema(&existing, &new, schema_digest) {
                transaction.commit()?;
                return Ok(Admission::Existing(existing));
            }
            return Err(StoreError::LogSchemaConflict(new.id));
        }
        transaction.execute(
            "INSERT INTO log_schemas (
                id, name, version, media_type, producer, producer_version,
                schema_bytes, schema_digest, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                new.id,
                new.name,
                new.version,
                new.media_type,
                new.producer,
                new.producer_version,
                new.schema_bytes,
                schema_digest.as_slice(),
                new.created_at,
            ],
        )?;
        let record =
            query_log_schema(&transaction, &new.id)?.ok_or_else(|| StoreError::NotFound {
                entity: "log schema",
                id: new.id.clone(),
            })?;
        transaction.commit()?;
        Ok(Admission::Created(record))
    }

    pub fn get_log_schema(&self, id: &str) -> Result<Option<LogSchemaRecord>> {
        query_log_schema(&self.connection, id)
    }

    pub fn append_log(&mut self, new: NewLogRecord) -> Result<LogRecord> {
        let transaction = self.immediate_transaction()?;
        let record = append_log_in_transaction(&transaction, &new)?;
        transaction.commit()?;
        Ok(record)
    }

    /// Appends all records atomically. Sequence numbers are assigned in input
    /// order independently for each job.
    pub fn append_logs(&mut self, records: &[NewLogRecord]) -> Result<Vec<LogRecord>> {
        let transaction = self.immediate_transaction()?;
        let appended = records
            .iter()
            .map(|record| append_log_in_transaction(&transaction, record))
            .collect::<Result<Vec<_>>>()?;
        transaction.commit()?;
        Ok(appended)
    }

    pub fn list_logs(&self, job_id: &str, after: u64, limit: usize) -> Result<Vec<LogRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT
                job_id, attempt_id, sequence, observed_at, emitted_at, stream,
                schema_id, payload, payload_digest
             FROM log_records
             WHERE job_id = ?1 AND sequence > ?2
             ORDER BY sequence
             LIMIT ?3",
        )?;
        let records = statement
            .query_map(
                params![job_id, u64_to_i64(after)?, usize_to_i64(limit)],
                log_from_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn register_toolset(&mut self, new: NewToolset) -> Result<Admission<ToolsetRecord>> {
        let definitions_digest = digest(&new.definitions_bytes);
        let transaction = self.immediate_transaction()?;
        if let Some(existing) = query_toolset(&transaction, &new.provider, &new.name, new.version)?
        {
            if same_toolset(&existing, &new, definitions_digest) {
                transaction.commit()?;
                return Ok(Admission::Existing(existing));
            }
            return Err(StoreError::ToolsetConflict {
                provider: new.provider,
                name: new.name,
                version: new.version,
            });
        }
        transaction.execute(
            "INSERT INTO toolsets (
                provider, name, version, definitions_schema_id, definitions_bytes,
                definitions_digest, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                new.provider,
                new.name,
                new.version,
                new.definitions_schema_id,
                new.definitions_bytes,
                definitions_digest.as_slice(),
                new.created_at,
            ],
        )?;
        let record = query_toolset(&transaction, &new.provider, &new.name, new.version)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "toolset",
                id: format!("{}/{}@{}", new.provider, new.name, new.version),
            })?;
        transaction.commit()?;
        Ok(Admission::Created(record))
    }

    pub fn get_toolset(
        &self,
        provider: &str,
        name: &str,
        version: u32,
    ) -> Result<Option<ToolsetRecord>> {
        query_toolset(&self.connection, provider, name, version)
    }

    /// Atomically appends the raw harness record and creates its durable tool
    /// call mailbox projection.
    pub fn record_pending_tool_call(
        &mut self,
        new: NewPendingToolCall,
        raw_record: NewLogRecord,
    ) -> Result<Admission<PendingToolCallRecord>> {
        ensure_tool_call_log_matches(&new, &raw_record)?;
        let transaction = self.immediate_transaction()?;
        if let Some(existing) = query_tool_call(&transaction, &new.job_id, &new.id)? {
            if same_pending_tool_call(&existing, &new) {
                transaction.commit()?;
                return Ok(Admission::Existing(existing));
            }
            return Err(StoreError::ToolCallConflict {
                job_id: new.job_id,
                call_id: new.id,
            });
        }
        let log = append_log_in_transaction(&transaction, &raw_record)?;
        insert_pending_tool_call(&transaction, &new, log.sequence)?;
        let record = require_tool_call(&transaction, &new.job_id, &new.id)?;
        transaction.commit()?;
        Ok(Admission::Created(record))
    }

    /// Creates a mailbox projection for a raw record that was already
    /// appended. Prefer [`Store::record_pending_tool_call`] when possible.
    pub fn create_pending_tool_call(
        &mut self,
        new: NewPendingToolCall,
        request_sequence: u64,
    ) -> Result<Admission<PendingToolCallRecord>> {
        let transaction = self.immediate_transaction()?;
        if let Some(existing) = query_tool_call(&transaction, &new.job_id, &new.id)? {
            if same_pending_tool_call(&existing, &new)
                && existing.request_sequence == request_sequence
            {
                transaction.commit()?;
                return Ok(Admission::Existing(existing));
            }
            return Err(StoreError::ToolCallConflict {
                job_id: new.job_id,
                call_id: new.id,
            });
        }
        insert_pending_tool_call(&transaction, &new, request_sequence)?;
        let record = require_tool_call(&transaction, &new.job_id, &new.id)?;
        transaction.commit()?;
        Ok(Admission::Created(record))
    }

    pub fn get_tool_call(
        &self,
        job_id: &str,
        call_id: &str,
    ) -> Result<Option<PendingToolCallRecord>> {
        query_tool_call(&self.connection, job_id, call_id)
    }

    pub fn list_pending_tool_calls(
        &self,
        job_id: &str,
        after_request_sequence: u64,
        limit: usize,
    ) -> Result<Vec<PendingToolCallRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT
                id, job_id, attempt_id, state, tool_name, arguments_schema_id,
                arguments_bytes, arguments_digest, request_sequence,
                result_schema_id, result_bytes, result_digest, result_is_error,
                result_sequence, created_at, answered_at
             FROM pending_tool_calls
             WHERE job_id = ?1 AND state = 'pending' AND request_sequence > ?2
             ORDER BY request_sequence
             LIMIT ?3",
        )?;
        let records = statement
            .query_map(
                params![
                    job_id,
                    u64_to_i64(after_request_sequence)?,
                    usize_to_i64(limit),
                ],
                tool_call_from_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(records)
    }

    /// Atomically appends the raw requester response and marks the mailbox item
    /// answered. Retrying the same result is idempotent and appends no duplicate
    /// record.
    pub fn answer_tool_call_with_log(
        &mut self,
        job_id: &str,
        call_id: &str,
        result: NewToolResult,
        raw_record: NewLogRecord,
    ) -> Result<Admission<PendingToolCallRecord>> {
        if raw_record.job_id != job_id {
            return Err(StoreError::ToolCallLogMismatch {
                job_id: job_id.to_owned(),
                attempt_id: raw_record.attempt_id.unwrap_or_default(),
            });
        }
        let result_digest = digest(&result.result_bytes);
        let transaction = self.immediate_transaction()?;
        let current = require_tool_call(&transaction, job_id, call_id)?;
        if raw_record.attempt_id.as_deref() != Some(current.attempt_id.as_str()) {
            return Err(StoreError::ToolCallLogMismatch {
                job_id: job_id.to_owned(),
                attempt_id: current.attempt_id,
            });
        }
        if current.state == ToolCallState::Answered {
            if same_tool_result(&current, &result, result_digest) {
                transaction.commit()?;
                return Ok(Admission::Existing(current));
            }
            return Err(StoreError::ToolResultConflict {
                job_id: job_id.to_owned(),
                call_id: call_id.to_owned(),
            });
        }
        let log = append_log_in_transaction(&transaction, &raw_record)?;
        update_tool_call_answer(&transaction, job_id, call_id, &result, log.sequence)?;
        let updated = require_tool_call(&transaction, job_id, call_id)?;
        transaction.commit()?;
        Ok(Admission::Created(updated))
    }

    /// Marks a mailbox item answered using a raw record that was already
    /// appended. Prefer [`Store::answer_tool_call_with_log`] when possible.
    pub fn answer_tool_call(
        &mut self,
        job_id: &str,
        call_id: &str,
        result: NewToolResult,
        result_sequence: u64,
    ) -> Result<Admission<PendingToolCallRecord>> {
        let result_digest = digest(&result.result_bytes);
        let transaction = self.immediate_transaction()?;
        let current = require_tool_call(&transaction, job_id, call_id)?;
        if current.state == ToolCallState::Answered {
            if same_tool_result(&current, &result, result_digest)
                && current.result_sequence == Some(result_sequence)
            {
                transaction.commit()?;
                return Ok(Admission::Existing(current));
            }
            return Err(StoreError::ToolResultConflict {
                job_id: job_id.to_owned(),
                call_id: call_id.to_owned(),
            });
        }
        update_tool_call_answer(&transaction, job_id, call_id, &result, result_sequence)?;
        let updated = require_tool_call(&transaction, job_id, call_id)?;
        transaction.commit()?;
        Ok(Admission::Created(updated))
    }

    fn immediate_transaction(&mut self) -> Result<Transaction<'_>> {
        Ok(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }
}

fn digest(bytes: &[u8]) -> Digest {
    Sha256::digest(bytes).into()
}

fn same_job_request(existing: &JobRecord, new: &NewJob, request_digest: Digest) -> bool {
    existing.label == new.label
        && existing.requester_program == new.requester_program
        && existing.requester_id == new.requester_id
        && existing.parent_job_id == new.parent_job_id
        && existing.request_schema_id == new.request_schema_id
        && existing.request_digest == request_digest
        && existing.request_bytes == new.request_bytes
}

fn same_log_schema(existing: &LogSchemaRecord, new: &NewLogSchema, schema_digest: Digest) -> bool {
    existing.name == new.name
        && existing.version == new.version
        && existing.media_type == new.media_type
        && existing.producer == new.producer
        && existing.producer_version == new.producer_version
        && existing.schema_digest == schema_digest
        && existing.schema_bytes == new.schema_bytes
}

fn same_toolset(existing: &ToolsetRecord, new: &NewToolset, definitions_digest: Digest) -> bool {
    existing.definitions_schema_id == new.definitions_schema_id
        && existing.definitions_digest == definitions_digest
        && existing.definitions_bytes == new.definitions_bytes
}

fn same_pending_tool_call(existing: &PendingToolCallRecord, new: &NewPendingToolCall) -> bool {
    existing.job_id == new.job_id
        && existing.attempt_id == new.attempt_id
        && existing.tool_name == new.tool_name
        && existing.arguments_schema_id == new.arguments_schema_id
        && existing.arguments_digest == digest(&new.arguments_bytes)
        && existing.arguments_bytes == new.arguments_bytes
}

fn same_tool_result(
    existing: &PendingToolCallRecord,
    new: &NewToolResult,
    result_digest: Digest,
) -> bool {
    existing.result_schema_id.as_deref() == Some(new.schema_id.as_str())
        && existing.result_digest == Some(result_digest)
        && existing.result_bytes.as_deref() == Some(new.result_bytes.as_slice())
        && existing.result_is_error == Some(new.is_error)
}

fn valid_attempt_transition(from: AttemptState, to: AttemptState) -> bool {
    match from {
        AttemptState::Pending => matches!(
            to,
            AttemptState::Starting
                | AttemptState::Running
                | AttemptState::Failed
                | AttemptState::Cancelled
                | AttemptState::Lost
        ),
        AttemptState::Starting => matches!(
            to,
            AttemptState::Running
                | AttemptState::Failed
                | AttemptState::Cancelled
                | AttemptState::TimedOut
                | AttemptState::Lost
        ),
        AttemptState::Running => matches!(
            to,
            AttemptState::WaitingOnRequester
                | AttemptState::Completed
                | AttemptState::Failed
                | AttemptState::Cancelled
                | AttemptState::TimedOut
                | AttemptState::Lost
        ),
        AttemptState::WaitingOnRequester => matches!(
            to,
            AttemptState::Running
                | AttemptState::Completed
                | AttemptState::Failed
                | AttemptState::Cancelled
                | AttemptState::TimedOut
                | AttemptState::Lost
        ),
        AttemptState::Completed
        | AttemptState::Failed
        | AttemptState::Cancelled
        | AttemptState::TimedOut
        | AttemptState::Lost => false,
    }
}

fn query_job(connection: &Connection, id: &str) -> Result<Option<JobRecord>> {
    let record = connection
        .query_row(
            "SELECT
                id, label, requester_program, requester_id, parent_job_id,
                request_schema_id, request_bytes, request_digest, state,
                current_attempt_id, cancellation_requested_at, created_at,
                updated_at, completed_at, terminal_reason
             FROM jobs
             WHERE id = ?1",
            [id],
            job_from_row,
        )
        .optional()?;
    Ok(record)
}

fn require_job(connection: &Connection, id: &str) -> Result<JobRecord> {
    query_job(connection, id)?.ok_or_else(|| StoreError::NotFound {
        entity: "job",
        id: id.to_owned(),
    })
}

fn job_from_row(row: &Row<'_>) -> rusqlite::Result<JobRecord> {
    Ok(JobRecord {
        id: row.get(0)?,
        label: row.get(1)?,
        requester_program: row.get(2)?,
        requester_id: row.get(3)?,
        parent_job_id: row.get(4)?,
        request_schema_id: row.get(5)?,
        request_bytes: row.get(6)?,
        request_digest: digest_from_row(row, 7)?,
        state: enum_from_row(row, 8)?,
        current_attempt_id: row.get(9)?,
        cancellation_requested_at: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        completed_at: row.get(13)?,
        terminal_reason: row.get(14)?,
    })
}

fn query_attempt(connection: &Connection, id: &str) -> Result<Option<AttemptRecord>> {
    let record = connection
        .query_row(
            "SELECT
                id, job_id, ordinal, harness, harness_version, adapter_version,
                state, process_id, process_group_id, created_at, started_at,
                completed_at, terminal_reason, terminal_message
             FROM attempts
             WHERE id = ?1",
            [id],
            attempt_from_row,
        )
        .optional()?;
    Ok(record)
}

fn require_attempt(connection: &Connection, id: &str) -> Result<AttemptRecord> {
    query_attempt(connection, id)?.ok_or_else(|| StoreError::NotFound {
        entity: "attempt",
        id: id.to_owned(),
    })
}

fn attempt_from_row(row: &Row<'_>) -> rusqlite::Result<AttemptRecord> {
    let ordinal: i64 = row.get(2)?;
    let process_id: Option<i64> = row.get(7)?;
    let process_group_id: Option<i64> = row.get(8)?;
    Ok(AttemptRecord {
        id: row.get(0)?,
        job_id: row.get(1)?,
        ordinal: u32::try_from(ordinal).map_err(|error| integer_conversion_error(2, error))?,
        harness: row.get(3)?,
        harness_version: row.get(4)?,
        adapter_version: row.get(5)?,
        state: enum_from_row(row, 6)?,
        process_id: process_id
            .map(u32::try_from)
            .transpose()
            .map_err(|error| integer_conversion_error(7, error))?,
        process_group_id: process_group_id
            .map(i32::try_from)
            .transpose()
            .map_err(|error| integer_conversion_error(8, error))?,
        created_at: row.get(9)?,
        started_at: row.get(10)?,
        completed_at: row.get(11)?,
        terminal_reason: row.get(12)?,
        terminal_message: row.get(13)?,
    })
}

fn query_log_schema(connection: &Connection, id: &str) -> Result<Option<LogSchemaRecord>> {
    let record = connection
        .query_row(
            "SELECT
                id, name, version, media_type, producer, producer_version,
                schema_bytes, schema_digest, created_at
             FROM log_schemas
             WHERE id = ?1",
            [id],
            log_schema_from_row,
        )
        .optional()?;
    Ok(record)
}

fn log_schema_from_row(row: &Row<'_>) -> rusqlite::Result<LogSchemaRecord> {
    Ok(LogSchemaRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        media_type: row.get(3)?,
        producer: row.get(4)?,
        producer_version: row.get(5)?,
        schema_bytes: row.get(6)?,
        schema_digest: digest_from_row(row, 7)?,
        created_at: row.get(8)?,
    })
}

fn append_log_in_transaction(
    transaction: &Transaction<'_>,
    new: &NewLogRecord,
) -> Result<LogRecord> {
    if let Some(attempt_id) = &new.attempt_id {
        let attempt = require_attempt(transaction, attempt_id)?;
        if attempt.job_id != new.job_id {
            return Err(StoreError::AttemptJobMismatch {
                job_id: new.job_id.clone(),
                attempt_id: attempt_id.clone(),
            });
        }
    }
    let sequence: i64 = transaction.query_row(
        "UPDATE jobs
         SET next_log_sequence = next_log_sequence + 1
         WHERE id = ?1
         RETURNING next_log_sequence - 1",
        [&new.job_id],
        |row| row.get(0),
    )?;
    let payload_digest = digest(&new.payload);
    transaction.execute(
        "INSERT INTO log_records (
            job_id, attempt_id, sequence, observed_at, emitted_at, stream,
            schema_id, payload, payload_digest
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            new.job_id,
            new.attempt_id,
            sequence,
            new.observed_at,
            new.emitted_at,
            new.stream,
            new.schema_id,
            new.payload,
            payload_digest.as_slice(),
        ],
    )?;
    Ok(LogRecord {
        job_id: new.job_id.clone(),
        attempt_id: new.attempt_id.clone(),
        sequence: u64::try_from(sequence)
            .map_err(|error| integer_conversion_store_error("log sequence", error))?,
        observed_at: new.observed_at.clone(),
        emitted_at: new.emitted_at.clone(),
        stream: new.stream.clone(),
        schema_id: new.schema_id.clone(),
        payload: new.payload.clone(),
        payload_digest,
    })
}

fn log_from_row(row: &Row<'_>) -> rusqlite::Result<LogRecord> {
    let sequence: i64 = row.get(2)?;
    Ok(LogRecord {
        job_id: row.get(0)?,
        attempt_id: row.get(1)?,
        sequence: u64::try_from(sequence).map_err(|error| integer_conversion_error(2, error))?,
        observed_at: row.get(3)?,
        emitted_at: row.get(4)?,
        stream: row.get(5)?,
        schema_id: row.get(6)?,
        payload: row.get(7)?,
        payload_digest: digest_from_row(row, 8)?,
    })
}

fn query_toolset(
    connection: &Connection,
    provider: &str,
    name: &str,
    version: u32,
) -> Result<Option<ToolsetRecord>> {
    let record = connection
        .query_row(
            "SELECT
                provider, name, version, definitions_schema_id,
                definitions_bytes, definitions_digest, created_at
             FROM toolsets
             WHERE provider = ?1 AND name = ?2 AND version = ?3",
            params![provider, name, version],
            toolset_from_row,
        )
        .optional()?;
    Ok(record)
}

fn toolset_from_row(row: &Row<'_>) -> rusqlite::Result<ToolsetRecord> {
    let version: i64 = row.get(2)?;
    Ok(ToolsetRecord {
        provider: row.get(0)?,
        name: row.get(1)?,
        version: u32::try_from(version).map_err(|error| integer_conversion_error(2, error))?,
        definitions_schema_id: row.get(3)?,
        definitions_bytes: row.get(4)?,
        definitions_digest: digest_from_row(row, 5)?,
        created_at: row.get(6)?,
    })
}

fn ensure_tool_call_log_matches(new: &NewPendingToolCall, log: &NewLogRecord) -> Result<()> {
    if log.job_id == new.job_id && log.attempt_id.as_deref() == Some(new.attempt_id.as_str()) {
        Ok(())
    } else {
        Err(StoreError::ToolCallLogMismatch {
            job_id: new.job_id.clone(),
            attempt_id: new.attempt_id.clone(),
        })
    }
}

fn insert_pending_tool_call(
    transaction: &Transaction<'_>,
    new: &NewPendingToolCall,
    request_sequence: u64,
) -> Result<()> {
    let arguments_digest = digest(&new.arguments_bytes);
    transaction.execute(
        "INSERT INTO pending_tool_calls (
            job_id, id, attempt_id, state, tool_name, arguments_schema_id,
            arguments_bytes, arguments_digest, request_sequence, created_at
         ) VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            new.job_id,
            new.id,
            new.attempt_id,
            new.tool_name,
            new.arguments_schema_id,
            new.arguments_bytes,
            arguments_digest.as_slice(),
            u64_to_i64(request_sequence)?,
            new.created_at,
        ],
    )?;
    Ok(())
}

fn update_tool_call_answer(
    transaction: &Transaction<'_>,
    job_id: &str,
    call_id: &str,
    result: &NewToolResult,
    result_sequence: u64,
) -> Result<()> {
    let result_digest = digest(&result.result_bytes);
    transaction.execute(
        "UPDATE pending_tool_calls
         SET state = 'answered',
             result_schema_id = ?3,
             result_bytes = ?4,
             result_digest = ?5,
             result_is_error = ?6,
             result_sequence = ?7,
             answered_at = ?8
         WHERE job_id = ?1 AND id = ?2 AND state = 'pending'",
        params![
            job_id,
            call_id,
            result.schema_id,
            result.result_bytes,
            result_digest.as_slice(),
            result.is_error,
            u64_to_i64(result_sequence)?,
            result.answered_at,
        ],
    )?;
    Ok(())
}

fn query_tool_call(
    connection: &Connection,
    job_id: &str,
    call_id: &str,
) -> Result<Option<PendingToolCallRecord>> {
    let record = connection
        .query_row(
            "SELECT
                id, job_id, attempt_id, state, tool_name, arguments_schema_id,
                arguments_bytes, arguments_digest, request_sequence,
                result_schema_id, result_bytes, result_digest, result_is_error,
                result_sequence, created_at, answered_at
             FROM pending_tool_calls
             WHERE job_id = ?1 AND id = ?2",
            params![job_id, call_id],
            tool_call_from_row,
        )
        .optional()?;
    Ok(record)
}

fn require_tool_call(
    connection: &Connection,
    job_id: &str,
    call_id: &str,
) -> Result<PendingToolCallRecord> {
    query_tool_call(connection, job_id, call_id)?.ok_or_else(|| StoreError::NotFound {
        entity: "tool call",
        id: format!("{job_id}/{call_id}"),
    })
}

fn tool_call_from_row(row: &Row<'_>) -> rusqlite::Result<PendingToolCallRecord> {
    let request_sequence: i64 = row.get(8)?;
    let result_digest = optional_digest_from_row(row, 11)?;
    let result_sequence: Option<i64> = row.get(13)?;
    Ok(PendingToolCallRecord {
        id: row.get(0)?,
        job_id: row.get(1)?,
        attempt_id: row.get(2)?,
        state: enum_from_row(row, 3)?,
        tool_name: row.get(4)?,
        arguments_schema_id: row.get(5)?,
        arguments_bytes: row.get(6)?,
        arguments_digest: digest_from_row(row, 7)?,
        request_sequence: u64::try_from(request_sequence)
            .map_err(|error| integer_conversion_error(8, error))?,
        result_schema_id: row.get(9)?,
        result_bytes: row.get(10)?,
        result_digest,
        result_is_error: row.get(12)?,
        result_sequence: result_sequence
            .map(u64::try_from)
            .transpose()
            .map_err(|error| integer_conversion_error(13, error))?,
        created_at: row.get(14)?,
        answered_at: row.get(15)?,
    })
}

fn enum_from_row<T>(row: &Row<'_>, index: usize) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let value: String = row.get(index)?;
    value.parse().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
    })
}

fn digest_from_row(row: &Row<'_>, index: usize) -> rusqlite::Result<Digest> {
    let bytes: Vec<u8> = row.get(index)?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            Type::Blob,
            Box::new(InvalidStoredValue(format!(
                "expected 32-byte digest, found {} bytes",
                bytes.len()
            ))),
        )
    })
}

fn optional_digest_from_row(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<Digest>> {
    let bytes: Option<Vec<u8>> = row.get(index)?;
    bytes
        .map(|bytes| {
            bytes.try_into().map_err(|bytes: Vec<u8>| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    Type::Blob,
                    Box::new(InvalidStoredValue(format!(
                        "expected 32-byte digest, found {} bytes",
                        bytes.len()
                    ))),
                )
            })
        })
        .transpose()
}

fn integer_conversion_error(
    index: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(error))
}

fn integer_conversion_store_error(
    field: &'static str,
    error: impl std::error::Error + Send + Sync + 'static,
) -> StoreError {
    StoreError::Sql(rusqlite::Error::FromSqlConversionFailure(
        0,
        Type::Integer,
        Box::new(InvalidStoredValue(format!("invalid {field}: {error}"))),
    ))
}

fn u64_to_i64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|error| integer_conversion_store_error("u64", error))
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-08-26T12:00:00Z";
    const LATER: &str = "2026-08-26T12:01:00Z";

    fn must_succeed<T>(result: Result<T>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("test operation failed: {error}"),
        }
    }

    fn must_exist<T>(value: Option<T>) -> T {
        match value {
            Some(value) => value,
            None => panic!("expected a stored record"),
        }
    }

    fn schema(id: &str) -> NewLogSchema {
        NewLogSchema {
            id: id.to_owned(),
            name: id.to_owned(),
            version: "1".to_owned(),
            media_type: "application/schema+json".to_owned(),
            producer: "test".to_owned(),
            producer_version: Some("1.0.0".to_owned()),
            schema_bytes: br#"{"type":"object"}"#.to_vec(),
            created_at: NOW.to_owned(),
        }
    }

    fn job(id: &str, requester_id: &str) -> NewJob {
        NewJob {
            id: id.to_owned(),
            label: format!("Job {id}"),
            requester_program: "annals".to_owned(),
            requester_id: requester_id.to_owned(),
            parent_job_id: None,
            request_schema_id: "request.v1".to_owned(),
            request_bytes: format!(r#"{{"id":"{id}"}}"#).into_bytes(),
            created_at: NOW.to_owned(),
        }
    }

    fn attempt(job_id: &str) -> NewAttempt {
        NewAttempt {
            id: format!("attempt-{job_id}"),
            job_id: job_id.to_owned(),
            ordinal: 1,
            harness: "codex-app-server".to_owned(),
            harness_version: "1.0.0".to_owned(),
            adapter_version: "1".to_owned(),
            created_at: NOW.to_owned(),
        }
    }

    fn prepared_store() -> Store {
        let mut store = must_succeed(Store::open_in_memory());
        must_succeed(store.put_log_schema(schema("request.v1")));
        must_succeed(store.put_log_schema(schema("harness.output.v1")));
        must_succeed(store.put_log_schema(schema("tool.arguments.v1")));
        must_succeed(store.put_log_schema(schema("tool.result.v1")));
        store
    }

    #[test]
    fn job_admission_is_exactly_idempotent() {
        let mut store = prepared_store();
        let request = job("job-1", "run-1");

        assert!(must_succeed(store.admit_job(request.clone())).was_created());
        let existing = must_succeed(store.admit_job(request.clone()));
        assert!(!existing.was_created());
        assert_eq!(existing.into_inner().request_bytes, request.request_bytes);

        let mut conflict = request;
        conflict.label = "different".to_owned();
        assert!(matches!(
            store.admit_job(conflict),
            Err(StoreError::JobConflict(id)) if id == "job-1"
        ));
    }

    #[test]
    fn requester_lookup_does_not_project_requester_domain_data() {
        let mut store = prepared_store();
        must_succeed(store.admit_job(job("job-1", "run-1")));
        must_succeed(store.admit_job(job("job-2", "run-1")));
        must_succeed(store.admit_job(job("job-3", "run-2")));

        let jobs = must_succeed(store.list_jobs_by_requester("annals", "run-1"));
        assert_eq!(
            jobs.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(),
            ["job-1", "job-2"]
        );
    }

    #[test]
    fn raw_log_bytes_are_opaque_and_schema_bound() {
        let mut store = prepared_store();
        must_succeed(store.admit_job(job("job-1", "run-1")));
        let raw = vec![b'{', b' ', 0xff, b'\n'];

        let appended = must_succeed(store.append_log(NewLogRecord {
            job_id: "job-1".to_owned(),
            attempt_id: None,
            observed_at: NOW.to_owned(),
            emitted_at: None,
            stream: "harness.output".to_owned(),
            schema_id: "harness.output.v1".to_owned(),
            payload: raw.clone(),
        }));

        assert_eq!(appended.sequence, 1);
        assert_eq!(appended.schema_id, "harness.output.v1");
        assert_eq!(appended.payload, raw);
        assert_eq!(appended.payload_digest, digest(&raw));
        assert_eq!(must_succeed(store.list_logs("job-1", 0, 10)), [appended]);
    }

    #[test]
    fn schemas_and_toolsets_are_immutable_and_byte_exact() {
        let mut store = prepared_store();
        let stored = must_exist(must_succeed(store.get_log_schema("request.v1")));
        assert_eq!(stored.schema_bytes, br#"{"type":"object"}"#);

        let toolset = NewToolset {
            provider: "annals".to_owned(),
            name: "liaison".to_owned(),
            version: 4,
            definitions_schema_id: "tool.arguments.v1".to_owned(),
            definitions_bytes: b"[ { \"name\": \"submit\" } ]\n".to_vec(),
            created_at: NOW.to_owned(),
        };
        assert!(must_succeed(store.register_toolset(toolset.clone())).was_created());
        assert!(!must_succeed(store.register_toolset(toolset.clone())).was_created());
        let stored = must_exist(must_succeed(store.get_toolset("annals", "liaison", 4)));
        assert_eq!(stored.definitions_bytes, toolset.definitions_bytes);
    }

    #[test]
    fn tool_call_mailbox_and_raw_logs_commit_together() {
        let mut store = prepared_store();
        must_succeed(store.admit_job(job("job-1", "run-1")));
        let created_attempt = must_succeed(store.create_attempt(attempt("job-1")));
        must_succeed(store.transition_attempt(
            &created_attempt.id,
            AttemptState::Running,
            NOW,
            None,
        ));

        let call = NewPendingToolCall {
            id: "call-1".to_owned(),
            job_id: "job-1".to_owned(),
            attempt_id: created_attempt.id.clone(),
            tool_name: "submit_reconciliation".to_owned(),
            arguments_schema_id: "tool.arguments.v1".to_owned(),
            arguments_bytes: b" {\"draft\":7}\n".to_vec(),
            created_at: NOW.to_owned(),
        };
        let call_log = NewLogRecord {
            job_id: "job-1".to_owned(),
            attempt_id: Some(created_attempt.id.clone()),
            observed_at: NOW.to_owned(),
            emitted_at: None,
            stream: "harness.output".to_owned(),
            schema_id: "harness.output.v1".to_owned(),
            payload: b"{\"method\":\"item/tool/call\"}\n".to_vec(),
        };
        let pending = must_succeed(store.record_pending_tool_call(call.clone(), call_log));
        assert!(pending.was_created());
        let pending = pending.into_inner();
        assert_eq!(pending.request_sequence, 1);
        assert_eq!(pending.arguments_bytes, call.arguments_bytes);
        assert_eq!(
            must_succeed(store.list_pending_tool_calls("job-1", 0, 10)),
            std::slice::from_ref(&pending)
        );

        let result = NewToolResult {
            schema_id: "tool.result.v1".to_owned(),
            result_bytes: b"{ \"ok\": true }\n".to_vec(),
            is_error: false,
            answered_at: NOW.to_owned(),
        };
        let result_log = NewLogRecord {
            job_id: "job-1".to_owned(),
            attempt_id: Some(created_attempt.id),
            observed_at: NOW.to_owned(),
            emitted_at: None,
            stream: "requester".to_owned(),
            schema_id: "tool.result.v1".to_owned(),
            payload: result.result_bytes.clone(),
        };
        let answered = must_succeed(store.answer_tool_call_with_log(
            "job-1",
            "call-1",
            result.clone(),
            result_log.clone(),
        ));
        assert!(answered.was_created());
        let answered = answered.into_inner();
        assert_eq!(answered.state, ToolCallState::Answered);
        assert_eq!(answered.result_sequence, Some(2));
        assert_eq!(answered.result_bytes, Some(result.result_bytes.clone()));
        assert_eq!(answered.result_is_error, Some(false));
        assert!(must_succeed(store.list_pending_tool_calls("job-1", 0, 10)).is_empty());

        let retry =
            must_succeed(store.answer_tool_call_with_log("job-1", "call-1", result, result_log));
        assert!(!retry.was_created());
        assert_eq!(must_succeed(store.list_logs("job-1", 0, 10)).len(), 2);
    }

    #[test]
    fn lifecycle_projection_and_recovery_are_explicit() {
        let mut store = prepared_store();
        must_succeed(store.admit_job(job("job-1", "run-1")));
        let attempt = must_succeed(store.create_attempt(attempt("job-1")));
        must_succeed(store.transition_attempt(&attempt.id, AttemptState::Starting, NOW, None));
        must_succeed(store.bind_attempt_process(&attempt.id, 123, 123));

        assert_eq!(must_succeed(store.running_attempts()).len(), 1);
        must_succeed(store.request_cancellation("job-1", NOW));
        must_succeed(store.transition_attempt_with_message(
            &attempt.id,
            AttemptState::Cancelled,
            LATER,
            Some("requested"),
            Some("cancelled by requester"),
        ));
        let attempt = must_exist(must_succeed(store.get_attempt(&attempt.id)));
        assert_eq!(
            attempt.terminal_message.as_deref(),
            Some("cancelled by requester")
        );
        let job = must_exist(must_succeed(store.get_job("job-1")));
        assert_eq!(job.state, JobState::Cancelled);
        assert_eq!(job.cancellation_requested_at.as_deref(), Some(NOW));
        assert_eq!(job.updated_at, LATER);
        assert!(must_succeed(store.running_attempts()).is_empty());
    }
}
