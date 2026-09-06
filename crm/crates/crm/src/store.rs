use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::str::FromStr as _;
use std::time::Duration;

use rusqlite::{
    Connection, OpenFlags, OptionalExtension as _, Transaction, TransactionBehavior, params,
};
use sha2::{Digest as _, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::error::io;
use crate::model::{
    CaseListItem, CaseRevision, Correlation, Delivery, MailboxReceipt, ProfileEntry,
    RevisionProposal, SearchResult, Stage, StewardUpdate, UpdateStatus,
};
use crate::{Error, Result};

const SCHEMA: &str = include_str!("../schema.sql");
const MIGRATE_V1_TO_V2: &str = include_str!("../migrations/001-to-002.sql");
pub const SCHEMA_VERSION: i64 = 2;
const WORKER_LEASE_MAX_AGE_SECONDS: i64 = 30 * 60;
const REQUIRED_TABLES: &[&str] = &[
    "crm_meta",
    "cases",
    "deliveries",
    "steward_updates",
    "case_revisions",
    "mailbox_receipts",
];
const REQUIRED_INDEXES: &[&str] = &[
    "one_running_update_per_case",
    "one_retry_per_update",
    "steward_updates_queue",
    "case_revisions_latest",
];
const REQUIRED_TRIGGERS: &[&str] = &[
    "deliveries_no_update",
    "deliveries_no_delete",
    "case_revisions_no_update",
    "case_revisions_no_delete",
    "mailbox_receipts_no_update",
    "mailbox_receipts_no_delete",
];
const REQUIRED_COLUMNS: &[(&str, &str)] = &[
    ("crm_meta", "worker_token"),
    ("crm_meta", "worker_pid"),
    ("crm_meta", "worker_acquired_at"),
    ("steward_updates", "result_posted"),
    ("steward_updates", "runtime_state"),
    ("steward_updates", "runtime_detail"),
];

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct InitResult {
    pub created: bool,
    pub schema_version: i64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct MigrationResult {
    pub changed: bool,
    pub from_schema_version: i64,
    pub schema_version: i64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DoctorResult {
    pub schema_version: i64,
    pub foreign_keys: &'static str,
    pub integrity: String,
}

#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
}

#[derive(Debug)]
pub struct WorkerLease {
    store: Store,
    token: String,
}

impl Drop for WorkerLease {
    fn drop(&mut self) {
        let _result = self.store.release_worker_lease(&self.token);
    }
}

impl WorkerLease {
    pub(crate) fn refresh(&mut self) -> Result<()> {
        let changed = self.store.connection(false)?.execute(
            "UPDATE crm_meta SET worker_acquired_at = ?2
             WHERE marker = 'crm' AND worker_token = ?1",
            params![self.token, now()?],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "worker_lease_lost",
                "CRM worker lease was replaced before it could be refreshed",
            ));
        }
        Ok(())
    }

    pub(crate) fn claim_next_or_release(&mut self) -> Result<Option<StewardUpdate>> {
        let mut connection = self.store.connection(false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let update_id = next_eligible_update_id_tx(&transaction)?;
        let changed = if let Some(update_id) = &update_id {
            freeze_update_tx(&transaction, update_id)?;
            transaction.execute(
                "UPDATE crm_meta SET worker_acquired_at = ?2
                 WHERE marker = 'crm' AND worker_token = ?1",
                params![self.token, now()?],
            )?
        } else {
            transaction.execute(
                "UPDATE crm_meta
                 SET worker_token = NULL, worker_pid = NULL, worker_acquired_at = NULL
                 WHERE marker = 'crm' AND worker_token = ?1",
                [&self.token],
            )?
        };
        if changed != 1 {
            return Err(Error::domain(
                "worker_lease_lost",
                "CRM worker lease was replaced before final queue handoff",
            ));
        }
        transaction.commit()?;
        update_id
            .map(|update_id| self.store.update(&update_id))
            .transpose()
    }

    pub(crate) fn release_and_has_eligible_queue(&mut self) -> Result<bool> {
        let mut connection = self.store.connection(false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let has_eligible_queue = next_eligible_update_id_tx(&transaction)?.is_some();
        let changed = transaction.execute(
            "UPDATE crm_meta
             SET worker_token = NULL, worker_pid = NULL, worker_acquired_at = NULL
             WHERE marker = 'crm' AND worker_token = ?1",
            [&self.token],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "worker_lease_lost",
                "CRM worker lease was replaced before resume handoff",
            ));
        }
        transaction.commit()?;
        Ok(has_eligible_queue)
    }
}

impl Store {
    pub fn init(path: &Path) -> Result<InitResult> {
        ensure_parent(path, true)?;
        let created = match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(Error::domain(
                        "database_file_invalid",
                        format!("database must be a regular file: {}", path.display()),
                    ));
                }
                false
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => true,
            Err(source) => return Err(io(path, source)),
        };
        if created {
            create_private_file(path)?;
        } else {
            secure_path(path)?;
        }
        let mut connection = open_connection(path, false)?;
        if created {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute_batch(SCHEMA)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
        } else {
            require_schema(&connection)?;
        }
        connection.pragma_update(None, "journal_mode", "WAL")?;
        secure_path(path)?;
        secure_sidecars(path)?;
        Ok(InitResult {
            created,
            schema_version: SCHEMA_VERSION,
        })
    }

    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        ensure_parent(&path, false)?;
        secure_path(&path)?;
        let connection = open_connection(&path, false)?;
        require_schema(&connection)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        secure_sidecars(&path)?;
        Ok(Self { path })
    }

    pub fn migrate(path: &Path, backup_path: &Path) -> Result<MigrationResult> {
        ensure_parent(path, false)?;
        secure_path(path)?;
        secure_sidecars(path)?;
        let mut connection = open_connection(path, false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 =
            transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == SCHEMA_VERSION {
            require_schema(&transaction)?;
            return Ok(MigrationResult {
                changed: false,
                from_schema_version: version,
                schema_version: SCHEMA_VERSION,
            });
        }
        if version != 1 {
            return Err(Error::domain(
                "schema_migration_unsupported",
                format!("CRM cannot migrate schema {version}; only schema 1 can migrate to 2"),
            ));
        }
        require_schema_version(&transaction, version)?;
        require_migration_quiescence(&transaction)?;
        check_integrity(&transaction)?;

        // The immediate transaction prevents writes between this committed snapshot
        // and migration. A separate reader lets VACUUM INTO include committed WAL
        // content without copying live SQLite database or sidecar bytes.
        create_migration_backup(path, backup_path, version)?;
        transaction.execute_batch(MIGRATE_V1_TO_V2)?;
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        require_schema(&transaction)?;
        check_integrity(&transaction)?;
        transaction.commit()?;
        secure_sidecars(path)?;
        Ok(MigrationResult {
            changed: true,
            from_schema_version: version,
            schema_version: SCHEMA_VERSION,
        })
    }

    pub fn doctor(path: &Path) -> Result<DoctorResult> {
        let store = Self::open(path.to_path_buf())?;
        let connection = store.connection(true)?;
        check_integrity(&connection)?;
        Ok(DoctorResult {
            schema_version: SCHEMA_VERSION,
            foreign_keys: "ok",
            integrity: "ok".to_owned(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn acquire_worker_lease(&self) -> Result<WorkerLease> {
        let token = format!("worker-{}", Uuid::now_v7());
        let pid = i64::from(process::id());
        let acquired = OffsetDateTime::now_utc();
        let acquired_at = format_timestamp(acquired)?;
        let mut connection = self.connection(false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (owner_token, owner_pid, owner_acquired_at): (
            Option<String>,
            Option<i64>,
            Option<String>,
        ) = transaction.query_row(
            "SELECT worker_token, worker_pid, worker_acquired_at
             FROM crm_meta WHERE marker = 'crm'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if owner_token.is_some()
            && !worker_owner_is_reclaimable(owner_pid, owner_acquired_at.as_deref(), acquired)?
        {
            return Err(Error::domain(
                "worker_already_running",
                format!(
                    "CRM worker lease is held by process {}",
                    owner_pid.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
                ),
            ));
        }
        let changed = transaction.execute(
            "UPDATE crm_meta
             SET worker_token = ?1, worker_pid = ?2, worker_acquired_at = ?3
             WHERE marker = 'crm' AND worker_token IS ?4",
            params![token, pid, acquired_at, owner_token],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "worker_lease_conflict",
                "CRM worker ownership changed during acquisition",
            ));
        }
        transaction.commit()?;
        Ok(WorkerLease {
            store: self.clone(),
            token,
        })
    }

    fn release_worker_lease(&self, token: &str) -> Result<()> {
        self.connection(false)?.execute(
            "UPDATE crm_meta
             SET worker_token = NULL, worker_pid = NULL, worker_acquired_at = NULL
             WHERE marker = 'crm' AND worker_token = ?1",
            [token],
        )?;
        Ok(())
    }

    pub fn create_profile_entry(&self, title: &str, body_md: &str) -> Result<ProfileEntry> {
        let title = title.trim();
        validate_text(title, "profile_title", 1_000, false)?;
        validate_text(body_md, "profile_body", 1024 * 1024, true)?;
        let entry = ProfileEntry {
            id: format!("profile-{}", Uuid::now_v7()),
            title: title.to_owned(),
            body_md: body_md.to_owned(),
            updated_at: now()?,
        };
        self.connection(false)?.execute(
            "INSERT INTO profile_entries (id, title, body_md, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![entry.id, entry.title, entry.body_md, entry.updated_at],
        )?;
        Ok(entry)
    }

    pub fn profile_entry(&self, id: &str) -> Result<ProfileEntry> {
        self.connection(true)?
            .query_row(
                "SELECT id, title, body_md, updated_at FROM profile_entries WHERE id = ?1",
                [id],
                profile_entry_from_row,
            )
            .optional()?
            .ok_or_else(|| profile_entry_not_found(id))
    }

    pub fn list_profile_entries(&self, limit: usize) -> Result<Vec<ProfileEntry>> {
        validate_limit(limit)?;
        let connection = self.connection(true)?;
        let mut statement = connection.prepare(
            "SELECT id, title, body_md, updated_at FROM profile_entries
             ORDER BY updated_at DESC, id LIMIT ?1",
        )?;
        statement
            .query_map([sql_usize(limit, "limit")?], profile_entry_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn update_profile_entry(
        &self,
        id: &str,
        title: &str,
        body_md: &str,
    ) -> Result<ProfileEntry> {
        let title = title.trim();
        validate_text(title, "profile_title", 1_000, false)?;
        validate_text(body_md, "profile_body", 1024 * 1024, true)?;
        let entry = ProfileEntry {
            id: id.to_owned(),
            title: title.to_owned(),
            body_md: body_md.to_owned(),
            updated_at: now()?,
        };
        let changed = self.connection(false)?.execute(
            "UPDATE profile_entries SET title = ?2, body_md = ?3, updated_at = ?4 WHERE id = ?1",
            params![entry.id, entry.title, entry.body_md, entry.updated_at],
        )?;
        if changed != 1 {
            return Err(profile_entry_not_found(id));
        }
        Ok(entry)
    }

    pub fn create_case(&self, title: &str, markdown: &str, stage: Stage) -> Result<CaseRevision> {
        let title = title.trim();
        validate_text(title, "case_title", 1_000, false)?;
        validate_text(markdown, "case_markdown", 1024 * 1024, true)?;
        let id = format!("case-{}", Uuid::now_v7());
        let now = now()?;
        let markdown_sha256 = digest(markdown.as_bytes());
        let mut connection = self.connection(false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO cases (id, title, head_revision, created_at, updated_at)
             VALUES (?1, ?2, 1, ?3, ?3)",
            params![id, title, now],
        )?;
        transaction.execute(
            "INSERT INTO case_revisions
                (case_id, revision, markdown, markdown_sha256, stage, advisory,
                 summary, source_update_id, created_at)
             VALUES (?1, 1, ?2, ?3, ?4, NULL, ?5, NULL, ?6)",
            params![
                id,
                markdown,
                markdown_sha256,
                stage.as_str(),
                "Initial case",
                now
            ],
        )?;
        transaction.commit()?;
        self.case_revision(&id, Some(1))
    }

    pub fn case_revision(&self, case_id: &str, revision: Option<u64>) -> Result<CaseRevision> {
        let connection = self.connection(true)?;
        let requested = revision
            .map(|value| sql_u64(value, "case revision"))
            .transpose()?;
        let mut statement = connection.prepare(
            "SELECT r.case_id, c.title, r.revision, r.markdown, r.markdown_sha256,
                    r.stage, r.advisory, r.summary, r.source_update_id, r.created_at
             FROM case_revisions r JOIN cases c ON c.id = r.case_id
             WHERE r.case_id = ?1 AND r.revision = COALESCE(?2, c.head_revision)",
        )?;
        statement
            .query_row(params![case_id, requested], revision_from_row)
            .optional()?
            .ok_or_else(|| {
                Error::domain(
                    "case_revision_not_found",
                    match revision {
                        Some(revision) => {
                            format!("case {case_id} has no revision {revision}")
                        }
                        None => format!("case {case_id} does not exist"),
                    },
                )
            })
    }

    pub fn list_cases(&self, limit: usize) -> Result<Vec<CaseListItem>> {
        validate_limit(limit)?;
        let connection = self.connection(true)?;
        let mut statement = connection.prepare(
            "SELECT c.id, c.title, r.revision, r.stage, r.advisory, r.summary, c.updated_at
             FROM cases c
             JOIN case_revisions r
               ON r.case_id = c.id AND r.revision = c.head_revision
             ORDER BY c.updated_at DESC, c.id
             LIMIT ?1",
        )?;
        let rows = statement.query_map([sql_usize(limit, "limit")?], |row| {
            let advisory: Option<String> = row.get(4)?;
            Ok(CaseListItem {
                case_id: row.get(0)?,
                title: row.get(1)?,
                revision: row_u64(row, 2)?,
                stage: row_stage(row, 3)?,
                attention: advisory.is_some(),
                advisory,
                summary: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn case_history(&self, case_id: &str) -> Result<Vec<CaseRevision>> {
        let connection = self.connection(true)?;
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM cases WHERE id = ?1)",
            [case_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(Error::domain(
                "case_not_found",
                format!("case {case_id} does not exist"),
            ));
        }
        let mut statement = connection.prepare(
            "SELECT r.case_id, c.title, r.revision, r.markdown, r.markdown_sha256,
                    r.stage, r.advisory, r.summary, r.source_update_id, r.created_at
             FROM case_revisions r JOIN cases c ON c.id = r.case_id
             WHERE r.case_id = ?1 ORDER BY r.revision",
        )?;
        let rows = statement.query_map([case_id], revision_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        validate_limit(limit)?;
        let query = query.trim();
        if query.is_empty() {
            return Err(Error::domain(
                "search_query_empty",
                "search query must not be empty",
            ));
        }
        let pattern = format!("%{}%", escape_like(query));
        let connection = self.connection(true)?;
        let mut statement = connection.prepare(
            "SELECT c.id, c.title, r.revision, r.stage, r.advisory, r.summary,
                    substr(replace(replace(r.markdown, char(10), ' '), char(13), ' '), 1, 240)
             FROM cases c
             JOIN case_revisions r
               ON r.case_id = c.id AND r.revision = c.head_revision
             WHERE c.title LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                OR r.markdown LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                OR COALESCE(r.advisory, '') LIKE ?1 ESCAPE '\\' COLLATE NOCASE
             ORDER BY c.updated_at DESC, c.id
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![pattern, sql_usize(limit, "limit")?], |row| {
            let advisory: Option<String> = row.get(4)?;
            Ok(SearchResult {
                case_id: row.get(0)?,
                title: row.get(1)?,
                revision: row_u64(row, 2)?,
                stage: row_stage(row, 3)?,
                attention: advisory.is_some(),
                advisory,
                summary: row.get(5)?,
                snippet: row.get(6)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn enqueue_delivery(
        &self,
        case_id: &str,
        label: &str,
        body: &str,
        source: Option<&str>,
    ) -> Result<StewardUpdate> {
        validate_text(body, "delivery_body", 1024 * 1024, true)?;
        let label = label.trim();
        validate_text(label, "delivery_label", 1_000, false)?;
        let source = source.map(str::trim).filter(|value| !value.is_empty());
        if let Some(source) = source {
            validate_text(source, "delivery_source", 4_000, false)?;
        }
        let delivery_id = format!("delivery-{}", Uuid::now_v7());
        let now = now()?;
        let mut connection = self.connection(false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_case_tx(&transaction, case_id)?;
        transaction.execute(
            "INSERT INTO deliveries
                (id, case_id, label, body, body_sha256, source, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                delivery_id,
                case_id,
                label,
                body,
                digest(body.as_bytes()),
                source,
                now
            ],
        )?;
        let update_id = insert_queued_update_tx(&transaction, case_id, &delivery_id, None, &now)?;
        transaction.commit()?;
        self.update(&update_id)
    }

    pub fn enqueue_retry(&self, update_id: &str) -> Result<StewardUpdate> {
        let mut connection = self.connection(false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (case_id, delivery_id, status): (String, String, String) = transaction
            .query_row(
                "SELECT case_id, delivery_id, status FROM steward_updates WHERE id = ?1",
                [update_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| {
                Error::domain(
                    "update_not_found",
                    format!("update {update_id} does not exist"),
                )
            })?;
        if !matches!(status.as_str(), "failed" | "lost") {
            return Err(Error::domain(
                "update_retry_not_allowed",
                format!("update {update_id} is {status}, not failed or lost"),
            ));
        }
        let successor_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM steward_updates WHERE retry_of = ?1)",
            [update_id],
            |row| row.get(0),
        )?;
        if successor_exists {
            return Err(Error::domain(
                "update_retry_exists",
                format!("update {update_id} already has a retry successor"),
            ));
        }
        let retry_id = insert_queued_update_tx(
            &transaction,
            &case_id,
            &delivery_id,
            Some(update_id),
            &now()?,
        )?;
        transaction.commit()?;
        self.update(&retry_id)
    }

    pub fn delivery(&self, delivery_id: &str) -> Result<Delivery> {
        let connection = self.connection(true)?;
        connection
            .query_row(
                "SELECT id, case_id, label, body, body_sha256, source, received_at
                 FROM deliveries WHERE id = ?1",
                [delivery_id],
                |row| {
                    Ok(Delivery {
                        id: row.get(0)?,
                        case_id: row.get(1)?,
                        label: row.get(2)?,
                        body: row.get(3)?,
                        body_sha256: row.get(4)?,
                        source: row.get(5)?,
                        received_at: row.get(6)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| {
                Error::domain(
                    "delivery_not_found",
                    format!("delivery {delivery_id} does not exist"),
                )
            })
    }

    pub fn update(&self, update_id: &str) -> Result<StewardUpdate> {
        let connection = self.connection(true)?;
        connection
            .query_row(
                "SELECT id, case_id, delivery_id, status, base_revision,
                        requester_id, job_id, admitted, applied_revision,
                        result_posted, runtime_state, runtime_detail, retry_of,
                        last_error, created_at, started_at, finished_at
                 FROM steward_updates WHERE id = ?1",
                [update_id],
                update_from_row,
            )
            .optional()?
            .ok_or_else(|| {
                Error::domain(
                    "update_not_found",
                    format!("update {update_id} does not exist"),
                )
            })
    }

    pub fn list_updates(&self, limit: usize) -> Result<Vec<StewardUpdate>> {
        validate_limit(limit)?;
        let connection = self.connection(true)?;
        let mut statement = connection.prepare(
            "SELECT id, case_id, delivery_id, status, base_revision,
                    requester_id, job_id, admitted, applied_revision,
                    result_posted, runtime_state, runtime_detail, retry_of,
                    last_error, created_at, started_at, finished_at
             FROM steward_updates ORDER BY created_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([sql_usize(limit, "limit")?], update_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn unsettled_updates(&self) -> Result<Vec<StewardUpdate>> {
        let connection = self.connection(true)?;
        let mut statement = connection.prepare(
            "SELECT id, case_id, delivery_id, status, base_revision,
                    requester_id, job_id, admitted, applied_revision,
                    result_posted, runtime_state, runtime_detail, retry_of,
                    last_error, created_at, started_at, finished_at
             FROM steward_updates
             WHERE status = 'running'
                OR (status = 'applied' AND runtime_state IS NULL)
             ORDER BY CASE status WHEN 'running' THEN 0 ELSE 1 END,
                      created_at, id",
        )?;
        let rows = statement.query_map([], update_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn claim_next(&self) -> Result<Option<StewardUpdate>> {
        let mut connection = self.connection(false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let update_id = next_eligible_update_id_tx(&transaction)?;
        let Some(update_id) = update_id else {
            return Ok(None);
        };
        freeze_update_tx(&transaction, &update_id)?;
        transaction.commit()?;
        self.update(&update_id).map(Some)
    }

    pub fn claim_or_resume(&self, update_id: &str) -> Result<StewardUpdate> {
        let mut connection = self.connection(false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let status: Option<String> = transaction
            .query_row(
                "SELECT status FROM steward_updates WHERE id = ?1",
                [update_id],
                |row| row.get(0),
            )
            .optional()?;
        let status = status.ok_or_else(|| {
            Error::domain(
                "update_not_found",
                format!("update {update_id} does not exist"),
            )
        })?;
        match status.as_str() {
            "queued" => freeze_update_tx(&transaction, update_id)?,
            "running" | "applied" => {}
            _ => {
                return Err(Error::domain(
                    "update_not_resumable",
                    format!("update {update_id} is {status}"),
                ));
            }
        }
        transaction.commit()?;
        self.update(update_id)
    }

    pub fn correlation(&self, update_id: &str) -> Result<Option<Correlation>> {
        let connection = self.connection(true)?;
        connection
            .query_row(
                "SELECT id, requester_id, job_id, request_json, request_sha256,
                        tool_after, admitted
                 FROM steward_updates
                 WHERE id = ?1 AND request_json IS NOT NULL",
                [update_id],
                |row| {
                    Ok(Correlation {
                        update_id: row.get(0)?,
                        requester_id: row.get(1)?,
                        job_id: row.get(2)?,
                        request_json: row.get(3)?,
                        request_sha256: row.get(4)?,
                        tool_after: row_u64(row, 5)?,
                        admitted: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn put_request(
        &self,
        update_id: &str,
        request_json: &str,
        request_sha256: &str,
    ) -> Result<Correlation> {
        let connection = self.connection(false)?;
        let changed = connection.execute(
            "UPDATE steward_updates
             SET request_json = ?2, request_sha256 = ?3
             WHERE id = ?1 AND status = 'running' AND request_json IS NULL",
            params![update_id, request_json, request_sha256],
        )?;
        let existing = self.correlation(update_id)?.ok_or_else(|| {
            Error::domain(
                "request_correlation_missing",
                format!("update {update_id} could not retain its request"),
            )
        })?;
        if (changed == 0
            && (existing.request_json != request_json || existing.request_sha256 != request_sha256))
            || digest(existing.request_json.as_bytes()) != existing.request_sha256
        {
            return Err(Error::domain(
                "request_correlation_conflict",
                format!("update {update_id} has different immutable request bytes"),
            ));
        }
        Ok(existing)
    }

    pub fn mark_admitted(&self, update_id: &str) -> Result<()> {
        self.connection(false)?.execute(
            "UPDATE steward_updates SET admitted = 1 WHERE id = ?1",
            [update_id],
        )?;
        Ok(())
    }

    pub fn advance_tool_after(&self, update_id: &str, sequence: u64) -> Result<()> {
        self.connection(false)?.execute(
            "UPDATE steward_updates SET tool_after = MAX(tool_after, ?2) WHERE id = ?1",
            params![update_id, sql_u64(sequence, "tool sequence")?],
        )?;
        Ok(())
    }

    pub fn mark_result_posted(&self, update_id: &str) -> Result<()> {
        let changed = self.connection(false)?.execute(
            "UPDATE steward_updates
             SET result_posted = 1
             WHERE id = ?1 AND status = 'applied' AND applied_revision IS NOT NULL",
            [update_id],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "update_result_not_postable",
                format!("update {update_id} has no applied result to mark posted"),
            ));
        }
        Ok(())
    }

    pub fn mark_runtime_finished(
        &self,
        update_id: &str,
        state: &str,
        detail: Option<&str>,
    ) -> Result<()> {
        let state = state.trim();
        validate_text(state, "runtime_state", 128, false)?;
        let mut connection = self.connection(false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, Option<String>, Option<String>)> = transaction
            .query_row(
                "SELECT status, runtime_state, runtime_detail
                 FROM steward_updates WHERE id = ?1",
                [update_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((status, existing_state, existing_detail)) = existing else {
            return Err(Error::domain(
                "update_not_found",
                format!("update {update_id} does not exist"),
            ));
        };
        if status != "applied" {
            return Err(Error::domain(
                "update_runtime_not_applicable",
                format!("update {update_id} has no applied revision"),
            ));
        }
        if let Some(existing_state) = existing_state {
            if existing_state == state && existing_detail.as_deref() == detail {
                return Ok(());
            }
            return Err(Error::domain(
                "update_runtime_conflict",
                format!("update {update_id} already retained a different runtime result"),
            ));
        }
        let changed = transaction.execute(
            "UPDATE steward_updates
             SET runtime_state = ?2, runtime_detail = ?3
             WHERE id = ?1 AND runtime_state IS NULL",
            params![update_id, state, detail],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "update_runtime_conflict",
                format!("update {update_id} runtime result changed concurrently"),
            ));
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn record_applied_diagnostic(&self, update_id: &str, detail: &str) -> Result<()> {
        let changed = self.connection(false)?.execute(
            "UPDATE steward_updates SET last_error = ?2
             WHERE id = ?1 AND status = 'applied' AND applied_revision IS NOT NULL",
            params![update_id, detail],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "update_not_applied",
                format!("update {update_id} has no applied revision"),
            ));
        }
        Ok(())
    }

    pub fn mailbox_receipt(&self, job_id: &str, call_id: &str) -> Result<Option<MailboxReceipt>> {
        let connection = self.connection(true)?;
        connection
            .query_row(
                "SELECT arguments_sha256, result_json, result_sha256, is_error, committed_revision
                 FROM mailbox_receipts WHERE job_id = ?1 AND call_id = ?2",
                params![job_id, call_id],
                |row| {
                    Ok(MailboxReceipt {
                        arguments_sha256: row.get(0)?,
                        result_json: row.get(1)?,
                        result_sha256: row.get(2)?,
                        is_error: row.get(3)?,
                        committed_revision: row_optional_u64(row, 4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn commit_proposal(
        &self,
        update_id: &str,
        job_id: &str,
        call_id: &str,
        arguments_sha256: &str,
        proposal: &RevisionProposal,
    ) -> Result<u64> {
        validate_proposal(proposal)?;
        let mut connection = self.connection(false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(receipt) = receipt_tx(&transaction, job_id, call_id)? {
            if receipt.arguments_sha256 != arguments_sha256 {
                return Err(Error::domain(
                    "mailbox_arguments_conflict",
                    format!("tool call {call_id} was replayed with different arguments"),
                ));
            }
            return receipt.committed_revision.ok_or_else(|| {
                Error::domain(
                    "mailbox_receipt_rejected",
                    format!("tool call {call_id} was previously rejected"),
                )
            });
        }
        let (case_id, status, base_revision, base_digest, correlated_job): (
            String,
            String,
            Option<i64>,
            Option<String>,
            String,
        ) = transaction
            .query_row(
                "SELECT case_id, status, base_revision, base_digest, job_id
                 FROM steward_updates WHERE id = ?1",
                [update_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| {
                Error::domain(
                    "update_not_found",
                    format!("update {update_id} does not exist"),
                )
            })?;
        if correlated_job != job_id {
            return Err(Error::domain(
                "update_job_conflict",
                format!("job {job_id} does not own update {update_id}"),
            ));
        }
        if status == "applied" {
            return Err(Error::domain(
                "update_already_applied",
                format!("update {update_id} already committed a revision"),
            ));
        }
        if status != "running" {
            return Err(Error::domain(
                "update_not_running",
                format!("update {update_id} is {status}"),
            ));
        }
        let base_revision = base_revision.ok_or_else(|| {
            Error::domain("update_base_missing", "running update has no frozen base")
        })?;
        let base_digest = base_digest.ok_or_else(|| {
            Error::domain("update_base_missing", "running update has no frozen digest")
        })?;
        let proposed_base = sql_u64(proposal.base_revision, "proposal base revision")?;
        if proposed_base != base_revision {
            return Err(Error::domain(
                "proposal_base_conflict",
                format!(
                    "proposal base {} does not match frozen base {base_revision}",
                    proposal.base_revision
                ),
            ));
        }
        let (head_revision, head_digest): (i64, String) = transaction.query_row(
            "SELECT c.head_revision, r.markdown_sha256
             FROM cases c JOIN case_revisions r
               ON r.case_id = c.id AND r.revision = c.head_revision
             WHERE c.id = ?1",
            [&case_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if head_revision != base_revision || head_digest != base_digest {
            return Err(Error::domain(
                "case_base_changed",
                format!("case {case_id} changed after update {update_id} froze its base"),
            ));
        }
        let revision = head_revision
            .checked_add(1)
            .ok_or_else(|| Error::domain("revision_overflow", "case revision number overflowed"))?;
        let advisory = proposal.advisory.as_deref();
        let now = now()?;
        let markdown_sha256 = digest(proposal.document_markdown.as_bytes());
        transaction.execute(
            "INSERT INTO case_revisions
                (case_id, revision, markdown, markdown_sha256, stage, advisory,
                 summary, source_update_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                case_id,
                revision,
                proposal.document_markdown,
                markdown_sha256,
                proposal.stage.as_str(),
                advisory,
                proposal.summary.trim(),
                update_id,
                now
            ],
        )?;
        transaction.execute(
            "UPDATE cases SET head_revision = ?2, updated_at = ?3 WHERE id = ?1",
            params![case_id, revision, now],
        )?;
        let result_json = serde_json::to_string(&serde_json::json!({
            "recorded": {
                "kind": "case_revision",
                "case_id": case_id,
                "revision": revision,
                "status": "recorded"
            }
        }))?;
        transaction.execute(
            "INSERT INTO mailbox_receipts
                (job_id, call_id, arguments_sha256, result_json, result_sha256,
                 is_error, committed_revision, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7)",
            params![
                job_id,
                call_id,
                arguments_sha256,
                result_json,
                digest(result_json.as_bytes()),
                revision,
                now
            ],
        )?;
        transaction.execute(
            "UPDATE steward_updates
             SET status = 'applied', applied_revision = ?2, last_error = NULL,
                 finished_at = ?3
             WHERE id = ?1 AND status = 'running'",
            params![update_id, revision, now],
        )?;
        transaction.commit()?;
        u64::try_from(revision)
            .map_err(|_| Error::domain("revision_invalid", "case revision is negative"))
    }

    pub fn record_rejection(
        &self,
        job_id: &str,
        call_id: &str,
        arguments_sha256: &str,
        result_json: &str,
    ) -> Result<()> {
        let mut connection = self.connection(false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(receipt) = receipt_tx(&transaction, job_id, call_id)? {
            if receipt.arguments_sha256 != arguments_sha256
                || receipt.result_json != result_json
                || !receipt.is_error
            {
                return Err(Error::domain(
                    "mailbox_rejection_conflict",
                    format!("tool call {call_id} has different persisted result bytes"),
                ));
            }
            return Ok(());
        }
        transaction.execute(
            "INSERT INTO mailbox_receipts
                (job_id, call_id, arguments_sha256, result_json, result_sha256,
                 is_error, committed_revision, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, NULL, ?6)",
            params![
                job_id,
                call_id,
                arguments_sha256,
                result_json,
                digest(result_json.as_bytes()),
                now()?
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn record_running_error(&self, update_id: &str, detail: &str) -> Result<()> {
        self.connection(false)?.execute(
            "UPDATE steward_updates SET last_error = ?2 WHERE id = ?1 AND status = 'running'",
            params![update_id, detail],
        )?;
        Ok(())
    }

    pub fn mark_failed(&self, update_id: &str, detail: &str) -> Result<()> {
        let changed = self.connection(false)?.execute(
            "UPDATE steward_updates
             SET status = 'failed', last_error = ?2, finished_at = ?3
             WHERE id = ?1 AND status = 'running' AND applied_revision IS NULL",
            params![update_id, detail, now()?],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "update_not_failable",
                format!("update {update_id} cannot be marked failed"),
            ));
        }
        Ok(())
    }

    pub fn mark_lost(&self, update_id: &str, detail: &str) -> Result<()> {
        let changed = self.connection(false)?.execute(
            "UPDATE steward_updates
             SET status = 'lost', last_error = ?2, finished_at = ?3
             WHERE id = ?1 AND status = 'running' AND applied_revision IS NULL",
            params![update_id, detail, now()?],
        )?;
        if changed != 1 {
            return Err(Error::domain(
                "update_not_lost",
                format!("update {update_id} cannot be marked lost"),
            ));
        }
        Ok(())
    }

    fn connection(&self, read_only: bool) -> Result<Connection> {
        open_connection(&self.path, read_only)
    }
}

fn insert_queued_update_tx(
    transaction: &Transaction<'_>,
    case_id: &str,
    delivery_id: &str,
    retry_of: Option<&str>,
    created_at: &str,
) -> Result<String> {
    let update_id = format!("update-{}", Uuid::now_v7());
    let requester_id = format!("case-steward:{update_id}");
    let job_id = format!("crm-case-steward-{}", Uuid::now_v7());
    transaction.execute(
        "INSERT INTO steward_updates
            (id, case_id, delivery_id, status, base_revision, base_digest,
             requester_id, job_id, request_json, request_sha256, tool_after,
             admitted, applied_revision, result_posted, runtime_state,
             runtime_detail, retry_of, last_error, created_at, started_at,
             finished_at)
         VALUES (?1, ?2, ?3, 'queued', NULL, NULL, ?4, ?5, NULL, NULL,
                 0, 0, NULL, 0, NULL, NULL, ?6, NULL, ?7, NULL, NULL)",
        params![
            update_id,
            case_id,
            delivery_id,
            requester_id,
            job_id,
            retry_of,
            created_at
        ],
    )?;
    Ok(update_id)
}

fn next_eligible_update_id_tx(transaction: &Transaction<'_>) -> Result<Option<String>> {
    transaction
        .query_row(
            "SELECT candidate.id
             FROM steward_updates candidate
             WHERE candidate.status = 'queued'
               AND NOT EXISTS (
                   SELECT 1 FROM steward_updates active
                   WHERE active.case_id = candidate.case_id
                     AND active.status = 'running'
               )
             ORDER BY candidate.created_at, candidate.id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

fn freeze_update_tx(transaction: &Transaction<'_>, update_id: &str) -> Result<()> {
    let (case_id, status): (String, String) = transaction.query_row(
        "SELECT case_id, status FROM steward_updates WHERE id = ?1",
        [update_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if status != "queued" {
        return Err(Error::domain(
            "update_not_queued",
            format!("update {update_id} is {status}"),
        ));
    }
    let (revision, markdown_sha256): (i64, String) = transaction.query_row(
        "SELECT c.head_revision, r.markdown_sha256
         FROM cases c JOIN case_revisions r
           ON r.case_id = c.id AND r.revision = c.head_revision
         WHERE c.id = ?1",
        [&case_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let changed = transaction.execute(
        "UPDATE steward_updates
         SET status = 'running', base_revision = ?2, base_digest = ?3,
             started_at = ?4, last_error = NULL
         WHERE id = ?1 AND status = 'queued'",
        params![update_id, revision, markdown_sha256, now()?],
    )?;
    if changed != 1 {
        return Err(Error::domain(
            "update_claim_conflict",
            format!("update {update_id} was claimed concurrently"),
        ));
    }
    Ok(())
}

fn require_case_tx(transaction: &Transaction<'_>, case_id: &str) -> Result<()> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM cases WHERE id = ?1)",
        [case_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(Error::domain(
            "case_not_found",
            format!("case {case_id} does not exist"),
        ));
    }
    Ok(())
}

fn receipt_tx(
    transaction: &Transaction<'_>,
    job_id: &str,
    call_id: &str,
) -> Result<Option<MailboxReceipt>> {
    transaction
        .query_row(
            "SELECT arguments_sha256, result_json, result_sha256, is_error, committed_revision
             FROM mailbox_receipts WHERE job_id = ?1 AND call_id = ?2",
            params![job_id, call_id],
            |row| {
                Ok(MailboxReceipt {
                    arguments_sha256: row.get(0)?,
                    result_json: row.get(1)?,
                    result_sha256: row.get(2)?,
                    is_error: row.get(3)?,
                    committed_revision: row_optional_u64(row, 4)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn validate_proposal(proposal: &RevisionProposal) -> Result<()> {
    validate_text(
        &proposal.document_markdown,
        "document_markdown",
        1024 * 1024,
        true,
    )?;
    validate_text(&proposal.summary, "summary", 1_000, false)?;
    if let Some(advisory) = &proposal.advisory {
        validate_text(advisory, "advisory", 4_000, false)?;
    }
    Ok(())
}

fn validate_text(value: &str, field: &'static str, max: usize, allow_empty: bool) -> Result<()> {
    if (!allow_empty && value.trim().is_empty()) || value.len() > max {
        return Err(Error::domain(
            "text_invalid",
            format!(
                "{field} must be {} through {max} UTF-8 bytes",
                usize::from(!allow_empty)
            ),
        ));
    }
    Ok(())
}

fn validate_limit(limit: usize) -> Result<()> {
    if !(1..=1_000).contains(&limit) {
        return Err(Error::domain(
            "limit_invalid",
            "limit must be from 1 through 1000",
        ));
    }
    Ok(())
}

fn revision_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CaseRevision> {
    let advisory: Option<String> = row.get(6)?;
    Ok(CaseRevision {
        case_id: row.get(0)?,
        title: row.get(1)?,
        revision: row_u64(row, 2)?,
        markdown: row.get(3)?,
        markdown_sha256: row.get(4)?,
        stage: row_stage(row, 5)?,
        attention: advisory.is_some(),
        advisory,
        summary: row.get(7)?,
        source_update_id: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn update_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StewardUpdate> {
    Ok(StewardUpdate {
        id: row.get(0)?,
        case_id: row.get(1)?,
        delivery_id: row.get(2)?,
        status: row_enum(row, 3, UpdateStatus::from_str)?,
        base_revision: row_optional_u64(row, 4)?,
        requester_id: row.get(5)?,
        job_id: row.get(6)?,
        admitted: row.get(7)?,
        applied_revision: row_optional_u64(row, 8)?,
        result_posted: row.get(9)?,
        runtime_state: row.get(10)?,
        runtime_detail: row.get(11)?,
        retry_of: row.get(12)?,
        last_error: row.get(13)?,
        created_at: row.get(14)?,
        started_at: row.get(15)?,
        finished_at: row.get(16)?,
    })
}

fn row_stage(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Stage> {
    row_enum(row, index, Stage::from_str)
}

fn row_enum<T>(
    row: &rusqlite::Row<'_>,
    index: usize,
    parse: impl FnOnce(&str) -> Result<T>,
) -> rusqlite::Result<T> {
    let value: String = row.get(index)?;
    parse(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn row_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn row_optional_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|value| {
            u64::try_from(value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    rusqlite::types::Type::Integer,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn open_connection(path: &Path, read_only: bool) -> Result<Connection> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX
    };
    let connection = Connection::open_with_flags(path, flags)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    Ok(connection)
}

fn require_schema(connection: &Connection) -> Result<()> {
    require_schema_version(connection, SCHEMA_VERSION)
}

fn require_schema_version(connection: &Connection, expected: i64) -> Result<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != expected {
        return Err(Error::domain(
            "schema_version_unsupported",
            format!(
                "CRM schema {version} is not supported; expected {expected}; \
                 schema 1 requires explicit `crm migrate --backup PATH`"
            ),
        ));
    }
    for table in REQUIRED_TABLES {
        require_schema_object(connection, "table", table)?;
    }
    for index in REQUIRED_INDEXES {
        require_schema_object(connection, "index", index)?;
    }
    for trigger in REQUIRED_TRIGGERS {
        require_schema_object(connection, "trigger", trigger)?;
    }
    for (table, column) in REQUIRED_COLUMNS {
        require_schema_column(connection, table, column)?;
    }
    if expected == 2 {
        require_schema_object(connection, "table", "profile_entries")?;
        for column in ["id", "title", "body_md", "updated_at"] {
            require_schema_column(connection, "profile_entries", column)?;
        }
    }
    let meta: Option<i64> = connection
        .query_row(
            "SELECT schema_version FROM crm_meta WHERE marker = 'crm'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if meta != Some(expected) {
        return Err(Error::domain(
            "schema_identity_invalid",
            "CRM schema identity row is absent or incompatible",
        ));
    }
    Ok(())
}

fn check_integrity(connection: &Connection) -> Result<()> {
    let violation: Option<String> = connection
        .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
        .optional()?;
    if violation.is_some() {
        return Err(Error::domain(
            "foreign_key_check_failed",
            "SQLite foreign_key_check reported a violation",
        ));
    }
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(Error::domain(
            "integrity_check_failed",
            "SQLite integrity_check reported a violation",
        ));
    }
    Ok(())
}

fn require_migration_quiescence(connection: &Connection) -> Result<()> {
    let pid: Option<i64> = connection.query_row(
        "SELECT worker_pid FROM crm_meta WHERE marker = 'crm'",
        [],
        |row| row.get(0),
    )?;
    if let Some(pid) = pid {
        let pid = u32::try_from(pid)
            .ok()
            .filter(|pid| *pid > 0)
            .ok_or_else(|| {
                Error::domain("worker_lease_invalid", "worker lease has no valid PID")
            })?;
        // Lease age cannot prove quiescence: even an expired owner can still run.
        if process_is_alive(pid)? {
            return Err(Error::domain(
                "migration_worker_active",
                "finish CRM workers before migrating the database",
            ));
        }
    }
    let active: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM steward_updates
         WHERE status = 'running' OR (status = 'applied' AND runtime_state IS NULL))",
        [],
        |row| row.get(0),
    )?;
    if active {
        return Err(Error::domain(
            "migration_update_unsettled",
            "settle running and applied-but-runtime-unsettled updates before migrating",
        ));
    }
    Ok(())
}

fn create_migration_backup(path: &Path, backup_path: &Path, version: i64) -> Result<()> {
    ensure_parent(backup_path, false)?;
    let absolute_path = fs::canonicalize(path).map_err(|source| io(path, source))?;
    let backup_parent = backup_path
        .parent()
        .ok_or_else(|| Error::domain("database_parent_missing", "backup path has no parent"))?;
    let backup_name = backup_path
        .file_name()
        .ok_or_else(|| Error::domain("migration_backup_invalid", "backup path has no filename"))?;
    let absolute_backup = fs::canonicalize(backup_parent)
        .map_err(|source| io(backup_parent, source))?
        .join(backup_name);
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut reserved = absolute_path.as_os_str().to_os_string();
        reserved.push(suffix);
        if absolute_backup == reserved {
            return Err(Error::domain(
                "migration_backup_invalid",
                "backup must be separate from the source database and its sidecars",
            ));
        }
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = backup_path.as_os_str().to_os_string();
        sidecar.push(suffix);
        match fs::symlink_metadata(&sidecar) {
            Ok(_) => {
                return Err(Error::domain(
                    "migration_backup_invalid",
                    "backup destination must have no existing SQLite sidecars",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io(PathBuf::from(sidecar), error)),
        }
    }
    let backup_text = backup_path.to_str().ok_or_else(|| {
        Error::domain(
            "migration_backup_invalid",
            "backup path must be valid UTF-8",
        )
    })?;
    create_private_file(backup_path)?;
    let reader = open_connection(path, true)?;
    reader.execute("VACUUM INTO ?1", [backup_text])?;
    let backup = open_connection(backup_path, true)?;
    require_schema_version(&backup, version)?;
    check_integrity(&backup)?;
    drop(backup);
    fs::File::open(backup_path)
        .and_then(|file| file.sync_all())
        .map_err(|source| io(backup_path, source))?;
    fs::File::open(backup_parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| io(backup_parent, source))?;
    Ok(())
}

fn profile_entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProfileEntry> {
    Ok(ProfileEntry {
        id: row.get(0)?,
        title: row.get(1)?,
        body_md: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

fn profile_entry_not_found(id: &str) -> Error {
    Error::domain(
        "profile_entry_not_found",
        format!("profile entry {id} does not exist"),
    )
}

fn require_schema_object(connection: &Connection, kind: &str, name: &str) -> Result<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2)",
        params![kind, name],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(Error::domain(
            "schema_incomplete",
            format!("CRM database is missing {kind} {name}"),
        ));
    }
    Ok(())
}

fn require_schema_column(connection: &Connection, table: &str, column: &str) -> Result<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2
         )",
        params![table, column],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(Error::domain(
            "schema_incomplete",
            format!("CRM database is missing column {table}.{column}"),
        ));
    }
    Ok(())
}

fn ensure_parent(path: &Path, create: bool) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        Error::domain(
            "database_parent_missing",
            format!("database path has no parent: {}", path.display()),
        )
    })?;
    let existed = parent.is_dir();
    if create {
        fs::create_dir_all(parent).map_err(|source| io(parent, source))?;
        if !existed {
            make_private_directory(parent)?;
        }
    } else if !parent.is_dir() {
        return Err(Error::domain(
            "database_not_initialized",
            format!("run `crm init` before using {}", path.display()),
        ));
    }
    inspect_private_directory(parent)?;
    Ok(())
}

#[cfg(unix)]
fn make_private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| io(path, source))
}

#[cfg(not(unix))]
fn make_private_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn inspect_private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = fs::symlink_metadata(path).map_err(|source| io(path, source))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || metadata.mode() & 0o077 != 0 {
        return Err(Error::domain(
            "database_directory_not_private",
            format!(
                "database directory must be a private regular directory: {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn inspect_private_directory(path: &Path) -> Result<()> {
    if !path.is_dir() {
        return Err(Error::domain(
            "database_directory_invalid",
            format!("database directory is invalid: {}", path.display()),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map(|_| ())
        .map_err(|source| io(path, source))
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> Result<()> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_| ())
        .map_err(|source| io(path, source))
}

#[cfg(unix)]
fn secure_path(path: &Path) -> Result<()> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let metadata = fs::symlink_metadata(path).map_err(|source| io(path, source))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Error::domain(
            "database_file_invalid",
            format!("database must be a regular file: {}", path.display()),
        ));
    }
    if metadata.mode() & 0o077 != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|source| io(path, source))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn secure_path(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Err(Error::domain(
            "database_file_invalid",
            format!("database must be a regular file: {}", path.display()),
        ));
    }
    Ok(())
}

fn secure_sidecars(path: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm"] {
        let mut sidecar_name = path.as_os_str().to_os_string();
        sidecar_name.push(suffix);
        let sidecar = PathBuf::from(sidecar_name);
        if sidecar.exists() {
            secure_path(&sidecar)?;
        }
    }
    Ok(())
}

fn worker_owner_is_reclaimable(
    pid: Option<i64>,
    acquired_at: Option<&str>,
    current_time: OffsetDateTime,
) -> Result<bool> {
    let pid = pid
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| Error::domain("worker_lease_invalid", "worker lease has no valid PID"))?;
    let acquired_at = acquired_at.ok_or_else(|| {
        Error::domain(
            "worker_lease_invalid",
            "worker lease has no acquisition time",
        )
    })?;
    let acquired_at = OffsetDateTime::parse(acquired_at, &Rfc3339).map_err(|_| {
        Error::domain(
            "worker_lease_invalid",
            "worker lease has an invalid acquisition time",
        )
    })?;
    let age_seconds = current_time
        .unix_timestamp()
        .checked_sub(acquired_at.unix_timestamp());
    if age_seconds.is_none_or(|age| !(0..WORKER_LEASE_MAX_AGE_SECONDS).contains(&age)) {
        return Ok(true);
    }
    process_is_alive(pid).map(|alive| !alive)
}

fn process_is_alive(pid: u32) -> Result<bool> {
    if pid == process::id() {
        return Ok(true);
    }
    Command::new("/bin/kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .map_err(|source| crate::error::io("/bin/kill", source))
}

fn now() -> Result<String> {
    format_timestamp(OffsetDateTime::now_utc())
}

fn format_timestamp(value: OffsetDateTime) -> Result<String> {
    value
        .format(&Rfc3339)
        .map_err(|error| Error::domain("time_format_failed", error.to_string()))
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sql_u64(value: u64, label: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| {
        Error::domain(
            "number_too_large",
            format!("{label} is too large for SQLite"),
        )
    })
}

fn sql_usize(value: usize, label: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| {
        Error::domain(
            "number_too_large",
            format!("{label} is too large for SQLite"),
        )
    })
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use tempfile::TempDir;

    use super::Store;
    use crate::model::{RevisionProposal, Stage, UpdateStatus};

    fn fixture() -> (TempDir, Store) {
        let temporary = TempDir::new().expect("temporary directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
                .expect("private temporary directory");
        }
        let path = temporary.path().join("crm.db");
        Store::init(&path).expect("initialize database");
        let store = Store::open(path).expect("open database");
        (temporary, store)
    }

    fn old_fixture() -> (TempDir, std::path::PathBuf, rusqlite::Connection) {
        let temporary = TempDir::new().expect("temporary directory");
        super::make_private_directory(temporary.path()).expect("private directory");
        let path = temporary.path().join("crm.db");
        super::create_private_file(&path).expect("private database");
        let connection = super::open_connection(&path, false).expect("old database");
        connection
            .execute_batch(include_str!("../tests/fixtures/schema-v1.sql"))
            .expect("schema one");
        connection
            .pragma_update(None, "user_version", 1)
            .expect("schema version one");
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .expect("WAL mode");
        connection
            .pragma_update(None, "wal_autocheckpoint", 0)
            .expect("retain WAL writes for backup test");
        let sha = super::digest(b"Synthetic retained text");
        connection
            .execute(
                "INSERT INTO cases VALUES ('case-synthetic', 'Synthetic case', 1, 't0', 't0')",
                [],
            )
            .expect("old case");
        connection
            .execute(
                "INSERT INTO deliveries VALUES
                 ('delivery-synthetic', 'case-synthetic', 'Note', 'Exact synthetic delivery',
                  ?1, 'synthetic:source', 't1')",
                [&sha],
            )
            .expect("old delivery");
        connection
            .execute(
                "INSERT INTO steward_updates
                 (id, case_id, delivery_id, status, requester_id, job_id,
                  request_json, request_sha256, last_error, created_at, finished_at)
                 VALUES ('update-synthetic', 'case-synthetic', 'delivery-synthetic',
                         'failed', 'requester-synthetic', 'job-synthetic',
                         '{\"synthetic\":true}', ?1, 'Synthetic failure', 't1', 't2')",
                [&sha],
            )
            .expect("old update");
        connection
            .execute(
                "INSERT INTO case_revisions VALUES
                 ('case-synthetic', 1, '# Synthetic\r\n\r\nText  \r\n', ?1, 'research',
                  'Synthetic non-blocking advisory', 'Synthetic summary', NULL, 't0')",
                [&sha],
            )
            .expect("old revision");
        connection
            .execute(
                "INSERT INTO mailbox_receipts VALUES
                 ('job-synthetic', 'call-synthetic', ?1, '{\"synthetic\":true}',
                  ?1, 1, NULL, 't2')",
                [&sha],
            )
            .expect("old receipt");
        (temporary, path, connection)
    }

    fn retained_rows(connection: &rusqlite::Connection) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
        [
            "cases",
            "deliveries",
            "steward_updates",
            "case_revisions",
            "mailbox_receipts",
        ]
        .into_iter()
        .map(|table| {
            let mut statement = connection
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .expect("snapshot table");
            let columns = statement.column_count();
            statement
                .query_map([], |row| (0..columns).map(|index| row.get(index)).collect())
                .expect("snapshot rows")
                .collect::<rusqlite::Result<Vec<_>>>()
                .expect("retained rows")
        })
        .collect()
    }

    #[test]
    fn profile_entries_preserve_exact_markdown_and_have_independent_mutable_identity() {
        let (_temporary, store) = fixture();
        let body = "# Synthetic vignette\r\n\r\nUnicode: café  \r\n\0End\n";
        let first = store
            .create_profile_entry("  Synthetic vignette  ", body)
            .expect("create entry");
        assert_eq!(first.title, "Synthetic vignette");
        assert_eq!(first.body_md, body);
        assert!(first.id.starts_with("profile-"));
        assert_eq!(store.profile_entry(&first.id).expect("read entry"), first);
        let second = store
            .create_profile_entry("Synthetic vignette", "Second entry")
            .expect("titles need not be unique");
        assert_ne!(first.id, second.id);
        let updated = store
            .update_profile_entry(&first.id, " Updated title ", "# Replacement\n\nExact  \n")
            .expect("replace entry");
        assert_eq!(updated.id, first.id);
        assert_eq!(updated.title, "Updated title");
        assert_eq!(updated.body_md, "# Replacement\n\nExact  \n");
        assert!(updated.updated_at >= first.updated_at);
        assert_eq!(
            store.profile_entry(&first.id).expect("updated entry"),
            updated
        );
        assert_eq!(
            store.profile_entry(&second.id).expect("second entry"),
            second
        );
        assert!(store.list_cases(100).expect("independent cases").is_empty());
        assert!(store.unsettled_updates().expect("no worker").is_empty());

        let connection = super::open_connection(store.path(), false).expect("raw connection");
        connection
            .execute("UPDATE profile_entries SET updated_at = 'same-time'", [])
            .expect("equal timestamps");
        let listed = store.list_profile_entries(100).expect("list entries");
        assert_eq!(listed.len(), 2);
        assert!(listed[0].id < listed[1].id);
        assert_eq!(
            store
                .list_profile_entries(1)
                .expect("limited entries")
                .len(),
            1
        );
    }

    #[test]
    fn profile_validation_and_missing_updates_do_not_mutate_entries() {
        let (_temporary, store) = fixture();
        let original = store
            .create_profile_entry("Synthetic", "")
            .expect("empty body");
        for (title, body) in [
            (" ".to_owned(), String::new()),
            ("x".repeat(1_001), String::new()),
            ("Valid".to_owned(), "x".repeat(1_048_577)),
        ] {
            assert!(store.create_profile_entry(&title, &body).is_err());
            assert!(
                store
                    .update_profile_entry(&original.id, &title, &body)
                    .is_err()
            );
        }
        assert_eq!(
            store.profile_entry(&original.id).expect("original remains"),
            original
        );
        assert_eq!(store.list_profile_entries(100).expect("one entry").len(), 1);
        assert!(store.list_profile_entries(0).is_err());
        assert_eq!(
            store
                .profile_entry("missing")
                .expect_err("missing read")
                .code(),
            "profile_entry_not_found"
        );
        assert_eq!(
            store
                .update_profile_entry("missing", "Valid", "body")
                .expect_err("missing update")
                .code(),
            "profile_entry_not_found"
        );
    }

    #[test]
    fn explicit_migration_preserves_old_rows_and_restorable_wal_snapshot() {
        let (temporary, path, old_connection) = old_fixture();
        let before = retained_rows(&old_connection);
        assert_eq!(
            Store::open(&path)
                .expect_err("ordinary open refuses old schema")
                .code(),
            "schema_version_unsupported"
        );
        assert_eq!(
            Store::init(&path).expect_err("init never migrates").code(),
            "schema_version_unsupported"
        );
        let backup_path = temporary.path().join("schema-one-backup.db");
        let result = Store::migrate(&path, &backup_path).expect("explicit migration");
        assert_eq!(
            result,
            super::MigrationResult {
                changed: true,
                from_schema_version: 1,
                schema_version: 2
            }
        );
        assert_eq!(retained_rows(&old_connection), before);
        let store = Store::open(&path).expect("migrated database");
        assert!(
            store
                .list_profile_entries(100)
                .expect("empty profile")
                .is_empty()
        );
        Store::doctor(&path).expect("migrated integrity");
        let backup = super::open_connection(&backup_path, true).expect("snapshot database");
        super::require_schema_version(&backup, 1).expect("old snapshot version");
        super::check_integrity(&backup).expect("snapshot integrity");
        assert_eq!(retained_rows(&backup), before);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&backup_path)
                    .expect("backup permissions")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let restore_path = temporary.path().join("restored.db");
        super::create_private_file(&restore_path).expect("private restore target");
        backup
            .execute(
                "VACUUM INTO ?1",
                [restore_path.to_str().expect("restore path")],
            )
            .expect("SQLite-aware restore");
        let restored = super::open_connection(&restore_path, true).expect("restored snapshot");
        super::require_schema_version(&restored, 1).expect("restored old schema");
        super::check_integrity(&restored).expect("restored integrity");
        assert_eq!(retained_rows(&restored), before);
        assert!(
            !Store::migrate(&path, &backup_path)
                .expect("idempotent migration")
                .changed
        );
        for table in ["deliveries", "case_revisions", "mailbox_receipts"] {
            assert!(
                old_connection
                    .execute(&format!("DELETE FROM {table}"), [])
                    .is_err()
            );
        }
    }

    #[test]
    fn migration_refuses_live_worker_even_when_lease_expired_and_unsettled_updates() {
        let (temporary, path, connection) = old_fixture();
        let backup_path = temporary.path().join("backup.db");
        connection
            .execute(
                "UPDATE crm_meta SET worker_token = 'synthetic-worker', worker_pid = ?1,
             worker_acquired_at = '2000-01-01T00:00:00Z'",
                [i64::from(std::process::id())],
            )
            .expect("live expired worker");
        assert_eq!(
            Store::migrate(&path, &backup_path)
                .expect_err("live worker")
                .code(),
            "migration_worker_active"
        );
        assert!(!backup_path.exists());
        connection.execute("UPDATE crm_meta SET worker_token = NULL, worker_pid = NULL, worker_acquired_at = NULL", []).expect("clear worker");
        connection
            .execute("UPDATE steward_updates SET status = 'running'", [])
            .expect("running attempt");
        assert_eq!(
            Store::migrate(&path, &backup_path)
                .expect_err("unsettled update")
                .code(),
            "migration_update_unsettled"
        );
        assert!(!backup_path.exists());
        connection
            .execute(
                "UPDATE steward_updates SET status = 'applied', applied_revision = 1",
                [],
            )
            .expect("unsettled runtime");
        assert_eq!(
            Store::migrate(&path, &backup_path)
                .expect_err("unsettled runtime")
                .code(),
            "migration_update_unsettled"
        );
        assert!(!backup_path.exists());
        connection
            .execute(
                "UPDATE steward_updates SET status = 'queued', applied_revision = NULL",
                [],
            )
            .expect("queued attempt");
        let before = retained_rows(&connection);
        Store::migrate(&path, &backup_path).expect("queued records can migrate");
        assert_eq!(retained_rows(&connection), before);
    }

    #[test]
    fn migration_failure_rolls_back_schema_and_rows_without_overwriting_backup() {
        let (temporary, path, connection) = old_fixture();
        let before = retained_rows(&connection);
        let backup_path = temporary.path().join("backup.db");
        std::fs::write(&backup_path, "existing backup").expect("existing backup");
        assert!(Store::migrate(&path, &backup_path).is_err());
        assert_eq!(
            std::fs::read_to_string(&backup_path).expect("backup untouched"),
            "existing backup"
        );
        assert_eq!(retained_rows(&connection), before);
        super::require_schema_version(&connection, 1).expect("schema remains old");
        let fresh_backup = temporary.path().join("fresh-backup.db");
        connection
            .execute("CREATE TABLE profile_entries (conflict TEXT)", [])
            .expect("synthetic DDL conflict");
        assert!(Store::migrate(&path, &fresh_backup).is_err());
        super::require_schema_version(&connection, 1).expect("DDL rolled back old schema");
        assert_eq!(retained_rows(&connection), before);
        let backup = super::open_connection(&fresh_backup, true).expect("retained valid backup");
        super::require_schema_version(&backup, 1).expect("backup schema one");
        assert_eq!(retained_rows(&backup), before);
    }

    #[cfg(unix)]
    #[test]
    fn init_rejects_symlink_and_non_regular_targets_before_touching_them() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::os::unix::fs::symlink;

        let temporary = TempDir::new().expect("temporary directory");
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private temporary directory");

        let target = temporary.path().join("target.db");
        Store::init(&target).expect("initialize target database");
        let connection = rusqlite::Connection::open(&target).expect("target database");
        connection
            .pragma_update(None, "journal_mode", "DELETE")
            .expect("select rollback journal mode");
        drop(connection);
        let target_before = std::fs::read(&target).expect("target bytes");

        let link = temporary.path().join("linked.db");
        symlink(&target, &link).expect("database symlink");
        let error = Store::init(&link).expect_err("symlink must be rejected");
        assert_eq!(error.code(), "database_file_invalid");
        assert_eq!(
            std::fs::read(&target).expect("unchanged target bytes"),
            target_before
        );
        let connection = rusqlite::Connection::open(&target).expect("target database");
        let journal_mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .expect("journal mode");
        assert_eq!(journal_mode, "delete");
        drop(connection);

        let absent_target = temporary.path().join("absent.db");
        let dangling_link = temporary.path().join("dangling.db");
        symlink(&absent_target, &dangling_link).expect("dangling database symlink");
        let error = Store::init(&dangling_link).expect_err("dangling symlink must be rejected");
        assert_eq!(error.code(), "database_file_invalid");
        assert!(!absent_target.exists());

        let directory = temporary.path().join("directory.db");
        std::fs::create_dir(&directory).expect("database-shaped directory");
        let error = Store::init(&directory).expect_err("directory must be rejected");
        assert_eq!(error.code(), "database_file_invalid");
    }

    fn worker_owner(store: &Store) -> (Option<String>, Option<i64>, Option<String>) {
        rusqlite::Connection::open(store.path())
            .expect("raw database")
            .query_row(
                "SELECT worker_token, worker_pid, worker_acquired_at
                 FROM crm_meta WHERE marker = 'crm'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("worker owner")
    }

    #[test]
    fn worker_lease_blocks_live_owner_and_releases_all_fields() {
        let (_temporary, store) = fixture();
        let lease = store.acquire_worker_lease().expect("worker lease");
        let error = store
            .acquire_worker_lease()
            .expect_err("live owner must exclude another worker");
        assert_eq!(error.code(), "worker_already_running");
        let (token, pid, acquired_at) = worker_owner(&store);
        assert_eq!(token.as_deref(), Some(lease.token.as_str()));
        assert_eq!(pid, Some(i64::from(std::process::id())));
        assert!(acquired_at.is_some());

        drop(lease);
        assert_eq!(worker_owner(&store), (None, None, None));
    }

    #[test]
    fn final_queue_handoff_serializes_with_concurrent_enqueue() {
        let (_temporary, store) = fixture();
        for sequence in 0..16 {
            let case = store
                .create_case(
                    &format!("Race {sequence}"),
                    &format!("# Race {sequence}\n"),
                    Stage::Research,
                )
                .expect("case");
            let mut owner = store.acquire_worker_lease().expect("worker lease");
            let barrier = Arc::new(Barrier::new(2));
            let enqueue_barrier = Arc::clone(&barrier);
            let enqueue_store = store.clone();
            let case_id = case.case_id.clone();
            let enqueue = thread::spawn(move || {
                enqueue_barrier.wait();
                enqueue_store
                    .enqueue_delivery(&case_id, "racing note", "Signal", None)
                    .expect("concurrent enqueue")
            });

            barrier.wait();
            let owner_claim = owner
                .claim_next_or_release()
                .expect("owner atomic claim or release");
            let queued = enqueue.join().expect("enqueue thread");
            if let Some(claimed) = owner_claim {
                assert_eq!(claimed.id, queued.id);
                store
                    .mark_failed(&claimed.id, "synthetic terminal result")
                    .expect("settle owner claim");
                assert!(
                    owner
                        .claim_next_or_release()
                        .expect("owner final release")
                        .is_none()
                );
            } else {
                let mut successor = store
                    .acquire_worker_lease()
                    .expect("successor acquires released lease");
                let claimed = successor
                    .claim_next_or_release()
                    .expect("successor claim")
                    .expect("queued update remains visible after release");
                assert_eq!(claimed.id, queued.id);
                store
                    .mark_failed(&claimed.id, "synthetic terminal result")
                    .expect("settle successor claim");
                assert!(
                    successor
                        .claim_next_or_release()
                        .expect("successor final release")
                        .is_none()
                );
            }
        }
    }

    #[test]
    fn worker_lease_reclaims_dead_and_expired_owners_conditionally() {
        let (_temporary, store) = fixture();
        let dead_owner = store.acquire_worker_lease().expect("dead owner lease");
        rusqlite::Connection::open(store.path())
            .expect("raw database")
            .execute(
                "UPDATE crm_meta SET worker_pid = 999999999 WHERE marker = 'crm'",
                [],
            )
            .expect("make owner dead");
        let replacement = store.acquire_worker_lease().expect("reclaim dead owner");
        let replacement_token = replacement.token.clone();
        drop(dead_owner);
        assert_eq!(
            worker_owner(&store).0.as_deref(),
            Some(replacement_token.as_str())
        );
        drop(replacement);

        let expired_owner = store.acquire_worker_lease().expect("expired owner lease");
        rusqlite::Connection::open(store.path())
            .expect("raw database")
            .execute(
                "UPDATE crm_meta SET worker_acquired_at = '2000-01-01T00:00:00Z'
                 WHERE marker = 'crm'",
                [],
            )
            .expect("expire live owner");
        let replacement = store.acquire_worker_lease().expect("reclaim expired owner");
        let replacement_token = replacement.token.clone();
        drop(expired_owner);
        assert_eq!(
            worker_owner(&store).0.as_deref(),
            Some(replacement_token.as_str())
        );
    }

    #[test]
    fn worker_lease_refresh_is_token_guarded() {
        let (_temporary, store) = fixture();
        let mut lease = store.acquire_worker_lease().expect("worker lease");
        rusqlite::Connection::open(store.path())
            .expect("raw database")
            .execute(
                "UPDATE crm_meta SET worker_acquired_at = '2000-01-01T00:00:00Z'
                 WHERE marker = 'crm'",
                [],
            )
            .expect("age worker lease");
        lease.refresh().expect("refresh owned lease");
        assert_ne!(
            worker_owner(&store).2.as_deref(),
            Some("2000-01-01T00:00:00Z")
        );

        rusqlite::Connection::open(store.path())
            .expect("raw database")
            .execute(
                "UPDATE crm_meta
                 SET worker_token = 'replacement', worker_pid = ?1,
                     worker_acquired_at = '2000-01-01T00:00:00Z'
                 WHERE marker = 'crm'",
                [i64::from(std::process::id())],
            )
            .expect("replace lease");
        let error = lease.refresh().expect_err("old token must not refresh");
        assert_eq!(error.code(), "worker_lease_lost");
        drop(lease);
        assert_eq!(worker_owner(&store).0.as_deref(), Some("replacement"));
    }

    #[test]
    fn applied_update_settles_only_after_runtime_result() {
        let (_temporary, store) = fixture();
        let case = store
            .create_case("Runtime", "# Runtime\n", Stage::Research)
            .expect("create case");
        let update = store
            .enqueue_delivery(&case.case_id, "note", "Signal", None)
            .expect("queue delivery");
        store.claim_next().expect("claim").expect("update");
        assert_eq!(
            store
                .unsettled_updates()
                .expect("unsettled")
                .into_iter()
                .next()
                .expect("running update")
                .id,
            update.id
        );
        let proposal = RevisionProposal {
            base_revision: 1,
            document_markdown: "# Runtime\n\nSignal\n".to_owned(),
            stage: Stage::Research,
            advisory: None,
            summary: "Recorded signal".to_owned(),
        };
        store
            .commit_proposal(
                &update.id,
                &update.job_id,
                "call-runtime",
                &"d".repeat(64),
                &proposal,
            )
            .expect("commit proposal");

        let applied = store.update(&update.id).expect("applied update");
        assert_eq!(applied.status, UpdateStatus::Applied);
        assert!(!applied.result_posted);
        assert!(applied.runtime_state.is_none());
        assert!(!applied.is_settled());
        assert!(applied.needs_worker());
        assert_eq!(
            store
                .unsettled_updates()
                .expect("unsettled")
                .into_iter()
                .next()
                .expect("applied update")
                .id,
            update.id
        );

        store
            .mark_result_posted(&update.id)
            .expect("mark result posted");
        store
            .mark_runtime_finished(&update.id, "completed", None)
            .expect("mark runtime finished");
        store
            .mark_runtime_finished(&update.id, "completed", None)
            .expect("repeat runtime result");
        let conflict = store
            .mark_runtime_finished(&update.id, "failed", Some("different"))
            .expect_err("runtime result is immutable");
        assert_eq!(conflict.code(), "update_runtime_conflict");
        store
            .record_applied_diagnostic(&update.id, "late observation")
            .expect("record applied diagnostic");

        let settled = store.update(&update.id).expect("settled update");
        assert!(settled.result_posted);
        assert_eq!(settled.runtime_state.as_deref(), Some("completed"));
        assert!(settled.is_settled());
        assert!(!settled.needs_worker());
        assert_eq!(settled.last_error.as_deref(), Some("late observation"));
        let listed = store.list_updates(1).expect("list updates");
        assert_eq!(listed[0].result_posted, settled.result_posted);
        assert_eq!(listed[0].runtime_state, settled.runtime_state);
        assert_eq!(listed[0].runtime_detail, settled.runtime_detail);
        assert!(store.unsettled_updates().expect("no unsettled").is_empty());
    }

    #[test]
    fn advisory_is_data_not_a_gate() {
        let (_temporary, store) = fixture();
        let case = store
            .create_case("Ada", "# Ada\n", Stage::Research)
            .expect("create case");
        let update = store
            .enqueue_delivery(&case.case_id, "note", "New lead", None)
            .expect("queue delivery");
        let claimed = store.claim_next().expect("claim").expect("update");
        assert_eq!(claimed.id, update.id);
        let advisory = "  Identity evidence is old.\n";
        let proposal = RevisionProposal {
            base_revision: 1,
            document_markdown: "# Ada\n\nNew lead\n".to_owned(),
            stage: Stage::Warranted,
            advisory: Some(advisory.to_owned()),
            summary: "Added lead".to_owned(),
        };
        let revision = store
            .commit_proposal(
                &update.id,
                &update.job_id,
                "call-1",
                &"a".repeat(64),
                &proposal,
            )
            .expect("commit proposal");
        assert_eq!(revision, 2);
        let current = store
            .case_revision(&case.case_id, None)
            .expect("read current");
        assert!(current.attention);
        assert_eq!(current.advisory.as_deref(), Some(advisory));
        assert_eq!(current.stage, Stage::Warranted);
        assert_eq!(
            store.update(&update.id).expect("update").status,
            UpdateStatus::Applied
        );

        let next = store
            .enqueue_delivery(&case.case_id, "more", "More information", None)
            .expect("queue despite advisory");
        assert_eq!(next.status, UpdateStatus::Queued);
    }

    #[test]
    fn mailbox_replay_is_idempotent_and_conflicts_are_rejected() {
        let (_temporary, store) = fixture();
        let case = store
            .create_case("Grace", "# Grace\n", Stage::Research)
            .expect("create case");
        let update = store
            .enqueue_delivery(&case.case_id, "note", "Signal", None)
            .expect("queue delivery");
        store.claim_next().expect("claim").expect("update");
        let proposal = RevisionProposal {
            base_revision: 1,
            document_markdown: "# Grace\n\nSignal\n".to_owned(),
            stage: Stage::Research,
            advisory: None,
            summary: "Recorded signal".to_owned(),
        };
        let digest = "b".repeat(64);
        let first = store
            .commit_proposal(&update.id, &update.job_id, "call-1", &digest, &proposal)
            .expect("first commit");
        let replay = store
            .commit_proposal(&update.id, &update.job_id, "call-1", &digest, &proposal)
            .expect("replay");
        assert_eq!(first, replay);
        let error = store
            .commit_proposal(
                &update.id,
                &update.job_id,
                "call-1",
                &"c".repeat(64),
                &proposal,
            )
            .expect_err("different arguments must conflict");
        assert_eq!(error.code(), "mailbox_arguments_conflict");
    }

    #[test]
    fn updates_for_one_case_are_serialized() {
        let (_temporary, store) = fixture();
        let case = store
            .create_case("Lin", "# Lin\n", Stage::Research)
            .expect("create case");
        let first = store
            .enqueue_delivery(&case.case_id, "one", "One", None)
            .expect("first");
        let second = store
            .enqueue_delivery(&case.case_id, "two", "Two", None)
            .expect("second");
        assert_eq!(
            store.claim_next().expect("claim").expect("first").id,
            first.id
        );
        assert!(store.claim_next().expect("second blocked").is_none());
        store.mark_failed(&first.id, "test").expect("release case");
        assert_eq!(
            store.claim_next().expect("claim").expect("second").id,
            second.id
        );
    }

    #[test]
    fn explicit_retry_reuses_delivery_and_gets_new_job_identity() {
        let (_temporary, store) = fixture();
        let case = store
            .create_case("Pat", "# Pat\n", Stage::Research)
            .expect("create case");
        let first = store
            .enqueue_delivery(&case.case_id, "note", "Signal", None)
            .expect("delivery");
        store.claim_next().expect("claim").expect("first");
        store.mark_failed(&first.id, "terminal").expect("fail");
        let retry = store.enqueue_retry(&first.id).expect("retry");
        assert_eq!(retry.delivery_id, first.delivery_id);
        assert_eq!(retry.retry_of.as_deref(), Some(first.id.as_str()));
        assert_ne!(retry.job_id, first.job_id);
        assert_ne!(retry.requester_id, first.requester_id);
        let error = store
            .enqueue_retry(&first.id)
            .expect_err("retry lineage must not branch");
        assert_eq!(error.code(), "update_retry_exists");
    }

    #[test]
    fn doctor_rejects_missing_immutable_schema_objects() {
        let (temporary, store) = fixture();
        let connection = rusqlite::Connection::open(store.path()).expect("raw database");
        connection
            .execute_batch("DROP TRIGGER deliveries_no_update;")
            .expect("drop trigger");
        drop(connection);
        let error = Store::doctor(store.path()).expect_err("doctor must reject missing trigger");
        assert_eq!(error.code(), "schema_incomplete");
        drop(temporary);
    }
}
