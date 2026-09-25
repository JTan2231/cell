//! An append-only application ledger. Status is supplied by the caller, never inferred.
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
CREATE TABLE legacy_references (
 platter_job_ref TEXT PRIMARY KEY NOT NULL,
 cast_job_id TEXT NOT NULL UNIQUE
);
CREATE TABLE entries (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 id TEXT NOT NULL UNIQUE,
 recorded_at TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('record','retraction')),
 cast_job_id TEXT,
 platter_job_ref TEXT REFERENCES legacy_references(platter_job_ref),
 status TEXT,
 notes TEXT,
 replaces TEXT UNIQUE REFERENCES entries(id),
 CHECK(cast_job_id IS NOT NULL OR platter_job_ref IS NOT NULL),
 CHECK(kind='record' OR (status IS NULL AND replaces IS NOT NULL)),
 CHECK(kind='retraction' OR status IS NOT NULL OR notes IS NOT NULL)
);
CREATE INDEX entries_job ON entries(cast_job_id,sequence);
CREATE INDEX entries_legacy_job ON entries(platter_job_ref,sequence);
CREATE TRIGGER entries_no_update BEFORE UPDATE ON entries
 BEGIN SELECT RAISE(ABORT,'Clew entries are append-only'); END;
CREATE TRIGGER entries_no_delete BEFORE DELETE ON entries
 BEGIN SELECT RAISE(ABORT,'Clew entries are append-only'); END;
CREATE TRIGGER legacy_references_no_update BEFORE UPDATE ON legacy_references
 BEGIN SELECT RAISE(ABORT,'Clew legacy references are immutable'); END;
CREATE TRIGGER legacy_references_no_delete BEFORE DELETE ON legacy_references
 BEGIN SELECT RAISE(ABORT,'Clew legacy references are immutable'); END;
PRAGMA user_version=2;
";
// A schema-one command that opened before migration must not append after cutover.
const INSERT_GUARD: &str = "
CREATE TRIGGER entries_require_cast BEFORE INSERT ON entries
 WHEN NEW.cast_job_id IS NULL
 BEGIN SELECT RAISE(ABORT,'Clew schema changed; reopen with the current program'); END;
";
const ENTRY_SELECT: &str = "SELECT e.sequence,e.id,e.recorded_at,e.kind,COALESCE(e.cast_job_id,b.cast_job_id),e.platter_job_ref,e.status,e.notes,e.replaces FROM entries e LEFT JOIN legacy_references b ON b.platter_job_ref=e.platter_job_ref";

pub struct Store {
    connection: Connection,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub sequence: i64,
    pub id: String,
    pub recorded_at: String,
    pub kind: String,
    pub cast_job_id: String,
    pub platter_job_ref: Option<String>,
    pub status: Option<String>,
    pub notes: Option<String>,
    pub replaces: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    pub cast_job_id: Option<String>,
    pub platter_job_ref: Option<String>,
    pub status: Option<String>,
    pub notes: Option<String>,
    pub replaces: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Current {
    pub cast_job_id: String,
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

pub(crate) fn private_file(path: &Path) -> Result<()> {
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

fn private_directory(root: &Path) -> Result<()> {
    ensure!(root.is_absolute(), "Clew state directory must be absolute");
    let metadata = std::fs::symlink_metadata(root)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Clew state directory must be a regular directory"
    );
    ensure!(
        metadata.permissions().mode() & 0o777 == 0o700,
        "Clew state directory must be private (0700)"
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

fn schema_version(connection: &Connection) -> Result<i64> {
    Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

fn require_current_schema(version: i64) -> Result<()> {
    ensure!(
        version != 1,
        "Clew schema one requires the guarded Cast-reference migration; deploy the current Clew release"
    );
    ensure!(version == 2, "unsupported Clew database schema");
    Ok(())
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
        private_directory(root)?;
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
        let version = schema_version(&tx)?;
        if created || version == 0 {
            let tables: i64 = tx.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |row| row.get(0))?;
            ensure!(tables == 0, "refusing to initialize a foreign database");
            tx.execute_batch(SCHEMA)?;
            tx.execute_batch(INSERT_GUARD)?;
        } else {
            require_current_schema(version)?;
        }
        tx.commit()?;
        std::fs::File::open(root)?.sync_all()?;
        Ok(Self { connection })
    }

    pub fn open(root: &Path, writable: bool) -> Result<Self> {
        let connection = connect(root, writable)?;
        require_current_schema(schema_version(&connection)?)?;
        Ok(Self { connection })
    }

    pub fn migration_needed(root: &Path) -> Result<bool> {
        match schema_version(&connect(root, false)?)? {
            1 => Ok(true),
            2 => Ok(false),
            _ => anyhow::bail!("unsupported Clew database schema"),
        }
    }

    pub fn legacy_references(root: &Path) -> Result<Vec<String>> {
        let connection = connect(root, false)?;
        ensure!(
            schema_version(&connection)? == 1,
            "migration requires schema one"
        );
        legacy_references(&connection)
    }

    /// Caller holds email admission and drains sends before entering migration.
    /// The transaction excludes old writers; the insertion guard rejects late old writes.
    pub fn migrate(
        root: &Path,
        mappings: &BTreeMap<String, String>,
        backup_path: &Path,
    ) -> Result<()> {
        private_directory(root)?;
        ensure!(
            backup_path.is_absolute(),
            "migration backup must be absolute"
        );
        private_directory(backup_path.parent().context("backup parent is absent")?)?;
        let mut connection = connect(root, true)?;
        // Rebuilding the self-referencing table requires deferred manual FK validation.
        connection.pragma_update(None, "foreign_keys", false)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(schema_version(&tx)? == 1, "migration requires schema one");
        let references = legacy_references(&tx)?;
        ensure!(
            references.iter().eq(mappings.keys()),
            "migration mappings must cover exactly every retained Platter reference"
        );
        ensure!(
            mappings.values().all(|id| !id.trim().is_empty()),
            "migration requires a Cast job ID for every retained reference"
        );
        let distinct: BTreeSet<_> = mappings.values().collect();
        ensure!(
            distinct.len() == mappings.len(),
            "multiple Platter histories map to one Cast job; resolve the mapping before migration"
        );
        backup_locked(root, backup_path)?;
        tx.execute_batch(
            "DROP TRIGGER entries_no_update; DROP TRIGGER entries_no_delete;
             DROP INDEX entries_job; ALTER TABLE entries RENAME TO entries_schema1;",
        )?;
        tx.execute_batch(SCHEMA)?;
        for (reference, cast_id) in mappings {
            tx.execute(
                "INSERT INTO legacy_references(platter_job_ref,cast_job_id) VALUES(?1,?2)",
                params![reference, cast_id],
            )?;
        }
        tx.execute_batch(
            "INSERT INTO entries(sequence,id,recorded_at,kind,platter_job_ref,status,notes,replaces)
             SELECT sequence,id,recorded_at,kind,platter_job_ref,status,notes,replaces FROM entries_schema1 ORDER BY sequence;
             DROP TABLE entries_schema1;",
        )?;
        tx.execute_batch(INSERT_GUARD)?;
        ensure!(
            !tx.prepare("PRAGMA foreign_key_check")?.exists([])?,
            "Clew ledger has a broken correction reference"
        );
        tx.commit()?;
        std::fs::File::open(root)?.sync_all()?;
        Ok(())
    }

    pub fn entry(&self, id: &str) -> Result<Option<Entry>> {
        get_entry(&self.connection, id)
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
        let mut statement = self
            .connection
            .prepare(&format!("{ENTRY_SELECT} ORDER BY e.sequence"))?;
        Ok(statement
            .query_map([], row_entry)?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Exact retries preserve the original target namespace, including after retraction.
    pub fn existing_record(&self, record: &Record) -> Result<Option<Entry>> {
        let entry = self.entry(&record.id)?;
        if let Some(entry) = &entry {
            require_exact_record(entry, record)?;
        }
        Ok(entry)
    }

    pub fn canonical_reference(&self, reference: &str) -> Result<String> {
        let mapped = self
            .connection
            .query_row(
                "SELECT cast_job_id FROM legacy_references WHERE platter_job_ref=?1",
                [reference],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(mapped) = mapped {
            ensure!(
                mapped == reference || !self.knows_job(reference)?,
                "reference is ambiguous between a Cast job and a legacy Platter reference"
            );
            return Ok(mapped);
        }
        Ok(reference.to_owned())
    }

    pub fn knows(&self, reference: &str) -> Result<bool> {
        self.knows_job(&self.canonical_reference(reference)?)
    }

    pub fn knows_job(&self, cast_job_id: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries e LEFT JOIN legacy_references b ON b.platter_job_ref=e.platter_job_ref WHERE COALESCE(e.cast_job_id,b.cast_job_id)=?1)",
            [cast_job_id], |row| row.get(0),
        )?)
    }

    pub fn record(&mut self, record: &Record) -> Result<Entry> {
        validate_id(&record.id)?;
        ensure!(
            record.cast_job_id.is_some() != record.platter_job_ref.is_some(),
            "supply exactly one Cast job ID or legacy Platter reference"
        );
        let reference = record
            .cast_job_id
            .as_ref()
            .or(record.platter_job_ref.as_ref())
            .context("job reference is required")?;
        ensure!(!reference.trim().is_empty(), "job reference is required");
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
        if let Some(entry) = get_entry(&tx, &record.id)? {
            require_exact_record(&entry, record)?;
            return Ok(entry);
        }
        let cast_job_id = match &record.cast_job_id {
            Some(id) => id.clone(),
            None => tx
                .query_row(
                    "SELECT cast_job_id FROM legacy_references WHERE platter_job_ref=?1",
                    [&record.platter_job_ref],
                    |row| row.get(0),
                )
                .optional()?
                .context("unknown legacy Platter reference; supply --cast-job JOB_ID")?,
        };
        if let Some(target) = &record.replaces {
            validate_target(&tx, target)?;
        }
        let at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?;
        tx.execute("INSERT INTO entries(id,recorded_at,kind,cast_job_id,platter_job_ref,status,notes,replaces) VALUES(?1,?2,'record',?3,?4,?5,?6,?7)",
            params![record.id, at, cast_job_id, record.platter_job_ref, record.status, record.notes, record.replaces])?;
        tx.commit()?;
        self.entry(&record.id)?.context("committed entry missing")
    }

    pub fn retract(&mut self, id: &str, target: &str, notes: Option<&str>) -> Result<Entry> {
        validate_id(id)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(entry) = get_entry(&tx, id)? {
            ensure!(
                entry.kind == "retraction"
                    && entry.replaces.as_deref() == Some(target)
                    && entry.notes.as_deref() == notes,
                "write ID is already bound to different content"
            );
            return Ok(entry);
        }
        let target_entry = validate_target(&tx, target)?;
        let at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?;
        tx.execute("INSERT INTO entries(id,recorded_at,kind,cast_job_id,platter_job_ref,notes,replaces) VALUES(?1,?2,'retraction',?3,?4,?5,?6)",
            params![id,at,target_entry.cast_job_id,target_entry.platter_job_ref,notes,target])?;
        tx.commit()?;
        self.entry(id)?.context("committed retraction missing")
    }

    pub fn current(&self) -> Result<Vec<Current>> {
        let entries = self.entries()?;
        let mut current: BTreeMap<String, Current> = BTreeMap::new();
        for entry in active_entries(&entries) {
            let item = current
                .entry(entry.cast_job_id.clone())
                .or_insert_with(|| Current {
                    cast_job_id: entry.cast_job_id.clone(),
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
        let cast_job_id = self.canonical_reference(reference)?;
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
            .filter(|entry| entry.cast_job_id == cast_job_id)
            .map(|entry| entry.id.clone())
            .collect();
        Ok(entries
            .into_iter()
            .filter(|entry| {
                entry.cast_job_id == cast_job_id
                    || entry.replaces.as_ref().is_some_and(|id| ids.contains(id))
            })
            .map(|entry| HistoryEntry {
                superseded_by: replaced.get(&entry.id).cloned(),
                entry,
            })
            .collect())
    }
}

fn legacy_references(connection: &Connection) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare("SELECT DISTINCT platter_job_ref FROM entries ORDER BY platter_job_ref")?;
    Ok(statement
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn backup_locked(root: &Path, backup_path: &Path) -> Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(backup_path)
        .context("migration backup must be a new private file")?
        .sync_all()?;
    let source = connect(root, false)?;
    let mut destination = Connection::open(backup_path)?;
    {
        let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
        backup.run_to_completion(128, std::time::Duration::from_millis(10), None)?;
    }
    destination.close().map_err(|(_, error)| error)?;
    std::fs::File::open(backup_path)?.sync_all()?;
    std::fs::File::open(backup_path.parent().context("backup parent is absent")?)?.sync_all()?;
    Ok(())
}

fn get_entry(connection: &Connection, id: &str) -> Result<Option<Entry>> {
    Ok(connection
        .query_row(&format!("{ENTRY_SELECT} WHERE e.id=?1"), [id], row_entry)
        .optional()?)
}

fn require_exact_record(entry: &Entry, record: &Record) -> Result<()> {
    let same_target = match (&record.cast_job_id, &record.platter_job_ref) {
        (Some(id), None) => entry.platter_job_ref.is_none() && entry.cast_job_id == *id,
        (None, Some(reference)) => entry.platter_job_ref.as_ref() == Some(reference),
        _ => false,
    };
    ensure!(
        entry.kind == "record"
            && same_target
            && entry.status == record.status
            && entry.notes == record.notes
            && entry.replaces == record.replaces,
        "write ID is already bound to different content"
    );
    Ok(())
}

/// Entries arrive in ledger sequence order from a single `SQLite` read.
pub(crate) fn active_entries(entries: &[Entry]) -> impl Iterator<Item = &Entry> {
    let replaced: BTreeSet<_> = entries
        .iter()
        .filter_map(|e| e.replaces.as_deref())
        .collect();
    entries
        .iter()
        .filter(move |entry| entry.kind == "record" && !replaced.contains(entry.id.as_str()))
}

fn validate_target(connection: &Connection, target: &str) -> Result<Entry> {
    connection.query_row(&format!("{ENTRY_SELECT} WHERE e.id=?1 AND e.kind='record' AND NOT EXISTS(SELECT 1 FROM entries replacement WHERE replacement.replaces=?1)"), [target], row_entry)
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
        cast_job_id: row.get(4)?,
        platter_job_ref: row.get(5)?,
        status: row.get(6)?,
        notes: row.get(7)?,
        replaces: row.get(8)?,
    })
}
