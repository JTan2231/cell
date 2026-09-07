use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub struct Store {
    connection: Connection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketRecord {
    pub id: String,
    pub opportunity: String,
    pub job_id: String,
    pub company: String,
    pub title: String,
    pub status: String,
    pub directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edition {
    pub day: String,
    pub status: String,
    pub subject: String,
    pub body: String,
    pub packet_ids: Vec<String>,
    pub attachments: Vec<String>,
    #[serde(default)]
    pub attachment_sha256: Vec<String>,
    pub idempotency_key: String,
    pub receipt: Option<String>,
}

impl Store {
    pub fn open_read_only(directory: &Path) -> Result<Self> {
        Self::read_path(&directory.join("packets.sqlite3"))
    }

    fn read_path(path: &Path) -> Result<Self> {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = std::fs::symlink_metadata(path)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1,
            "database must be a regular file"
        );
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        ensure!(
            connection.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? == 1,
            "unsupported Platter database schema"
        );
        let check: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        ensure!(check == "ok", "Platter database integrity check failed");
        let store = Self { connection };
        store.list()?;
        let mut editions = store.connection.prepare("SELECT json FROM editions")?;
        for json in editions.query_map([], |row| row.get::<_, String>(0))? {
            let _: Edition = serde_json::from_str(&json?)?;
        }
        drop(editions);
        Ok(store)
    }

    /// `SQLite` produces a consistent standalone snapshot, including WAL content.
    pub fn backup(&self, destination: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;
        ensure!(destination.is_absolute(), "backup path must be absolute");
        match std::fs::symlink_metadata(destination) {
            Ok(_) => {
                let retained = Self::read_path(destination)?;
                ensure!(
                    self.snapshot()? == retained.snapshot()?,
                    "existing backup does not match current state"
                );
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let parent = destination
            .parent()
            .ok_or_else(|| anyhow::anyhow!("backup parent missing"))?;
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

    fn snapshot(&self) -> Result<serde_json::Value> {
        let mut statement = self
            .connection
            .prepare("SELECT day,json FROM editions ORDER BY day")?;
        let editions = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(serde_json::json!({"packets":self.list()?,"editions":editions}))
    }

    pub fn open(directory: &Path) -> Result<Self> {
        use std::os::unix::fs::PermissionsExt as _;
        crate::private_dir(directory)?;
        let path = directory.join("packets.sqlite3");
        let connection = Connection::open(&path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(
            matches!(version, 0 | 1),
            "unsupported Platter database schema"
        );
        if version == 0 {
            let tables: i64 = connection.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |row| row.get(0))?;
            ensure!(tables == 0, "refusing to initialize a foreign database");
        }
        connection.execute_batch("PRAGMA foreign_keys=ON;
          CREATE TABLE IF NOT EXISTS packets(id TEXT PRIMARY KEY, opportunity TEXT UNIQUE NOT NULL, job_id TEXT NOT NULL, company TEXT NOT NULL, title TEXT NOT NULL, status TEXT NOT NULL, directory TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS editions(day TEXT PRIMARY KEY, json TEXT NOT NULL);
          PRAGMA user_version=1;")?;
        Ok(Self { connection })
    }

    pub fn packet(&self, opportunity: &str) -> Result<Option<PacketRecord>> {
        self.connection.query_row("SELECT id,opportunity,job_id,company,title,status,directory FROM packets WHERE opportunity=?1", [opportunity], row_packet).optional().map_err(Into::into)
    }

    pub fn insert(&self, record: &PacketRecord) -> Result<()> {
        self.connection.execute(
            "INSERT INTO packets VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                record.id,
                record.opportunity,
                record.job_id,
                record.company,
                record.title,
                record.status,
                record.directory
            ],
        )?;
        Ok(())
    }

    pub fn status(&self, id: &str, status: &str) -> Result<()> {
        ensure!(
            matches!(
                status,
                "preparing" | "declined" | "ready" | "deferred" | "stale" | "reserved" | "sent"
            ),
            "invalid packet state"
        );
        let changed = self.connection.execute(
            "UPDATE packets SET status=?2 WHERE id=?1 AND status NOT IN ('reserved','sent')",
            params![id, status],
        )?;
        ensure!(changed == 1, "packet absent or already reserved/sent");
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<PacketRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT id,opportunity,job_id,company,title,status,directory FROM packets ORDER BY id",
        )?;
        Ok(statement
            .query_map([], row_packet)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn edition(&self, day: &str) -> Result<Option<Edition>> {
        let json: Option<String> = self
            .connection
            .query_row("SELECT json FROM editions WHERE day=?1", [day], |row| {
                row.get(0)
            })
            .optional()?;
        json.map(|json| serde_json::from_str(&json).map_err(Into::into))
            .transpose()
    }

    pub fn freeze(&mut self, edition: &Edition) -> Result<()> {
        ensure!(
            !edition.packet_ids.is_empty() && edition.packet_ids.len() <= 3,
            "edition must have one through three packets"
        );
        let tx = self.connection.transaction()?;
        for id in &edition.packet_ids {
            ensure!(
                tx.execute(
                    "UPDATE packets SET status='reserved' WHERE id=?1 AND status='ready'",
                    [id]
                )? == 1,
                "packet already reserved or sent"
            );
        }
        tx.execute(
            "INSERT INTO editions VALUES (?1,?2)",
            params![edition.day, serde_json::to_string(edition)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn save_edition(&mut self, edition: &Edition) -> Result<()> {
        let tx = self.connection.transaction()?;
        tx.execute(
            "UPDATE editions SET json=?2 WHERE day=?1",
            params![edition.day, serde_json::to_string(edition)?],
        )?;
        if edition.status == "sent" {
            for id in &edition.packet_ids {
                tx.execute(
                    "UPDATE packets SET status='sent' WHERE id=?1 AND status='reserved'",
                    [id],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn row_packet(row: &rusqlite::Row<'_>) -> rusqlite::Result<PacketRecord> {
    Ok(PacketRecord {
        id: row.get(0)?,
        opportunity: row.get(1)?,
        job_id: row.get(2)?,
        company: row.get(3)?,
        title: row.get(4)?,
        status: row.get(5)?,
        directory: row.get(6)?,
    })
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
        let mut edition = Edition {
            day: "2026-09-06".into(),
            status: "frozen".into(),
            subject: "Jobs".into(),
            body: "Brief".into(),
            packet_ids: vec![record.id.clone()],
            attachments: vec![],
            attachment_sha256: vec![],
            idempotency_key: "jobs/day".into(),
            receipt: None,
        };
        store.freeze(&edition)?;
        assert!(store.freeze(&edition).is_err());
        assert!(store.status(&record.id, "ready").is_err());
        edition.status = "sent".into();
        store.save_edition(&edition)?;
        assert_eq!(
            store.packet(&record.opportunity)?.map(|p| p.status),
            Some("sent".into())
        );
        assert!(store.insert(&record).is_err());
        Ok(())
    }
}
