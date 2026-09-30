//! An append-only ledger. Status is supplied by the caller, never inferred.
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
CREATE TABLE threads (
 id TEXT PRIMARY KEY NOT NULL,
 name TEXT NOT NULL UNIQUE CHECK(length(trim(name)) > 0)
);
CREATE TABLE external_references (
 id INTEGER PRIMARY KEY,
 namespace TEXT NOT NULL CHECK(length(trim(namespace)) > 0),
 external_id TEXT NOT NULL CHECK(length(trim(external_id)) > 0),
 UNIQUE(namespace,external_id)
);
CREATE TABLE entries (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 id TEXT NOT NULL UNIQUE,
 recorded_at TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('record','retraction')),
 thread_id TEXT REFERENCES threads(id),
 status TEXT,
 notes TEXT,
 replaces TEXT UNIQUE REFERENCES entries(id),
 request_json TEXT NOT NULL,
 CHECK(kind='record' OR (status IS NULL AND replaces IS NOT NULL)),
 CHECK(kind='retraction' OR status IS NOT NULL OR notes IS NOT NULL)
);
CREATE TABLE entry_references (
 entry_id TEXT NOT NULL REFERENCES entries(id) DEFERRABLE INITIALLY DEFERRED,
 reference_id INTEGER NOT NULL REFERENCES external_references(id),
 role TEXT NOT NULL CHECK(role IN ('link','application_report')),
 PRIMARY KEY(entry_id,reference_id,role)
);
CREATE UNIQUE INDEX entry_application ON entry_references(entry_id) WHERE role='application_report';
CREATE INDEX reference_entries ON entry_references(reference_id,entry_id);
CREATE INDEX entries_thread ON entries(thread_id,sequence);
CREATE TRIGGER entries_no_update BEFORE UPDATE ON entries
 BEGIN SELECT RAISE(ABORT,'Clew entries are append-only'); END;
CREATE TRIGGER entries_no_delete BEFORE DELETE ON entries
 BEGIN SELECT RAISE(ABORT,'Clew entries are append-only'); END;
CREATE TRIGGER entry_references_no_update BEFORE UPDATE ON entry_references
 BEGIN SELECT RAISE(ABORT,'Clew entry references are immutable'); END;
CREATE TRIGGER entry_references_no_delete BEFORE DELETE ON entry_references
 BEGIN SELECT RAISE(ABORT,'Clew entry references are immutable'); END;
CREATE TRIGGER entry_references_no_late_insert BEFORE INSERT ON entry_references
 WHEN EXISTS(SELECT 1 FROM entries WHERE id=NEW.entry_id)
 BEGIN SELECT RAISE(ABORT,'Clew entry references must commit with their entry'); END;
CREATE TRIGGER application_reference_namespace BEFORE INSERT ON entry_references
 WHEN NEW.role='application_report' AND
 (SELECT namespace FROM external_references WHERE id=NEW.reference_id) != 'cast.job'
 BEGIN SELECT RAISE(ABORT,'Application reports require a Cast job reference'); END;
CREATE TRIGGER threads_no_update BEFORE UPDATE ON threads
 BEGIN SELECT RAISE(ABORT,'Clew thread identities are immutable'); END;
CREATE TRIGGER threads_no_delete BEFORE DELETE ON threads
 BEGIN SELECT RAISE(ABORT,'Clew threads are retained'); END;
CREATE TRIGGER external_references_no_update BEFORE UPDATE ON external_references
 BEGIN SELECT RAISE(ABORT,'Clew external references are immutable'); END;
CREATE TRIGGER external_references_no_delete BEFORE DELETE ON external_references
 BEGIN SELECT RAISE(ABORT,'Clew external references are retained'); END;
CREATE TRIGGER legacy_references_no_update BEFORE UPDATE ON legacy_references
 BEGIN SELECT RAISE(ABORT,'Clew legacy references are immutable'); END;
CREATE TRIGGER legacy_references_no_delete BEFORE DELETE ON legacy_references
 BEGIN SELECT RAISE(ABORT,'Clew legacy references are immutable'); END;
PRAGMA user_version=3;
";
const ENTRY_SELECT: &str = "SELECT e.sequence,e.id,e.recorded_at,e.kind,t.id,t.name,e.status,e.notes,e.replaces FROM entries e LEFT JOIN threads t ON t.id=e.thread_id";

pub struct Store {
    connection: Connection,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reference {
    pub namespace: String,
    pub external_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct EntryReference {
    pub namespace: String,
    pub external_id: String,
    pub role: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Thread {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub sequence: i64,
    pub id: String,
    pub recorded_at: String,
    pub kind: String,
    pub thread: Option<Thread>,
    pub references: Vec<EntryReference>,
    pub status: Option<String>,
    pub notes: Option<String>,
    pub replaces: Option<String>,
}

impl Entry {
    #[must_use]
    pub fn application_job_id(&self) -> Option<&str> {
        self.references
            .iter()
            .find(|reference| {
                reference.namespace == "cast.job" && reference.role == "application_report"
            })
            .map(|reference| reference.external_id.as_str())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    pub cast_job_id: Option<String>,
    pub platter_job_ref: Option<String>,
    pub thread: Option<String>,
    #[serde(default)]
    pub references: Vec<Reference>,
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

#[derive(Clone, Debug, Serialize)]
pub struct HistoryEntry {
    #[serde(flatten)]
    pub entry: Entry,
    pub superseded_by: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ThreadHistory {
    pub thread: Thread,
    pub status: Option<String>,
    pub status_entry_id: Option<String>,
    pub history: Vec<HistoryEntry>,
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
        !matches!(version, 1 | 2),
        "Clew schema {version} requires the guarded ledger migration; deploy the current Clew release"
    );
    ensure!(version == 3, "unsupported Clew database schema");
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

    pub fn schema_version_at(root: &Path) -> Result<i64> {
        schema_version(&connect(root, false)?)
    }

    pub fn migration_needed(root: &Path) -> Result<bool> {
        match Self::schema_version_at(root)? {
            1 | 2 => Ok(true),
            3 => Ok(false),
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

    /// Caller holds email admission and drains sends. Migration excludes old writers.
    pub fn migrate(
        root: &Path,
        mappings: &BTreeMap<String, String>,
        backup_path: &Path,
    ) -> Result<()> {
        migrate_ledger(root, 1, Some(mappings), backup_path)
    }

    /// Schema two already retains all identities needed for the general ledger.
    pub fn migrate_current(root: &Path, backup_path: &Path) -> Result<()> {
        migrate_ledger(root, 2, None, backup_path)
    }

    pub fn entry(&self, id: &str) -> Result<Option<Entry>> {
        let tx = self.connection.unchecked_transaction()?;
        let entry = get_entry(&tx, id)?;
        tx.commit()?;
        Ok(entry)
    }

    pub fn history_entry(&self, id: &str) -> Result<Option<HistoryEntry>> {
        Ok(history_entries(self.entries()?)
            .into_iter()
            .find(|entry| entry.entry.id == id))
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
            "Clew ledger has a broken reference"
        );
        Ok(())
    }

    pub fn entries(&self) -> Result<Vec<Entry>> {
        let tx = self.connection.unchecked_transaction()?;
        let mut entries = {
            let mut statement = tx.prepare(&format!("{ENTRY_SELECT} ORDER BY e.sequence"))?;
            statement
                .query_map([], row_entry)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for entry in &mut entries {
            entry.references = entry_references(&tx, &entry.id)?;
        }
        tx.commit()?;
        Ok(entries)
    }

    /// Exact retries preserve submitted thread and reference namespaces after corrections.
    pub fn existing_record(&self, record: &Record) -> Result<Option<Entry>> {
        let entry = self.entry(&record.id)?;
        if entry.is_some() {
            require_exact_request(&self.connection, &record.id, &record_request(record)?)?;
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

    pub fn legacy_aliases(&self) -> Result<Vec<(String, String)>> {
        let mut statement = self.connection.prepare(
            "SELECT platter_job_ref,cast_job_id FROM legacy_references ORDER BY platter_job_ref",
        )?;
        Ok(statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn knows_job(&self, cast_job_id: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM entry_references e JOIN external_references r ON r.id=e.reference_id WHERE r.namespace='cast.job' AND r.external_id=?1 AND e.role='application_report')",
            [cast_job_id], |row| row.get(0),
        )?)
    }

    pub fn record(&mut self, record: &Record) -> Result<Entry> {
        validate_record(record)?;
        let request = record_request(record)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(entry) = get_entry(&tx, &record.id)? {
            require_exact_request(&tx, &record.id, &request)?;
            return Ok(entry);
        }
        let thread = if let Some(target) = &record.replaces {
            let target = validate_target(&tx, target)?;
            if let Some(name) = &record.thread {
                ensure!(
                    target
                        .thread
                        .as_ref()
                        .is_some_and(|thread| thread.name == *name),
                    "a correction must keep the target's thread"
                );
            }
            target.thread
        } else {
            record
                .thread
                .as_deref()
                .map(|name| get_or_create_thread(&tx, name))
                .transpose()?
        };
        let application_job = match (&record.cast_job_id, &record.platter_job_ref) {
            (Some(id), None) => Some(id.clone()),
            (None, Some(reference)) => Some(
                tx.query_row(
                    "SELECT cast_job_id FROM legacy_references WHERE platter_job_ref=?1",
                    [reference],
                    |row| row.get(0),
                )
                .optional()?
                .context("unknown legacy Platter reference; supply --cast-job JOB_ID")?,
            ),
            (None, None) => None,
            _ => unreachable!("validated application target"),
        };
        let at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?;
        for reference in normalized_references(&record.references) {
            attach_reference(
                &tx,
                &record.id,
                &reference.namespace,
                &reference.external_id,
                "link",
            )?;
        }
        if let Some(job) = application_job {
            attach_reference(&tx, &record.id, "cast.job", &job, "application_report")?;
        }
        tx.execute(
            "INSERT INTO entries(id,recorded_at,kind,thread_id,status,notes,replaces,request_json) VALUES(?1,?2,'record',?3,?4,?5,?6,?7)",
            params![record.id,at,thread.as_ref().map(|thread| &thread.id),record.status,record.notes,record.replaces,request],
        )?;
        tx.commit()?;
        self.entry(&record.id)?.context("committed entry missing")
    }

    pub fn retract(&mut self, id: &str, target: &str, notes: Option<&str>) -> Result<Entry> {
        validate_id(id)?;
        let request = retraction_request(id, target, notes)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(entry) = get_entry(&tx, id)? {
            require_exact_request(&tx, id, &request)?;
            return Ok(entry);
        }
        let target_entry = validate_target(&tx, target)?;
        let at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?;
        for reference in &target_entry.references {
            attach_reference(
                &tx,
                id,
                &reference.namespace,
                &reference.external_id,
                &reference.role,
            )?;
        }
        tx.execute(
            "INSERT INTO entries(id,recorded_at,kind,thread_id,notes,replaces,request_json) VALUES(?1,?2,'retraction',?3,?4,?5,?6)",
            params![id,at,target_entry.thread.as_ref().map(|thread| &thread.id),notes,target,request],
        )?;
        tx.commit()?;
        self.entry(id)?.context("committed retraction missing")
    }

    pub fn current(&self) -> Result<Vec<Current>> {
        let entries = self.entries()?;
        let mut current: BTreeMap<String, Current> = BTreeMap::new();
        for entry in active_entries(&entries) {
            let Some(job) = entry.application_job_id() else {
                continue;
            };
            let item = current.entry(job.to_owned()).or_insert_with(|| Current {
                cast_job_id: job.to_owned(),
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
        let mut ids = BTreeSet::new();
        // Corrections target earlier entries, so one ledger-order pass covers their lineage.
        for entry in &entries {
            if entry.application_job_id() == Some(cast_job_id.as_str())
                || entry.replaces.as_ref().is_some_and(|id| ids.contains(id))
            {
                ids.insert(entry.id.clone());
            }
        }
        Ok(history_entries(entries)
            .into_iter()
            .filter(|entry| ids.contains(&entry.entry.id))
            .collect())
    }

    pub fn search(&self, query: &str) -> Result<Vec<HistoryEntry>> {
        ensure!(!query.trim().is_empty(), "search query must be nonblank");
        let query = query.to_lowercase();
        Ok(history_entries(self.entries()?)
            .into_iter()
            .filter(|history| {
                let entry = &history.entry;
                entry.id.to_lowercase().contains(&query)
                    || entry.thread.as_ref().is_some_and(|thread| {
                        thread.name.to_lowercase().contains(&query)
                            || thread.id.to_lowercase().contains(&query)
                    })
                    || [&entry.notes, &entry.status]
                        .into_iter()
                        .flatten()
                        .any(|text| text.to_lowercase().contains(&query))
                    || entry.references.iter().any(|reference| {
                        reference.namespace.to_lowercase().contains(&query)
                            || reference.external_id.to_lowercase().contains(&query)
                    })
            })
            .collect())
    }

    pub fn thread(&self, name: &str) -> Result<ThreadHistory> {
        let thread = find_thread(&self.connection, name)?.context("unknown Clew thread")?;
        let entries: Vec<_> = self
            .entries()?
            .into_iter()
            .filter(|entry| {
                entry
                    .thread
                    .as_ref()
                    .is_some_and(|item| item.id == thread.id)
            })
            .collect();
        let latest_status = active_entries(&entries)
            .filter(|entry| entry.status.is_some())
            .last();
        let status = latest_status.and_then(|entry| entry.status.clone());
        let status_entry_id = latest_status.map(|entry| entry.id.clone());
        Ok(ThreadHistory {
            thread,
            status,
            status_entry_id,
            history: history_entries(entries),
        })
    }
}

fn normalized_references(references: &[Reference]) -> Vec<Reference> {
    references
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn record_request(record: &Record) -> Result<String> {
    let mut record = record.clone();
    record.references = normalized_references(&record.references);
    Ok(serde_json::to_string(
        &serde_json::json!({"kind":"record","record":record}),
    )?)
}

fn retraction_request(id: &str, target: &str, notes: Option<&str>) -> Result<String> {
    Ok(serde_json::to_string(
        &serde_json::json!({"kind":"retraction","id":id,"target":target,"notes":notes}),
    )?)
}

fn require_exact_request(connection: &Connection, id: &str, request: &str) -> Result<()> {
    let original: String = connection.query_row(
        "SELECT request_json FROM entries WHERE id=?1",
        [id],
        |row| row.get(0),
    )?;
    ensure!(
        original == request,
        "write ID is already bound to different content"
    );
    Ok(())
}

fn validate_record(record: &Record) -> Result<()> {
    validate_id(&record.id)?;
    ensure!(
        record.cast_job_id.is_none() || record.platter_job_ref.is_none(),
        "supply at most one Cast job ID or legacy Platter reference"
    );
    for value in [
        &record.cast_job_id,
        &record.platter_job_ref,
        &record.thread,
        &record.status,
        &record.notes,
    ]
    .into_iter()
    .flatten()
    {
        ensure!(!value.trim().is_empty(), "supplied fields must be nonblank");
    }
    ensure!(
        record.status.is_some() || record.notes.is_some(),
        "supply a status or notes"
    );
    for reference in &record.references {
        ensure!(
            !reference.namespace.trim().is_empty() && !reference.external_id.trim().is_empty(),
            "reference namespace and external ID must be nonblank"
        );
    }
    Ok(())
}

fn find_thread(connection: &Connection, name: &str) -> Result<Option<Thread>> {
    Ok(connection
        .query_row("SELECT id,name FROM threads WHERE name=?1", [name], |row| {
            Ok(Thread {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })
        .optional()?)
}

fn get_or_create_thread(connection: &Connection, name: &str) -> Result<Thread> {
    if let Some(thread) = find_thread(connection, name)? {
        return Ok(thread);
    }
    let thread = Thread {
        id: uuid::Uuid::now_v7().to_string(),
        name: name.to_owned(),
    };
    connection.execute(
        "INSERT INTO threads(id,name) VALUES(?1,?2)",
        params![thread.id, thread.name],
    )?;
    Ok(thread)
}

fn attach_reference(
    connection: &Connection,
    entry_id: &str,
    namespace: &str,
    external_id: &str,
    role: &str,
) -> Result<()> {
    connection.execute("INSERT INTO external_references(namespace,external_id) VALUES(?1,?2) ON CONFLICT(namespace,external_id) DO NOTHING", params![namespace,external_id])?;
    connection.execute("INSERT INTO entry_references(entry_id,reference_id,role) SELECT ?1,id,?4 FROM external_references WHERE namespace=?2 AND external_id=?3", params![entry_id,namespace,external_id,role])?;
    Ok(())
}

fn entry_references(connection: &Connection, id: &str) -> Result<Vec<EntryReference>> {
    let mut statement = connection.prepare("SELECT r.namespace,r.external_id,e.role FROM entry_references e JOIN external_references r ON r.id=e.reference_id WHERE e.entry_id=?1 ORDER BY r.namespace,r.external_id,e.role")?;
    Ok(statement
        .query_map([id], |row| {
            Ok(EntryReference {
                namespace: row.get(0)?,
                external_id: row.get(1)?,
                role: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

fn get_entry(connection: &Connection, id: &str) -> Result<Option<Entry>> {
    let mut entry = connection
        .query_row(&format!("{ENTRY_SELECT} WHERE e.id=?1"), [id], row_entry)
        .optional()?;
    if let Some(entry) = &mut entry {
        entry.references = entry_references(connection, id)?;
    }
    Ok(entry)
}

fn history_entries(entries: Vec<Entry>) -> Vec<HistoryEntry> {
    let replaced: BTreeMap<_, _> = entries
        .iter()
        .filter_map(|entry| {
            entry
                .replaces
                .as_ref()
                .map(|id| (id.clone(), entry.id.clone()))
        })
        .collect();
    entries
        .into_iter()
        .map(|entry| HistoryEntry {
            superseded_by: replaced.get(&entry.id).cloned(),
            entry,
        })
        .collect()
}

/// Entries arrive in committed ledger order. Superseded entries remain retained.
pub(crate) fn active_entries(entries: &[Entry]) -> impl Iterator<Item = &Entry> {
    let replaced: BTreeSet<_> = entries
        .iter()
        .filter_map(|entry| entry.replaces.as_deref())
        .collect();
    entries
        .iter()
        .filter(move |entry| entry.kind == "record" && !replaced.contains(entry.id.as_str()))
}

fn validate_target(connection: &Connection, target: &str) -> Result<Entry> {
    let entry = get_entry(connection, target)?
        .context("correction target is absent, retracted, or already replaced")?;
    let superseded: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM entries WHERE replaces=?1)",
        [target],
        |row| row.get(0),
    )?;
    ensure!(
        entry.kind == "record" && !superseded,
        "correction target is absent, retracted, or already replaced"
    );
    Ok(entry)
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
    let thread_id: Option<String> = row.get(4)?;
    let thread = if let Some(id) = thread_id {
        Some(Thread {
            id,
            name: row.get(5)?,
        })
    } else {
        None
    };
    Ok(Entry {
        sequence: row.get(0)?,
        id: row.get(1)?,
        recorded_at: row.get(2)?,
        kind: row.get(3)?,
        thread,
        references: Vec::new(),
        status: row.get(6)?,
        notes: row.get(7)?,
        replaces: row.get(8)?,
    })
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

struct OldEntry {
    sequence: i64,
    id: String,
    recorded_at: String,
    kind: String,
    cast_job_id: Option<String>,
    platter_job_ref: Option<String>,
    status: Option<String>,
    notes: Option<String>,
    replaces: Option<String>,
}

fn migration_mappings(
    connection: &Connection,
    version: i64,
    supplied_mappings: Option<&BTreeMap<String, String>>,
) -> Result<BTreeMap<String, String>> {
    if version == 1 {
        let mappings = supplied_mappings
            .context("schema-one migration requires reference mappings")?
            .clone();
        let references = legacy_references(connection)?;
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
        Ok(mappings)
    } else {
        let mut statement = connection.prepare(
            "SELECT platter_job_ref,cast_job_id FROM legacy_references ORDER BY platter_job_ref",
        )?;
        Ok(statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
}

fn migrate_ledger(
    root: &Path,
    version: i64,
    supplied_mappings: Option<&BTreeMap<String, String>>,
    backup_path: &Path,
) -> Result<()> {
    private_directory(root)?;
    ensure!(
        backup_path.is_absolute(),
        "migration backup must be absolute"
    );
    private_directory(backup_path.parent().context("backup parent is absent")?)?;
    let mut connection = connect(root, true)?;
    // The rebuilt table references itself. Check all foreign keys before commit.
    connection.pragma_update(None, "foreign_keys", false)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure!(
        schema_version(&tx)? == version,
        "migration requires schema {version}"
    );
    let mappings = migration_mappings(&tx, version, supplied_mappings)?;
    let old_entries = {
        let select = if version == 1 {
            "SELECT sequence,id,recorded_at,kind,NULL,platter_job_ref,status,notes,replaces FROM entries ORDER BY sequence"
        } else {
            "SELECT sequence,id,recorded_at,kind,cast_job_id,platter_job_ref,status,notes,replaces FROM entries ORDER BY sequence"
        };
        let mut statement = tx.prepare(select)?;
        statement
            .query_map([], |row| {
                Ok(OldEntry {
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
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    backup_locked(root, backup_path)?;
    tx.execute_batch(
        "DROP TRIGGER entries_no_update; DROP TRIGGER entries_no_delete;
        DROP TRIGGER IF EXISTS entries_require_cast; DROP INDEX entries_job;
        DROP INDEX IF EXISTS entries_legacy_job; ALTER TABLE entries RENAME TO entries_previous;",
    )?;
    if version == 2 {
        tx.execute_batch("DROP TRIGGER legacy_references_no_update; DROP TRIGGER legacy_references_no_delete; DROP TABLE legacy_references;")?;
    }
    tx.execute_batch(SCHEMA)?;
    for (reference, job) in &mappings {
        tx.execute(
            "INSERT INTO legacy_references(platter_job_ref,cast_job_id) VALUES(?1,?2)",
            params![reference, job],
        )?;
    }
    for entry in old_entries {
        let job = match (&entry.cast_job_id, &entry.platter_job_ref) {
            (Some(job), _) => job.clone(),
            (None, Some(reference)) => mappings
                .get(reference)
                .context("legacy entry has no Cast mapping")?
                .clone(),
            _ => anyhow::bail!("retained application entry has no job identity"),
        };
        let request = if entry.kind == "record" {
            record_request(&Record {
                id: entry.id.clone(),
                cast_job_id: if entry.platter_job_ref.is_none() {
                    Some(job.clone())
                } else {
                    None
                },
                platter_job_ref: entry.platter_job_ref.clone(),
                status: entry.status.clone(),
                notes: entry.notes.clone(),
                replaces: entry.replaces.clone(),
                ..Record::default()
            })?
        } else {
            retraction_request(
                &entry.id,
                entry
                    .replaces
                    .as_deref()
                    .context("retraction has no target")?,
                entry.notes.as_deref(),
            )?
        };
        attach_reference(&tx, &entry.id, "cast.job", &job, "application_report")?;
        tx.execute("INSERT INTO entries(sequence,id,recorded_at,kind,status,notes,replaces,request_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![entry.sequence,entry.id,entry.recorded_at,entry.kind,entry.status,entry.notes,entry.replaces,request])?;
    }
    tx.execute_batch("DROP TABLE entries_previous;")?;
    ensure!(
        !tx.prepare("PRAGMA foreign_key_check")?.exists([])?,
        "Clew ledger has a broken reference"
    );
    tx.commit()?;
    std::fs::File::open(root)?.sync_all()?;
    Ok(())
}
