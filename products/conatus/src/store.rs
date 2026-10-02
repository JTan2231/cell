use anyhow::{Context, Result, ensure};
use fs2::FileExt as _;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::Path;
use std::time::Duration;

use crate::Config;

pub struct Store {
    connection: Connection,
}

// Lifecycle is local metadata. Frozen documents and Annals handoffs never include it.
const RECORD_SELECT: &str = "SELECT records.*, EXISTS(
    SELECT 1 FROM settings WHERE key = 'archived-want/' || records.id
) AS archived FROM records";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WantState {
    Active,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub wording: String,
    pub source_data: Value,
    pub work_name: String,
    pub captured_at: i64,
    pub queued_at: Option<i64>,
    pub receipt: Option<Value>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<WantState>,
    #[serde(skip)]
    pub document: String,
}

pub fn private_directory(path: &Path) -> Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    Ok(())
}

pub fn runner_idle(root: &Path) -> Result<bool> {
    let path = root.join("runner.lock");
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error.into()),
        Ok(metadata) => ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Conatus runner lock must be a regular file"
        ),
    }
    let file = OpenOptions::new().read(true).write(true).open(path)?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub fn runner_lock(root: &Path) -> Result<File> {
    private_directory(root)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(root.join("runner.lock"))?;
    file.try_lock_exclusive()
        .context("another Conatus update or library mutation is running")?;
    Ok(file)
}

impl Store {
    pub fn create(root: &Path) -> Result<Self> {
        private_directory(root)?;
        let path = root.join("conatus.db");
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(&path)?;
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(
            version <= 1,
            "unsupported Conatus database schema {version}"
        );
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS records (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL CHECK(kind IN ('want', 'decision')),
                source TEXT NOT NULL,
                wording TEXT NOT NULL,
                source_data TEXT NOT NULL,
                work_name TEXT NOT NULL UNIQUE,
                document TEXT NOT NULL,
                captured_at INTEGER NOT NULL,
                queued_at INTEGER,
                receipt TEXT,
                error TEXT
             );
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
        Ok(Self { connection })
    }

    pub fn open(root: &Path) -> Result<Self> {
        let connection =
            Connection::open_with_flags(root.join("conatus.db"), OpenFlags::SQLITE_OPEN_READ_WRITE)
                .context("Conatus is not initialized; run conatus init")?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(
            version == 1,
            "unsupported Conatus database schema {version}"
        );
        Ok(Self { connection })
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row("SELECT value FROM settings WHERE key = ?", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO settings(key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    pub fn config(&self) -> Result<Config> {
        serde_json::from_str(
            &self
                .setting("config")?
                .context("run conatus init to finish setup")?,
        )
        .context("invalid stored Conatus configuration")
    }

    pub fn configure(&mut self, config: &Config, cursor: &str) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO settings(key, value) VALUES ('config', ?)",
            [serde_json::to_string(config)?],
        )?;
        transaction.execute(
            "INSERT INTO settings(key, value) VALUES ('cursor', ?)",
            [cursor],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn capture(&self, record: &Record) -> Result<()> {
        insert_record(&self.connection, record)?;
        Ok(())
    }

    /// Intake and cursor advancement share the same local commit.
    pub fn accept_page(&mut self, records: &[Record], cursor: &str) -> Result<()> {
        let transaction = self.connection.transaction()?;
        for record in records {
            insert_record(&transaction, record)?;
        }
        transaction.execute(
            "UPDATE settings SET value = ? WHERE key = 'cursor'",
            [cursor],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn record(&self, id: &str) -> Result<Record> {
        self.connection
            .query_row(
                &format!("{RECORD_SELECT} WHERE id = ?"),
                [id],
                decode_record,
            )
            .optional()?
            .context("unknown Conatus record")
    }

    pub fn record_for_work(&self, name: &str) -> Result<Option<Record>> {
        Ok(self
            .connection
            .query_row(
                &format!("{RECORD_SELECT} WHERE work_name = ?"),
                [name],
                decode_record,
            )
            .optional()?)
    }

    pub fn list(&self, kind: &str, limit: usize) -> Result<Value> {
        self.list_records(kind, limit, (kind == "want").then_some(WantState::Active))
    }

    pub fn list_wants(&self, limit: usize, state: Option<WantState>) -> Result<Value> {
        self.list_records("want", limit, state)
    }

    fn list_records(&self, kind: &str, limit: usize, state: Option<WantState>) -> Result<Value> {
        ensure!(
            (1..=100).contains(&limit),
            "limit must be between 1 and 100"
        );
        let mut statement = self.connection.prepare(&format!(
            "{RECORD_SELECT} WHERE kind = ?1 AND (?2 IS NULL OR archived = ?2)
             ORDER BY captured_at DESC, id DESC LIMIT ?3"
        ))?;
        let mut records: Vec<Record> = statement
            .query_map(
                params![
                    kind,
                    state.map(|s| s == WantState::Archived),
                    i64::try_from(limit + 1)?
                ],
                decode_record,
            )?
            .collect::<rusqlite::Result<_>>()?;
        let has_more = records.len() > limit;
        records.truncate(limit);
        Ok(json!({"kind":kind,"items":records,"has_more":has_more}))
    }

    pub fn set_want_state(&mut self, id: &str, state: WantState) -> Result<Value> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut record = transaction
            .query_row(
                &format!("{RECORD_SELECT} WHERE id = ?"),
                [id],
                decode_record,
            )
            .optional()?
            .context("unknown Conatus record")?;
        ensure!(record.kind == "want", "record is not a want");
        let key = format!("archived-want/{id}");
        let changed = match state {
            WantState::Archived => transaction.execute(
                "INSERT INTO settings(key, value) VALUES (?, 'true') ON CONFLICT(key) DO NOTHING",
                [&key],
            )?,
            WantState::Active => {
                transaction.execute("DELETE FROM settings WHERE key = ?", [&key])?
            }
        };
        record.state = Some(state);
        transaction.commit()?;
        Ok(json!({"changed":changed != 0,"record":record}))
    }

    pub fn pending(&self) -> Result<Vec<Record>> {
        let mut statement = self.connection.prepare(&format!(
            "{RECORD_SELECT} WHERE queued_at IS NULL ORDER BY captured_at, id"
        ))?;
        Ok(statement
            .query_map([], decode_record)?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Read active wants and all decisions in one `SQLite` statement, without a list limit.
    pub fn email_records(&self) -> Result<Vec<Record>> {
        let mut statement = self
            .connection
            .prepare(&format!("{RECORD_SELECT} WHERE kind = 'decision' OR NOT archived ORDER BY captured_at DESC, id DESC"))?;
        Ok(statement
            .query_map([], decode_record)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn queued(&self, id: &str, receipt: &Value) -> Result<()> {
        self.connection.execute(
            "UPDATE records SET queued_at = ?, receipt = ?, error = NULL WHERE id = ?",
            params![crate::now()?, serde_json::to_string(receipt)?, id],
        )?;
        Ok(())
    }

    pub fn failed_handoff(&self, id: &str, error: &str) -> Result<()> {
        self.connection
            .execute("UPDATE records SET error = ? WHERE id = ?", [error, id])?;
        Ok(())
    }

    pub fn status(&self) -> Result<Value> {
        let mut statement = self.connection.prepare(
            "SELECT kind, COUNT(*), SUM(queued_at IS NULL), SUM(queued_at IS NOT NULL),
                    SUM(error IS NOT NULL), MAX(captured_at), MAX(queued_at)
             FROM records GROUP BY kind ORDER BY kind",
        )?;
        let counts: Vec<Value> = statement
            .query_map([], |row| {
                Ok(json!({
                    "kind":row.get::<_, String>(0)?,
                    "captured_records":row.get::<_, i64>(1)?,
                    "pending_handoffs":row.get::<_, i64>(2)?,
                    "queued_records":row.get::<_, i64>(3)?,
                    "handoff_errors":row.get::<_, i64>(4)?,
                    "latest_capture_at":row.get::<_, Option<i64>>(5)?,
                    "latest_enqueue_receipt_at":row.get::<_, Option<i64>>(6)?
                }))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let wants = self.connection.query_row(
            &format!(
                "SELECT COUNT(*) FILTER (WHERE NOT archived), COUNT(*) FILTER (WHERE archived)
                      FROM ({RECORD_SELECT}) WHERE kind = 'want'"
            ),
            [],
            |row| Ok(json!({"active":row.get::<_, i64>(0)?,"archived":row.get::<_, i64>(1)?})),
        )?;
        Ok(json!({
            "intake":counts,
            "wants":wants,
            "cursor":self.setting("cursor")?,
            "paused":self.setting("paused")?.as_deref() == Some("true"),
            "last_feed_read_at":self.setting("last_feed_read_at")?.map(|s| s.parse::<i64>()).transpose()?,
            "last_update":self.setting("last_update")?.map(|s| serde_json::from_str::<Value>(&s)).transpose()?
        }))
    }
}

fn insert_record(connection: &Connection, record: &Record) -> Result<()> {
    connection.execute(
        "INSERT INTO records(id, kind, source, wording, source_data, work_name, document, captured_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO NOTHING",
        params![record.id, record.kind, record.source, record.wording,
            serde_json::to_string(&record.source_data)?, record.work_name, record.document,
            record.captured_at],
    )?;
    Ok(())
}

fn decode_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<Record> {
    fn json_column(row: &rusqlite::Row<'_>, name: &str) -> rusqlite::Result<Value> {
        let text: String = row.get(name)?;
        serde_json::from_str(&text).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
    }
    let receipt: Option<String> = row.get("receipt")?;
    let kind: String = row.get("kind")?;
    let state = if kind == "want" {
        Some(if row.get("archived")? {
            WantState::Archived
        } else {
            WantState::Active
        })
    } else {
        None
    };
    Ok(Record {
        id: row.get("id")?,
        kind,
        state,
        source: row.get("source")?,
        wording: row.get("wording")?,
        source_data: json_column(row, "source_data")?,
        work_name: row.get("work_name")?,
        document: row.get("document")?,
        captured_at: row.get("captured_at")?,
        queued_at: row.get("queued_at")?,
        receipt: receipt.map(|_| json_column(row, "receipt")).transpose()?,
        error: row.get("error")?,
    })
}
