//! Annals-owned, read-only usage attribution access.
//! Consumers receive typed records; SQL and inbox receipt decoding remain here.
#![allow(clippy::missing_errors_doc)]

use rusqlite::{Connection, OpenFlags, OptionalExtension as _};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DeliveryRecord {
    pub id: i64,
    pub source_name: String,
    pub status: String,
    pub result: Option<String>,
    pub work_id: Option<i64>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ModelRunIdentity {
    pub id: i64,
    pub work_id: i64,
    pub work_label: String,
    pub base_revision: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReceiptSummary {
    pub id: String,
    #[serde(default)]
    pub attempts: u32,
    pub ingestion_id: Option<i64>,
    pub model_run_token: Option<String>,
    pub reconciliation_id: Option<i64>,
    pub result_status: Option<String>,
}

#[derive(Debug, Default)]
pub struct Receipts {
    pub by_token: HashMap<String, ReceiptSummary>,
    pub by_delivery: HashMap<i64, ReceiptSummary>,
}

#[derive(Debug)]
pub struct Library {
    connection: Connection,
}

impl Library {
    pub fn open(path: &Path) -> Result<Self, Error> {
        Ok(Self {
            connection: Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?,
        })
    }
    pub fn deliveries(&self, limit: usize) -> Result<Vec<DeliveryRecord>, Error> {
        read_deliveries(&self.connection, limit)
    }
    pub fn model_run_identities(&self) -> Result<HashMap<String, ModelRunIdentity>, Error> {
        read_model_run_identities(&self.connection)
    }
    pub fn has_model_run(&self, token: &str) -> Result<bool, Error> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM model_runs WHERE token = ?1)",
                [token],
                |row| row.get::<_, bool>(0),
            )
            .map_err(Into::into)
    }
    pub fn selected_model_run_id(
        &self,
        reconciliation_id: Option<i64>,
    ) -> Result<Option<i64>, Error> {
        selected_model_run_id(&self.connection, reconciliation_id)
    }
}

fn read_deliveries(connection: &Connection, limit: usize) -> Result<Vec<DeliveryRecord>, Error> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut statement = connection.prepare(
        "SELECT id, source_name, status, result, work_id \
         FROM ingestions ORDER BY id DESC LIMIT ?1",
    )?;
    let records = statement.query_map([limit], |row| {
        Ok(DeliveryRecord {
            id: row.get(0)?,
            source_name: row.get(1)?,
            status: row.get(2)?,
            result: row.get(3)?,
            work_id: row.get(4)?,
        })
    })?;
    records.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_model_run_identities(
    connection: &Connection,
) -> Result<HashMap<String, ModelRunIdentity>, Error> {
    let mut statement = connection.prepare(
        "SELECT m.token, m.id, m.work_id, w.label, m.base_revision \
         FROM model_runs AS m JOIN works AS w ON w.id = m.work_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            ModelRunIdentity {
                id: row.get(1)?,
                work_id: row.get(2)?,
                work_label: row.get(3)?,
                base_revision: row.get(4)?,
            },
        ))
    })?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(Into::into)
}

pub fn read_receipts(spool: &Path) -> Result<Receipts, Error> {
    let mut receipts = Receipts::default();
    for state in ["processing", "done", "duplicates", "failed", "skipped"] {
        let directory = spool.join(state);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(Error::ReadDirectory { directory, source }),
        };
        for entry in entries {
            let path = entry?.path().join("job.json");
            let document = match fs::read_to_string(&path) {
                Ok(document) => document,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => return Err(Error::ReadReceipt { path, source }),
            };
            let receipt: ReceiptSummary = serde_json::from_str(&document)?;
            if let Some(delivery_id) = receipt.ingestion_id {
                receipts.by_delivery.insert(delivery_id, receipt.clone());
            }
            if let Some(token) = receipt.model_run_token.clone() {
                if let Some(existing) = receipts.by_token.get(&token)
                    && existing.id != receipt.id
                {
                    return Err(Error::DuplicateModelRunReceipt {
                        token,
                        first_job: existing.id.clone(),
                        second_job: receipt.id,
                    });
                }
                receipts.by_token.insert(token, receipt);
            }
        }
    }
    Ok(receipts)
}

fn selected_model_run_id(
    library: &Connection,
    reconciliation_id: Option<i64>,
) -> Result<Option<i64>, Error> {
    let Some(reconciliation_id) = reconciliation_id else {
        return Ok(None);
    };
    library
        .query_row(
            "SELECT model_run_id FROM reconciliations WHERE id = ?1",
            [reconciliation_id],
            |row| row.get(0),
        )
        .optional()
        .map(Option::flatten)
        .map_err(Into::into)
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("unable to read inbox directory {directory}: {source}")]
    ReadDirectory {
        directory: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unable to read job receipt {path}: {source}")]
    ReadReceipt {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("model run token {token} appears in multiple inbox jobs: {first_job} and {second_job}")]
    DuplicateModelRunReceipt {
        token: String,
        first_job: String,
        second_job: String,
    },
}
