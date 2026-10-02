//! Private `SQLite` state. Every mutation holds the product lock and an immediate transaction.
#![allow(clippy::missing_errors_doc)]
use crate::{
    Result,
    models::{
        Company, CompanyDraft, Config, Coverage, DiscoveryQuery, Evidence, Job, Snapshot, Source,
    },
    normalize_url, now, stable_id, timestamp,
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
        let path = directory.join("cast.sqlite3");
        if !path.is_file() {
            return Err("Cast state is not initialized; run cast init".into());
        }
        let connection =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let schema: Option<String> = connection
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if schema.as_deref() != Some("1") {
            return Err("unsupported Cast state schema".into());
        }
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
            .open(directory.join("cast.lock"))?;
        initialization_lock
            .try_lock_exclusive()
            .map_err(|_| "another Cast mutation is running")?;
        let initialization_lock = ProductLock {
            file: initialization_lock,
            locked: None,
        };
        let path = directory.join("cast.sqlite3");
        if path.exists() {
            return Self::open(directory);
        }
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)?;
        let connection = Connection::open(&path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
          CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
          INSERT OR IGNORE INTO meta VALUES('schema_version','1');
          INSERT OR IGNORE INTO meta VALUES('snapshot_revision','0');
          CREATE TABLE IF NOT EXISTS companies(id TEXT PRIMARY KEY,body TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS company_aliases(alias TEXT PRIMARY KEY,company_id TEXT NOT NULL REFERENCES companies(id));
          CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY,body TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS job_aliases(alias TEXT PRIMARY KEY,job_id TEXT NOT NULL REFERENCES jobs(id));
          CREATE TABLE IF NOT EXISTS sources(id TEXT PRIMARY KEY,body TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS coverage(id TEXT PRIMARY KEY,body TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS revisions(kind TEXT NOT NULL,id TEXT NOT NULL,revision INTEGER NOT NULL,body TEXT NOT NULL,PRIMARY KEY(kind,id,revision));
          CREATE TABLE IF NOT EXISTS source_scan_seen(source_id TEXT NOT NULL,job_id TEXT NOT NULL,PRIMARY KEY(source_id,job_id));
          CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY,started_at TEXT NOT NULL,started_epoch INTEGER NOT NULL,finished_at TEXT,status TEXT NOT NULL,note TEXT);
          CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY,run_id TEXT NOT NULL REFERENCES runs(id),provider TEXT NOT NULL,created_at TEXT NOT NULL,epoch INTEGER NOT NULL,reserved_units INTEGER NOT NULL,charged_units INTEGER,status TEXT NOT NULL);")?;
        connection.execute(
            "INSERT INTO meta(key,value) VALUES('config',?1)",
            [serde_json::to_string(&Config::default())?],
        )?;
        drop(connection);
        drop(initialization_lock);
        Self::open(directory)
    }

    pub fn lock(&self) -> Result<ProductLock> {
        if self.locked.load(Ordering::Acquire) {
            return Err("another Cast mutation is running".into());
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .open(self.directory.join("cast.lock"))?;
        file.try_lock_exclusive()
            .map_err(|_| "another Cast mutation is running")?;
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
            .map_err(|_| "Cast state mutex poisoned")?;
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
            .map_err(|_| "Cast state mutex poisoned")?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let result = f(&tx)?;
        tx.execute(
            "UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='snapshot_revision'",
            [],
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn config(&self) -> Result<Config> {
        self.read(read_config)
    }

    pub fn set_config(&self, config: &Config) -> Result<()> {
        if config.schema_version != 1
            || config.budgets.runtime_seconds == 0
            || config.budgets.runtime_seconds > 3600
        {
            return Err("invalid configuration version or runtime bound".into());
        }
        let mut providers = std::collections::HashSet::new();
        for provider in &config.automatic_excluded_ats {
            if !matches!(provider.as_str(), "ashby" | "greenhouse" | "lever")
                || !providers.insert(provider)
            {
                return Err("invalid or duplicate automatic ATS exclusion".into());
            }
        }
        let mut ids = std::collections::HashSet::new();
        for query in &config.queries {
            if query.id.is_empty()
                || !ids.insert(&query.id)
                || !matches!(query.provider.as_str(), "hn" | "brave" | "theirstack")
                || query.interval_seconds == 0
            {
                return Err("invalid or duplicate discovery query".into());
            }
            reject_secrets(&query.params)?;
        }
        self.write(|tx| {
            for coverage in all::<Coverage>(tx, "coverage")? {
                let unchanged = config.queries.iter().any(|query| {
                    query.id == coverage.query_id
                        && query_fingerprint(query) == coverage.query_fingerprint
                });
                if !unchanged {
                    tx.execute("DELETE FROM coverage WHERE id=?1", [&coverage.query_id])?;
                }
            }
            tx.execute(
                "UPDATE meta SET value=?1 WHERE key='config'",
                [serde_json::to_string(config)?],
            )?;
            Ok(())
        })
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
                    coverage: all(c, "coverage")?,
                })
            })();
            c.execute_batch("ROLLBACK")?;
            result
        })
    }

    pub fn sources(&self) -> Result<Vec<Source>> {
        self.read(|c| all(c, "sources"))
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
        self.manual_source(input, company_id, true)
    }

    fn manual_source(
        &self,
        input: &str,
        company_id: Option<&str>,
        enabled: bool,
    ) -> Result<Source> {
        let url = crate::adapters::canonical_board_url(input).unwrap_or(normalize_url(input)?);
        self.write(|tx| {
            if crate::adapters::board_identity(&url).is_some() {
                if company_id.is_some() {
                    return Err(
                        "ATS ownership is determined by its tenant; omit --company-id".into(),
                    );
                }
                return source_inner_with_enabled(tx, "", &url, enabled);
            }
            let company_id = if let Some(id) = company_id {
                get::<Company>(tx, "companies", id)?.ok_or("company not found")?;
                id.to_owned()
            } else {
                let board = crate::adapters::board_identity(&url);
                let host = url::Url::parse(&url)?
                    .host_str()
                    .unwrap_or_default()
                    .to_owned();
                let draft = CompanyDraft {
                    name: board.clone().unwrap_or_else(|| host.clone()),
                    domain: board.is_none().then_some(host),
                    provider_id: board,
                    website_url: Some(url.clone()),
                    ..Default::default()
                };
                company_inner(tx, &draft)?.id
            };
            source_inner_with_enabled(tx, &company_id, &url, enabled)
        })
    }

    pub fn disable_source(&self, id: &str) -> Result<Source> {
        self.write(|tx| {
            let mut source: Source = get(tx, "sources", id)?.ok_or("source not found")?;
            source.enabled = false;
            put(tx, "sources", id, &source)?;
            Ok(source)
        })
    }

    /// Applies current ATS tenant and HTML adapter rules to stored associations.
    pub fn reconcile_ownership(&self) -> Result<Value> {
        self.write(|tx| {
            let mut moved_sources=0_u64;
            let mut moved_jobs=0_u64;
            let mut quarantined_jobs=0_u64;
            let mut renamed_candidates=0_u64;
            let mut cleared_shared_identities=0_u64;
            for mut source in all::<Source>(tx,"sources")? {
                if crate::adapters::board_identity(&source.url).is_some() {
                    let owner=ats_company_inner(tx,&source.url,None)?;
                    if source.company_id!=owner.id {
                        source.company_id=owner.id;
                        source.note=Some("ATS tenant ownership reconciled independently from linking pages".into());
                        put(tx,"sources",&source.id,&source)?;
                        moved_sources+=1;
                    }
                }
            }
            for mut job in all::<Job>(tx,"jobs")? {
                let source=get::<Source>(tx,"sources",&job.source_id)?;
                let board_source=source.as_ref().filter(|source|crate::adapters::board_identity(&source.url).is_some());
                let mut changed=false;
                if let Some(source)=board_source {
                    if job.company_id!=source.company_id {
                        job.company_id.clone_from(&source.company_id);
                        merge_evidence(&mut job.evidence,&[Evidence {source_url:source.url.clone(),kind:"identity_reconciliation".into(),note:"Employer association updated to the ATS provider/tenant identity".into(),..Default::default()}]);
                        moved_jobs+=1;
                        changed=true;
                    }
                } else if (job.evidence.iter().any(|e|e.kind=="employer_jsonld") && !job.evidence.iter().any(|e|e.kind=="employer_jsonld_owned")) || (job.evidence.iter().any(|e| matches!(e.kind.as_str(),"employer_jsonld"|"employer_jsonld_owned")) && (crate::adapters::is_discovery_directory(&job.url) || source.as_ref().is_some_and(|source|crate::adapters::is_discovery_directory(&source.url)))) {
                    if job.availability!="unknown" || !job.evidence.iter().any(|e|e.kind=="identity_unresolved") {
                        job.availability="unknown".into();
                        job.missing_complete_snapshots=0;
                        job.first_missing_at=None;
                        merge_evidence(&mut job.evidence,&[Evidence {source_url:job.url.clone(),kind:"identity_unresolved".into(),note:"JSON-LD observation does not match current adapter rules; recorded status set to unknown".into(),..Default::default()}]);
                        quarantined_jobs+=1;
                        changed=true;
                    }
                    if let Some(mut source)=source {
                        source.status="needs_identity_review".into();
                        source.note=Some("JSON-LD does not match the current employer URL rule".into());
                        source.next_due_at=0;
                        put(tx,"sources",&source.id,&source)?;
                    }
                }
                if changed {
                    job.revision+=1;
                    put_revision(tx,"job",&job.id,job.revision,&job)?;
                    put(tx,"jobs",&job.id,&job)?;
                }
            }
            for mut company in all::<Company>(tx,"companies")? {
                let mut changed=false;
                if let Some(domain)=company.domain.clone() && company.name!=domain && !company.evidence.is_empty() && company.evidence.iter().all(|e| matches!(e.kind.as_str(),"search_result_unverified"|"identity_unresolved")) {
                    company.name=domain;
                    merge_evidence(&mut company.evidence,&[Evidence {source_url:company.website_url.clone().unwrap_or_default(),kind:"identity_unresolved".into(),note:"Search candidate name reset to its domain by the current adapter rules".into(),..Default::default()}]);
                    renamed_candidates+=1;
                    changed=true;
                }
                if clear_shared_identity(tx,&mut company)? {cleared_shared_identities+=1;changed=true;}
                if repair_hn_display_name(tx,&mut company)? {renamed_candidates+=1;changed=true;}
                if changed {
                    company.revision+=1;
                    put_revision(tx,"company",&company.id,company.revision,&company)?;
                    put(tx,"companies",&company.id,&company)?;
                }
            }
            Ok(json!({"moved_sources":moved_sources,"moved_jobs":moved_jobs,"quarantined_jobs":quarantined_jobs,"renamed_candidates":renamed_candidates,"cleared_shared_identities":cleared_shared_identities,"retained":"source/job IDs, request ledger, run history, query coverage and cursors"}))
        })
    }

    pub fn status(&self) -> Result<Value> {
        let snapshot = self.snapshot()?;
        let budgets = self.config()?.budgets;
        self.read(|c| { let today=timestamp().div_euclid(86400)*86400;
            let mut usage=serde_json::Map::new();
            for provider in ["theirstack","brave","http","hn"] {let total:u64=c.query_row("SELECT COALESCE(SUM(COALESCE(charged_units,reserved_units)),0) FROM requests WHERE provider=?1",[provider],row_u64)?;let daily:u64=c.query_row("SELECT COALESCE(SUM(COALESCE(charged_units,reserved_units)),0) FROM requests WHERE provider=?1 AND epoch>=?2",params![provider,today],row_u64)?;usage.insert(provider.into(),json!({"total_units":total,"daily_units":daily}));}
            let requests:u64=c.query_row("SELECT COUNT(*) FROM requests WHERE epoch>=?1",[today],row_u64)?;
            let last:Option<Value>=c.query_row("SELECT id,started_at,finished_at,status,note FROM runs ORDER BY started_epoch DESC,rowid DESC LIMIT 1",[],|r|Ok(json!({"id":r.get::<_,String>(0)?,"started_at":r.get::<_,String>(1)?,"finished_at":r.get::<_,Option<String>>(2)?,"status":r.get::<_,String>(3)?,"note":r.get::<_,Option<String>>(4)?}))).optional()?;
            Ok(json!({"schema_version":2,"snapshot_revision":snapshot.snapshot_revision,"companies":snapshot.companies.len(),"jobs":snapshot.jobs.len(),"sources":snapshot.source_health.len(),"http_requests_today":requests,"usage":usage,"last_run":last,"budgets":budgets,
                "source_health":snapshot.source_health.into_iter().filter(|source| !matches!(source.status.as_str(), "complete" | "resolved" | "observed")).map(|source| json!({"id":source.id,"status":source.status,"enabled":source.enabled,"last_attempt_at":source.last_attempt_at,"last_success_at":source.last_success_at,"note":source.note})).collect::<Vec<_>>(),
                "coverage":snapshot.coverage.into_iter().filter(|coverage| coverage.status != "complete").map(|coverage| json!({"query_id":coverage.query_id,"provider":coverage.provider,"status":coverage.status,"last_attempt_at":coverage.last_attempt_at,"last_complete_at":coverage.last_complete_at,"note":coverage.note})).collect::<Vec<_>>()}))
        })
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
            "cast.sqlite3",
            "cast.sqlite3-wal",
            "cast.sqlite3-shm",
            "cast.sqlite3-journal",
            "cast.lock",
        ];
        if (parent == state
            && protected.contains(&filename.to_string_lossy().to_lowercase().as_str()))
            || path
                .canonicalize()
                .is_ok_and(|resolved| protected.iter().any(|name| resolved == state.join(name)))
        {
            return Err("export target is protected Cast state".into());
        }
        let temporary = parent.join(format!(".cast-export-{}.tmp", uuid::Uuid::now_v7()));
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

fn repair_hn_display_name(tx: &Transaction<'_>, company: &mut Company) -> Result<bool> {
    if crate::adapters::plausible_company_name(&company.name)
        || !company
            .evidence
            .iter()
            .any(|e| e.kind == "hn_hiring_comment")
        || company.evidence.iter().any(|e| {
            !matches!(
                e.kind.as_str(),
                "hn_hiring_comment"
                    | "ats_tenant_identity"
                    | "identity_reconciliation"
                    | "identity_unresolved"
            )
        })
    {
        return Ok(false);
    }
    let board = all::<Source>(tx, "sources")?
        .iter()
        .filter(|source| source.company_id == company.id)
        .find_map(|source| crate::adapters::board_identity(&source.url));
    let replacement = board
        .map(|board| format!("ats:{board}"))
        .or_else(|| company.domain.clone())
        .unwrap_or_else(|| "Unknown employer".into());
    if replacement == company.name {
        return Ok(false);
    }
    let prior = std::mem::replace(&mut company.name, replacement);
    let source_url = company
        .evidence
        .first()
        .map_or_else(String::new, |e| e.source_url.clone());
    merge_evidence(
        &mut company.evidence,
        &[Evidence {
            source_url,
            kind: "identity_reconciliation".into(),
            note: format!(
                "HN display name updated to its source identity; previous display: {prior}"
            ),
            ..Default::default()
        }],
    );
    Ok(true)
}

fn clear_shared_identity(tx: &Transaction<'_>, company: &mut Company) -> Result<bool> {
    let mut removed = Vec::new();
    if company.domain.as_ref().is_some_and(|domain| {
        crate::adapters::is_discovery_directory(&format!("https://{domain}/"))
    }) && let Some(domain) = company.domain.take()
    {
        removed.push(format!("https://{domain}/"));
    }
    if company
        .website_url
        .as_deref()
        .is_some_and(crate::adapters::is_discovery_directory)
        && let Some(website) = company.website_url.take()
    {
        removed.push(website);
    }
    if removed.is_empty() {
        return Ok(false);
    }
    let aliases = {
        let mut statement = tx.prepare("SELECT alias FROM company_aliases WHERE company_id=?1")?;
        statement
            .query_map([&company.id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for alias in aliases {
        let url = alias
            .strip_prefix("domain:")
            .map(|domain| format!("https://{domain}/"))
            .or_else(|| alias.strip_prefix("website:").map(str::to_owned));
        if url
            .as_deref()
            .is_some_and(crate::adapters::is_discovery_directory)
        {
            tx.execute("DELETE FROM company_aliases WHERE alias=?1", [alias])?;
        }
    }
    for url in removed {
        merge_evidence(&mut company.evidence,&[Evidence {source_url:url,kind:"identity_unresolved".into(),note:"Shared-host company domain and website fields cleared by adapter rules; source data retained".into(),..Default::default()}]);
    }
    Ok(true)
}

fn all<T: DeserializeOwned>(c: &Connection, table: &str) -> Result<Vec<T>> {
    let mut statement = c.prepare(&format!("SELECT body FROM {table} ORDER BY id"))?;
    let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
fn query_fingerprint(query: &DiscoveryQuery) -> String {
    stable_id(
        "query",
        &json!([query.provider, query.terms, query.params]).to_string(),
    )
}
fn row_u64(row: &rusqlite::Row<'_>) -> rusqlite::Result<u64> {
    let value: i64 = row.get(0)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
}
fn get<T: DeserializeOwned>(c: &Connection, table: &str, id: &str) -> Result<Option<T>> {
    let body: Option<String> = c
        .query_row(
            &format!("SELECT body FROM {table} WHERE id=?1"),
            [id],
            |r| r.get(0),
        )
        .optional()?;
    body.map(|v| serde_json::from_str(&v).map_err(Into::into))
        .transpose()
}
fn put(c: &Connection, table: &str, id: &str, body: &impl serde::Serialize) -> Result<()> {
    c.execute(&format!("INSERT INTO {table}(id,body) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body"),params![id,serde_json::to_string(body)?])?;
    Ok(())
}
fn put_revision(
    c: &Connection,
    kind: &str,
    id: &str,
    revision: u64,
    body: &impl serde::Serialize,
) -> Result<()> {
    c.execute(
        "INSERT INTO revisions VALUES(?1,?2,?3,?4)",
        params![
            kind,
            id,
            i64::try_from(revision)?,
            serde_json::to_string(body)?
        ],
    )?;
    Ok(())
}
fn alias(c: &Connection, table: &str, column: &str, key: &str) -> Result<Option<String>> {
    Ok(c.query_row(
        &format!("SELECT {column} FROM {table} WHERE alias=?1"),
        [key],
        |r| r.get(0),
    )
    .optional()?)
}
fn add_alias(c: &Connection, table: &str, column: &str, key: &str, id: &str) -> Result<()> {
    c.execute(
        &format!("INSERT OR IGNORE INTO {table}(alias,{column}) VALUES(?1,?2)"),
        params![key, id],
    )?;
    Ok(())
}

fn company_inner(tx: &Transaction<'_>, draft: &CompanyDraft) -> Result<Company> {
    let website = draft
        .website_url
        .as_deref()
        .and_then(|u| normalize_url(u).ok());
    let domain = draft
        .domain
        .as_deref()
        .map(|d| d.trim().trim_start_matches("www.").to_lowercase())
        .filter(|d| !d.is_empty());
    let mut aliases = Vec::new();
    if let Some(provider) = &draft.provider_id {
        aliases.push(format!("provider:{provider}"));
    }
    if let Some(domain) = &domain {
        aliases.push(format!("domain:{domain}"));
    }
    if aliases.is_empty() {
        if let Some(url) = &website {
            aliases.push(format!("website:{url}"));
        } else {
            aliases.push(format!(
                "unresolved:{}:{}",
                draft.name,
                draft.evidence.first().map_or("", |e| e.source_url.as_str())
            ));
        }
    }
    let mut existing = None;
    for key in &aliases {
        if let Some(id) = alias(tx, "company_aliases", "company_id", key)? {
            existing = Some(id);
            break;
        }
    }
    let id = existing.unwrap_or_else(|| stable_id("company", &aliases[0]));
    let previous = get::<Company>(tx, "companies", &id)?;
    let mut company = previous.clone().unwrap_or_else(|| Company {
        id: id.clone(),
        revision: 0,
        name: draft.name.clone(),
        domain: None,
        website_url: None,
        first_seen_at: now(),
        last_seen_at: now(),
        relevance_reasons: vec![],
        evidence: vec![],
    });
    if company.name.is_empty() || company.name == company.domain.clone().unwrap_or_default() {
        company.name.clone_from(&draft.name);
    }
    if company.domain.is_none() {
        company.domain = domain;
    }
    if company.website_url.is_none() {
        company.website_url = website;
    }
    merge_evidence(&mut company.evidence, &draft.evidence);
    for reason in &draft.relevance_reasons {
        if !company.relevance_reasons.contains(reason) {
            company.relevance_reasons.push(reason.clone());
        }
    }
    company.last_seen_at = now();
    let material = |c: &Company| {
        json!([
            c.name,
            c.domain,
            c.website_url,
            c.relevance_reasons,
            c.evidence
        ])
    };
    if previous
        .as_ref()
        .is_none_or(|p| material(p) != material(&company))
    {
        company.revision += 1;
        put_revision(tx, "company", &id, company.revision, &company)?;
    }
    put(tx, "companies", &id, &company)?;
    for key in aliases {
        add_alias(tx, "company_aliases", "company_id", &key, &id)?;
    }
    Ok(company)
}

fn read_config(c: &Connection) -> Result<Config> {
    let body: String = c.query_row("SELECT value FROM meta WHERE key='config'", [], |r| {
        r.get(0)
    })?;
    Ok(serde_json::from_str(&body)?)
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
        put_revision(tx, "company", &id, company.revision, &company)?;
        put(tx, "companies", &id, &company)?;
    }
    tx.execute("INSERT INTO company_aliases(alias,company_id) VALUES(?1,?2) ON CONFLICT(alias) DO UPDATE SET company_id=excluded.company_id",params![key,id])?;
    Ok(company)
}

fn source_inner(tx: &Transaction<'_>, company_id: &str, url: &str) -> Result<Source> {
    source_inner_with_enabled(tx, company_id, url, true)
}

fn source_inner_with_enabled(
    tx: &Transaction<'_>,
    company_id: &str,
    url: &str,
    enabled: bool,
) -> Result<Source> {
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
            put(tx, "sources", &id, &existing)?;
        }
        return Ok(existing);
    }
    let source = Source {
        discovery_depth: 0,
        id: id.clone(),
        company_id: company_id.into(),
        url: url.into(),
        enabled,
        status: "never_checked".into(),
        last_attempt_at: None,
        last_success_at: None,
        next_due_at: 0,
        cursor: None,
        note: None,
    };
    put(tx, "sources", &id, &source)?;
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

fn reject_secrets(value: &Value) -> Result<()> {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let key = key.to_lowercase();
                if ["key", "token", "secret", "authorization", "password"]
                    .iter()
                    .any(|s| key.contains(s))
                {
                    return Err(
                        "credentials belong in environment variables, not configuration".into(),
                    );
                }
                reject_secrets(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                reject_secrets(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
