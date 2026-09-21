//! Private send records are separate from the unchanged application ledger.
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Local, Timelike as _, Utc};
use email::api::{Client, Message, ReplyOptions};
use fs2::FileExt as _;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
    path::Path,
};

use crate::{
    digest::Digest,
    store::{Store, private_file},
};

const DATABASE: &str = "email.sqlite3";

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Delivery {
    occurrence: String,
    pub digest: Digest,
    idempotency_key: String,
    attempted_at: Option<i64>,
    accepted_id: Option<String>,
}

fn open(root: &Path, writable: bool) -> Result<Connection> {
    let metadata = std::fs::symlink_metadata(root)?;
    ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.permissions().mode() & 0o777 == 0o700,
        "Clew state directory must be private (0700)"
    );
    let path = root.join(DATABASE);
    if writable {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => file.sync_all()?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    private_file(&path)?;
    let mut connection = Connection::open_with_flags(
        path,
        if writable {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        },
    )?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA temp_store=MEMORY;")?;
    if writable {
        connection.execute_batch("PRAGMA synchronous=FULL;")?;
        let tx = connection.transaction()?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == 0 {
            let tables: i64 = tx.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |row| row.get(0))?;
            ensure!(
                tables == 0,
                "refusing to initialize a foreign email database"
            );
            tx.execute_batch("CREATE TABLE deliveries (occurrence TEXT PRIMARY KEY, record TEXT NOT NULL); PRAGMA user_version=1;")?;
        }
        tx.commit()?;
        std::fs::File::open(root)?.sync_all()?;
    }
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    ensure!(version == 1, "unsupported Clew email schema");
    Ok(connection)
}

fn delivery(connection: &Connection, id: &str) -> Result<Option<Delivery>> {
    let record: Option<String> = connection
        .query_row(
            "SELECT record FROM deliveries WHERE occurrence=?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    record
        .map(|record| serde_json::from_str(&record).map_err(Into::into))
        .transpose()
}

pub(crate) fn retained(root: &Path, id: &str) -> Result<Option<Delivery>> {
    delivery(&open(root, false)?, id)
}

fn save(connection: &Connection, outgoing: &Delivery) -> Result<()> {
    connection.execute("INSERT INTO deliveries(occurrence,record) VALUES(?1,?2) ON CONFLICT(occurrence) DO UPDATE SET record=excluded.record",
        params![outgoing.occurrence, serde_json::to_string(outgoing)?])?;
    Ok(())
}

fn scheduled_occurrence(now: DateTime<Local>) -> Result<String> {
    let date = if now.hour() < 9 {
        now.date_naive()
            .pred_opt()
            .context("local date has no predecessor")?
    } else {
        now.date_naive()
    };
    Ok(format!("daily/{date}"))
}

fn check_retry(attempted: i64, now: i64, retry: bool, occurrence: &str) -> Result<()> {
    ensure!(
        retry,
        "email occurrence {occurrence} has uncertain acceptance; inspect the provider before using email send --retry {occurrence}"
    );
    ensure!(
        now.checked_sub(attempted)
            .is_some_and(|age| (0..23 * 3600).contains(&age)),
        "email retry is outside the safe provider idempotency window; no message was sent"
    );
    Ok(())
}

/// Send only with explicit authority, or the standing authority of an enabled binding.
pub fn send(root: &Path, scheduled: bool, retry: Option<&str>) -> Result<Value> {
    ensure!(
        !scheduled || retry.is_none(),
        "--scheduled conflicts with --retry"
    );
    // Missing ledger state must fail before creating any send state.
    Store::open(root, false)?;
    let gate = crate::gate(root);
    let _admission = match gate.enter() {
        Ok(admission) => admission,
        Err(cell_maintenance::Error::Held) if scheduled => {
            return Ok(json!({"scheduled":true,"skipped":"deployment_maintenance"}));
        }
        Err(error) => return Err(error.into()),
    };
    let lock_path = root.join("email.lock");
    if lock_path.exists() || lock_path.is_symlink() {
        private_file(&lock_path)?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(lock_path)?;
    lock.try_lock_exclusive()
        .context("another Clew email send is running")?;
    let store = open(root, true)?;
    let occurrence = if let Some(id) = retry {
        id.to_owned()
    } else if scheduled {
        scheduled_occurrence(Local::now())?
    } else {
        format!("manual/{}", uuid::Uuid::now_v7())
    };
    let mut outgoing = if let Some(existing) = delivery(&store, &occurrence)? {
        existing
    } else {
        ensure!(retry.is_none(), "unknown email occurrence");
        let outgoing = Delivery {
            occurrence: occurrence.clone(),
            digest: crate::digest::prepare(root)?,
            idempotency_key: format!("clew-email/{}", uuid::Uuid::now_v7()),
            attempted_at: None,
            accepted_id: None,
        };
        save(&store, &outgoing)?;
        outgoing
    };
    if let Some(id) = &outgoing.accepted_id {
        return Ok(json!({"occurrence":occurrence,"accepted_id":id,"already_accepted":true}));
    }
    if let Some(attempted) = outgoing.attempted_at {
        check_retry(
            attempted,
            Utc::now().timestamp(),
            retry.is_some(),
            &occurrence,
        )?;
    }
    let client = Client::new(crate::home()?.join(".local/bin/email"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    outgoing.attempted_at.get_or_insert(Utc::now().timestamp());
    save(&store, &outgoing)?;
    let receipt = runtime.block_on(client.send_with_options(&Message {
        subject: outgoing.digest.subject.clone(),
        body: outgoing.digest.body.clone(),
        idempotency_key: Some(outgoing.idempotency_key.clone()),
    }, &[], &ReplyOptions::default()))
        .with_context(|| format!("email occurrence {occurrence} has uncertain acceptance; inspect the provider before retrying"))?;
    outgoing.accepted_id = Some(receipt.id.clone());
    save(&store, &outgoing).with_context(|| format!("email occurrence {occurrence} acceptance receipt could not be saved; inspect the provider before retrying"))?;
    Ok(
        json!({"occurrence":occurrence,"accepted_id":receipt.id,"application_count":outgoing.digest.application_count,"already_accepted":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    #[test]
    fn daily_identity_and_retry_boundaries() -> Result<()> {
        let before = Local
            .with_ymd_and_hms(2026, 9, 20, 8, 59, 59)
            .single()
            .context("local time")?;
        let at = Local
            .with_ymd_and_hms(2026, 9, 20, 9, 0, 0)
            .single()
            .context("local time")?;
        assert_eq!(scheduled_occurrence(before)?, "daily/2026-09-19");
        assert_eq!(scheduled_occurrence(at)?, "daily/2026-09-20");
        assert!(check_retry(100, 100, true, "id").is_ok());
        assert!(check_retry(100, 99, true, "id").is_err());
        assert!(check_retry(100, 100 + 23 * 3600, true, "id").is_err());
        assert!(check_retry(100, 101, false, "id").is_err());
        Ok(())
    }
}
