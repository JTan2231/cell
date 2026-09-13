//! Platter-owned content and workflow state. Nucleus owns execution history.
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: i64 = 3;
pub const DATABASE: &str = "packets.sqlite3";

pub struct Store {
    pub(crate) connection: Connection,
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketRecord {
    pub id: String,
    pub opportunity: String,
    pub job_id: String,
    pub company: String,
    pub title: String,
    pub status: String,
    /// Accepted only when decoding predecessor records; never used by current artifacts.
    #[serde(default, skip_serializing)]
    pub directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub opportunity: String,
    pub cast_job_id: String,
    pub company: String,
    pub title: String,
    pub eligible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub run_id: Option<String>,
    pub kind: String,
    pub filename: String,
    pub media_type: String,
    pub sha256: String,
    pub content: Vec<u8>,
}

/// The packet IDs and hashes are read projections of the ordered artifacts.
/// They are not separately persisted on an edition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edition {
    pub day: String,
    pub status: String,
    pub subject: String,
    pub body: String,
    pub packet_ids: Vec<String>,
    /// Artifact IDs, never filesystem paths.
    pub attachments: Vec<String>,
    #[serde(default)]
    pub attachment_sha256: Vec<String>,
    pub idempotency_key: String,
    pub receipt: Option<String>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS maintenance_holds(owner TEXT PRIMARY KEY);
CREATE TABLE jobs(
 opportunity TEXT PRIMARY KEY, cast_job_id TEXT NOT NULL, company TEXT NOT NULL,
 title TEXT NOT NULL, eligible INTEGER NOT NULL CHECK(eligible IN (0,1)));
CREATE TABLE runs(
 id TEXT PRIMARY KEY, opportunity TEXT NOT NULL REFERENCES jobs(opportunity),
 status TEXT NOT NULL, inputs TEXT NOT NULL, executions TEXT NOT NULL DEFAULT '{}',
 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
 updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
CREATE INDEX runs_job ON runs(opportunity,created_at);
CREATE TABLE artifacts(
 id TEXT PRIMARY KEY, run_id TEXT REFERENCES runs(id), kind TEXT NOT NULL,
 filename TEXT NOT NULL, media_type TEXT NOT NULL, sha256 TEXT NOT NULL,
 content BLOB NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
 UNIQUE(run_id,kind));
CREATE TABLE editions(
 id TEXT PRIMARY KEY, day TEXT NOT NULL, status TEXT NOT NULL,
 subject TEXT NOT NULL, body TEXT NOT NULL, idempotency_key TEXT NOT NULL UNIQUE,
 receipt TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
CREATE TABLE edition_attachments(
 edition_id TEXT NOT NULL REFERENCES editions(id),
 position INTEGER NOT NULL CHECK(position >= 0),
 artifact_id TEXT NOT NULL REFERENCES artifacts(id),
 PRIMARY KEY(edition_id,position), UNIQUE(edition_id,artifact_id));
CREATE TRIGGER immutable_artifact BEFORE UPDATE ON artifacts
 BEGIN SELECT RAISE(ABORT,'artifact content is immutable'); END;
CREATE TRIGGER immutable_inputs BEFORE UPDATE OF opportunity,inputs ON runs
 BEGIN SELECT RAISE(ABORT,'captured inputs are immutable'); END;
CREATE TRIGGER immutable_edition BEFORE UPDATE OF id,day,subject,body,idempotency_key ON editions
 BEGIN SELECT RAISE(ABORT,'edition payload is immutable'); END;
CREATE TRIGGER immutable_attachment BEFORE UPDATE ON edition_attachments
 BEGIN SELECT RAISE(ABORT,'edition attachments are immutable'); END;
";

impl Store {
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Open control state as well as a supported legacy database during cutover.
    pub(crate) fn control(root: &Path) -> Result<Self> {
        use std::os::unix::fs::PermissionsExt as _;
        crate::private_dir(root)?;
        let path = root.join(DATABASE);
        if path.try_exists()? {
            regular_file(&path)?;
        }
        let connection = Connection::open(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY; PRAGMA synchronous=FULL;",
        )?;
        let store = Self {
            connection,
            root: root.into(),
        };
        let version = store.version()?;
        ensure!(
            matches!(version, 0 | 1 | 2 | SCHEMA_VERSION),
            "unsupported Platter database schema"
        );
        if version == 0 {
            let tables: i64 = store.connection.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |r| r.get(0))?;
            ensure!(tables == 0, "refusing to initialize a foreign database");
            let tx = store.connection.unchecked_transaction()?;
            tx.execute_batch(SCHEMA)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        } else if version == 1 {
            store.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS maintenance_holds(owner TEXT PRIMARY KEY);",
            )?;
        }
        Ok(store)
    }

    pub fn open(root: &Path) -> Result<Self> {
        let store = Self::control(root)?;
        ensure!(
            store.version()? == SCHEMA_VERSION,
            "legacy state requires platter migrate under maintenance"
        );
        Ok(store)
    }

    pub fn open_read_only(root: &Path) -> Result<Self> {
        Self::read_path(&root.join(DATABASE))
    }

    fn read_path(path: &Path) -> Result<Self> {
        regular_file(path)?;
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY;")?;
        let store = Self {
            connection,
            root: path.parent().context("database parent missing")?.into(),
        };
        ensure!(
            store.version()? == SCHEMA_VERSION,
            "legacy state requires platter migrate under maintenance"
        );
        Ok(store)
    }

    pub(crate) fn version(&self) -> Result<i64> {
        Ok(self
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    pub fn setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let value: Option<String> = self
            .connection
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        value
            .map(|value| serde_json::from_str(&value).map_err(Into::into))
            .transpose()
    }

    pub fn set_setting(&self, key: &str, value: &impl Serialize) -> Result<()> {
        self.connection.execute("INSERT INTO settings VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, serde_json::to_string(value)?])?;
        Ok(())
    }

    pub fn initialize(
        &self,
        config: &crate::Config,
        template: &crate::resume::ResumeTemplate,
    ) -> Result<()> {
        ensure!(
            self.setting::<Value>("config")?.is_none(),
            "Platter is already initialized"
        );
        let tx = self.connection.unchecked_transaction()?;
        let artifact = self.put_artifact(
            None,
            "template",
            "original-resume.tex",
            "application/x-tex",
            template.source.as_bytes(),
        )?;
        self.set_setting("config", config)?;
        self.set_setting("template", &artifact.id)?;
        self.set_setting("template_origin", &template.source_path)?;
        tx.commit()?;
        Ok(())
    }

    pub fn template(&self) -> Result<crate::resume::ResumeTemplate> {
        let id: String = self
            .setting("template")?
            .context("original resume is not initialized")?;
        self.template_artifact(&id)
    }

    pub fn template_artifact(&self, id: &str) -> Result<crate::resume::ResumeTemplate> {
        let artifact = self.artifact(id)?;
        ensure!(
            artifact.kind == "template",
            "artifact is not a resume template"
        );
        crate::resume::ResumeTemplate::from_source(
            format!("platter:artifact:{id}"),
            String::from_utf8(artifact.content)?,
        )
    }

    pub fn insert_run(&self, record: &PacketRecord, inputs: &impl Serialize) -> Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        self.connection.execute(
            "INSERT INTO jobs VALUES(?1,?2,?3,?4,1) ON CONFLICT(opportunity) DO NOTHING",
            params![
                record.opportunity,
                record.job_id,
                record.company,
                record.title
            ],
        )?;
        self.connection.execute(
            "INSERT INTO runs(id,opportunity,status,inputs) VALUES(?1,?2,?3,?4)",
            params![
                record.id,
                record.opportunity,
                record.status,
                serde_json::to_string(inputs)?
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn insert(&self, record: &PacketRecord) -> Result<()> {
        self.insert_run(record, &serde_json::json!({}))
    }

    pub fn packet(&self, opportunity: &str) -> Result<Option<PacketRecord>> {
        Ok(self.connection.query_row(&format!("{PACKET_SELECT} WHERE j.opportunity=?1 ORDER BY r.created_at DESC,r.id DESC LIMIT 1"), [opportunity], row_packet).optional()?)
    }

    pub fn run(&self, id: &str) -> Result<PacketRecord> {
        Ok(self.connection.query_row(
            &format!("{PACKET_SELECT} WHERE r.id=?1"),
            [id],
            row_packet,
        )?)
    }

    pub fn inputs<T: DeserializeOwned>(&self, id: &str) -> Result<T> {
        let json: String =
            self.connection
                .query_row("SELECT inputs FROM runs WHERE id=?1", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn execution(&self, run: &str, stage: &str) -> Result<Option<Value>> {
        let json: String =
            self.connection
                .query_row("SELECT executions FROM runs WHERE id=?1", [run], |r| {
                    r.get(0)
                })?;
        let executions: Value = serde_json::from_str(&json)?;
        Ok(executions.get(stage).cloned())
    }

    pub fn save_execution(&self, run: &str, stage: &str, value: &impl Serialize) -> Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        let json: String =
            self.connection
                .query_row("SELECT executions FROM runs WHERE id=?1", [run], |r| {
                    r.get(0)
                })?;
        let mut executions: Value = serde_json::from_str(&json)?;
        executions[stage] = serde_json::to_value(value)?;
        self.connection.execute(
            "UPDATE runs SET executions=?2,updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![run, serde_json::to_string(&executions)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn is_eligible(&self, opportunity: &str) -> Result<bool> {
        Ok(self
            .connection
            .query_row(
                "SELECT eligible FROM jobs WHERE opportunity=?1",
                [opportunity],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(true))
    }

    pub fn set_eligible(&self, opportunity: &str, eligible: bool) -> Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        ensure!(
            self.connection.execute(
                "UPDATE jobs SET eligible=?2 WHERE opportunity=?1 OR cast_job_id=?1",
                params![opportunity, eligible]
            )? == 1,
            "job is absent or ambiguous"
        );
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn exclude_job(&self, record: &PacketRecord) -> Result<()> {
        self.connection.execute(
            "INSERT INTO jobs(opportunity,cast_job_id,company,title,eligible) VALUES(?1,?2,?3,?4,0)
             ON CONFLICT(opportunity) DO UPDATE SET eligible=0",
            params![
                record.opportunity,
                record.job_id,
                record.company,
                record.title
            ],
        )?;
        Ok(())
    }

    pub fn jobs(&self) -> Result<Vec<JobRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT opportunity,cast_job_id,company,title,eligible FROM jobs ORDER BY opportunity",
        )?;
        Ok(statement
            .query_map([], |r| {
                Ok(JobRecord {
                    opportunity: r.get(0)?,
                    cast_job_id: r.get(1)?,
                    company: r.get(2)?,
                    title: r.get(3)?,
                    eligible: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn status(&self, id: &str, status: &str) -> Result<()> {
        ensure!(
            matches!(
                status,
                "preparing" | "declined" | "ready" | "deferred" | "stale"
            ),
            "invalid preparation state"
        );
        ensure!(
            self.connection.execute(
                "UPDATE runs SET status=?2,updated_at=CURRENT_TIMESTAMP WHERE id=?1",
                params![id, status]
            )? == 1,
            "run is absent"
        );
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<PacketRecord>> {
        let mut statement = self
            .connection
            .prepare(&format!("{PACKET_SELECT} ORDER BY r.id"))?;
        Ok(statement
            .query_map([], row_packet)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn put_artifact(
        &self,
        run: Option<&str>,
        kind: &str,
        filename: &str,
        media_type: &str,
        content: &[u8],
    ) -> Result<Artifact> {
        ensure!(
            !filename.is_empty()
                && filename != "."
                && filename != ".."
                && !filename
                    .chars()
                    .any(|c| c.is_control() || c == '/' || c == '\\'),
            "invalid artifact filename"
        );
        if let Some(run) = run
            && let Some(existing) = self.run_artifact(run, kind)?
        {
            ensure!(
                existing.content == content
                    && existing.filename == filename
                    && existing.media_type == media_type,
                "a different output is already committed for this run"
            );
            return Ok(existing);
        }
        let artifact = Artifact {
            id: format!("artifact_{}", uuid::Uuid::now_v7()),
            run_id: run.map(str::to_owned),
            kind: kind.into(),
            filename: filename.into(),
            media_type: media_type.into(),
            sha256: digest(content),
            content: content.into(),
        };
        self.connection.execute("INSERT INTO artifacts(id,run_id,kind,filename,media_type,sha256,content) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![artifact.id,artifact.run_id,artifact.kind,artifact.filename,artifact.media_type,artifact.sha256,artifact.content])?;
        Ok(artifact)
    }

    pub fn artifact(&self, id: &str) -> Result<Artifact> {
        let artifact = self.connection.query_row(
            "SELECT id,run_id,kind,filename,media_type,sha256,content FROM artifacts WHERE id=?1",
            [id],
            row_artifact,
        )?;
        ensure!(
            digest(&artifact.content) == artifact.sha256,
            "retained artifact integrity mismatch"
        );
        Ok(artifact)
    }

    pub fn run_artifact(&self, run: &str, kind: &str) -> Result<Option<Artifact>> {
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM artifacts WHERE run_id=?1 AND kind=?2",
                params![run, kind],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| self.artifact(&id)).transpose()
    }

    pub fn content<T: DeserializeOwned>(&self, run: &str, kind: &str) -> Result<Option<T>> {
        self.run_artifact(run, kind)?
            .map(|a| serde_json::from_slice(&a.content).map_err(Into::into))
            .transpose()
    }

    pub fn put_content(&self, run: &str, kind: &str, value: &impl Serialize) -> Result<Artifact> {
        self.put_artifact(
            Some(run),
            kind,
            &format!("{kind}.json"),
            "application/json",
            &serde_json::to_vec(value)?,
        )
    }

    pub fn edition(&self, id: &str) -> Result<Option<Edition>> {
        let edition = self
            .connection
            .query_row(
                "SELECT day,status,subject,body,idempotency_key,receipt FROM editions WHERE id=?1",
                [id],
                |r| {
                    Ok(Edition {
                        day: r.get(0)?,
                        status: r.get(1)?,
                        subject: r.get(2)?,
                        body: r.get(3)?,
                        idempotency_key: r.get(4)?,
                        receipt: r.get(5)?,
                        packet_ids: vec![],
                        attachments: vec![],
                        attachment_sha256: vec![],
                    })
                },
            )
            .optional()?;
        let Some(mut edition) = edition else {
            return Ok(None);
        };
        let mut statement = self.connection.prepare(
            "SELECT artifact_id FROM edition_attachments WHERE edition_id=?1 ORDER BY position",
        )?;
        let ids = statement
            .query_map([id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            let artifact = self.artifact(&id)?;
            if let Some(run) = artifact.run_id {
                edition.packet_ids.push(run);
            }
            edition.attachments.push(id);
            edition.attachment_sha256.push(artifact.sha256);
        }
        Ok(Some(edition))
    }

    pub fn freeze(&mut self, edition: &Edition) -> Result<()> {
        self.freeze_as(&edition.day, edition, true)
    }

    pub fn freeze_as(&self, id: &str, edition: &Edition, spend: bool) -> Result<()> {
        ensure!(
            (1..=3).contains(&edition.attachments.len()),
            "edition must contain one through three attachments"
        );
        let tx = self.connection.unchecked_transaction()?;
        self.insert_edition(id, edition)?;
        if spend {
            for artifact_id in &edition.attachments {
                let artifact = self.artifact(artifact_id)?;
                let run = artifact
                    .run_id
                    .context("selected PDF has no preparation run")?;
                ensure!(self.connection.execute("UPDATE jobs SET eligible=0 WHERE eligible=1 AND opportunity=(SELECT opportunity FROM runs WHERE id=?1 AND status='ready')", [run])? == 1, "job is ineligible or packet is not ready");
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn insert_edition(&self, id: &str, edition: &Edition) -> Result<()> {
        self.connection.execute("INSERT INTO editions(id,day,status,subject,body,idempotency_key,receipt) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![id,edition.day,edition.status,edition.subject,edition.body,edition.idempotency_key,edition.receipt])?;
        for (position, artifact_id) in edition.attachments.iter().enumerate() {
            let artifact = self.artifact(artifact_id)?;
            ensure!(
                artifact.media_type == "application/pdf" && artifact.content.starts_with(b"%PDF-"),
                "attachment is not a retained PDF"
            );
            self.connection.execute(
                "INSERT INTO edition_attachments VALUES(?1,?2,?3)",
                params![id, i64::try_from(position)?, artifact_id],
            )?;
        }
        Ok(())
    }

    pub fn begin_send(&self, key: &str) -> Result<()> {
        ensure!(
            self.connection.execute(
                "UPDATE editions SET status='sending' WHERE idempotency_key=?1 AND status='frozen'",
                [key]
            )? == 1,
            "edition is already sending, accepted, or held"
        );
        Ok(())
    }

    pub fn save_edition(&mut self, edition: &Edition) -> Result<()> {
        ensure!(
            edition.status == "sent" && edition.receipt.is_some(),
            "only a retained acceptance may finish a send"
        );
        ensure!(self.connection.execute("UPDATE editions SET status='sent',receipt=?2 WHERE idempotency_key=?1 AND status='sending'", params![edition.idempotency_key,edition.receipt])? == 1, "edition has no pending send");
        Ok(())
    }

    /// A complete current-schema snapshot. Never silently replace an existing backup.
    pub fn backup(&self, destination: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;
        ensure!(
            destination.is_absolute(),
            "backup must use an absolute path"
        );
        if destination.try_exists()? {
            let retained = Self::read_path(destination)?;
            ensure!(
                self.content_fingerprint()? == retained.content_fingerprint()?,
                "existing backup does not match the retained library"
            );
            return Ok(());
        }
        let parent = destination.parent().context("backup parent missing")?;
        crate::private_dir(parent)?;
        let temporary = tempfile::NamedTempFile::new_in(parent)?;
        self.connection.execute(
            "VACUUM INTO ?1",
            [temporary.path().to_string_lossy().as_ref()],
        )?;
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o600))?;
        temporary.as_file().sync_all()?;
        temporary.persist_noclobber(destination)?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    }

    fn content_fingerprint(&self) -> Result<String> {
        use rusqlite::types::ValueRef;
        let mut hash = Sha256::new();
        for query in [
            "SELECT * FROM settings WHERE key NOT IN ('migration_backup','migration_cleanup') ORDER BY key",
            "SELECT * FROM jobs ORDER BY opportunity",
            "SELECT * FROM runs ORDER BY id",
            "SELECT * FROM artifacts ORDER BY id",
            "SELECT * FROM editions ORDER BY id",
            "SELECT * FROM edition_attachments ORDER BY edition_id,position",
            "SELECT * FROM maintenance_holds ORDER BY owner",
        ] {
            hash.update(query.as_bytes());
            let mut statement = self.connection.prepare(query)?;
            let columns = statement.column_count();
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                hash.update(b"row");
                for column in 0..columns {
                    match row.get_ref(column)? {
                        ValueRef::Null => hash.update(b"null"),
                        ValueRef::Integer(value) => {
                            hash.update(b"int");
                            hash.update(value.to_le_bytes());
                        }
                        ValueRef::Real(value) => {
                            hash.update(b"real");
                            hash.update(value.to_bits().to_le_bytes());
                        }
                        ValueRef::Text(value) | ValueRef::Blob(value) => {
                            hash.update(b"bytes");
                            hash.update(value.len().to_le_bytes());
                            hash.update(value);
                        }
                    }
                }
            }
        }
        Ok(format!("{:x}", hash.finalize()))
    }

    pub(crate) fn create_schema(&self) -> Result<()> {
        self.connection.execute_batch(SCHEMA)?;
        Ok(())
    }
}

const PACKET_SELECT: &str = "SELECT r.id,j.opportunity,j.cast_job_id,j.company,j.title,r.status FROM runs r JOIN jobs j ON j.opportunity=r.opportunity";
fn row_packet(r: &rusqlite::Row<'_>) -> rusqlite::Result<PacketRecord> {
    Ok(PacketRecord {
        id: r.get(0)?,
        opportunity: r.get(1)?,
        job_id: r.get(2)?,
        company: r.get(3)?,
        title: r.get(4)?,
        status: r.get(5)?,
        directory: String::new(),
    })
}
fn row_artifact(r: &rusqlite::Row<'_>) -> rusqlite::Result<Artifact> {
    Ok(Artifact {
        id: r.get(0)?,
        run_id: r.get(1)?,
        kind: r.get(2)?,
        filename: r.get(3)?,
        media_type: r.get(4)?,
        sha256: r.get(5)?,
        content: r.get(6)?,
    })
}
pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn regular_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1,
        "private state must be a regular single-link file"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reserved_or_sent_opportunity_cannot_be_selected_twice() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(dir.path())?;
        let record = PacketRecord {
            id: "p1".into(),
            opportunity: "ashby:employer:role".into(),
            job_id: "cast1".into(),
            company: "Example".into(),
            title: "Engineer".into(),
            status: "ready".into(),
            directory: "/private/p1".into(),
        };
        store.insert(&record)?;
        let pdf = store.put_artifact(
            Some(&record.id),
            "resume-pdf",
            "resume.pdf",
            "application/pdf",
            b"%PDF-fixture",
        )?;
        let mut edition = Edition {
            day: "2026-09-06".into(),
            status: "frozen".into(),
            subject: "Jobs".into(),
            body: "Brief".into(),
            packet_ids: vec![record.id.clone()],
            attachments: vec![pdf.id],
            attachment_sha256: vec![pdf.sha256],
            idempotency_key: "jobs/day".into(),
            receipt: None,
        };
        store.freeze(&edition)?;
        assert!(store.freeze(&edition).is_err());
        assert!(!store.is_eligible(&record.opportunity)?);
        store.begin_send(&edition.idempotency_key)?;
        edition.status = "sent".into();
        edition.receipt = Some("Accepted fixture".into());
        store.save_edition(&edition)?;
        assert_eq!(
            store.packet(&record.opportunity)?.map(|p| p.status),
            Some("ready".into())
        );
        assert!(store.insert(&record).is_err());
        Ok(())
    }
}
