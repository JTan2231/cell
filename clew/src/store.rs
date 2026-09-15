//! One append-only table. Status is supplied by the caller, never inferred.
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub const DATABASE: &str = "ledger.sqlite3";
const SCHEMA: &str = "
CREATE TABLE entries (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 id TEXT NOT NULL UNIQUE,
 recorded_at TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('record','retraction')),
 platter_job_ref TEXT NOT NULL,
 status TEXT,
 notes TEXT,
 replaces TEXT UNIQUE REFERENCES entries(id),
 CHECK(kind='record' OR (status IS NULL AND replaces IS NOT NULL)),
 CHECK(kind='retraction' OR status IS NOT NULL OR notes IS NOT NULL)
);
CREATE INDEX entries_job ON entries(platter_job_ref,sequence);
CREATE TRIGGER entries_no_update BEFORE UPDATE ON entries
 BEGIN SELECT RAISE(ABORT,'Clew entries are append-only'); END;
CREATE TRIGGER entries_no_delete BEFORE DELETE ON entries
 BEGIN SELECT RAISE(ABORT,'Clew entries are append-only'); END;
PRAGMA user_version=1;
";

pub struct Store {
    connection: Connection,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub sequence: i64,
    pub id: String,
    pub recorded_at: String,
    pub kind: String,
    pub platter_job_ref: String,
    pub status: Option<String>,
    pub notes: Option<String>,
    pub replaces: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    pub platter_job_ref: String,
    pub status: Option<String>,
    pub notes: Option<String>,
    pub replaces: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Current {
    pub platter_job_ref: String,
    pub status: Option<String>,
    pub status_entry_id: Option<String>,
    pub latest_entry: Entry,
}

#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    #[serde(flatten)]
    pub entry: Entry,
    pub superseded_by: Option<String>,
}

fn private_file(path: &Path) -> Result<()> {
    let metadata =
        std::fs::symlink_metadata(path).context("Clew is not initialized; run clew init")?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Clew database must be a regular file"
    );
    ensure!(
        metadata.permissions().mode() & 0o777 == 0o600,
        "Clew database must be private (0600)"
    );
    Ok(())
}

fn connect(root: &Path, writable: bool) -> Result<Connection> {
    let path = root.join(DATABASE);
    private_file(&path)?;
    let connection = Connection::open_with_flags(
        path,
        if writable {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        },
    )?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY;")?;
    if writable {
        connection.execute_batch("PRAGMA synchronous=FULL;")?;
    }
    Ok(connection)
}

impl Store {
    pub fn initialize(root: &Path) -> Result<Self> {
        ensure!(root.is_absolute(), "Clew state directory must be absolute");
        if !root.exists() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(root)?;
        }
        let metadata = std::fs::symlink_metadata(root)?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Clew state directory must be a regular directory"
        );
        ensure!(
            metadata.permissions().mode() & 0o777 == 0o700,
            "Clew state directory must be private (0700)"
        );
        let path = root.join(DATABASE);
        let created = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => {
                file.sync_all()?;
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(error.into()),
        };
        let mut connection = connect(root, true)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if created || version == 0 {
            let tables: i64 = tx.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |row| row.get(0))?;
            ensure!(tables == 0, "refusing to initialize a foreign database");
            tx.execute_batch(SCHEMA)?;
        } else {
            ensure!(
                version == 1,
                "unsupported Clew database; existing state is never reinitialized"
            );
        }
        tx.commit()?;
        std::fs::File::open(root)?.sync_all()?;
        Ok(Self { connection })
    }

    pub fn open(root: &Path, writable: bool) -> Result<Self> {
        let connection = connect(root, writable)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        ensure!(version == 1, "unsupported Clew database schema");
        Ok(Self { connection })
    }

    pub fn entry(&self, id: &str) -> Result<Option<Entry>> {
        Ok(self.connection.query_row("SELECT sequence,id,recorded_at,kind,platter_job_ref,status,notes,replaces FROM entries WHERE id=?1", [id], row_entry).optional()?)
    }

    pub fn check(&self) -> Result<()> {
        let integrity: String = self
            .connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        ensure!(integrity == "ok", "Clew database integrity check failed");
        ensure!(
            !self
                .connection
                .prepare("PRAGMA foreign_key_check")?
                .exists([])?,
            "Clew ledger has a broken correction reference"
        );
        Ok(())
    }

    pub fn entries(&self) -> Result<Vec<Entry>> {
        let mut statement = self.connection.prepare("SELECT sequence,id,recorded_at,kind,platter_job_ref,status,notes,replaces FROM entries ORDER BY sequence")?;
        Ok(statement
            .query_map([], row_entry)?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Exact retries resolve before Platter is needed, including after retraction.
    pub fn existing_record(&self, record: &Record) -> Result<Option<Entry>> {
        let entry = self.entry(&record.id)?;
        if let Some(entry) = &entry {
            ensure!(
                entry.kind == "record"
                    && entry.platter_job_ref == record.platter_job_ref
                    && entry.status == record.status
                    && entry.notes == record.notes
                    && entry.replaces == record.replaces,
                "write ID is already bound to different content"
            );
        }
        Ok(entry)
    }

    pub fn knows(&self, reference: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE platter_job_ref=?1)",
            [reference],
            |row| row.get(0),
        )?)
    }

    pub fn record(&mut self, record: &Record) -> Result<Entry> {
        validate_id(&record.id)?;
        ensure!(
            !record.platter_job_ref.trim().is_empty(),
            "opportunity reference is required"
        );
        for value in [&record.status, &record.notes].into_iter().flatten() {
            ensure!(
                !value.trim().is_empty(),
                "status and notes must be nonblank when supplied"
            );
        }
        ensure!(
            record.status.is_some() || record.notes.is_some(),
            "supply a status or notes"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = tx.query_row("SELECT sequence,id,recorded_at,kind,platter_job_ref,status,notes,replaces FROM entries WHERE id=?1", [&record.id], row_entry).optional()?;
        if let Some(entry) = existing {
            ensure!(
                entry.kind == "record"
                    && entry.platter_job_ref == record.platter_job_ref
                    && entry.status == record.status
                    && entry.notes == record.notes
                    && entry.replaces == record.replaces,
                "write ID is already bound to different content"
            );
            return Ok(entry);
        }
        if let Some(target) = &record.replaces {
            validate_target(&tx, target)?;
        }
        let at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?;
        tx.execute("INSERT INTO entries(id,recorded_at,kind,platter_job_ref,status,notes,replaces) VALUES(?1,?2,'record',?3,?4,?5,?6)",
            params![record.id, at, record.platter_job_ref, record.status, record.notes, record.replaces])?;
        tx.commit()?;
        self.entry(&record.id)?.context("committed entry missing")
    }

    pub fn retract(&mut self, id: &str, target: &str, notes: Option<&str>) -> Result<Entry> {
        validate_id(id)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = tx.query_row("SELECT sequence,id,recorded_at,kind,platter_job_ref,status,notes,replaces FROM entries WHERE id=?1", [id], row_entry).optional()?;
        if let Some(entry) = existing {
            ensure!(
                entry.kind == "retraction"
                    && entry.replaces.as_deref() == Some(target)
                    && entry.notes.as_deref() == notes,
                "write ID is already bound to different content"
            );
            return Ok(entry);
        }
        let reference = validate_target(&tx, target)?;
        let at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?;
        tx.execute("INSERT INTO entries(id,recorded_at,kind,platter_job_ref,notes,replaces) VALUES(?1,?2,'retraction',?3,?4,?5)", params![id,at,reference,notes,target])?;
        tx.commit()?;
        self.entry(id)?.context("committed retraction missing")
    }

    pub fn current(&self) -> Result<Vec<Current>> {
        let entries = self.entries()?;
        let replaced: BTreeSet<_> = entries
            .iter()
            .filter_map(|entry| entry.replaces.as_deref())
            .collect();
        let mut current: BTreeMap<String, Current> = BTreeMap::new();
        for entry in &entries {
            if entry.kind != "record" || replaced.contains(entry.id.as_str()) {
                continue;
            }
            let item = current
                .entry(entry.platter_job_ref.clone())
                .or_insert_with(|| Current {
                    platter_job_ref: entry.platter_job_ref.clone(),
                    status: None,
                    status_entry_id: None,
                    latest_entry: entry.clone(),
                });
            if let Some(status) = &entry.status {
                item.status = Some(status.clone());
                item.status_entry_id = Some(entry.id.clone());
            }
            item.latest_entry = entry.clone();
        }
        Ok(current.into_values().collect())
    }

    pub fn history(&self, reference: &str) -> Result<Vec<HistoryEntry>> {
        let entries = self.entries()?;
        let replaced: BTreeMap<_, _> = entries
            .iter()
            .filter_map(|entry| {
                entry
                    .replaces
                    .as_ref()
                    .map(|target| (target.clone(), entry.id.clone()))
            })
            .collect();
        let ids: BTreeSet<_> = entries
            .iter()
            .filter(|entry| entry.platter_job_ref == reference)
            .map(|entry| entry.id.clone())
            .collect();
        Ok(entries
            .into_iter()
            .filter(|entry| {
                entry.platter_job_ref == reference
                    || entry.replaces.as_ref().is_some_and(|id| ids.contains(id))
            })
            .map(|entry| HistoryEntry {
                superseded_by: replaced.get(&entry.id).cloned(),
                entry,
            })
            .collect())
    }
}

fn validate_target(connection: &Connection, target: &str) -> Result<String> {
    connection.query_row("SELECT platter_job_ref FROM entries WHERE id=?1 AND kind='record' AND NOT EXISTS(SELECT 1 FROM entries e WHERE e.replaces=?1)", [target], |row| row.get(0))
        .optional()?.context("correction target is absent, retracted, or already replaced")
}

fn validate_id(id: &str) -> Result<()> {
    ensure!(
        (1..=128).contains(&id.len())
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.')),
        "write ID must contain 1 through 128 ASCII letters, digits, dots, underscores or hyphens"
    );
    Ok(())
}

fn row_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    Ok(Entry {
        sequence: row.get(0)?,
        id: row.get(1)?,
        recorded_at: row.get(2)?,
        kind: row.get(3)?,
        platter_job_ref: row.get(4)?,
        status: row.get(5)?,
        notes: row.get(6)?,
        replaces: row.get(7)?,
    })
}
