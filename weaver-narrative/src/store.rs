//! One document record with transient Nucleus mailbox recovery state.
use anyhow::{Context, Result, ensure};
use nucleus_core::JobRequestV1;
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DATABASE: &str = "weaver.sqlite";

pub fn now() -> Result<i64> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    )?)
}

pub fn regular(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink() && meta.nlink() == 1,
        "state must be a regular single-link file"
    );
    Ok(())
}

pub fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "state must be a regular directory"
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn runner_lock(root: &Path) -> Result<File> {
    private_directory(root)?;
    let path = root.join("runner.lock");
    if path.symlink_metadata().is_ok() {
        regular(&path)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    fs2::FileExt::try_lock_exclusive(&file).context("another Weaver operation is active")?;
    Ok(file)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub created_at: i64,
    pub request: JobRequestV1,
    pub markdown: Option<String>,
    pub finished_at: Option<i64>,
    pub error: Option<String>,
}

impl Document {
    pub fn view(&self) -> Result<Value> {
        let prompt: Value = serde_json::from_str(&self.request.prompt)?;
        Ok(
            json!({"id":self.id,"nucleus_job_id":self.request.id,"direction":prompt["direction"],
            "created_at":self.created_at,"markdown":self.markdown,
            "finished_at":self.finished_at,"error":self.error}),
        )
    }
}

pub struct Store {
    pub connection: Connection,
}

impl Store {
    pub fn initialize(root: &Path) -> Result<Self> {
        private_directory(root)?;
        let path = root.join(DATABASE);
        if !path.try_exists()? {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?
                .sync_all()?;
        }
        regular(&path)?;
        let mut connection = Connection::open(&path)?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version == 0 {
            let count: i64 = connection.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )?;
            ensure!(
                count == 0,
                "unrecognized Weaver state; refusing initialization"
            );
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("schema.sql"))?;
            tx.commit()?;
        }
        Self::open(root, false)
    }

    pub fn open(root: &Path, read_only: bool) -> Result<Self> {
        let path = root.join(DATABASE);
        regular(&path)
            .context("Weaver is not initialized; run weaver init --annals-config PATH")?;
        let connection = Connection::open_with_flags(
            path,
            if read_only {
                OpenFlags::SQLITE_OPEN_READ_ONLY
            } else {
                OpenFlags::SQLITE_OPEN_READ_WRITE
            },
        )?;
        connection.busy_timeout(Duration::from_secs(10))?;
        if !read_only {
            connection.execute_batch("PRAGMA synchronous=FULL;")?;
        }
        let version: i64 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(version == 1, "unsupported Weaver database schema");
        Ok(Self { connection })
    }

    pub fn create(&self, request: &JobRequestV1) -> Result<Document> {
        request.validate()?;
        ensure!(
            request.requester.program == "weaver" && request.requester.id == request.id.as_str(),
            "invalid Weaver request identity"
        );
        self.connection.execute(
            "INSERT INTO documents(id,created_at,request) VALUES(?1,?2,?3)",
            params![request.id.as_str(), now()?, serde_json::to_string(request)?],
        )?;
        self.document(request.id.as_str())
    }

    pub fn document(&self, id: &str) -> Result<Document> {
        let (created_at, raw, markdown, finished_at, error): (
            i64,
            String,
            Option<String>,
            Option<i64>,
            Option<String>,
        ) = self
            .connection
            .query_row(
                "SELECT created_at,request,markdown,finished_at,error FROM documents WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .context("Weaver document does not exist")?;
        let request: JobRequestV1 = serde_json::from_str(&raw)?;
        ensure!(
            request.id.as_str() == id
                && request.requester.program == "weaver"
                && request.requester.id == id,
            "stored request identity differs"
        );
        Ok(Document {
            id: id.to_owned(),
            created_at,
            request,
            markdown,
            finished_at,
            error,
        })
    }

    pub fn list(&self, limit: usize) -> Result<Value> {
        ensure!((1..=100).contains(&limit), "limit must be 1 through 100");
        let mut query = self.connection.prepare("SELECT id,created_at,markdown IS NOT NULL,finished_at,error FROM documents ORDER BY created_at DESC,rowid DESC LIMIT ?1")?;
        let mut items = query.query_map([i64::try_from(limit + 1)?], |r| Ok(json!({
            "id":r.get::<_,String>(0)?,"nucleus_job_id":r.get::<_,String>(0)?,"created_at":r.get::<_,i64>(1)?,
            "saved":r.get::<_,bool>(2)?,"finished_at":r.get::<_,Option<i64>>(3)?,"error":r.get::<_,Option<String>>(4)?
        })))?.collect::<Result<Vec<_>,_>>()?;
        let has_more = items.len() > limit;
        items.truncate(limit);
        Ok(json!({"documents":items,"has_more":has_more}))
    }

    pub fn finish(&self, id: &str, error: Option<&str>) -> Result<()> {
        self.connection.execute("UPDATE documents SET finished_at=COALESCE(finished_at,?2),error=?3,pending_reply=NULL WHERE id=?1",
            params![id,now()?,error])?;
        Ok(())
    }
}
