//! Private delivery records and short-lived processing buffers.

use crate::corpus::Corpus;
use crate::{Result, fail};
use chrono::{Datelike, Timelike};
use fs2::FileExt;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const WORK_LIFETIME: i64 = 24 * 60 * 60;
pub const RECEIPT_LIFETIME: i64 = 35 * 24 * 60 * 60;
pub const SEND_WINDOW: i64 = 23 * 60 * 60;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub hour: u32,
    pub minute: u32,
    pub timezone: String,
    pub receiving_domain: String,
    pub email_executable: PathBuf,
    pub paused: bool,
}

impl Config {
    /// Return paused configuration for the current user.
    ///
    /// # Errors
    /// Returns an error when the user's absolute home cannot be resolved.
    pub fn defaults() -> Result<Self> {
        Ok(Self {
            hour: 9,
            minute: 0,
            timezone: "America/Chicago".into(),
            receiving_domain: "woovrunea.resend.app".into(),
            email_executable: crate::home()?.join(".local/bin/email"),
            paused: true,
        })
    }

    /// Check time, zone, mailbox length, domain syntax and executable path.
    ///
    /// # Errors
    /// Returns an error when any configured value is outside its supported range.
    pub fn validate(&self) -> Result<()> {
        if self.hour > 23 || self.minute > 59 || self.timezone.parse::<chrono_tz::Tz>().is_err() {
            return Err(fail("invalid daily time or IANA time zone"));
        }
        if !self.email_executable.is_absolute()
            || self.receiving_domain.len() + "mentor.".len() + 32 + 1 > 254
            || !self.receiving_domain.contains('.')
            || self.receiving_domain.split('.').any(|part| {
                part.is_empty()
                    || part.len() > 63
                    || part.starts_with('-')
                    || part.ends_with('-')
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
        {
            return Err(fail("invalid Email executable or receiving domain"));
        }
        Ok(())
    }

    /// Return the current local date once its configured delivery time is due.
    ///
    /// # Errors
    /// Returns an error for invalid configuration or an unrepresentable timestamp.
    pub fn due_date(&self, now: i64) -> Result<Option<String>> {
        self.validate()?;
        let utc =
            chrono::DateTime::from_timestamp(now, 0).ok_or_else(|| fail("invalid current time"))?;
        let local = utc.with_timezone(&self.timezone.parse::<chrono_tz::Tz>()?);
        if (local.hour(), local.minute()) < (self.hour, self.minute) {
            return Ok(None);
        }
        Ok(Some(format!(
            "{:04}-{:02}-{:02}",
            local.year(),
            local.month(),
            local.day()
        )))
    }
}

#[derive(Clone)]
pub struct Assignment {
    pub date: String,
    pub problem_id: String,
    pub corpus_id: String,
    pub token: String,
    pub reply_to: String,
}

#[derive(Clone)]
pub struct Incoming {
    pub id: String,
    pub assignment_date: String,
    pub state: String,
    pub received_at: i64,
    pub expires_at: i64,
    pub message_id: String,
    pub references: Vec<String>,
    pub answer: Option<String>,
    pub request_json: Option<String>,
    pub job_id: Option<String>,
}

#[derive(Clone)]
pub struct Outgoing {
    pub id: String,
    pub incoming_id: Option<String>,
    pub payload: String,
    pub first_attempt: Option<i64>,
    pub attempts: u32,
    pub expires_at: i64,
}

pub struct Store {
    pub connection: Connection,
}

/// Create or require a private directory at an absolute path.
///
/// # Errors
/// Rejects relative paths, symlinks, nonprivate directories and filesystem errors.
pub fn private_directory(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(fail("state paths must be absolute"));
    }
    match fs::symlink_metadata(path) {
        Ok(meta)
            if meta.is_dir()
                && !meta.file_type().is_symlink()
                && meta.mode().trailing_zeros() >= 6 =>
        {
            Ok(())
        }
        Ok(_) => Err(fail(
            "state directory must be private and must not be a symlink",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true).mode(0o700).create(path)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn private_file(path: &Path, create: bool) -> Result<File> {
    if create && !path.try_exists()? {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(file) => return Ok(file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.nlink() != 1
        || meta.mode() & 0o077 != 0
    {
        return Err(fail("state file must be a private regular file"));
    }
    Ok(OpenOptions::new().read(true).write(true).open(path)?)
}

/// Hold the exclusive worker/administration lock until the returned file drops.
///
/// # Errors
/// Returns an error for unsafe state paths, filesystem failure or a busy lock.
pub fn runner_lock(root: &Path) -> Result<File> {
    private_directory(root)?;
    let file = private_file(&root.join("runner.lock"), true)?;
    file.try_lock_exclusive()
        .map_err(|_| fail("another Mentor operation is active"))?;
    Ok(file)
}

impl Store {
    /// Open existing supported state or atomically initialize paused schema one.
    ///
    /// # Errors
    /// Rejects unsafe files, unsupported or unversioned nonempty state, invalid
    /// bundled content, and filesystem or database failures.
    pub fn initialize(root: &Path) -> Result<Self> {
        private_directory(root)?;
        let path = root.join("mentor.sqlite3");
        let _file = private_file(&path, true)?;
        let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let store = Self { connection };
        store.configure_connection()?;
        let version: i64 = store
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == 0 {
            let tables: i64 = store.connection.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |row| row.get(0),
            )?;
            if tables != 0 {
                return Err(fail(
                    "unversioned nonempty database is not a Mentor database",
                ));
            }
            let corpus = Corpus::bundled()?;
            let corpus_id = corpus.digest()?;
            let config = serde_json::to_string(&Config::defaults()?)?;
            let seed = crate::random_token()?;
            let transaction = store.connection.unchecked_transaction()?;
            transaction.execute_batch(SCHEMA)?;
            transaction.execute(
                "INSERT INTO corpora(id,body) VALUES(?1,?2)",
                params![corpus_id, serde_json::to_string(&corpus)?],
            )?;
            for (key, value) in [
                ("config", config),
                ("shuffle_seed", seed),
                ("active_corpus", corpus_id),
            ] {
                transaction.execute(
                    "INSERT INTO metadata(key,value) VALUES(?1,?2)",
                    params![key, value],
                )?;
            }
            transaction.commit()?;
        } else if version != 1 {
            return Err(fail("unsupported Mentor database version"));
        }
        Ok(store)
    }

    /// Open an existing private schema-one database.
    ///
    /// # Errors
    /// Returns an error for absent, unsafe or unsupported state and database errors.
    pub fn open(root: &Path) -> Result<Self> {
        private_directory(root)?;
        let path = root.join("mentor.sqlite3");
        let _file = private_file(&path, false)?;
        let store = Self {
            connection: Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?,
        };
        store.configure_connection()?;
        let version: i64 = store
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != 1 {
            return Err(fail("initialize a supported Mentor database first"));
        }
        Ok(store)
    }

    fn configure_connection(&self) -> Result<()> {
        self.connection.busy_timeout(Duration::from_secs(5))?;
        self.connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON; PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA temp_store=MEMORY;")?;
        Ok(())
    }

    /// Read one operational metadata value.
    ///
    /// # Errors
    /// Returns database read or value-decoding errors.
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row("SELECT value FROM metadata WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Replace one operational metadata value.
    ///
    /// # Errors
    /// Returns database write errors.
    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.connection.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
        Ok(())
    }

    /// Read and validate the persisted configuration.
    ///
    /// # Errors
    /// Returns an error for absent or invalid configuration and database errors.
    pub fn config(&self) -> Result<Config> {
        let config: Config = serde_json::from_str(
            &self
                .meta("config")?
                .ok_or_else(|| fail("Mentor configuration is absent"))?,
        )?;
        config.validate()?;
        Ok(config)
    }

    /// Validate and replace the persisted configuration.
    ///
    /// # Errors
    /// Returns validation, serialization or database write errors.
    pub fn set_config(&self, config: &Config) -> Result<()> {
        config.validate()?;
        self.set_meta("config", &serde_json::to_string(config)?)
    }

    /// Retain a validated immutable collection and make its digest active.
    ///
    /// # Errors
    /// Returns corpus validation, serialization or database transaction errors.
    pub fn import_corpus(&self, corpus: &Corpus) -> Result<String> {
        let id = corpus.digest()?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT OR IGNORE INTO corpora(id,body) VALUES(?1,?2)",
            params![id, serde_json::to_string(corpus)?],
        )?;
        transaction.execute("INSERT INTO metadata(key,value) VALUES('active_corpus',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [&id])?;
        transaction.commit()?;
        Ok(id)
    }

    /// Read one retained collection by its content digest.
    ///
    /// # Errors
    /// Returns an error for an absent or invalid collection and database errors.
    pub fn corpus(&self, id: &str) -> Result<Corpus> {
        let body: String =
            self.connection
                .query_row("SELECT body FROM corpora WHERE id=?1", [id], |row| {
                    row.get(0)
                })?;
        Corpus::from_json(&body)
    }

    /// Read the active collection and its digest.
    ///
    /// # Errors
    /// Returns an error for an absent selector, invalid content or database failure.
    pub fn active_corpus(&self) -> Result<(String, Corpus)> {
        let id = self
            .meta("active_corpus")?
            .ok_or_else(|| fail("no active problem collection"))?;
        let corpus = self.corpus(&id)?;
        Ok((id, corpus))
    }

    /// Find the original assignment associated with an opaque reply token.
    ///
    /// # Errors
    /// Returns database read or record-decoding errors.
    pub fn assignment(&self, token: &str) -> Result<Option<Assignment>> {
        Ok(self
            .connection
            .query_row(
                "SELECT date,problem_id,corpus_id,token,reply_to FROM assignments WHERE token=?1",
                [token],
                assignment_row,
            )
            .optional()?)
    }

    /// Read the assignment reserved for one local calendar date.
    ///
    /// # Errors
    /// Returns an error when the assignment is absent or its database read fails.
    pub fn assignment_date(&self, date: &str) -> Result<Assignment> {
        Ok(self.connection.query_row(
            "SELECT date,problem_id,corpus_id,token,reply_to FROM assignments WHERE date=?1",
            [date],
            assignment_row,
        )?)
    }

    /// Report whether the supplied local date already has a reservation.
    ///
    /// # Errors
    /// Returns database read errors.
    pub fn assigned_today(&self, date: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM assignments WHERE date=?1)",
            [date],
            |row| row.get(0),
        )?)
    }

    /// Read problem identities already reserved across every retained collection.
    ///
    /// # Errors
    /// Returns database read or record-decoding errors.
    pub fn used_problem_ids(&self) -> Result<std::collections::HashSet<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT problem_id FROM assignments")?;
        Ok(statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Commit one assignment and its exact pending email together.
    ///
    /// # Errors
    /// Returns duplicate identity, missing corpus or database transaction errors.
    pub fn reserve_assignment(
        &self,
        assignment: &Assignment,
        payload: &str,
        now: i64,
    ) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute("INSERT INTO assignments(date,problem_id,corpus_id,token,reply_to,created_at) VALUES(?1,?2,?3,?4,?5,?6)", params![assignment.date,assignment.problem_id,assignment.corpus_id,assignment.token,assignment.reply_to,now])?;
        transaction.execute("INSERT INTO outbox(id,payload,state,created_at,expires_at,next_attempt) VALUES(?1,?2,'pending',?3,?4,?3)", params![format!("problem/{}",assignment.date),payload,now,now+WORK_LIFETIME])?;
        transaction.commit()?;
        Ok(())
    }

    /// Report whether an incoming provider ID has already been captured.
    ///
    /// # Errors
    /// Returns database read errors.
    pub fn seen(&self, id: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM incoming WHERE id=?1)",
            [id],
            |row| row.get(0),
        )?)
    }

    /// Capture an unseen reply and optional extraction guidance atomically.
    ///
    /// # Errors
    /// Returns reference serialization or database transaction errors.
    pub fn capture(&self, incoming: &Incoming, now: i64, response: Option<&str>) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        let inserted = transaction.execute("INSERT OR IGNORE INTO incoming(id,assignment_date,state,received_at,ingested_at,expires_at,message_id,references_json,answer) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![incoming.id,incoming.assignment_date,incoming.state,incoming.received_at,now,incoming.expires_at,incoming.message_id,serde_json::to_string(&incoming.references)?,incoming.answer])?;
        if let Some(payload) = response.filter(|_| inserted == 1) {
            transaction.execute("INSERT INTO outbox(id,incoming_id,payload,state,created_at,expires_at,next_attempt) VALUES(?1,?2,?3,'pending',?4,?5,?4)", params![format!("critique/{}",incoming.id),incoming.id,payload,now,incoming.expires_at])?;
            transaction.execute(
                "UPDATE incoming SET state='responding',answer=NULL WHERE id=?1",
                [&incoming.id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Read up to twenty unfinished answers, with active grading first.
    ///
    /// # Errors
    /// Returns database read or record-decoding errors.
    pub fn pending(&self) -> Result<Vec<Incoming>> {
        let mut statement = self.connection.prepare("SELECT id,assignment_date,state,received_at,expires_at,message_id,references_json,answer,request_json,job_id FROM incoming WHERE state IN ('pending','grading') ORDER BY (state='grading') DESC,received_at,id LIMIT 20")?;
        Ok(statement
            .query_map([], incoming_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Replace a pending answer buffer with its exact immutable grading request.
    ///
    /// # Errors
    /// Returns database write errors.
    pub fn start_grading(&self, id: &str, job_id: &str, request: &str) -> Result<()> {
        self.connection.execute("UPDATE incoming SET state='grading',job_id=?2,request_json=?3,answer=NULL WHERE id=?1 AND state='pending'", params![id,job_id,request])?;
        Ok(())
    }

    /// Commit a response and clear its answer/request buffers together.
    ///
    /// # Errors
    /// Returns database transaction errors.
    pub fn queue_response(&self, incoming: &Incoming, payload: &str, now: i64) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute("INSERT OR IGNORE INTO outbox(id,incoming_id,payload,state,created_at,expires_at,next_attempt) VALUES(?1,?2,?3,'pending',?4,?5,?4)", params![format!("critique/{}",incoming.id),incoming.id,payload,now,incoming.expires_at])?;
        transaction.execute(
            "UPDATE incoming SET state='responding',answer=NULL,request_json=NULL WHERE id=?1",
            [&incoming.id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Mark a reply failed, purge its content and retain any cancellation duty.
    ///
    /// # Errors
    /// Returns database write errors.
    pub fn fail_incoming(&self, id: &str, reason: &str, cancel: bool) -> Result<()> {
        self.connection.execute("UPDATE incoming SET state='failed',answer=NULL,request_json=NULL,error_code=?2,cancel_pending=?3 WHERE id=?1", params![id,reason,cancel])?;
        Ok(())
    }

    /// Purge expired work content and eligible old terminal metadata atomically.
    ///
    /// # Errors
    /// Returns database transaction errors; no partial expiry is committed.
    pub fn expire(&self, now: i64) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute("UPDATE incoming SET state='expired',answer=NULL,request_json=NULL,error_code='work_expired',cancel_pending=(job_id IS NOT NULL) WHERE expires_at<=?1 AND state IN ('pending','grading','responding')", [now])?;
        transaction.execute("UPDATE outbox SET state=CASE WHEN first_attempt IS NULL THEN 'failed' ELSE 'unknown' END,payload=NULL,error_code='send_window_expired' WHERE state='pending' AND (expires_at<=?1 OR (first_attempt IS NOT NULL AND first_attempt+?2<=?1))", params![now,SEND_WINDOW])?;
        transaction.execute("DELETE FROM outbox WHERE state!='pending' AND created_at<?1 AND (incoming_id IS NULL OR incoming_id IN (SELECT id FROM incoming WHERE cancel_pending=0))", [now-RECEIPT_LIFETIME])?;
        transaction.execute("DELETE FROM incoming WHERE received_at<?1 AND state IN ('sent','ignored','failed','expired') AND cancel_pending=0 AND NOT EXISTS(SELECT 1 FROM outbox WHERE outbox.incoming_id=incoming.id)", [now-RECEIPT_LIFETIME])?;
        transaction.commit()?;
        Ok(())
    }

    /// Read up to twenty pending cancellation pairs of incoming ID and job ID.
    ///
    /// # Errors
    /// Returns database read or record-decoding errors.
    pub fn cancellation_jobs(&self) -> Result<Vec<(String, String)>> {
        let mut statement = self.connection.prepare(
            "SELECT id,job_id FROM incoming WHERE cancel_pending=1 AND job_id IS NOT NULL LIMIT 20",
        )?;
        Ok(statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Clear the cancellation duty after Nucleus confirms terminal or absent work.
    ///
    /// # Errors
    /// Returns database write errors.
    pub fn finish_cancellation(&self, id: &str) -> Result<()> {
        self.connection
            .execute("UPDATE incoming SET cancel_pending=0 WHERE id=?1", [id])?;
        Ok(())
    }

    /// Read up to twenty frozen pending emails whose next attempt is due.
    ///
    /// # Errors
    /// Returns database read or record-decoding errors.
    pub fn outgoing(&self, now: i64) -> Result<Vec<Outgoing>> {
        let mut statement = self.connection.prepare("SELECT id,incoming_id,payload,first_attempt,attempts,expires_at FROM outbox WHERE state='pending' AND next_attempt<=?1 AND payload IS NOT NULL ORDER BY created_at,id LIMIT 20")?;
        Ok(statement
            .query_map([now], |row| {
                Ok(Outgoing {
                    id: row.get(0)?,
                    incoming_id: row.get(1)?,
                    payload: row.get(2)?,
                    first_attempt: row.get(3)?,
                    attempts: row.get(4)?,
                    expires_at: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Record the first-attempt deadline and attempt count before submission.
    ///
    /// # Errors
    /// Returns database write errors.
    pub fn begin_send(&self, id: &str, now: i64) -> Result<()> {
        self.connection.execute("UPDATE outbox SET first_attempt=COALESCE(first_attempt,?2),attempts=attempts+1 WHERE id=?1 AND state='pending'", params![id,now])?;
        Ok(())
    }

    /// Set a later attempt for an email whose acceptance is unresolved.
    ///
    /// # Errors
    /// Returns database write errors.
    pub fn retry_send(&self, id: &str, next: i64) -> Result<()> {
        self.connection.execute("UPDATE outbox SET next_attempt=?2,error_code='email_submission_unresolved' WHERE id=?1 AND state='pending'", params![id,next])?;
        Ok(())
    }

    /// Commit acceptance and purge the outgoing and related incoming content.
    ///
    /// # Errors
    /// Returns database transaction errors; acceptance must be retried unchanged.
    pub fn sent(&self, outgoing: &Outgoing, receipt: &str, now: i64) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute("UPDATE outbox SET state='sent',payload=NULL,receipt=?2,accepted_at=?3,error_code=NULL WHERE id=?1", params![outgoing.id,receipt,now])?;
        if let Some(incoming) = &outgoing.incoming_id {
            transaction.execute("UPDATE incoming SET state='sent',answer=NULL,request_json=NULL,error_code=NULL WHERE id=?1", [incoming])?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Report content-free configuration, retained-record counts and timestamps.
    ///
    /// # Errors
    /// Returns invalid configuration, missing corpus or database read errors.
    pub fn status(&self) -> Result<Value> {
        let config = self.config()?;
        let (_, corpus) = self.active_corpus()?;
        let used = self.used_problem_ids()?;
        let remaining = corpus
            .problems
            .iter()
            .filter(|p| !used.contains(&p.id))
            .count();
        let counts = |sql: &str| -> Result<Value> {
            let mut statement = self.connection.prepare(sql)?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            let mut values = serde_json::Map::new();
            for row in rows {
                let (state, count) = row?;
                values.insert(state, json!(count));
            }
            Ok(Value::Object(values))
        };
        Ok(
            json!({"paused":config.paused,"daily_time":format!("{:02}:{:02}",config.hour,config.minute),"timezone":config.timezone,
            "unused_problems_in_active_corpus":remaining,"assigned_problem_count":used.len(),
            "retained_incoming_message_counts":counts("SELECT state,count(*) FROM incoming GROUP BY state")?,
            "retained_outgoing_message_counts":counts("SELECT state,count(*) FROM outbox GROUP BY state")?,
            "last_poll_completed_at":self.meta("last_poll_completed_at")?.and_then(|s|s.parse::<i64>().ok()),
            "last_tick_completed_at":self.meta("last_tick_completed_at")?.and_then(|s|s.parse::<i64>().ok()),
            "last_tick_error_codes":self.meta("last_tick_errors")?.unwrap_or_default(),
            "timestamp_unit":"Unix seconds UTC"}),
        )
    }

    /// Count incoming and outgoing records that prevent a drained state.
    ///
    /// # Errors
    /// Returns database read errors.
    pub fn outstanding_work(&self) -> Result<i64> {
        Ok(self.connection.query_row("SELECT (SELECT count(*) FROM incoming WHERE state IN ('pending','grading','responding') OR cancel_pending=1) + (SELECT count(*) FROM outbox WHERE state='pending')", [], |row|row.get(0))?)
    }
}

fn assignment_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Assignment> {
    Ok(Assignment {
        date: row.get(0)?,
        problem_id: row.get(1)?,
        corpus_id: row.get(2)?,
        token: row.get(3)?,
        reply_to: row.get(4)?,
    })
}

fn incoming_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Incoming> {
    let refs: String = row.get(6)?;
    let references = serde_json::from_str(&refs).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Incoming {
        id: row.get(0)?,
        assignment_date: row.get(1)?,
        state: row.get(2)?,
        received_at: row.get(3)?,
        expires_at: row.get(4)?,
        message_id: row.get(5)?,
        references,
        answer: row.get(7)?,
        request_json: row.get(8)?,
        job_id: row.get(9)?,
    })
}

const SCHEMA: &str = "
CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE TABLE corpora(id TEXT PRIMARY KEY,body TEXT NOT NULL);
CREATE TABLE assignments(date TEXT PRIMARY KEY,problem_id TEXT NOT NULL UNIQUE,corpus_id TEXT NOT NULL REFERENCES corpora(id),token TEXT NOT NULL UNIQUE,reply_to TEXT NOT NULL,created_at INTEGER NOT NULL);
CREATE TABLE incoming(id TEXT PRIMARY KEY,assignment_date TEXT NOT NULL REFERENCES assignments(date),state TEXT NOT NULL CHECK(state IN ('pending','grading','responding','sent','ignored','failed','expired')),received_at INTEGER NOT NULL,ingested_at INTEGER NOT NULL,expires_at INTEGER NOT NULL,message_id TEXT NOT NULL,references_json TEXT NOT NULL,answer TEXT,request_json TEXT,job_id TEXT,cancel_pending INTEGER NOT NULL DEFAULT 0,error_code TEXT);
CREATE TABLE outbox(id TEXT PRIMARY KEY,incoming_id TEXT REFERENCES incoming(id),payload TEXT,state TEXT NOT NULL CHECK(state IN ('pending','sent','failed','unknown')),created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL,first_attempt INTEGER,next_attempt INTEGER NOT NULL,attempts INTEGER NOT NULL DEFAULT 0,receipt TEXT,accepted_at INTEGER,error_code TEXT);
CREATE INDEX incoming_pending ON incoming(state,received_at);
CREATE INDEX outbox_pending ON outbox(state,next_attempt);
PRAGMA user_version=1;
";
