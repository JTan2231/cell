//! Private `SQLite` state. Every mutation holds the product lock and an immediate transaction.
#![allow(clippy::missing_errors_doc)]
use crate::{
    Result,
    models::{Company, Evidence, Snapshot, Source},
    normalize_url, now, stable_id,
};
use fs2::FileExt;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone)]
pub struct Store {
    connection: Arc<Mutex<Connection>>,
    pub directory: PathBuf,
    locked: Arc<AtomicBool>,
}

pub struct ProductLock {
    file: File,
    locked: Option<Arc<AtomicBool>>,
}
impl Drop for ProductLock {
    fn drop(&mut self) {
        // Closing only this descriptor does not release flock while a child
        // retains a duplicate between fork and exec. Release before advertising
        // that the local store is available for another mutation.
        let _ = FileExt::unlock(&self.file);
        if let Some(locked) = &self.locked {
            locked.store(false, Ordering::Release);
        }
    }
}

impl Store {
    pub fn open(directory: &Path) -> Result<Self> {
        let path = directory.join("milieu.sqlite3");
        if !path.is_file() {
            return Err("Milieu state is not initialized; run milieu init".into());
        }
        let connection =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let schema: Option<String> = connection
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match schema.as_deref() {
            Some("2") => {}
            Some("1") => {
                return Err(
                    "legacy Milieu state schema 1 is no longer supported; select schema-two state"
                        .into(),
                );
            }
            _ => return Err("unsupported Milieu state schema".into()),
        }
        drop(connection);
        let connection =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys=ON")?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            directory: directory.to_owned(),
            locked: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn init(directory: &Path) -> Result<Self> {
        fs::create_dir_all(directory)?;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        let initialization_lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .open(directory.join("milieu.lock"))?;
        initialization_lock
            .try_lock_exclusive()
            .map_err(|_| "another Milieu mutation is running")?;
        let initialization_lock = ProductLock {
            file: initialization_lock,
            locked: None,
        };
        let path = directory.join("milieu.sqlite3");
        if path.exists() {
            return Self::open(directory);
        }
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)?;
        let mut connection = Connection::open(&path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let tx = connection.transaction()?;
        tx.execute_batch(
            "
          CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
          INSERT INTO meta VALUES('schema_version','2');
          INSERT INTO meta VALUES('snapshot_revision','0');",
        )?;
        crate::current_store::initialize(&tx)?;
        tx.commit()?;
        drop(connection);
        drop(initialization_lock);
        Self::open(directory)
    }

    pub fn lock(&self) -> Result<ProductLock> {
        if self.locked.load(Ordering::Acquire) {
            return Err("another Milieu mutation is running".into());
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .open(self.directory.join("milieu.lock"))?;
        file.try_lock_exclusive()
            .map_err(|_| "another Milieu mutation is running")?;
        self.locked.store(true, Ordering::Release);
        Ok(ProductLock {
            file,
            locked: Some(self.locked.clone()),
        })
    }

    fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "Milieu state mutex poisoned")?;
        f(&connection)
    }

    fn write<T>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let _mutation_lock = if self.locked.load(Ordering::Acquire) {
            None
        } else {
            Some(self.lock()?)
        };
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "Milieu state mutex poisoned")?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let result = f(&tx)?;
        tx.execute(
            "UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='snapshot_revision'",
            [],
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        self.read(|c| {
            c.execute_batch("BEGIN DEFERRED")?;
            let result = (|| {
                Ok(Snapshot {
                    schema_version: 1,
                    snapshot_revision: c.query_row(
                        "SELECT CAST(value AS INTEGER) FROM meta WHERE key='snapshot_revision'",
                        [],
                        row_u64,
                    )?,
                    captured_at: now(),
                    companies: all(c, "companies")?,
                    jobs: all(c, "jobs")?,
                    source_health: all(c, "sources")?,
                    coverage: Vec::new(),
                })
            })();
            c.execute_batch("ROLLBACK")?;
            result
        })
    }

    pub fn sources(&self) -> Result<Vec<Source>> {
        self.read(|c| all(c, "sources"))
    }

    /// Stores an accepted company in current-model state.
    pub fn accept_company(&self, company: &Company) -> Result<()> {
        self.write(|tx| crate::current_store::put_company(tx, company))
    }

    /// Stores a source with an explicit operator and retained export metadata.
    pub fn accept_source(&self, source: &crate::current::Source, metadata: &Source) -> Result<()> {
        self.write(|tx| crate::current_store::put_current_source(tx, source, metadata))
    }

    /// Replaces one accepted opportunity and its workplace/source associations atomically.
    /// The caller resolves duplicates and conflicting source values before this call.
    pub fn accept_job(&self, record: &crate::current::AcceptedJob) -> Result<()> {
        self.write(|tx| crate::current_store::replace_job(tx, record))
    }

    pub fn source(&self, id: &str) -> Result<Source> {
        self.read(|c| get(c, "sources", id)?.ok_or_else(|| "source not found".into()))
    }

    pub fn add_source(&self, company_id: &str, url: &str) -> Result<Source> {
        let url = crate::adapters::canonical_board_url(url).unwrap_or(normalize_url(url)?);
        self.write(|tx| {
            get::<Company>(tx, "companies", company_id)?.ok_or("company not found")?;
            source_inner(tx, company_id, &url)
        })
    }

    pub fn add_manual_source(&self, input: &str, company_id: Option<&str>) -> Result<Source> {
        let url = crate::adapters::canonical_board_url(input).unwrap_or(normalize_url(input)?);
        self.write(|tx| {
            if crate::adapters::board_identity(&url).is_some() {
                if company_id.is_some() {
                    return Err(
                        "ATS ownership is determined by its tenant; omit --company-id".into(),
                    );
                }
                return source_inner(tx, "", &url);
            }
            let company_id = if let Some(id) = company_id {
                get::<Company>(tx, "companies", id)?.ok_or("company not found")?;
                id.to_owned()
            } else {
                website_company_inner(tx, &url)?.id
            };
            source_inner(tx, &company_id, &url)
        })
    }

    pub fn status(&self) -> Result<Value> {
        let snapshot = self.snapshot()?;
        Ok(json!({
            "schema_version": 3,
            "snapshot_revision": snapshot.snapshot_revision,
            "companies": snapshot.companies.len(),
            "jobs": snapshot.jobs.len(),
            "sources": snapshot.source_health.len(),
        }))
    }

    pub fn atomic_export(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let parent = parent.canonicalize()?;
        let filename = path.file_name().ok_or("export target must be a file")?;
        let path = parent.join(filename);
        let state = self.directory.canonicalize()?;
        let protected = [
            "milieu.sqlite3",
            "milieu.sqlite3-wal",
            "milieu.sqlite3-shm",
            "milieu.sqlite3-journal",
            "milieu.lock",
        ];
        if (parent == state
            && protected.contains(&filename.to_string_lossy().to_lowercase().as_str()))
            || path
                .canonicalize()
                .is_ok_and(|resolved| protected.iter().any(|name| resolved == state.join(name)))
        {
            return Err("export target is protected Milieu state".into());
        }
        let temporary = parent.join(format!(".milieu-export-{}.tmp", uuid::Uuid::now_v7()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&serde_json::to_vec_pretty(&self.snapshot()?)?)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, &path)?;
            File::open(&parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

fn all<T: DeserializeOwned>(c: &Connection, table: &str) -> Result<Vec<T>> {
    crate::current_store::all(c, table)
}

fn row_u64(row: &rusqlite::Row<'_>) -> rusqlite::Result<u64> {
    let value: i64 = row.get(0)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
}

fn get<T: DeserializeOwned>(c: &Connection, table: &str, id: &str) -> Result<Option<T>> {
    crate::current_store::get(c, table, id)
}

fn alias(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT value FROM meta WHERE key=?1",
        [format!("company_alias:{key}")],
        |r| r.get(0),
    )
    .optional()?)
}

fn website_company_inner(tx: &Transaction<'_>, url: &str) -> Result<Company> {
    let host = url::Url::parse(url)?
        .host_str()
        .ok_or("source URL has no hostname")?
        .to_owned();
    let domain = host.trim_start_matches("www.").to_lowercase();
    let key = format!("domain:{domain}");
    let id = alias(tx, &key)?.unwrap_or_else(|| stable_id("company", &key));
    let mut company = get::<Company>(tx, "companies", &id)?.unwrap_or_else(|| Company {
        id: id.clone(),
        revision: 0,
        name: host.clone(),
        domain: None,
        website_url: None,
        first_seen_at: now(),
        last_seen_at: now(),
        relevance_reasons: vec![],
        evidence: vec![],
    });
    if company.name.is_empty() || company.name == company.domain.clone().unwrap_or_default() {
        company.name = host;
    }
    company.domain.get_or_insert(domain);
    company.website_url.get_or_insert_with(|| url.into());
    company.last_seen_at = now();
    crate::current_store::put_company(tx, &company)?;
    tx.execute(
        "INSERT OR IGNORE INTO meta(key,value) VALUES(?1,?2)",
        params![format!("company_alias:{key}"), id],
    )?;
    get(tx, "companies", &company.id)?.ok_or_else(|| "stored company disappeared".into())
}

fn ats_company_inner(tx: &Transaction<'_>, url: &str, origin: Option<&str>) -> Result<Company> {
    let board = crate::adapters::board_identity(url).ok_or("unsupported ATS board")?;
    let canonical = crate::adapters::canonical_board_url(url).ok_or("unsupported ATS board")?;
    let provider = format!("ats:{board}");
    let key = format!("provider:{provider}");
    let id = stable_id("company", &key);
    let previous = get::<Company>(tx, "companies", &id)?;
    let mut company = previous.clone().unwrap_or_else(|| Company {
        id: id.clone(),
        revision: 0,
        name: provider,
        domain: None,
        website_url: Some(canonical.clone()),
        first_seen_at: now(),
        last_seen_at: now(),
        relevance_reasons: vec!["Independently identified supported ATS tenant".into()],
        evidence: vec![],
    });
    let evidence = Evidence {
        source_url: origin.unwrap_or(&canonical).into(),
        kind: "ats_tenant_identity".into(),
        note: format!("ATS tenant {board}; stored under its provider/tenant company identity"),
        ..Default::default()
    };
    let before = company.evidence.len();
    merge_evidence(&mut company.evidence, &[evidence]);
    if previous.is_none() || company.evidence.len() != before {
        company.revision += 1;
        crate::current_store::put_company(tx, &company)?;
    }
    tx.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![format!("company_alias:{key}"), id],
    )?;
    Ok(company)
}

fn source_inner(tx: &Transaction<'_>, company_id: &str, url: &str) -> Result<Source> {
    let canonical = crate::adapters::canonical_board_url(url);
    let url = canonical.as_deref().unwrap_or(url);
    let owner = canonical
        .as_ref()
        .map(|_| ats_company_inner(tx, url, None))
        .transpose()?;
    let company_id = owner
        .as_ref()
        .map_or(company_id, |company| company.id.as_str());
    let id = stable_id("source", url);
    if let Some(mut existing) = get::<Source>(tx, "sources", &id)? {
        if canonical.is_some() && existing.company_id != company_id {
            existing.company_id = company_id.into();
            crate::current_store::put_source(tx, &existing)?;
        }
        return Ok(existing);
    }
    let source = Source {
        discovery_depth: 0,
        id: id.clone(),
        company_id: company_id.into(),
        url: url.into(),
        enabled: true,
        status: "never_checked".into(),
        last_attempt_at: None,
        last_success_at: None,
        next_due_at: 0,
        cursor: None,
        note: None,
    };
    crate::current_store::put_source(tx, &source)?;
    Ok(source)
}

fn merge_evidence(existing: &mut Vec<Evidence>, incoming: &[Evidence]) {
    for item in incoming {
        if !existing
            .iter()
            .any(|e| e.source_url == item.source_url && e.kind == item.kind && e.note == item.note)
        {
            let mut item = item.clone();
            item.observed_at.get_or_insert_with(now);
            item.parser_version
                .get_or_insert_with(|| env!("CARGO_PKG_VERSION").into());
            existing.push(item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_state_retains_source_identity_and_rejects_legacy_without_conversion() -> Result<()> {
        let directory = std::env::temp_dir().join(format!("milieu-store-{}", uuid::Uuid::now_v7()));
        let result = (|| {
            let store = Store::init(&directory)?;
            store.accept_company(&Company {
                id: "retained-company".into(),
                revision: 0,
                name: "Retained employer".into(),
                domain: Some("example.com".into()),
                website_url: None,
                first_seen_at: "2026-10-01T00:00:00Z".into(),
                last_seen_at: "2026-10-01T00:00:00Z".into(),
                relevance_reasons: vec![],
                evidence: vec![],
            })?;
            store.write(|tx| {
                tx.execute(
                    "INSERT INTO meta(key,value) VALUES('company_alias:domain:example.com','retained-company')",
                    [],
                )?;
                Ok(())
            })?;
            let source = store.add_manual_source("https://www.example.com/careers", None)?;
            let retained = store.add_manual_source("https://www.example.com/careers", None)?;
            assert_eq!(source.id, retained.id);
            assert_eq!(source.company_id, retained.company_id);
            assert_eq!(source.company_id, "retained-company");
            let snapshot = store.snapshot()?;
            assert_eq!(snapshot.schema_version, 1);
            assert_eq!(snapshot.companies.len(), 1);
            assert_eq!(snapshot.source_health.len(), 1);
            assert!(snapshot.coverage.is_empty());
            let config_count: i64 = store.read(|connection| {
                Ok(connection.query_row(
                    "SELECT COUNT(*) FROM meta WHERE key='config'",
                    [],
                    |row| row.get(0),
                )?)
            })?;
            assert_eq!(config_count, 0);
            drop(store);

            let legacy = directory.join("legacy");
            fs::create_dir(&legacy)?;
            let database = legacy.join("milieu.sqlite3");
            let connection = Connection::open(&database)?;
            connection.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL); INSERT INTO meta VALUES('schema_version','1');")?;
            drop(connection);
            let original = fs::read(&database)?;
            for result in [Store::open(&legacy), Store::init(&legacy)] {
                let error = result.err().ok_or("legacy state was accepted")?;
                assert!(
                    error
                        .to_string()
                        .contains("schema 1 is no longer supported")
                );
            }
            assert_eq!(fs::read(&database)?, original);
            Ok(())
        })();
        fs::remove_dir_all(&directory)?;
        result
    }
}
