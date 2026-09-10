//! Three product records. Tool replies belong to their exact agent attempt.
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DATABASE: &str = "paperboy.sqlite";

pub const SCHEMA: &str = r"
CREATE TABLE briefs (
 id TEXT PRIMARY KEY,
 occurrence TEXT NOT NULL UNIQUE,
 scheduled_for INTEGER NOT NULL,
 window_start INTEGER NOT NULL,
 window_end INTEGER NOT NULL CHECK(window_end > window_start),
 timezone TEXT NOT NULL,
 source_pointers TEXT NOT NULL,
 created_at INTEGER NOT NULL,
 subject TEXT,
 body TEXT,
 summary_recorded_at INTEGER,
 producing_attempt TEXT,
 email_key TEXT NOT NULL UNIQUE,
 CHECK((subject IS NULL AND body IS NULL AND summary_recorded_at IS NULL AND producing_attempt IS NULL)
    OR (subject IS NOT NULL AND body IS NOT NULL AND summary_recorded_at IS NOT NULL AND producing_attempt IS NOT NULL))
);
CREATE TABLE agent_attempts (
 id TEXT PRIMARY KEY,
 brief_id TEXT NOT NULL REFERENCES briefs(id),
 job_id TEXT NOT NULL UNIQUE,
 request TEXT NOT NULL,
 created_at INTEGER NOT NULL,
 submitted_at INTEGER,
 finished_at INTEGER,
 outcome TEXT NOT NULL,
 failure TEXT,
 tool_replies TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX agent_brief ON agent_attempts(brief_id, created_at);
CREATE TABLE email_attempts (
 id TEXT PRIMARY KEY,
 brief_id TEXT NOT NULL REFERENCES briefs(id),
 started_at INTEGER NOT NULL,
 finished_at INTEGER,
 outcome TEXT NOT NULL CHECK(outcome IN ('in_progress','accepted','failed','uncertain')),
 provider_message_id TEXT,
 failure TEXT
);
CREATE INDEX email_brief ON email_attempts(brief_id, started_at);
CREATE TRIGGER freeze_summary BEFORE UPDATE OF subject,body,producing_attempt,summary_recorded_at ON briefs
WHEN OLD.body IS NOT NULL AND (NEW.subject IS NOT OLD.subject OR NEW.body IS NOT OLD.body OR NEW.producing_attempt IS NOT OLD.producing_attempt OR NEW.summary_recorded_at IS NOT OLD.summary_recorded_at)
BEGIN SELECT RAISE(ABORT,'accepted summary is immutable'); END;
PRAGMA user_version=1;
";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brief {
    pub id: String,
    pub occurrence: String,
    pub scheduled_for: i64,
    pub window_start: i64,
    pub window_end: i64,
    pub timezone: String,
    pub source_pointers: Value,
    pub subject: Option<String>,
    pub body: Option<String>,
    pub summary_recorded_at: Option<i64>,
    pub producing_attempt: Option<String>,
    pub email_key: String,
}

pub struct Store {
    pub connection: Connection,
}

pub fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "private state must be a regular directory"
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn regular(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1,
        "state file must be a regular single-link file"
    );
    Ok(())
}

pub fn runner_lock(root: &Path) -> Result<File> {
    private_directory(root)?;
    let path = root.join("runner.lock");
    if path.exists() {
        regular(&path)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    fs2::FileExt::try_lock_exclusive(&file).context("another Paperboy operation is active")?;
    Ok(file)
}

impl Store {
    pub fn initialize(root: &Path) -> Result<Self> {
        private_directory(root)?;
        let path = root.join(DATABASE);
        if path.exists() {
            return Self::open(root);
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.sync_all()?;
        let mut connection = Connection::open(&path)?;
        let transaction = connection.transaction()?;
        transaction.execute_batch(SCHEMA)?;
        transaction.commit()?;
        Self::open(root)
    }

    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join(DATABASE);
        regular(&path).context("Paperboy is not initialized; run paperboy init")?;
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        connection.busy_timeout(Duration::from_secs(10))?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(version == 1, "unsupported Paperboy database version");
        Ok(Self { connection })
    }

    pub fn backup(&self, target: &Path) -> Result<()> {
        if target.exists() {
            regular(target)?;
            self.connection.execute(
                "ATTACH DATABASE ?1 AS deployment_backup",
                [target.to_str().context("backup path must be UTF-8")?],
            )?;
            let result = (|| -> Result<()> {
                let integrity: String = self.connection.query_row(
                    "PRAGMA deployment_backup.quick_check",
                    [],
                    |row| row.get(0),
                )?;
                ensure!(
                    integrity == "ok",
                    "retained deployment backup failed integrity"
                );
                for table in ["briefs", "agent_attempts", "email_attempts"] {
                    let difference: bool = self.connection.query_row(&format!("SELECT EXISTS(SELECT * FROM main.{table} EXCEPT SELECT * FROM deployment_backup.{table}) OR EXISTS(SELECT * FROM deployment_backup.{table} EXCEPT SELECT * FROM main.{table})"), [], |row| row.get(0))?;
                    ensure!(
                        !difference,
                        "retained deployment backup differs from held Paperboy state"
                    );
                }
                Ok(())
            })();
            self.connection
                .execute_batch("DETACH DATABASE deployment_backup")?;
            return result;
        }
        ensure!(
            target.is_absolute() && !target.exists(),
            "backup must be an absent absolute path"
        );
        private_directory(target.parent().context("backup parent missing")?)?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(target)?;
        drop(file);
        self.connection.backup("main", target, None)?;
        File::open(target)?.sync_all()?;
        Ok(())
    }

    pub fn brief(&self, id: &str) -> Result<Brief> {
        let row = self.connection.query_row("SELECT id,occurrence,scheduled_for,window_start,window_end,timezone,source_pointers,subject,body,summary_recorded_at,producing_attempt,email_key FROM briefs WHERE id=?1", [id], |r| {
            Ok((Brief { id:r.get(0)?,occurrence:r.get(1)?,scheduled_for:r.get(2)?,window_start:r.get(3)?,window_end:r.get(4)?,timezone:r.get(5)?,source_pointers:Value::Null,subject:r.get(7)?,body:r.get(8)?,summary_recorded_at:r.get(9)?,producing_attempt:r.get(10)?,email_key:r.get(11)? }, r.get::<_,String>(6)?))
        })?;
        let (mut brief, pointers) = row;
        brief.source_pointers = serde_json::from_str(&pointers)?;
        Ok(brief)
    }

    pub fn create(&self, occurrence: &str, end: i64, timezone: &str) -> Result<Brief> {
        let id = uuid::Uuid::now_v7().to_string();
        let pointers = json!({"source":"Conversations: normal-user local Codex history","discover":"list_conversations","read":"read_conversation"});
        self.connection.execute("INSERT OR IGNORE INTO briefs(id,occurrence,scheduled_for,window_start,window_end,timezone,source_pointers,created_at,email_key) VALUES(?1,?2,?3,?4,?3,?5,?6,?7,?8)",params![id,occurrence,end,end-86400,timezone,pointers.to_string(),crate::now(),format!("paperboy/{id}")])?;
        let actual: String = self.connection.query_row(
            "SELECT id FROM briefs WHERE occurrence=?1",
            [occurrence],
            |r| r.get(0),
        )?;
        self.brief(&actual)
    }

    pub fn accepted_receipt(&self, brief: &str) -> Result<Option<String>> {
        Ok(self.connection.query_row("SELECT provider_message_id FROM email_attempts WHERE brief_id=?1 AND outcome='accepted' ORDER BY started_at DESC LIMIT 1",[brief],|r|r.get(0)).optional()?)
    }

    pub fn list(&self, limit: usize) -> Result<Value> {
        ensure!((1..=1000).contains(&limit), "limit must be from 1 to 1000");
        let mut statement = self
            .connection
            .prepare("SELECT id FROM briefs ORDER BY created_at DESC,id DESC LIMIT ?1")?;
        let ids = statement
            .query_map([i64::try_from(limit + 1)?], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let items = ids.iter().take(limit).map(|id| {
            let brief=self.brief(id)?;
            Ok(json!({"id":brief.id,"occurrence":brief.occurrence,"window_start":brief.window_start,"window_end":brief.window_end,"summary_recorded_at":brief.summary_recorded_at,"provider_message_id":self.accepted_receipt(id)?}))
        }).collect::<Result<Vec<Value>>>()?;
        Ok(json!({"items":items,"has_more":ids.len()>limit}))
    }

    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join(DATABASE)
    }
}

#[cfg(test)]
mod deployment_backup_tests {
    use super::*;

    #[test]
    fn an_interrupted_backup_reuses_only_matching_retained_records() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let store = Store::initialize(temporary.path())?;
        store.create("fixture-one", 200_000, "UTC")?;
        let backup = temporary.path().join("deployment.sqlite");
        store.backup(&backup)?;
        let bytes = fs::read(&backup)?;
        store.backup(&backup)?;
        assert_eq!(fs::read(&backup)?, bytes);
        store.create("fixture-two", 300_000, "UTC")?;
        assert!(store.backup(&backup).is_err());
        assert_eq!(fs::read(&backup)?, bytes);
        Ok(())
    }
}
