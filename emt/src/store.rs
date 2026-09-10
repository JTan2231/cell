//! EMT-owned incidents and correspondence. Execution activity stays in Nucleus.
use crate::{Result, fail};
use fs2::FileExt as _;
use rusqlite::{Connection, OpenFlags, OptionalExtension as _, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub receiving_domain: String,
    pub email_executable: PathBuf,
    pub clockwork_executable: PathBuf,
    pub cell_root: PathBuf,
    pub agent_cwd: PathBuf,
    pub model: String,
    pub paused: bool,
    pub poll_after: Option<String>,
}

impl Config {
    pub fn defaults() -> Result<Self> {
        let home = crate::home()?;
        Ok(Self {
            receiving_domain: String::new(),
            email_executable: home.join(".local/bin/email"),
            clockwork_executable: home.join(".local/bin/clockwork"),
            cell_root: home.join("rust/cell"),
            agent_cwd: home,
            model: "gpt-5.6-terra".into(),
            paused: true,
            poll_after: None,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.receiving_domain.len() + 37 > 254
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
            || !self.email_executable.is_absolute()
            || !self.clockwork_executable.is_absolute()
            || !self.cell_root.is_absolute()
            || !self.cell_root.is_dir()
            || !self.agent_cwd.is_absolute()
            || !self.agent_cwd.is_dir()
            || self.model.trim().is_empty()
        {
            return Err(fail(
                "configure an Email receiving domain, absolute provider paths, existing Cell and agent directories, and a model",
            ));
        }
        Ok(())
    }

    pub fn load(root: &Path) -> Result<Self> {
        Ok(serde_json::from_slice(&fs::read(
            root.join("config.json"),
        )?)?)
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        write_private(&root.join("config.json"), &serde_json::to_vec_pretty(self)?)
    }
}

#[derive(Clone, Serialize)]
pub struct Exchange {
    pub id: String,
    pub incident_id: String,
    pub kind: String,
    pub incoming_json: Option<String>,
    pub nucleus_job_id: String,
    pub request_json: Option<String>,
    pub request_digest: Option<String>,
    pub job_admitted: bool,
    pub state: String,
    pub created_at: i64,
    pub deadline_at: i64,
    pub mail_json: Option<String>,
    pub send_state: Option<String>,
    pub first_send_at: Option<i64>,
    pub last_send_at: Option<i64>,
    pub send_attempts: u32,
    pub email_id: Option<String>,
}

const EXCHANGE_SELECT: &str = "SELECT id,incident_id,kind,incoming_json,nucleus_job_id,request_json,request_digest,job_admitted,state,created_at,deadline_at,mail_json,send_state,first_send_at,last_send_at,send_attempts,email_id FROM exchanges";

fn exchange_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Exchange> {
    Ok(Exchange {
        id: row.get(0)?,
        incident_id: row.get(1)?,
        kind: row.get(2)?,
        incoming_json: row.get(3)?,
        nucleus_job_id: row.get(4)?,
        request_json: row.get(5)?,
        request_digest: row.get(6)?,
        job_admitted: row.get(7)?,
        state: row.get(8)?,
        created_at: row.get(9)?,
        deadline_at: row.get(10)?,
        mail_json: row.get(11)?,
        send_state: row.get(12)?,
        first_send_at: row.get(13)?,
        last_send_at: row.get(14)?,
        send_attempts: row.get(15)?,
        email_id: row.get(16)?,
    })
}

pub struct Store {
    pub connection: Connection,
}

impl Store {
    pub fn initialize(root: &Path) -> Result<Self> {
        private_directory(root)?;
        let path = root.join("emt.sqlite3");
        private_file(&path, true)?;
        let connection = Connection::open(&path)?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version == 0 {
            let tables: i64 = connection.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |row|row.get(0))?;
            if tables != 0 {
                return Err(fail("unversioned nonempty database is not EMT state"));
            }
            connection.execute_batch(include_str!("schema.sql"))?;
        } else if version != 1 {
            return Err(fail("unsupported EMT database schema"));
        }
        if !root.join("config.json").try_exists()? {
            let records: i64 = connection.query_row(
                "SELECT (SELECT count(*) FROM incidents) + (SELECT count(*) FROM exchanges)",
                [],
                |row| row.get(0),
            )?;
            if records != 0 {
                return Err(fail("EMT configuration is missing from nonempty state"));
            }
            Config::defaults()?.save(root)?;
        }
        drop(connection);
        Self::open(root)
    }

    pub fn open(root: &Path) -> Result<Self> {
        private_file(&root.join("emt.sqlite3"), false)?;
        let connection = Connection::open_with_flags(
            root.join("emt.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != 1 {
            return Err(fail("unsupported EMT database schema"));
        }
        Ok(Self { connection })
    }

    pub fn config(&self) -> Result<Config> {
        // Used by the shared installation interface; paths come from the selected database.
        let path = self
            .connection
            .path()
            .ok_or_else(|| fail("EMT database path is unavailable"))?;
        Config::load(
            Path::new(path)
                .parent()
                .ok_or_else(|| fail("EMT state root is unavailable"))?,
        )
    }

    pub fn cursor(&self) -> Result<u64> {
        let cursor: i64 = self.connection.query_row(
            "SELECT COALESCE(MAX(feed_cursor),0) FROM incidents",
            [],
            |row| row.get(0),
        )?;
        Ok(u64::try_from(cursor)?)
    }

    pub fn capture_incident(
        &mut self,
        incident: &clockwork::api::IncidentRecord,
        cursor: u64,
        basic: &clockwork::api::NotificationView,
        config: &Config,
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let reply_to = basic.reply_to.clone().unwrap_or_else(|| {
            format!(
                "emt.{}@{}",
                incident.id.replace('-', ""),
                config.receiving_domain
            )
        });
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO incidents(id,binding_key,feed_cursor,reply_to,clockwork_json,basic_email_json) VALUES(?1,?2,?3,?4,?5,?6)",
            params![incident.id, incident.key, i64::try_from(cursor)?, reply_to, serde_json::to_string(incident)?, serde_json::to_string(basic)?],
        )?;
        if inserted != 0 && incident.resumed_at.is_none() && incident.key != "emt/worker" {
            let id = uuid::Uuid::now_v7().to_string();
            let now = crate::now();
            transaction.execute("INSERT INTO exchanges(id,incident_id,kind,nucleus_job_id,created_at,deadline_at) VALUES(?1,?2,'diagnosis',?1,?3,?4)", params![id,incident.id,now,now+300])?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn exchange(&self, id: &str) -> Result<Exchange> {
        Ok(self.connection.query_row(
            &format!("{EXCHANGE_SELECT} WHERE id=?1"),
            [id],
            exchange_row,
        )?)
    }

    pub fn next(&self) -> Result<Option<Exchange>> {
        Ok(self.connection.query_row(
            &format!("{EXCHANGE_SELECT} WHERE state IN ('pending','running') ORDER BY (state='running') DESC,(kind='reply') DESC,created_at,id LIMIT 1"), [], exchange_row,
        ).optional()?)
    }

    pub fn incident(&self, id: &str) -> Result<Value> {
        Ok(self.connection.query_row("SELECT binding_key,reply_to,clockwork_json,basic_email_json FROM incidents WHERE id=?1", [id], |row| {
            Ok(json!({"id":id,"binding_key":row.get::<_,String>(0)?,"reply_to":row.get::<_,String>(1)?,
                "clockwork":row.get::<_,String>(2)?,"basic_email":row.get::<_,String>(3)?}))
        })?)
    }

    pub fn exchanges(&self, incident: &str) -> Result<Vec<Exchange>> {
        let mut query = self.connection.prepare(&format!(
            "{EXCHANGE_SELECT} WHERE incident_id=?1 ORDER BY created_at,id"
        ))?;
        Ok(query
            .query_map([incident], exchange_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn incident_for_route(&self, address: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT id FROM incidents WHERE reply_to=?1",
                [address],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn capture_reply(
        &self,
        incident: &str,
        message: &email::api::ReceivedMessage,
    ) -> Result<()> {
        let id = uuid::Uuid::now_v7().to_string();
        let now = crate::now();
        self.connection.execute("INSERT OR IGNORE INTO exchanges(id,incident_id,kind,incoming_id,incoming_json,nucleus_job_id,created_at,deadline_at) VALUES(?1,?2,'reply',?3,?4,?1,?5,?6)",
            params![id,incident,message.id,serde_json::to_string(message)?,now,
                time::OffsetDateTime::parse(&message.created_at,&time::format_description::well_known::Rfc3339)
                    .map_or(0,|timestamp|timestamp.unix_timestamp().saturating_add(900))])?;
        Ok(())
    }

    pub fn seen(&self, id: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM exchanges WHERE incoming_id=?1)",
            [id],
            |row| row.get(0),
        )?)
    }

    pub fn outstanding_work(&self) -> Result<i64> {
        Ok(self.connection.query_row("SELECT count(*) FROM exchanges WHERE state IN ('pending','running') OR send_state='pending'", [], |row| row.get(0))?)
    }

    pub fn status(&self) -> Result<Value> {
        let mut query = self
            .connection
            .prepare("SELECT state,count(*) FROM exchanges GROUP BY state")?;
        let counts = query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<std::collections::BTreeMap<_, _>, _>>()?;
        let incidents: i64 =
            self.connection
                .query_row("SELECT count(*) FROM incidents", [], |row| row.get(0))?;
        let uncertain: i64 = self.connection.query_row(
            "SELECT count(*) FROM exchanges WHERE send_state='uncertain'",
            [],
            |row| row.get(0),
        )?;
        Ok(
            json!({"incident_count":incidents,"exchange_states":counts,"uncertain_email_count":uncertain,"outstanding_work_record_count":self.outstanding_work()?}),
        )
    }
}

pub fn private_directory(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(fail("state path must be absolute"));
    }
    if !path.exists() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.mode() & 0o077 != 0 {
        return Err(fail("EMT directories must be private and nonsymbolic"));
    }
    Ok(())
}

fn private_file(path: &Path, create: bool) -> Result<File> {
    if create && !path.exists() {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?
            .sync_all()?;
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.nlink() != 1
        || metadata.mode() & 0o077 != 0
    {
        return Err(fail("EMT files must be private regular files"));
    }
    Ok(OpenOptions::new().read(true).write(true).open(path)?)
}

pub fn lock(root: &Path, name: &str) -> Result<File> {
    private_directory(root)?;
    let file = private_file(&root.join(name), true)?;
    file.try_lock_exclusive()
        .map_err(|_| fail("EMT operation is busy"))?;
    Ok(file)
}

pub fn runner_lock(root: &Path) -> Result<File> {
    lock(root, "runner.lock")
}

pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| fail("file has no parent"))?;
    private_directory(parent)?;
    let temporary = parent.join(format!(".pending-{}", uuid::Uuid::now_v7()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
