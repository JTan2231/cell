use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use sha2::{Digest as _, Sha256};

use crate::error::{AppError, AppResult, Context as _};
use crate::model::{
    Candidate, DecisionEventAuthoritySpan, DecisionEventDecision, DecisionEventEnvelope,
    DecisionEventReview, Delivery, DigestSnapshot, Observation, ObservationClassification,
    ObservationFailure, ObservationStatus, PersistedCandidate, PersistedObservationClassification,
    Run, SourceMessage, StoredCandidate, StoredSource,
};

const SCHEMA: &str = include_str!("../schema.sql");
const MIGRATION_2: &str = include_str!("../migration_2.sql");
const MIGRATION_3: &str = include_str!("../migration_3.sql");
const SCHEMA_VERSION: i64 = 3;
const EVENT_STREAM: &str = "decisions.lifecycle";
const EVENT_ENVELOPE_VERSION: i64 = 1;

pub(crate) struct Store {
    connection: Connection,
    database_path: PathBuf,
}

pub(crate) struct RunOperationLock {
    file: fs::File,
}

impl Drop for RunOperationLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ClassificationReceipt {
    pub(crate) result_json: String,
    pub(crate) is_error: bool,
    pub(crate) classification: Option<Vec<Candidate>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ObservationClassificationReceipt {
    pub(crate) result_json: String,
    pub(crate) is_error: bool,
    pub(crate) classification: Option<ObservationClassification>,
}

#[derive(Debug, Clone)]
pub(crate) struct ObservationProjection {
    pub(crate) run: Run,
    pub(crate) source_manifest_hash: String,
    pub(crate) observations_covered: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct DecisionEventWatermark {
    pub(crate) stream: &'static str,
    pub(crate) envelope_version: i64,
    pub(crate) cursor: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct DecisionEventPage {
    pub(crate) stream: &'static str,
    pub(crate) envelope_version: i64,
    pub(crate) after_cursor: String,
    pub(crate) next_cursor: String,
    pub(crate) watermark_cursor: String,
    pub(crate) has_more: bool,
    pub(crate) events: Vec<DecisionEventItem>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct DecisionEventItem {
    pub(crate) cursor: String,
    pub(crate) event: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunJobCorrelation {
    pub(crate) nucleus_job_id: String,
    pub(crate) admitted: bool,
    pub(crate) status: String,
}

impl Store {
    #[allow(clippy::too_many_lines)]
    pub(crate) fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            let created_parent = match fs::symlink_metadata(parent) {
                Ok(_) => false,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir_all(parent).context(
                        "state_directory_failed",
                        format!("unable to create {}", parent.display()),
                    )?;
                    true
                }
                Err(error) => {
                    return Err(AppError::new(
                        "state_directory_failed",
                        format!("unable to inspect {}: {error}", parent.display()),
                    ));
                }
            };
            let metadata = fs::symlink_metadata(parent).context(
                "state_directory_failed",
                format!("unable to inspect {}", parent.display()),
            )?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(AppError::new(
                    "state_directory_unsafe",
                    format!(
                        "state directory must be a regular directory: {}",
                        parent.display()
                    ),
                ));
            }
            if created_parent {
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).context(
                    "state_directory_failed",
                    format!("unable to make {} private", parent.display()),
                )?;
            }
        }
        inspect_private_database_file(path, false)?;
        let sidecars = database_sidecars(path);
        for sidecar in &sidecars {
            inspect_private_database_file(sidecar, false)?;
        }
        let mut connection = Connection::open(path).context(
            "database_open_failed",
            format!("unable to open {}", path.display()),
        )?;
        inspect_private_database_file(path, true)?;
        connection.busy_timeout(Duration::from_secs(5)).context(
            "database_config_failed",
            "unable to set SQLite busy timeout",
        )?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .context("database_config_failed", "unable to enable foreign keys")?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .context(
                "database_schema_failed",
                "unable to inspect database schema",
            )?;
        match version {
            0 => {
                connection.execute_batch(SCHEMA).context(
                    "database_schema_failed",
                    "unable to initialize Decisions schema",
                )?;
                connection
                    .execute(
                        "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(1, ?1)",
                        [now_unix()],
                    )
                    .context("database_schema_failed", "unable to record schema migration")?;
                connection
                    .execute(
                        "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(2, ?1)",
                        [now_unix()],
                    )
                    .context("database_schema_failed", "unable to record schema migration")?;
                connection
                    .execute(
                        "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(3, ?1)",
                        [now_unix()],
                    )
                    .context("database_schema_failed", "unable to record schema migration")?;
            }
            1 => {
                connection.execute_batch(MIGRATION_2).context(
                    "database_schema_failed",
                    "unable to migrate Decisions schema from version 1 to version 2",
                )?;
                migrate_v2_to_v3(&mut connection)?;
            }
            2 => migrate_v2_to_v3(&mut connection)?,
            SCHEMA_VERSION => {}
            newer if newer > SCHEMA_VERSION => {
                return Err(AppError::new(
                    "database_schema_too_new",
                    format!(
                        "database schema {newer} is newer than supported schema {SCHEMA_VERSION}"
                    ),
                ));
            }
            older => {
                return Err(AppError::new(
                    "database_migration_required",
                    format!("database schema {older} must be migrated to {SCHEMA_VERSION}"),
                ));
            }
        }
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .context("database_config_failed", "unable to enable WAL")?;
        for sidecar in &sidecars {
            inspect_private_database_file(sidecar, false)?;
        }
        Ok(Self {
            connection,
            database_path: path.to_path_buf(),
        })
    }

    pub(crate) fn lock_run_operations(&self) -> AppResult<RunOperationLock> {
        let lock_path = run_operation_lock_path(&self.database_path);
        inspect_private_database_file(&lock_path, false)?;
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&lock_path)
            .context(
                "run_operation_lock_failed",
                "unable to open the private Decisions run-operation lock",
            )?;
        inspect_private_database_file(&lock_path, true)?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|_error| {
            AppError::new(
                "run_operation_busy",
                "another Decisions build or abandonment operation is active",
            )
        })?;
        Ok(RunOperationLock { file })
    }

    pub(crate) fn lock_observation_processing(&self) -> AppResult<RunOperationLock> {
        self.open_observation_processing_lock(false)
    }

    pub(crate) fn wait_for_observation_processing(&self) -> AppResult<RunOperationLock> {
        self.open_observation_processing_lock(true)
    }

    fn open_observation_processing_lock(&self, wait: bool) -> AppResult<RunOperationLock> {
        let lock_path = observation_operation_lock_path(&self.database_path);
        inspect_private_database_file(&lock_path, false)?;
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&lock_path)
            .context(
                "observation_lock_failed",
                "unable to open the private Decisions observation lock",
            )?;
        inspect_private_database_file(&lock_path, true)?;
        let lock_result = if wait {
            fs2::FileExt::lock_exclusive(&file)
        } else {
            fs2::FileExt::try_lock_exclusive(&file)
        };
        lock_result.map_err(|_error| {
            AppError::new(
                if wait {
                    "observation_lock_failed"
                } else {
                    "observation_busy"
                },
                if wait {
                    "unable to wait for the active Decisions observation"
                } else {
                    "another Decisions observation is being processed"
                },
            )
        })?;
        Ok(RunOperationLock { file })
    }

    pub(crate) fn schema_version(&self) -> AppResult<i64> {
        self.connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .context(
                "database_schema_failed",
                "unable to inspect database schema",
            )
    }

    pub(crate) fn event_watermark(&self) -> AppResult<DecisionEventWatermark> {
        let sequence = event_watermark_sequence(&self.connection)?;
        Ok(DecisionEventWatermark {
            stream: EVENT_STREAM,
            envelope_version: EVENT_ENVELOPE_VERSION,
            cursor: encode_event_cursor(sequence),
        })
    }

    pub(crate) fn read_events(
        &self,
        after_cursor: &str,
        limit: usize,
    ) -> AppResult<DecisionEventPage> {
        if !(1..=1000).contains(&limit) {
            return Err(AppError::new(
                "event_limit_invalid",
                "event read limit must be between 1 and 1000",
            ));
        }
        let after = decode_event_cursor(after_cursor)?;
        let watermark = event_watermark_sequence(&self.connection)?;
        if after > watermark {
            return Err(AppError::new(
                "event_cursor_ahead",
                "event cursor is ahead of the current Decisions watermark",
            ));
        }
        let sql_limit = i64::try_from(limit.saturating_add(1)).map_err(|_| {
            AppError::new(
                "event_limit_invalid",
                "event read limit exceeds the supported SQLite range",
            )
        })?;
        let rows = {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT sequence, envelope_json
                     FROM decision_events
                     WHERE sequence>?1 AND sequence<=?2
                     ORDER BY sequence
                     LIMIT ?3",
                )
                .context(
                    "database_read_failed",
                    "unable to prepare the decision event stream",
                )?;
            statement
                .query_map(params![after, watermark, sql_limit], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .context(
                    "database_read_failed",
                    "unable to read the decision event stream",
                )?
                .collect::<Result<Vec<_>, _>>()
                .context(
                    "database_read_failed",
                    "unable to decode the decision event stream",
                )?
        };
        let has_more = rows.len() > limit;
        let rows = rows.into_iter().take(limit).collect::<Vec<_>>();
        let next = rows.last().map_or(after, |(sequence, _)| *sequence);
        let events = rows
            .into_iter()
            .map(|(sequence, payload)| {
                let event =
                    serde_json::from_str::<serde_json::Value>(&payload).map_err(|_error| {
                        AppError::new(
                            "decision_event_invalid",
                            "a persisted decision event envelope is invalid",
                        )
                    })?;
                Ok(DecisionEventItem {
                    cursor: encode_event_cursor(sequence),
                    event,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        Ok(DecisionEventPage {
            stream: EVENT_STREAM,
            envelope_version: EVENT_ENVELOPE_VERSION,
            after_cursor: after_cursor.to_owned(),
            next_cursor: encode_event_cursor(next),
            watermark_cursor: encode_event_cursor(watermark),
            has_more,
            events,
        })
    }

    pub(crate) fn activate_observer(&self, requested_at: i64) -> AppResult<i64> {
        self.connection
            .execute(
                "INSERT OR IGNORE INTO product_metadata(key, value, created_at)
                 VALUES('observer_baseline_at', ?1, ?1)",
                [requested_at],
            )
            .context(
                "database_write_failed",
                "unable to persist the observer activation baseline",
            )?;
        self.observer_baseline_at()?.ok_or_else(|| {
            AppError::new(
                "observer_activation_failed",
                "the observer baseline was not durably recorded",
            )
        })
    }

    pub(crate) fn observer_baseline_at(&self) -> AppResult<Option<i64>> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM product_metadata WHERE key='observer_baseline_at'",
                [],
                |row| row.get(0),
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to read the observer activation baseline",
            )?;
        value
            .map(|value| {
                value.parse::<i64>().map_err(|_| {
                    AppError::new(
                        "observer_baseline_invalid",
                        "the persisted observer baseline is not a Unix timestamp",
                    )
                })
            })
            .transpose()
    }

    pub(crate) fn ingest_observation(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> AppResult<Observation> {
        let identity = format!("{session_id}\n{turn_id}");
        let digest = format!("{:x}", Sha256::digest(identity.as_bytes()));
        let id = format!("o_{}", &digest[..20]);
        let now = now_unix();
        self.connection
            .execute(
                "INSERT OR IGNORE INTO observations(
                    id, session_id, turn_id, status, created_at, updated_at
                 ) VALUES(?1, ?2, ?3, 'queued', ?4, ?4)",
                params![id, session_id, turn_id, now],
            )
            .context(
                "database_write_failed",
                "unable to durably enqueue the completed turn",
            )?;
        self.observation_by_correlation(session_id, turn_id)
    }

    pub(crate) fn ingest_reconciled_observation(
        &mut self,
        host_id: &str,
        thread_id: &str,
        turn_id: &str,
        source_completed_at: i64,
    ) -> AppResult<(Observation, bool)> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to lock reconciled observation admission",
            )?;
        let matching = {
            let mut statement = transaction
                .prepare(
                    "SELECT id FROM observations
                     WHERE turn_id=?1
                       AND (thread_id IS NULL OR host_id=?2)
                     ORDER BY id",
                )
                .context(
                    "database_read_failed",
                    "unable to prepare reconciled observation matching",
                )?;
            statement
                .query_map(params![turn_id, host_id], |row| row.get::<_, String>(0))
                .context(
                    "database_read_failed",
                    "unable to read reconciled observation matching",
                )?
                .collect::<Result<Vec<_>, _>>()
                .context(
                    "database_read_failed",
                    "unable to decode reconciled observation matching",
                )?
        };
        if matching.len() > 1 {
            return Err(AppError::new(
                "observation_correlation_ambiguous",
                "multiple hook observations match the exact reconciled root turn",
            ));
        }
        let existing = matching.into_iter().next();
        if let Some(id) = existing.as_deref()
            && observation_source_was_abandoned(&transaction, id)?
        {
            return Err(AppError::new(
                "observation_source_abandoned_conflict",
                "a source explicitly abandoned as unavailable later appeared in completed-turn reconciliation",
            ));
        }
        let inserted = existing.is_none();
        let id = existing.unwrap_or_else(|| {
            let identity = format!("{thread_id}\n{turn_id}");
            let digest = format!("{:x}", Sha256::digest(identity.as_bytes()));
            format!("o_{}", &digest[..20])
        });
        let now = now_unix();
        transaction
            .execute(
                "INSERT OR IGNORE INTO observations(
                    id, session_id, turn_id, host_id, thread_id,
                    source_completed_at, status, created_at, updated_at
                 ) VALUES(?1, ?2, ?3, ?4, ?2, ?5, 'queued', ?6, ?6)",
                params![id, thread_id, turn_id, host_id, source_completed_at, now],
            )
            .context(
                "database_write_failed",
                "unable to durably enqueue the exact reconciled root turn",
            )?;
        let changed = transaction
            .execute(
                "UPDATE observations SET
                    host_id=COALESCE(host_id, ?2),
                    thread_id=COALESCE(thread_id, ?3),
                    source_completed_at=COALESCE(source_completed_at, ?4),
                    source_not_completed_at=NULL, next_attempt_at=NULL,
                    updated_at=?5
                 WHERE id=?1
                   AND (host_id IS NULL OR host_id=?2)
                   AND (source_completed_at IS NULL OR source_completed_at=?4)",
                params![id, host_id, thread_id, source_completed_at, now],
            )
            .context(
                "database_write_failed",
                "unable to bind the reconciled completion frontier",
            )?;
        if changed != 1 {
            return Err(AppError::new(
                "observation_source_conflict",
                "the completed-turn frontier changed after reconciliation",
            ));
        }
        transaction.commit().context(
            "database_write_failed",
            "unable to commit exact reconciled observation admission",
        )?;
        Ok((self.observation(&id)?, inserted))
    }

    pub(crate) fn observation_status(&self) -> AppResult<ObservationStatus> {
        self.observation_status_window(None)
    }

    pub(crate) fn observation_status_window(
        &self,
        window: Option<(i64, i64)>,
    ) -> AppResult<ObservationStatus> {
        let (window_start, window_end) =
            window.map_or((None, None), |(start, end)| (Some(start), Some(end)));
        let mut counts = [0_usize; 4];
        let mut statement = self
            .connection
            .prepare(
                "SELECT status, COUNT(*) FROM observations
                 WHERE ?1 IS NULL OR EXISTS (
                     SELECT 1 FROM observation_authority_items items
                     WHERE items.observation_id=observations.id
                       AND items.occurred_at>=?1 AND items.occurred_at<?2
                 ) OR (
                     status!='complete' AND NOT EXISTS (
                         SELECT 1 FROM observation_authority_items items
                         WHERE items.observation_id=observations.id
                     )
                 )
                 GROUP BY status",
            )
            .context("database_read_failed", "unable to prepare observer status")?;
        let rows = statement
            .query_map(params![window_start, window_end], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .context("database_read_failed", "unable to read observer status")?;
        for row in rows {
            let (status, count) =
                row.context("database_read_failed", "unable to decode observer status")?;
            let count = usize::try_from(count).unwrap_or(usize::MAX);
            match status.as_str() {
                "queued" => counts[0] = count,
                "processing" => counts[1] = count,
                "complete" => counts[2] = count,
                "failed" => counts[3] = count,
                _ => {}
            }
        }
        let failures = {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT id, COALESCE(failure_code, 'unknown')
                     FROM observations
                     WHERE status='failed' AND (
                         ?1 IS NULL OR EXISTS (
                             SELECT 1 FROM observation_authority_items items
                             WHERE items.observation_id=observations.id
                               AND items.occurred_at>=?1 AND items.occurred_at<?2
                         ) OR NOT EXISTS (
                             SELECT 1 FROM observation_authority_items items
                             WHERE items.observation_id=observations.id
                         )
                     )
                     ORDER BY updated_at, id",
                )
                .context(
                    "database_read_failed",
                    "unable to prepare failed observation status",
                )?;
            statement
                .query_map(params![window_start, window_end], |row| {
                    Ok(ObservationFailure {
                        id: row.get(0)?,
                        failure_code: row.get(1)?,
                    })
                })
                .context(
                    "database_read_failed",
                    "unable to read failed observation status",
                )?
                .collect::<Result<Vec<_>, _>>()
                .context(
                    "database_read_failed",
                    "unable to decode failed observation status",
                )?
        };
        Ok(ObservationStatus {
            observer_baseline_at: self.observer_baseline_at()?,
            queued: counts[0],
            processing: counts[1],
            complete: counts[2],
            failed: counts[3],
            failures,
        })
    }

    pub(crate) fn next_observation_before(
        &self,
        created_cutoff: Option<i64>,
    ) -> AppResult<Option<Observation>> {
        self.next_observation_before_at(created_cutoff, now_unix())
    }

    fn next_observation_before_at(
        &self,
        created_cutoff: Option<i64>,
        ready_at: i64,
    ) -> AppResult<Option<Observation>> {
        self.connection
            .query_row(
                "SELECT id, session_id, turn_id, host_id, thread_id,
                        status, scope_level, attempt_epoch, outcome, file_change_count,
                        authority_occurred_at, failure_code
                 FROM observations
                 WHERE status IN ('queued', 'processing')
                   AND (?1 IS NULL OR COALESCE(source_completed_at, created_at)<=?1)
                   AND (status='processing' OR next_attempt_at IS NULL OR next_attempt_at<=?2)
                 ORDER BY CASE status WHEN 'processing' THEN 0 ELSE 1 END,
                          COALESCE(next_attempt_at, created_at), created_at, id LIMIT 1",
                params![created_cutoff, ready_at],
                decode_observation,
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to read the next turn observation",
            )
    }

    pub(crate) fn next_observation_for_projection(
        &self,
        completion_cutoff: i64,
        admission_watermark: i64,
        window_start: i64,
        window_end: i64,
    ) -> AppResult<Option<Observation>> {
        self.next_observation_for_projection_at(
            completion_cutoff,
            admission_watermark,
            window_start,
            window_end,
            now_unix(),
        )
    }

    fn next_observation_for_projection_at(
        &self,
        completion_cutoff: i64,
        admission_watermark: i64,
        window_start: i64,
        window_end: i64,
        ready_at: i64,
    ) -> AppResult<Option<Observation>> {
        self.connection
            .query_row(
                "SELECT id, session_id, turn_id, host_id, thread_id,
                        status, scope_level, attempt_epoch, outcome, file_change_count,
                        authority_occurred_at, failure_code
                 FROM observations
                 WHERE status IN ('queued', 'processing')
                   AND rowid<=?2
                   AND (status='processing' OR next_attempt_at IS NULL OR next_attempt_at<=?3)
                   AND (
                       (source_completed_at IS NOT NULL AND source_completed_at<=?1)
                       OR (source_completed_at IS NULL
                           AND (source_not_completed_at IS NULL
                                OR source_not_completed_at<=?1))
                   )
                   AND (
                       NOT EXISTS (
                           SELECT 1 FROM observation_authority_items items
                           WHERE items.observation_id=observations.id
                       )
                       OR EXISTS (
                           SELECT 1 FROM observation_authority_items items
                           WHERE items.observation_id=observations.id
                             AND items.occurred_at>=?4 AND items.occurred_at<?5
                       )
                   )
                 ORDER BY CASE status WHEN 'processing' THEN 0 ELSE 1 END,
                          COALESCE(next_attempt_at, created_at), created_at, id LIMIT 1",
                params![
                    completion_cutoff,
                    admission_watermark,
                    ready_at,
                    window_start,
                    window_end
                ],
                decode_observation,
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to read the next turn observation for daily projection",
            )
    }

    pub(crate) fn observation_admission_watermark(&self) -> AppResult<i64> {
        self.connection
            .query_row(
                "SELECT COALESCE(MAX(rowid), 0) FROM observations",
                [],
                |row| row.get(0),
            )
            .context(
                "database_read_failed",
                "unable to capture the durable observation admission watermark",
            )
    }

    fn observation_by_correlation(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> AppResult<Observation> {
        self.connection
            .query_row(
                "SELECT id, session_id, turn_id, host_id, thread_id,
                        status, scope_level, attempt_epoch, outcome, file_change_count,
                        authority_occurred_at, failure_code
                 FROM observations WHERE session_id=?1 AND turn_id=?2",
                params![session_id, turn_id],
                decode_observation,
            )
            .context(
                "database_read_failed",
                "unable to verify the enqueued completed turn",
            )
    }

    pub(crate) fn observation(&self, id: &str) -> AppResult<Observation> {
        self.connection
            .query_row(
                "SELECT id, session_id, turn_id, host_id, thread_id,
                        status, scope_level, attempt_epoch, outcome, file_change_count,
                        authority_occurred_at, failure_code
                 FROM observations WHERE id=?1",
                [id],
                decode_observation,
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to read the turn observation",
            )?
            .ok_or_else(|| {
                AppError::new("observation_not_found", format!("unknown observation {id}"))
            })
    }

    pub(crate) fn retry_observation(&self, id: &str) -> AppResult<Observation> {
        let changed = self
            .connection
            .execute(
                "UPDATE observations SET status='queued', attempt_epoch=attempt_epoch+1,
                    failure_code=NULL, failure_detail=NULL, completed_at=NULL,
                    next_attempt_at=NULL, updated_at=?2
                 WHERE id=?1 AND status='failed'",
                params![id, now_unix()],
            )
            .context(
                "database_write_failed",
                "unable to requeue the failed turn observation",
            )?;
        if changed != 1 {
            let observation = self.observation(id)?;
            return Err(AppError::new(
                "observation_not_failed",
                format!(
                    "observation {} is {}, not failed",
                    observation.id, observation.status
                ),
            ));
        }
        self.observation(id)
    }

    pub(crate) fn abandon_unavailable_observation(&mut self, id: &str) -> AppResult<Observation> {
        let observation = self.observation(id)?;
        if observation.status == "complete"
            && observation.outcome.as_deref() == Some("not_eligible")
            && observation.failure_code.as_deref() == Some("conversation_source_abandoned")
        {
            return Ok(observation);
        }
        if observation.status != "queued" {
            return Err(AppError::new(
                "observation_source_abandon_invalid",
                format!(
                    "observation {} is {}, not an unbound queued source",
                    observation.id, observation.status
                ),
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to lock the unavailable observation recovery",
            )?;
        let now = now_unix();
        let changed = transaction
            .execute(
                "UPDATE observations SET status='complete', outcome='not_eligible',
                    next_attempt_at=NULL,
                    failure_code='conversation_source_abandoned',
                    failure_detail='operator confirmed the unresolved Stop-hook source is permanently unavailable',
                    completed_at=?2, updated_at=?2
                 WHERE id=?1 AND status='queued' AND scope_level=0
                   AND outcome IS NULL
                   AND host_id IS NULL AND thread_id IS NULL
                   AND source_digest IS NULL AND source_completed_at IS NULL
                   AND source_not_completed_at IS NULL
                   AND next_attempt_at IS NOT NULL
                   AND file_change_count=0 AND authority_occurred_at IS NULL
                   AND failure_code IS NULL AND completed_at IS NULL
                   AND NOT EXISTS (
                       SELECT 1 FROM observation_jobs jobs
                       WHERE jobs.observation_id=observations.id
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM observation_authority_items items
                       WHERE items.observation_id=observations.id
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM observation_candidates candidates
                       WHERE candidates.observation_id=observations.id
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM authority_verdicts verdicts
                       WHERE verdicts.observation_id=observations.id
                   )",
                params![id, now],
            )
            .context(
                "database_write_failed",
                "unable to abandon the unavailable observation source",
            )?;
        if changed != 1 {
            return Err(AppError::new(
                "observation_source_abandon_unsafe",
                "only an observer-deferred pending source with no bound source, classification job, authority, verdict, or candidate can be abandoned",
            ));
        }
        transaction.commit().context(
            "database_write_failed",
            "unable to commit the unavailable observation recovery",
        )?;
        self.observation(id)
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub(crate) fn bind_observation_source(
        &mut self,
        id: &str,
        host_id: &str,
        thread_id: &str,
        source_completed_at: i64,
        source_digest: &str,
        file_change_count: usize,
        authorities: &[SourceMessage],
    ) -> AppResult<()> {
        if authorities.is_empty() {
            return Err(AppError::new(
                "observation_authority_missing",
                "an eligible observation must contain at least one user authority item",
            ));
        }
        let authority_occurred_at = authorities
            .iter()
            .map(|source| source.occurred_at)
            .min()
            .ok_or_else(|| {
                AppError::new(
                    "observation_authority_missing",
                    "an eligible observation must contain at least one user authority item",
                )
            })?;
        let count = i64::try_from(file_change_count).map_err(|_| {
            AppError::new(
                "source_activity_invalid",
                "file-change count exceeds the supported SQLite range",
            )
        })?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to lock the completed-turn source admission",
            )?;
        let changed = transaction
            .execute(
                "UPDATE observations SET
                    host_id=COALESCE(host_id, ?2),
                    thread_id=COALESCE(thread_id, ?3),
                    source_completed_at=COALESCE(source_completed_at, ?4),
                    source_not_completed_at=NULL, next_attempt_at=NULL,
                    source_digest=COALESCE(source_digest, ?5),
                    file_change_count=?6,
                    authority_occurred_at=COALESCE(authority_occurred_at, ?7),
                    status=CASE WHEN status='queued' THEN 'processing' ELSE status END,
                    updated_at=?8
                 WHERE id=?1 AND status IN ('queued', 'processing')
                   AND (host_id IS NULL OR host_id=?2)
                   AND (thread_id IS NULL OR thread_id=?3)
                   AND (source_completed_at IS NULL OR source_completed_at=?4)
                   AND (source_digest IS NULL OR source_digest=?5)
                   AND (authority_occurred_at IS NULL OR authority_occurred_at=?7)",
                params![
                    id,
                    host_id,
                    thread_id,
                    source_completed_at,
                    source_digest,
                    count,
                    authority_occurred_at,
                    now_unix()
                ],
            )
            .context(
                "database_write_failed",
                "unable to bind the authoritative completed turn",
            )?;
        if changed != 1 {
            return Err(AppError::new(
                "observation_source_conflict",
                "the completed turn changed after observation admission",
            ));
        }
        for authority in authorities {
            if authority.host_id != host_id
                || authority.thread_id != thread_id
                || authority.turn_id.is_empty()
                || authority.role != crate::model::MessageRole::User
            {
                return Err(AppError::new(
                    "observation_authority_invalid",
                    "an observation authority item is not an exact user item in the bound thread",
                ));
            }
            transaction
                .execute(
                    "INSERT OR IGNORE INTO observation_authority_items(
                        observation_id, item_id, host_id, thread_id, turn_id,
                        occurred_at, timestamp_precision
                     ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        id,
                        authority.item_id,
                        authority.host_id,
                        authority.thread_id,
                        authority.turn_id,
                        authority.occurred_at,
                        authority.precision.as_str()
                    ],
                )
                .context(
                    "database_write_failed",
                    "unable to persist an observation authority item",
                )?;
            let agrees: bool = transaction
                .query_row(
                    "SELECT host_id=?3 AND thread_id=?4 AND turn_id=?5
                            AND occurred_at=?6 AND timestamp_precision=?7
                     FROM observation_authority_items
                     WHERE observation_id=?1 AND item_id=?2",
                    params![
                        id,
                        authority.item_id,
                        authority.host_id,
                        authority.thread_id,
                        authority.turn_id,
                        authority.occurred_at,
                        authority.precision.as_str()
                    ],
                    |row| row.get(0),
                )
                .context(
                    "database_read_failed",
                    "unable to verify an observation authority item",
                )?;
            if !agrees {
                return Err(AppError::new(
                    "observation_source_conflict",
                    "the completed-turn authority set changed after observation admission",
                ));
            }
        }
        let stored_count: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM observation_authority_items WHERE observation_id=?1",
                [id],
                |row| row.get(0),
            )
            .context(
                "database_read_failed",
                "unable to verify the observation authority set",
            )?;
        if usize::try_from(stored_count).ok() != Some(authorities.len()) {
            return Err(AppError::new(
                "observation_source_conflict",
                "the completed-turn authority set changed after observation admission",
            ));
        }
        transaction.commit().context(
            "database_write_failed",
            "unable to commit the completed-turn source admission",
        )
    }

    pub(crate) fn mark_observation_not_eligible(
        &self,
        id: &str,
        host_id: &str,
        thread_id: &str,
        source_completed_at: i64,
        authority_occurred_at: Option<i64>,
    ) -> AppResult<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE observations SET status='complete', outcome='not_eligible',
                    host_id=COALESCE(host_id, ?2), thread_id=COALESCE(thread_id, ?3),
                    source_completed_at=COALESCE(source_completed_at, ?4),
                    source_not_completed_at=NULL, next_attempt_at=NULL,
                    authority_occurred_at=COALESCE(authority_occurred_at, ?5),
                    completed_at=?6, updated_at=?6
                 WHERE id=?1 AND status IN ('queued', 'processing')
                   AND (host_id IS NULL OR host_id=?2)
                   AND (thread_id IS NULL OR thread_id=?3)
                   AND (source_completed_at IS NULL OR source_completed_at=?4)",
                params![
                    id,
                    host_id,
                    thread_id,
                    source_completed_at,
                    authority_occurred_at,
                    now_unix()
                ],
            )
            .context(
                "database_write_failed",
                "unable to close an out-of-scope turn observation",
            )?;
        if changed != 1 {
            return Err(AppError::new(
                "observation_source_conflict",
                "the ineligible completed turn changed after observation admission",
            ));
        }
        Ok(())
    }

    pub(crate) fn advance_observation_scope(&self, id: &str) -> AppResult<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE observations SET scope_level=1, status='queued', updated_at=?2
                 WHERE id=?1 AND status='processing' AND scope_level=0",
                params![id, now_unix()],
            )
            .context(
                "database_write_failed",
                "unable to expand turn observation context",
            )?;
        if changed != 1 {
            return Err(AppError::new(
                "observation_state_conflict",
                "turn observation no longer permits context expansion",
            ));
        }
        Ok(())
    }

    pub(crate) fn fail_observation(&self, id: &str, code: &str, detail: &str) -> AppResult<()> {
        self.connection
            .execute(
                "UPDATE observations SET status='failed', failure_code=?2,
                    failure_detail=?3, completed_at=?4, updated_at=?4
                 WHERE id=?1 AND status IN ('queued', 'processing')",
                params![id, code, detail, now_unix()],
            )
            .context(
                "database_write_failed",
                "unable to record failed turn observation",
            )?;
        Ok(())
    }

    pub(crate) fn defer_observation(
        &self,
        id: &str,
        source_not_completed_at: Option<i64>,
        next_attempt_at: i64,
    ) -> AppResult<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE observations SET status='queued',
                    source_not_completed_at=COALESCE(source_not_completed_at, ?2),
                    next_attempt_at=?3, failure_code=NULL, failure_detail=NULL,
                    updated_at=?4
                 WHERE id=?1 AND status IN ('queued', 'processing')",
                params![id, source_not_completed_at, next_attempt_at, now_unix()],
            )
            .context(
                "database_write_failed",
                "unable to defer a completed-turn source that is not ready",
            )?;
        if changed != 1 {
            return Err(AppError::new(
                "observation_state_conflict",
                "the turn observation changed while source retry was deferred",
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn begin_or_resume_run(
        &mut self,
        report_date: &str,
        window_start: i64,
        window_end: i64,
        source_manifest_hash: &str,
    ) -> AppResult<Run> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to lock daily run admission",
            )?;
        if let Some(run) = transaction
            .query_row(
                "SELECT id, report_date, status, content_revision
                 FROM runs
                 WHERE report_date=?1 AND window_start=?2 AND window_end=?3
                   AND source_manifest_hash=?4 AND status='building'
                 ORDER BY started_at DESC LIMIT 1",
                params![report_date, window_start, window_end, source_manifest_hash],
                |row| {
                    Ok(Run {
                        id: row.get(0)?,
                        report_date: row.get(1)?,
                        status: row.get(2)?,
                        content_revision: row.get(3)?,
                    })
                },
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to inspect resumable decision run",
            )?
        {
            transaction
                .commit()
                .context("database_write_failed", "unable to resume daily run")?;
            return Ok(run);
        }
        let incompatible_building: Option<String> = transaction
            .query_row(
                "SELECT id FROM runs
                 WHERE report_date=?1 AND status IN ('building', 'abandoning')
                 ORDER BY started_at DESC LIMIT 1",
                [report_date],
                |row| row.get(0),
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to inspect active decision run",
            )?;
        if let Some(run_id) = incompatible_building {
            return Err(AppError::new(
                "building_run_source_changed",
                format!(
                    "run {run_id} remains active for {report_date}, but it cannot be resumed from this source; retry `decisions daily abandon --date {report_date}` to reconcile it before authorizing a new attempt"
                ),
            ));
        }
        let run = Run {
            id: format!("run_{}", uuid::Uuid::now_v7()),
            report_date: report_date.to_owned(),
            status: "building".to_owned(),
            content_revision: 0,
        };
        transaction
            .execute(
                "INSERT INTO runs(
                    id, run_kind, report_date, window_start, window_end,
                    source_manifest_hash, status, started_at
                 ) VALUES(?1, 'legacy_scan', ?2, ?3, ?4, ?5, 'building', ?6)",
                params![
                    run.id,
                    run.report_date,
                    window_start,
                    window_end,
                    source_manifest_hash,
                    now_unix()
                ],
            )
            .context("database_write_failed", "unable to begin decision run")?;
        transaction.commit().context(
            "database_write_failed",
            "unable to commit decision run admission",
        )?;
        Ok(run)
    }

    #[cfg(test)]
    pub(crate) fn plan_job(
        &mut self,
        run_id: &str,
        thread_id: &str,
        nucleus_job_id: &str,
    ) -> AppResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("database_write_failed", "unable to lock Nucleus job plan")?;
        let run_status: String = transaction
            .query_row("SELECT status FROM runs WHERE id=?1", [run_id], |row| {
                row.get(0)
            })
            .context("database_read_failed", "unable to verify decision run")?;
        if run_status != "building" {
            return Err(AppError::new(
                "run_not_building",
                format!("run {run_id} is not accepting new Nucleus jobs"),
            ));
        }
        transaction
            .execute(
                "INSERT OR IGNORE INTO run_jobs(run_id, thread_id, nucleus_job_id, status)
                 VALUES(?1, ?2, ?3, 'planned')",
                params![run_id, thread_id, nucleus_job_id],
            )
            .context(
                "database_write_failed",
                "unable to persist planned Nucleus job",
            )?;
        let stored_job_id: String = transaction
            .query_row(
                "SELECT nucleus_job_id FROM run_jobs WHERE run_id=?1 AND thread_id=?2",
                params![run_id, thread_id],
                |row| row.get(0),
            )
            .context(
                "database_read_failed",
                "unable to verify planned Nucleus job",
            )?;
        if stored_job_id != nucleus_job_id {
            return Err(AppError::new(
                "job_identity_conflict",
                format!("thread {thread_id} already has a different job in run {run_id}"),
            ));
        }
        transaction
            .commit()
            .context("database_write_failed", "unable to commit Nucleus job plan")?;
        Ok(())
    }

    pub(crate) fn plan_observation_job(
        &mut self,
        observation_id: &str,
        scope_level: i64,
        attempt: usize,
        nucleus_job_id: &str,
    ) -> AppResult<()> {
        let attempt = i64::try_from(attempt).map_err(|_| {
            AppError::new(
                "observation_attempt_invalid",
                "observation attempt exceeds the supported SQLite range",
            )
        })?;
        self.connection
            .execute(
                "INSERT OR IGNORE INTO observation_jobs(
                    observation_id, scope_level, attempt, nucleus_job_id, status
                 ) VALUES(?1, ?2, ?3, ?4, 'planned')",
                params![observation_id, scope_level, attempt, nucleus_job_id],
            )
            .context(
                "database_write_failed",
                "unable to persist the observation job correlation",
            )?;
        let stored: String = self
            .connection
            .query_row(
                "SELECT nucleus_job_id FROM observation_jobs
                 WHERE observation_id=?1 AND scope_level=?2 AND attempt=?3",
                params![observation_id, scope_level, attempt],
                |row| row.get(0),
            )
            .context(
                "database_read_failed",
                "unable to verify the observation job correlation",
            )?;
        if stored != nucleus_job_id {
            return Err(AppError::new(
                "job_identity_conflict",
                "the observation scope already has a different Nucleus job",
            ));
        }
        Ok(())
    }

    pub(crate) fn begin_job_admission(&mut self, nucleus_job_id: &str) -> AppResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to lock Nucleus job admission",
            )?;
        let observation_job: Option<(String, String)> = transaction
            .query_row(
                "SELECT observations.status, observation_jobs.status
                 FROM observation_jobs
                 JOIN observations ON observations.id=observation_jobs.observation_id
                 WHERE observation_jobs.nucleus_job_id=?1",
                [nucleus_job_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to verify observation job admission",
            )?;
        if let Some((observation_status, job_status)) = observation_job {
            if observation_status != "processing" {
                return Err(AppError::new(
                    "observation_not_processing",
                    "the completed-turn observation no longer permits Nucleus admission",
                ));
            }
            if job_status == "failed" {
                return Err(AppError::new(
                    "job_state_conflict",
                    "a terminally failed Nucleus attempt cannot be admitted again",
                ));
            }
            transaction
                .execute(
                    "UPDATE observation_jobs SET
                        status=CASE WHEN status='planned' THEN 'submitted' ELSE status END,
                        admitted_at=COALESCE(admitted_at, ?2)
                     WHERE nucleus_job_id=?1",
                    params![nucleus_job_id, now_unix()],
                )
                .context(
                    "database_write_failed",
                    "unable to persist observation job admission intent",
                )?;
            return transaction.commit().context(
                "database_write_failed",
                "unable to commit observation job admission intent",
            );
        }
        let (run_status, job_status): (String, String) = transaction
            .query_row(
                "SELECT runs.status, run_jobs.status
                 FROM run_jobs JOIN runs ON runs.id=run_jobs.run_id
                 WHERE run_jobs.nucleus_job_id=?1",
                [nucleus_job_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context(
                "database_read_failed",
                "unable to verify Nucleus job admission",
            )?;
        if run_status != "building" {
            return Err(AppError::new(
                "run_not_building",
                "the decision run no longer permits Nucleus admission",
            ));
        }
        if job_status == "failed" {
            return Err(AppError::new(
                "job_state_conflict",
                "a terminally failed Nucleus attempt cannot be admitted again",
            ));
        }
        transaction
            .execute(
                "UPDATE run_jobs SET
                    status=CASE WHEN status='planned' THEN 'submitted' ELSE status END,
                    admitted_at=COALESCE(admitted_at, ?2)
                 WHERE nucleus_job_id=?1",
                params![nucleus_job_id, now_unix()],
            )
            .context(
                "database_write_failed",
                "unable to persist Nucleus admission intent",
            )?;
        transaction.commit().context(
            "database_write_failed",
            "unable to commit Nucleus admission intent",
        )
    }

    pub(crate) fn prepare_abandon(
        &mut self,
        report_date: &str,
    ) -> AppResult<(Run, Vec<RunJobCorrelation>)> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("database_write_failed", "unable to lock run abandonment")?;
        let mut run = transaction
            .query_row(
                "SELECT id, report_date, status, content_revision
                 FROM runs
                 WHERE report_date=?1 AND status IN ('building', 'abandoning')
                 ORDER BY started_at DESC LIMIT 1",
                [report_date],
                |row| {
                    Ok(Run {
                        id: row.get(0)?,
                        report_date: row.get(1)?,
                        status: row.get(2)?,
                        content_revision: row.get(3)?,
                    })
                },
            )
            .optional()
            .context("database_read_failed", "unable to inspect active daily run")?
            .ok_or_else(|| {
                AppError::new(
                    "building_run_not_found",
                    format!("no active build exists for {report_date}"),
                )
            })?;
        transaction
            .execute(
                "UPDATE runs SET status='abandoning'
                 WHERE id=?1 AND status='building'",
                [run.id.as_str()],
            )
            .context("database_write_failed", "unable to begin run abandonment")?;
        "abandoning".clone_into(&mut run.status);
        let jobs = {
            let mut statement = transaction
                .prepare(
                    "SELECT nucleus_job_id, admitted_at IS NOT NULL, status
                     FROM run_jobs WHERE run_id=?1 ORDER BY nucleus_job_id",
                )
                .context("database_read_failed", "unable to inspect run jobs")?;
            statement
                .query_map([run.id.as_str()], |row| {
                    Ok(RunJobCorrelation {
                        nucleus_job_id: row.get(0)?,
                        admitted: row.get::<_, i64>(1)? != 0,
                        status: row.get(2)?,
                    })
                })
                .context("database_read_failed", "unable to read run jobs")?
                .collect::<Result<Vec<_>, _>>()
                .context("database_read_failed", "unable to decode run jobs")?
        };
        transaction.commit().context(
            "database_write_failed",
            "unable to commit run abandonment intent",
        )?;
        Ok((run, jobs))
    }

    pub(crate) fn finish_abandon(
        &mut self,
        run_id: &str,
        expected_jobs: &[RunJobCorrelation],
    ) -> AppResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("database_write_failed", "unable to lock run abandonment")?;
        let status: String = transaction
            .query_row("SELECT status FROM runs WHERE id=?1", [run_id], |row| {
                row.get(0)
            })
            .context("database_read_failed", "unable to verify abandoned run")?;
        if status != "abandoning" {
            return Err(AppError::new(
                "run_not_abandoning",
                format!("run {run_id} is not awaiting abandonment"),
            ));
        }
        let stored_jobs = {
            let mut statement = transaction
                .prepare(
                    "SELECT nucleus_job_id, admitted_at IS NOT NULL, status
                     FROM run_jobs WHERE run_id=?1 ORDER BY nucleus_job_id",
                )
                .context("database_read_failed", "unable to verify abandoned jobs")?;
            statement
                .query_map([run_id], |row| {
                    Ok(RunJobCorrelation {
                        nucleus_job_id: row.get(0)?,
                        admitted: row.get::<_, i64>(1)? != 0,
                        status: row.get(2)?,
                    })
                })
                .context("database_read_failed", "unable to read abandoned jobs")?
                .collect::<Result<Vec<_>, _>>()
                .context("database_read_failed", "unable to decode abandoned jobs")?
        };
        if stored_jobs != expected_jobs {
            return Err(AppError::new(
                "abandonment_job_set_changed",
                "run jobs changed while abandonment was being reconciled",
            ));
        }
        transaction
            .execute(
                "UPDATE run_jobs SET status='failed',
                    failure_detail='explicit abandonment after terminal reconciliation'
                 WHERE run_id=?1 AND status != 'complete'",
                [run_id],
            )
            .context("database_write_failed", "unable to close abandoned jobs")?;
        let changed = transaction
            .execute(
                "UPDATE runs SET status='failed', failure_code='abandoned',
                    failure_detail='explicitly abandoned after Nucleus reconciliation',
                    completed_at=?2
                 WHERE id=?1 AND status='abandoning'",
                params![run_id, now_unix()],
            )
            .context("database_write_failed", "unable to finish run abandonment")?;
        if changed != 1 {
            return Err(AppError::new(
                "run_state_conflict",
                format!("run {run_id} changed while abandonment was committing"),
            ));
        }
        transaction
            .commit()
            .context("database_write_failed", "unable to commit run abandonment")
    }

    pub(crate) fn restore_build_after_unresolved_admission(
        &mut self,
        run_id: &str,
        expected_jobs: &[RunJobCorrelation],
    ) -> AppResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to lock unresolved abandonment recovery",
            )?;
        let status: String = transaction
            .query_row("SELECT status FROM runs WHERE id=?1", [run_id], |row| {
                row.get(0)
            })
            .context(
                "database_read_failed",
                "unable to verify unresolved abandonment",
            )?;
        if status != "abandoning" {
            return Err(AppError::new(
                "run_not_abandoning",
                format!("run {run_id} is not awaiting abandonment recovery"),
            ));
        }
        let stored_jobs = {
            let mut statement = transaction
                .prepare(
                    "SELECT nucleus_job_id, admitted_at IS NOT NULL, status
                     FROM run_jobs WHERE run_id=?1 ORDER BY nucleus_job_id",
                )
                .context(
                    "database_read_failed",
                    "unable to verify unresolved admission jobs",
                )?;
            statement
                .query_map([run_id], |row| {
                    Ok(RunJobCorrelation {
                        nucleus_job_id: row.get(0)?,
                        admitted: row.get::<_, i64>(1)? != 0,
                        status: row.get(2)?,
                    })
                })
                .context(
                    "database_read_failed",
                    "unable to read unresolved admission jobs",
                )?
                .collect::<Result<Vec<_>, _>>()
                .context(
                    "database_read_failed",
                    "unable to decode unresolved admission jobs",
                )?
        };
        if stored_jobs != expected_jobs {
            return Err(AppError::new(
                "abandonment_job_set_changed",
                "run jobs changed while unresolved admission was being checked",
            ));
        }
        let changed = transaction
            .execute(
                "UPDATE runs SET status='building'
                 WHERE id=?1 AND status='abandoning'",
                [run_id],
            )
            .context(
                "database_write_failed",
                "unable to restore resumable decision build",
            )?;
        if changed != 1 {
            return Err(AppError::new(
                "run_state_conflict",
                format!("run {run_id} changed during abandonment recovery"),
            ));
        }
        transaction.commit().context(
            "database_write_failed",
            "unable to commit resumable decision build recovery",
        )
    }

    pub(crate) fn persist_job_request_digest(
        &self,
        nucleus_job_id: &str,
        request_digest: &str,
    ) -> AppResult<()> {
        let table = if self.is_observation_job(nucleus_job_id)? {
            "observation_jobs"
        } else {
            "run_jobs"
        };
        self.connection
            .execute(
                &format!(
                    "UPDATE {table} SET request_digest=?2
                     WHERE nucleus_job_id=?1 AND request_digest IS NULL"
                ),
                params![nucleus_job_id, request_digest],
            )
            .context(
                "database_write_failed",
                "unable to persist Nucleus request digest",
            )?;
        let stored: Option<String> = self
            .connection
            .query_row(
                &format!("SELECT request_digest FROM {table} WHERE nucleus_job_id=?1"),
                [nucleus_job_id],
                |row| row.get(0),
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to verify Nucleus request digest",
            )?
            .flatten();
        match stored {
            Some(stored) if stored == request_digest => Ok(()),
            Some(_) => Err(AppError::new(
                "job_request_conflict",
                format!("resumed Nucleus request digest changed for {nucleus_job_id}"),
            )),
            None => Err(AppError::new(
                "job_not_found",
                format!("planned Nucleus job is unavailable: {nucleus_job_id}"),
            )),
        }
    }

    pub(crate) fn mark_job(
        &self,
        nucleus_job_id: &str,
        status: &str,
        failure_detail: Option<&str>,
    ) -> AppResult<bool> {
        let observation = self.is_observation_job(nucleus_job_id)?;
        let table = if observation {
            "observation_jobs"
        } else {
            "run_jobs"
        };
        let receipt_table = if observation {
            "observation_classification_receipts"
        } else {
            "classification_receipts"
        };
        let changed = match status {
            "submitted" => self.connection.execute(
                &format!(
                    "UPDATE {table} SET status='submitted', failure_detail=NULL,
                        admitted_at=COALESCE(admitted_at, ?2)
                     WHERE nucleus_job_id=?1 AND status IN ('planned', 'submitted')"
                ),
                params![nucleus_job_id, now_unix()],
            ),
            "complete" => self.connection.execute(
                &format!(
                    "UPDATE {table} SET status='complete', failure_detail=NULL
                     WHERE nucleus_job_id=?1 AND status NOT IN ('complete', 'failed')"
                ),
                [nucleus_job_id],
            ),
            "failed" => self.connection.execute(
                &format!(
                    "UPDATE {table} SET status='failed', failure_detail=?2
                     WHERE nucleus_job_id=?1 AND status NOT IN ('complete', 'failed')
                       AND NOT EXISTS (
                           SELECT 1 FROM {receipt_table}
                           WHERE nucleus_job_id=?1 AND is_error=0
                       )"
                ),
                params![nucleus_job_id, failure_detail],
            ),
            _ => {
                return Err(AppError::new(
                    "job_status_invalid",
                    format!("unsupported Nucleus job status: {status}"),
                ));
            }
        }
        .context(
            "database_write_failed",
            "unable to update Nucleus job correlation",
        )?;
        let exists: bool = self
            .connection
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE nucleus_job_id=?1)"),
                [nucleus_job_id],
                |row| row.get(0),
            )
            .context(
                "database_read_failed",
                "unable to verify Nucleus job correlation",
            )?;
        if !exists {
            return Err(AppError::new(
                "job_not_found",
                format!("planned Nucleus job is unavailable: {nucleus_job_id}"),
            ));
        }
        Ok(changed == 1)
    }

    pub(crate) fn job_status(&self, nucleus_job_id: &str) -> AppResult<String> {
        let table = if self.is_observation_job(nucleus_job_id)? {
            "observation_jobs"
        } else {
            "run_jobs"
        };
        self.connection
            .query_row(
                &format!("SELECT status FROM {table} WHERE nucleus_job_id=?1"),
                [nucleus_job_id],
                |row| row.get(0),
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to read Nucleus job correlation",
            )?
            .ok_or_else(|| {
                AppError::new(
                    "job_not_found",
                    format!("planned Nucleus job is unavailable: {nucleus_job_id}"),
                )
            })
    }

    pub(crate) fn classification_receipt(
        &self,
        nucleus_job_id: &str,
        call_id: &str,
    ) -> AppResult<Option<ClassificationReceipt>> {
        let table = if self.is_observation_job(nucleus_job_id)? {
            "observation_classification_receipts"
        } else {
            "classification_receipts"
        };
        self.connection
            .query_row(
                &format!(
                    "SELECT result_json, is_error, classification_json
                     FROM {table}
                     WHERE nucleus_job_id=?1 AND call_id=?2"
                ),
                params![nucleus_job_id, call_id],
                |row| {
                    let classification_json: Option<String> = row.get(2)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        classification_json,
                    ))
                },
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to read classification receipt",
            )?
            .map(|(result_json, is_error, classification_json)| {
                let classification = classification_json
                    .map(|value| decode_classification(&value))
                    .transpose()?;
                Ok(ClassificationReceipt {
                    result_json,
                    is_error: is_error != 0,
                    classification,
                })
            })
            .transpose()
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn persist_classification_receipt(
        &mut self,
        nucleus_job_id: &str,
        call_id: &str,
        result_json: &str,
        is_error: bool,
        classification: Option<&[Candidate]>,
    ) -> AppResult<ClassificationReceipt> {
        let observation = self.is_observation_job(nucleus_job_id)?;
        let job_table = if observation {
            "observation_jobs"
        } else {
            "run_jobs"
        };
        let receipt_table = if observation {
            "observation_classification_receipts"
        } else {
            "classification_receipts"
        };
        let classification_json = classification
            .map(|candidates| {
                candidates
                    .iter()
                    .map(PersistedCandidate::from_candidate)
                    .collect::<Vec<_>>()
            })
            .map(|candidates| serde_json::to_string(&candidates))
            .transpose()
            .context(
                "classification_receipt_invalid",
                "unable to encode validated classification",
            )?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to lock classification receipt",
            )?;
        if !is_error && classification_json.is_some() {
            let job_status: String = transaction
                .query_row(
                    &format!("SELECT status FROM {job_table} WHERE nucleus_job_id=?1"),
                    [nucleus_job_id],
                    |row| row.get(0),
                )
                .context(
                    "database_read_failed",
                    "unable to verify classification receipt winner",
                )?;
            if job_status == "failed" {
                return Err(AppError::new(
                    "classification_receipt_late",
                    "accepted classification arrived after terminal failure was committed",
                ));
            }
        }
        transaction
            .execute(
                &format!(
                    "INSERT OR IGNORE INTO {receipt_table}(
                        nucleus_job_id, call_id, result_json, is_error,
                        classification_json, created_at
                     ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)"
                ),
                params![
                    nucleus_job_id,
                    call_id,
                    result_json,
                    i64::from(is_error),
                    classification_json,
                    now_unix()
                ],
            )
            .context(
                "database_write_failed",
                "unable to persist classification receipt",
            )?;
        let stored = transaction
            .query_row(
                &format!(
                    "SELECT result_json, is_error, classification_json
                     FROM {receipt_table}
                     WHERE nucleus_job_id=?1 AND call_id=?2"
                ),
                params![nucleus_job_id, call_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .context(
                "database_read_failed",
                "unable to verify classification receipt",
            )?;
        if stored.0 != result_json
            || stored.1 != i64::from(is_error)
            || stored.2 != classification_json
        {
            return Err(AppError::new(
                "classification_receipt_conflict",
                format!("tool call {call_id} already has different durable result bytes"),
            ));
        }
        if stored.1 == 0 && stored.2.is_some() {
            let changed = transaction
                .execute(
                    &format!(
                        "UPDATE {job_table} SET status='complete', failure_detail=NULL
                         WHERE nucleus_job_id=?1 AND status NOT IN ('complete', 'failed')"
                    ),
                    [nucleus_job_id],
                )
                .context(
                    "database_write_failed",
                    "unable to preserve accepted classification state",
                )?;
            if changed > 1 {
                return Err(AppError::new(
                    "job_state_conflict",
                    "accepted classification matched multiple job correlations",
                ));
            }
        }
        transaction.commit().context(
            "database_write_failed",
            "unable to commit classification receipt",
        )?;
        let classification = stored
            .2
            .map(|value| decode_classification(&value))
            .transpose()?;
        Ok(ClassificationReceipt {
            result_json: stored.0,
            is_error: stored.1 != 0,
            classification,
        })
    }

    pub(crate) fn persisted_classification(
        &self,
        nucleus_job_id: &str,
    ) -> AppResult<Option<Vec<Candidate>>> {
        let table = if self.is_observation_job(nucleus_job_id)? {
            "observation_classification_receipts"
        } else {
            "classification_receipts"
        };
        let value: Option<String> = self
            .connection
            .query_row(
                &format!(
                    "SELECT classification_json FROM {table}
                     WHERE nucleus_job_id=?1 AND is_error=0 LIMIT 1"
                ),
                [nucleus_job_id],
                |row| row.get(0),
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to read persisted classification",
            )?;
        value.map(|value| decode_classification(&value)).transpose()
    }

    pub(crate) fn observation_classification_receipt(
        &self,
        nucleus_job_id: &str,
        call_id: &str,
    ) -> AppResult<Option<ObservationClassificationReceipt>> {
        self.connection
            .query_row(
                "SELECT result_json, is_error, classification_json
                 FROM observation_classification_receipts
                 WHERE nucleus_job_id=?1 AND call_id=?2",
                params![nucleus_job_id, call_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to read observation classification receipt",
            )?
            .map(|(result_json, is_error, classification_json)| {
                let classification = classification_json
                    .map(|value| decode_observation_classification(&value))
                    .transpose()?;
                Ok(ObservationClassificationReceipt {
                    result_json,
                    is_error: is_error != 0,
                    classification,
                })
            })
            .transpose()
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub(crate) fn persist_observation_classification_receipt(
        &mut self,
        nucleus_job_id: &str,
        call_id: &str,
        result_json: &str,
        is_error: bool,
        classification: Option<&ObservationClassification>,
    ) -> AppResult<ObservationClassificationReceipt> {
        let classification_json = classification
            .map(PersistedObservationClassification::from_classification)
            .map(|classification| serde_json::to_string(&classification))
            .transpose()
            .context(
                "classification_receipt_invalid",
                "unable to encode validated observation classification",
            )?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to lock observation classification receipt",
            )?;
        if !is_error && classification_json.is_some() {
            let job_status: String = transaction
                .query_row(
                    "SELECT status FROM observation_jobs WHERE nucleus_job_id=?1",
                    [nucleus_job_id],
                    |row| row.get(0),
                )
                .context(
                    "database_read_failed",
                    "unable to verify observation classification winner",
                )?;
            if job_status == "failed" {
                return Err(AppError::new(
                    "classification_receipt_late",
                    "accepted classification arrived after terminal failure was committed",
                ));
            }
        }
        transaction
            .execute(
                "INSERT OR IGNORE INTO observation_classification_receipts(
                    nucleus_job_id, call_id, result_json, is_error,
                    classification_json, created_at
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    nucleus_job_id,
                    call_id,
                    result_json,
                    i64::from(is_error),
                    classification_json,
                    now_unix()
                ],
            )
            .context(
                "database_write_failed",
                "unable to persist observation classification receipt",
            )?;
        let stored = transaction
            .query_row(
                "SELECT result_json, is_error, classification_json
                 FROM observation_classification_receipts
                 WHERE nucleus_job_id=?1 AND call_id=?2",
                params![nucleus_job_id, call_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .context(
                "database_read_failed",
                "unable to verify observation classification receipt",
            )?;
        if stored.0 != result_json
            || stored.1 != i64::from(is_error)
            || stored.2 != classification_json
        {
            return Err(AppError::new(
                "classification_receipt_conflict",
                format!("tool call {call_id} already has different durable result bytes"),
            ));
        }
        if stored.1 == 0 && stored.2.is_some() {
            transaction
                .execute(
                    "UPDATE observation_jobs SET status='complete', failure_detail=NULL
                     WHERE nucleus_job_id=?1 AND status NOT IN ('complete', 'failed')",
                    [nucleus_job_id],
                )
                .context(
                    "database_write_failed",
                    "unable to preserve accepted observation classification state",
                )?;
        }
        transaction.commit().context(
            "database_write_failed",
            "unable to commit observation classification receipt",
        )?;
        let classification = stored
            .2
            .map(|value| decode_observation_classification(&value))
            .transpose()?;
        Ok(ObservationClassificationReceipt {
            result_json: stored.0,
            is_error: stored.1 != 0,
            classification,
        })
    }

    pub(crate) fn persisted_observation_classification(
        &self,
        nucleus_job_id: &str,
    ) -> AppResult<Option<ObservationClassification>> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT classification_json
                 FROM observation_classification_receipts
                 WHERE nucleus_job_id=?1 AND is_error=0 LIMIT 1",
                [nucleus_job_id],
                |row| row.get(0),
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to read persisted observation classification",
            )?;
        value
            .map(|value| decode_observation_classification(&value))
            .transpose()
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn complete_observation(
        &mut self,
        observation_id: &str,
        classification: &ObservationClassification,
    ) -> AppResult<()> {
        if classification.needs_context {
            return Err(AppError::new(
                "classification_incomplete",
                "a context-expansion request cannot complete an observation",
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to lock observation completion",
            )?;
        let (status, host_id, thread_id, turn_id): (String, String, String, String) = transaction
            .query_row(
                "SELECT status, host_id, thread_id, turn_id
                 FROM observations WHERE id=?1",
                [observation_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .context(
                "database_read_failed",
                "unable to verify the completing observation",
            )?;
        if status != "processing" && status != "queued" {
            return Err(AppError::new(
                "observation_state_conflict",
                format!("observation {observation_id} is not awaiting completion"),
            ));
        }
        let expected = {
            let mut statement = transaction
                .prepare(
                    "SELECT item_id, host_id, thread_id, turn_id, occurred_at,
                            timestamp_precision
                     FROM observation_authority_items
                     WHERE observation_id=?1 ORDER BY item_id",
                )
                .context(
                    "database_read_failed",
                    "unable to prepare the admitted authority set",
                )?;
            statement
                .query_map([observation_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                })
                .context(
                    "database_read_failed",
                    "unable to read the admitted authority set",
                )?
                .collect::<Result<Vec<_>, _>>()
                .context(
                    "database_read_failed",
                    "unable to decode the admitted authority set",
                )?
        };
        if expected.is_empty() {
            return Err(AppError::new(
                "observation_authority_missing",
                "the observation has no durably admitted authority items",
            ));
        }
        let mut submitted = classification.authority_verdicts.clone();
        submitted.sort_by(|left, right| left.authority.item_id.cmp(&right.authority.item_id));
        if submitted.len() != expected.len() {
            return Err(AppError::new(
                "classification_coverage_invalid",
                "the classification does not cover every admitted authority item exactly once",
            ));
        }
        for (verdict, expected) in submitted.iter().zip(&expected) {
            let authority = &verdict.authority;
            if authority.item_id != expected.0
                || authority.host_id != expected.1
                || authority.thread_id != expected.2
                || authority.turn_id != expected.3
                || authority.occurred_at != expected.4
                || authority.precision.as_str() != expected.5
                || authority.role != crate::model::MessageRole::User
                || authority.host_id != host_id
                || authority.thread_id != thread_id
                || authority.turn_id != turn_id
            {
                return Err(AppError::new(
                    "classification_coverage_invalid",
                    "the classification authority set differs from the admitted user items",
                ));
            }
            let candidate_count = classification
                .candidates
                .iter()
                .filter(|candidate| candidate.authority.item_id == authority.item_id)
                .count();
            if (verdict.verdict == crate::model::AuthorityVerdict::Decision && candidate_count == 0)
                || (verdict.verdict == crate::model::AuthorityVerdict::NoDecision
                    && candidate_count != 0)
            {
                return Err(AppError::new(
                    "classification_coverage_invalid",
                    "an authority verdict disagrees with its decision candidates",
                ));
            }
        }
        if classification.candidates.iter().any(|candidate| {
            !submitted.iter().any(|verdict| {
                verdict.verdict == crate::model::AuthorityVerdict::Decision
                    && verdict.authority.item_id == candidate.authority.item_id
            })
        }) {
            return Err(AppError::new(
                "classification_coverage_invalid",
                "a decision candidate has no matching decision verdict",
            ));
        }
        if classification
            .candidates
            .iter()
            .any(|candidate| candidate.confidence == crate::model::Confidence::Low)
        {
            return Err(AppError::new(
                "classification_confidence_invalid",
                "observation decisions must use high or medium confidence",
            ));
        }
        for candidate in &classification.candidates {
            let inserted = persist_candidate_in(&transaction, candidate)?;
            if inserted {
                append_admission_event(&transaction, &candidate.id)?;
            }
            transaction
                .execute(
                    "INSERT OR IGNORE INTO observation_candidates(observation_id, candidate_id)
                     VALUES(?1, ?2)",
                    params![observation_id, candidate.id],
                )
                .context(
                    "database_write_failed",
                    "unable to attach candidate to observation",
                )?;
        }
        for verdict in &submitted {
            let authority = &verdict.authority;
            transaction
                .execute(
                    "INSERT INTO authority_verdicts(
                        observation_id, item_id, host_id, thread_id, turn_id,
                        occurred_at, timestamp_precision, verdict
                     ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        observation_id,
                        authority.item_id,
                        authority.host_id,
                        authority.thread_id,
                        authority.turn_id,
                        authority.occurred_at,
                        authority.precision.as_str(),
                        verdict.verdict.as_str()
                    ],
                )
                .context(
                    "database_write_failed",
                    "unable to persist an authority verdict",
                )?;
        }
        let outcome = if submitted
            .iter()
            .any(|verdict| verdict.verdict == crate::model::AuthorityVerdict::Decision)
        {
            "decision"
        } else {
            "no_decision"
        };
        let changed = transaction
            .execute(
                "UPDATE observations SET status='complete', outcome=?2,
                    failure_code=NULL, failure_detail=NULL, next_attempt_at=NULL,
                    completed_at=?3, updated_at=?3
                 WHERE id=?1 AND status IN ('queued', 'processing')",
                params![observation_id, outcome, now_unix()],
            )
            .context(
                "database_write_failed",
                "unable to complete the turn observation",
            )?;
        if changed != 1 {
            return Err(AppError::new(
                "observation_state_conflict",
                "the turn observation changed while completion was committing",
            ));
        }
        transaction.commit().context(
            "database_write_failed",
            "unable to commit the completed turn observation",
        )
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn project_observations(
        &mut self,
        report_date: &str,
        window_start: i64,
        window_end: i64,
        completed_cutoff: i64,
        admission_watermark: i64,
    ) -> AppResult<ObservationProjection> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to lock the daily observation projection",
            )?;
        let blockers: i64 = transaction
            .query_row(
                "SELECT COUNT(DISTINCT observations.id)
                 FROM observations
                 JOIN observation_authority_items items
                   ON items.observation_id=observations.id
                 WHERE items.occurred_at>=?1 AND items.occurred_at<?2
                   AND observations.source_completed_at<=?3
                   AND observations.rowid<=?4
                   AND observations.status!='complete'",
                params![
                    window_start,
                    window_end,
                    completed_cutoff,
                    admission_watermark
                ],
                |row| row.get(0),
            )
            .context(
                "database_read_failed",
                "unable to inspect incomplete daily observations",
            )?;
        let unscoped_unresolved: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM observations
                 WHERE status!='complete'
                   AND rowid<=?2
                   AND (
                       (source_completed_at IS NOT NULL AND source_completed_at<=?1)
                       OR (source_completed_at IS NULL
                           AND (source_not_completed_at IS NULL
                                OR source_not_completed_at<=?1))
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM observation_authority_items items
                       WHERE items.observation_id=observations.id
                   )",
                params![completed_cutoff, admission_watermark],
                |row| row.get(0),
            )
            .context(
                "database_read_failed",
                "unable to inspect unresolved observations without source provenance",
            )?;
        if blockers != 0 || unscoped_unresolved != 0 {
            return Err(AppError::new(
                "observation_coverage_incomplete",
                format!(
                    "daily projection is blocked by {blockers} incomplete scoped observations and {unscoped_unresolved} unresolved observations"
                ),
            ));
        }
        let missing_verdicts: i64 = transaction
            .query_row(
                "SELECT COUNT(*)
                 FROM observation_authority_items items
                 JOIN observations ON observations.id=items.observation_id
                 LEFT JOIN authority_verdicts verdicts
                   ON verdicts.observation_id=items.observation_id
                  AND verdicts.item_id=items.item_id
                 WHERE items.occurred_at>=?1 AND items.occurred_at<?2
                   AND observations.source_completed_at<=?3
                   AND observations.rowid<=?4
                   AND observations.status='complete'
                   AND observations.outcome IN ('decision', 'no_decision')
                   AND verdicts.item_id IS NULL",
                params![
                    window_start,
                    window_end,
                    completed_cutoff,
                    admission_watermark
                ],
                |row| row.get(0),
            )
            .context(
                "database_read_failed",
                "unable to verify daily authority coverage",
            )?;
        if missing_verdicts != 0 {
            return Err(AppError::new(
                "observation_coverage_incomplete",
                "daily projection is missing a durable authority verdict",
            ));
        }
        let manifest_rows = {
            let mut statement = transaction
                .prepare(
                    "SELECT verdicts.observation_id, verdicts.item_id,
                            verdicts.host_id, verdicts.thread_id, verdicts.turn_id,
                            verdicts.occurred_at, verdicts.timestamp_precision,
                            verdicts.verdict
                     FROM authority_verdicts verdicts
                     JOIN observations ON observations.id=verdicts.observation_id
                     WHERE verdicts.occurred_at>=?1 AND verdicts.occurred_at<?2
                       AND observations.source_completed_at<=?3
                       AND observations.rowid<=?4
                     ORDER BY verdicts.host_id, verdicts.thread_id,
                              verdicts.turn_id, verdicts.item_id",
                )
                .context(
                    "database_read_failed",
                    "unable to prepare daily authority manifest",
                )?;
            statement
                .query_map(
                    params![
                        window_start,
                        window_end,
                        completed_cutoff,
                        admission_watermark
                    ],
                    |row| {
                        Ok(format!(
                            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, i64>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                        ))
                    },
                )
                .context(
                    "database_read_failed",
                    "unable to read daily authority manifest",
                )?
                .collect::<Result<Vec<_>, _>>()
                .context(
                    "database_read_failed",
                    "unable to decode daily authority manifest",
                )?
        };
        let candidate_rows = {
            let mut statement = transaction
                .prepare(
                    "SELECT DISTINCT candidates.id, candidates.decided_at,
                            candidates.timestamp_precision, candidates.statement,
                            candidates.disposition, candidates.confidence,
                            COALESCE(candidates.rationale, ''),
                            COALESCE(candidates.supersedes_id, ''),
                            candidates.authority_start, candidates.authority_end
                     FROM candidates
                     JOIN observation_candidates observed
                       ON observed.candidate_id=candidates.id
                     JOIN observations ON observations.id=observed.observation_id
                     WHERE candidates.decided_at>=?1 AND candidates.decided_at<?2
                       AND observations.source_completed_at<=?3
                       AND observations.rowid<=?4
                     ORDER BY candidates.id",
                )
                .context(
                    "database_read_failed",
                    "unable to prepare daily candidate manifest",
                )?;
            statement
                .query_map(
                    params![
                        window_start,
                        window_end,
                        completed_cutoff,
                        admission_watermark
                    ],
                    |row| {
                        Ok(format!(
                            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, i64>(8)?,
                            row.get::<_, i64>(9)?,
                        ))
                    },
                )
                .context(
                    "database_read_failed",
                    "unable to read daily candidate manifest",
                )?
                .collect::<Result<Vec<_>, _>>()
                .context(
                    "database_read_failed",
                    "unable to decode daily candidate manifest",
                )?
        };
        let candidate_source_rows = {
            let mut statement = transaction
                .prepare(
                    "SELECT DISTINCT sources.candidate_id, sources.source_role,
                            sources.host_id, sources.thread_id, sources.turn_id,
                            sources.item_id, sources.message_role, sources.occurred_at,
                            sources.timestamp_precision
                     FROM candidate_sources sources
                     JOIN candidates ON candidates.id=sources.candidate_id
                     JOIN observation_candidates observed
                       ON observed.candidate_id=candidates.id
                     JOIN observations ON observations.id=observed.observation_id
                     WHERE candidates.decided_at>=?1 AND candidates.decided_at<?2
                       AND observations.source_completed_at<=?3
                       AND observations.rowid<=?4
                     ORDER BY sources.candidate_id, sources.source_role, sources.item_id",
                )
                .context(
                    "database_read_failed",
                    "unable to prepare daily candidate-source manifest",
                )?;
            statement
                .query_map(
                    params![
                        window_start,
                        window_end,
                        completed_cutoff,
                        admission_watermark
                    ],
                    |row| {
                        Ok(format!(
                            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, i64>(7)?,
                            row.get::<_, String>(8)?,
                        ))
                    },
                )
                .context(
                    "database_read_failed",
                    "unable to read daily candidate-source manifest",
                )?
                .collect::<Result<Vec<_>, _>>()
                .context(
                    "database_read_failed",
                    "unable to decode daily candidate-source manifest",
                )?
        };
        let source_manifest_hash = hex_digest(&format!(
            "verdicts\n{}\ncandidates\n{}\ncandidate-sources\n{}",
            manifest_rows.join("\n--\n"),
            candidate_rows.join("\n--\n"),
            candidate_source_rows.join("\n--\n")
        ));
        let identity = format!(
            "{report_date}\n{window_start}\n{window_end}\n{completed_cutoff}\n{admission_watermark}\n{source_manifest_hash}"
        );
        let identity_hash = hex_digest(&identity);
        let run_id = format!("run_obs_{}", &identity_hash[..20]);
        let now = now_unix();
        transaction
            .execute(
                "INSERT OR IGNORE INTO runs(
                    id, run_kind, report_date, window_start, window_end,
                    source_manifest_hash, coverage_cutoff_at,
                    observation_admission_watermark, status,
                    started_at, completed_at
                 ) VALUES(?1, 'observation_projection', ?2, ?3, ?4, ?5, ?6, ?7,
                          'complete', ?8, ?8)",
                params![
                    run_id,
                    report_date,
                    window_start,
                    window_end,
                    source_manifest_hash,
                    completed_cutoff,
                    admission_watermark,
                    now
                ],
            )
            .context(
                "database_write_failed",
                "unable to persist daily observation projection",
            )?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO run_candidates(run_id, candidate_id)
                 SELECT ?1, candidates.id
                 FROM candidates
                 JOIN observation_candidates observed
                   ON observed.candidate_id=candidates.id
                 JOIN observations ON observations.id=observed.observation_id
                 WHERE candidates.decided_at>=?2 AND candidates.decided_at<?3
                   AND observations.source_completed_at<=?4
                   AND observations.rowid<=?5",
                params![
                    run_id,
                    window_start,
                    window_end,
                    completed_cutoff,
                    admission_watermark
                ],
            )
            .context(
                "database_write_failed",
                "unable to attach observed decisions to daily projection",
            )?;
        let observations_covered: i64 = transaction
            .query_row(
                "SELECT COUNT(DISTINCT verdicts.observation_id)
                 FROM authority_verdicts verdicts
                 JOIN observations ON observations.id=verdicts.observation_id
                 WHERE verdicts.occurred_at>=?1 AND verdicts.occurred_at<?2
                   AND observations.source_completed_at<=?3
                   AND observations.rowid<=?4",
                params![
                    window_start,
                    window_end,
                    completed_cutoff,
                    admission_watermark
                ],
                |row| row.get(0),
            )
            .context(
                "database_read_failed",
                "unable to count projected observations",
            )?;
        let run = transaction
            .query_row(
                "SELECT id, report_date, status, content_revision
                 FROM runs WHERE id=?1 AND run_kind='observation_projection'",
                [run_id],
                |row| {
                    Ok(Run {
                        id: row.get(0)?,
                        report_date: row.get(1)?,
                        status: row.get(2)?,
                        content_revision: row.get(3)?,
                    })
                },
            )
            .context(
                "database_read_failed",
                "unable to verify daily observation projection",
            )?;
        transaction.commit().context(
            "database_write_failed",
            "unable to commit daily observation projection",
        )?;
        Ok(ObservationProjection {
            run,
            source_manifest_hash,
            observations_covered: usize::try_from(observations_covered).unwrap_or(usize::MAX),
        })
    }

    fn is_observation_job(&self, nucleus_job_id: &str) -> AppResult<bool> {
        self.connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM observation_jobs WHERE nucleus_job_id=?1
                 )",
                [nucleus_job_id],
                |row| row.get(0),
            )
            .context(
                "database_read_failed",
                "unable to resolve the Nucleus job owner",
            )
    }

    #[cfg(test)]
    pub(crate) fn complete_run(&mut self, run_id: &str, candidates: &[Candidate]) -> AppResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("database_write_failed", "unable to lock decision run")?;
        for candidate in candidates {
            if candidate.confidence == crate::model::Confidence::Low {
                continue;
            }
            transaction
                .execute(
                    "INSERT INTO candidates(
                        id, decided_at, timestamp_precision, statement, disposition,
                        confidence, rationale, supersedes_id, authority_start,
                        authority_end, created_at
                     ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                     ON CONFLICT(id) DO UPDATE SET
                        decided_at=excluded.decided_at,
                        timestamp_precision=excluded.timestamp_precision,
                        statement=excluded.statement,
                        disposition=excluded.disposition,
                        confidence=excluded.confidence,
                        rationale=excluded.rationale,
                        supersedes_id=excluded.supersedes_id,
                        authority_start=excluded.authority_start,
                        authority_end=excluded.authority_end",
                    params![
                        candidate.id,
                        candidate.decided_at,
                        candidate.precision.as_str(),
                        candidate.statement,
                        candidate.disposition.as_str(),
                        candidate.confidence.as_str(),
                        candidate.rationale,
                        candidate.supersedes_id,
                        i64::try_from(candidate.authority_start).map_err(|_| AppError::new(
                            "source_span_invalid",
                            "authority span start exceeds SQLite range"
                        ))?,
                        i64::try_from(candidate.authority_end).map_err(|_| AppError::new(
                            "source_span_invalid",
                            "authority span end exceeds SQLite range"
                        ))?,
                        now_unix()
                    ],
                )
                .context(
                    "database_write_failed",
                    "unable to persist decision candidate",
                )?;
            transaction
                .execute(
                    "INSERT OR IGNORE INTO run_candidates(run_id, candidate_id) VALUES(?1, ?2)",
                    params![run_id, candidate.id],
                )
                .context("database_write_failed", "unable to attach candidate to run")?;
            transaction
                .execute(
                    "DELETE FROM candidate_sources WHERE candidate_id=?1",
                    [candidate.id.as_str()],
                )
                .context(
                    "database_write_failed",
                    "unable to refresh candidate sources",
                )?;
            insert_source(
                &transaction,
                &candidate.id,
                "authority",
                &candidate.authority,
            )?;
            for source in &candidate.context {
                insert_source(&transaction, &candidate.id, "context", source)?;
            }
        }
        let changed = transaction
            .execute(
                "UPDATE runs
                 SET status='complete', completed_at=?2
                 WHERE id=?1 AND status='building'",
                params![run_id, now_unix()],
            )
            .context("database_write_failed", "unable to complete decision run")?;
        if changed != 1 {
            return Err(AppError::new(
                "run_state_conflict",
                format!("run {run_id} is no longer building"),
            ));
        }
        transaction
            .commit()
            .context("database_write_failed", "unable to commit decision run")?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn fail_run(&self, run_id: &str, code: &str, detail: &str) -> AppResult<()> {
        self.connection
            .execute(
                "UPDATE runs SET status='failed', failure_code=?2,
                    failure_detail=?3, completed_at=?4
                 WHERE id=?1 AND status='building'",
                params![run_id, code, detail, now_unix()],
            )
            .context(
                "database_write_failed",
                "unable to record failed decision run",
            )?;
        Ok(())
    }

    pub(crate) fn latest_complete_run(&self, report_date: &str) -> AppResult<Run> {
        self.connection
            .query_row(
                "SELECT id, report_date, status, content_revision
                 FROM runs WHERE report_date=?1 AND status='complete'
                   AND run_kind='observation_projection'
                 ORDER BY completed_at DESC LIMIT 1",
                [report_date],
                |row| {
                    Ok(Run {
                        id: row.get(0)?,
                        report_date: row.get(1)?,
                        status: row.get(2)?,
                        content_revision: row.get(3)?,
                    })
                },
            )
            .optional()
            .context("database_read_failed", "unable to read latest decision run")?
            .ok_or_else(|| {
                AppError::new(
                    "digest_not_built",
                    format!("no complete decision run exists for {report_date}; run `decisions daily build --date {report_date}`"),
                )
            })
    }

    pub(crate) fn candidates_for_run(&self, run_id: &str) -> AppResult<Vec<StoredCandidate>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT c.id, rc.run_id, c.decided_at, c.timestamp_precision,
                        c.statement, c.disposition, c.confidence, c.rationale,
                        c.supersedes_id, c.authority_start, c.authority_end,
                        c.review_state
                 FROM candidates c
                 JOIN run_candidates rc ON rc.candidate_id=c.id
                 WHERE rc.run_id=?1
                 ORDER BY c.decided_at, c.id",
            )
            .context("database_read_failed", "unable to prepare candidates")?;
        let rows = statement
            .query_map([run_id], |row| {
                Ok(StoredCandidate {
                    id: row.get(0)?,
                    run_id: row.get(1)?,
                    decided_at: row.get(2)?,
                    timestamp_precision: row.get(3)?,
                    statement: row.get(4)?,
                    disposition: row.get(5)?,
                    confidence: row.get(6)?,
                    rationale: row.get(7)?,
                    supersedes_id: row.get(8)?,
                    authority_start: row.get(9)?,
                    authority_end: row.get(10)?,
                    review_state: row.get(11)?,
                    sources: Vec::new(),
                })
            })
            .context("database_read_failed", "unable to read candidates")?;
        let mut candidates = rows
            .collect::<Result<Vec<_>, _>>()
            .context("database_read_failed", "unable to decode candidates")?;
        for candidate in &mut candidates {
            candidate.sources = self.sources_for(&candidate.id)?;
        }
        Ok(candidates)
    }

    pub(crate) fn run_coverage_cutoff(&self, run_id: &str) -> AppResult<Option<i64>> {
        self.connection
            .query_row(
                "SELECT coverage_cutoff_at FROM runs WHERE id=?1",
                [run_id],
                |row| row.get(0),
            )
            .context(
                "database_read_failed",
                "unable to read the projection completion cutoff",
            )
    }

    pub(crate) fn candidate(&self, id: &str) -> AppResult<StoredCandidate> {
        let mut candidate = self
            .connection
            .query_row(
                "SELECT c.id, COALESCE((
                            SELECT rc.run_id FROM run_candidates rc
                            JOIN runs r ON r.id=rc.run_id
                            WHERE rc.candidate_id=c.id ORDER BY r.started_at DESC LIMIT 1
                        ), ''),
                        c.decided_at, c.timestamp_precision, c.statement,
                        c.disposition, c.confidence, c.rationale, c.supersedes_id,
                        c.authority_start, c.authority_end, c.review_state
                 FROM candidates c WHERE c.id=?1",
                [id],
                |row| {
                    Ok(StoredCandidate {
                        id: row.get(0)?,
                        run_id: row.get(1)?,
                        decided_at: row.get(2)?,
                        timestamp_precision: row.get(3)?,
                        statement: row.get(4)?,
                        disposition: row.get(5)?,
                        confidence: row.get(6)?,
                        rationale: row.get(7)?,
                        supersedes_id: row.get(8)?,
                        authority_start: row.get(9)?,
                        authority_end: row.get(10)?,
                        review_state: row.get(11)?,
                        sources: Vec::new(),
                    })
                },
            )
            .optional()
            .context("database_read_failed", "unable to read candidate")?
            .ok_or_else(|| AppError::new("decision_not_found", format!("unknown decision {id}")))?;
        candidate.sources = self.sources_for(id)?;
        Ok(candidate)
    }

    fn sources_for(&self, candidate_id: &str) -> AppResult<Vec<StoredSource>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT source_role, host_id, thread_id, turn_id, item_id,
                        message_role, occurred_at, timestamp_precision
                 FROM candidate_sources WHERE candidate_id=?1
                 ORDER BY CASE source_role WHEN 'authority' THEN 0 ELSE 1 END, item_id",
            )
            .context("database_read_failed", "unable to prepare decision sources")?;
        statement
            .query_map([candidate_id], |row| {
                Ok(StoredSource {
                    source_role: row.get(0)?,
                    host_id: row.get(1)?,
                    thread_id: row.get(2)?,
                    turn_id: row.get(3)?,
                    item_id: row.get(4)?,
                    message_role: row.get(5)?,
                    occurred_at: row.get(6)?,
                    timestamp_precision: row.get(7)?,
                })
            })
            .context("database_read_failed", "unable to read decision sources")?
            .collect::<Result<Vec<_>, _>>()
            .context("database_read_failed", "unable to decode decision sources")
    }

    pub(crate) fn review(&mut self, id: &str, action: &str) -> AppResult<StoredCandidate> {
        let state = match action {
            "confirm" => "confirmed",
            "dismiss" => "dismissed",
            _ => {
                return Err(AppError::new(
                    "invalid_review",
                    "review must be confirm or dismiss",
                ));
            }
        };
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("database_write_failed", "unable to lock decision review")?;
        let changed = transaction
            .execute(
                "UPDATE candidates SET review_state=?2 WHERE id=?1",
                params![id, state],
            )
            .context("database_write_failed", "unable to update decision review")?;
        if changed == 0 {
            return Err(AppError::new(
                "decision_not_found",
                format!("unknown decision {id}"),
            ));
        }
        let reviewed_at = now_unix();
        transaction
            .execute(
                "INSERT INTO reviews(candidate_id, action, reviewed_at, review_source)
                 VALUES(?1, ?2, ?3, 'cli')",
                params![id, action, reviewed_at],
            )
            .context("database_write_failed", "unable to append decision review")?;
        let review_id = transaction.last_insert_rowid();
        append_review_event(&transaction, id, review_id, action, reviewed_at, "cli")?;
        transaction
            .execute(
                "UPDATE runs SET content_revision=content_revision+1
                 WHERE id IN (SELECT run_id FROM run_candidates WHERE candidate_id=?1)",
                [id],
            )
            .context(
                "database_write_failed",
                "unable to invalidate digest snapshots",
            )?;
        transaction
            .commit()
            .context("database_write_failed", "unable to commit decision review")?;
        self.candidate(id)
    }

    pub(crate) fn snapshot(
        &self,
        run: &Run,
        subject: &str,
        body: &str,
    ) -> AppResult<DigestSnapshot> {
        let digest_hash = hex_digest(&format!("{subject}\n{body}"));
        self.connection
            .execute(
                "INSERT INTO digest_snapshots(
                    run_id, content_revision, subject, body, digest_hash, frozen_at
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(run_id, content_revision) DO NOTHING",
                params![
                    run.id,
                    run.content_revision,
                    subject,
                    body,
                    digest_hash,
                    now_unix()
                ],
            )
            .context("database_write_failed", "unable to freeze digest")?;
        self.connection
            .query_row(
                "SELECT subject, body, digest_hash FROM digest_snapshots
                 WHERE run_id=?1 AND content_revision=?2",
                params![run.id, run.content_revision],
                |row| {
                    Ok(DigestSnapshot {
                        run_id: run.id.clone(),
                        report_date: run.report_date.clone(),
                        content_revision: run.content_revision,
                        subject: row.get(0)?,
                        body: row.get(1)?,
                        digest_hash: row.get(2)?,
                    })
                },
            )
            .context("database_read_failed", "unable to read frozen digest")
    }

    pub(crate) fn begin_delivery(
        &mut self,
        run: &Run,
        scheduled_occurrence: Option<&str>,
    ) -> AppResult<Delivery> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to lock email delivery admission",
            )?;
        if let Some(occurrence) = scheduled_occurrence {
            if let Some(existing) = delivery_for_occurrence_in(&transaction, occurrence)? {
                transaction.commit().context(
                    "database_write_failed",
                    "unable to commit scheduled delivery recovery",
                )?;
                return Ok(existing);
            }
        } else if let Some(existing) = manual_delivery_for_recovery_in(&transaction, run)? {
            transaction.commit().context(
                "database_write_failed",
                "unable to commit manual delivery recovery",
            )?;
            return Ok(existing);
        }
        let id = format!("delivery_{}", uuid::Uuid::now_v7());
        let (kind, key) = match scheduled_occurrence {
            Some(occurrence) => ("scheduled", format!("codex-decisions-daily/{occurrence}")),
            None => (
                "manual",
                format!("codex-decisions-manual/{}", uuid::Uuid::now_v7()),
            ),
        };
        transaction
            .execute(
                "INSERT INTO deliveries(
                    id, run_id, content_revision, kind, occurrence_date,
                    idempotency_key, status, created_at
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)",
                params![
                    id,
                    run.id,
                    run.content_revision,
                    kind,
                    scheduled_occurrence,
                    key,
                    now_unix()
                ],
            )
            .context("database_write_failed", "unable to persist email delivery")?;
        transaction
            .commit()
            .context("database_write_failed", "unable to commit email delivery")?;
        Ok(Delivery {
            id,
            run_id: run.id.clone(),
            kind: kind.to_owned(),
            occurrence_date: scheduled_occurrence.map(str::to_owned),
            idempotency_key: key,
            status: "pending".to_owned(),
            email_id: None,
        })
    }

    pub(crate) fn snapshot_for_delivery(&self, delivery: &Delivery) -> AppResult<DigestSnapshot> {
        self.connection
            .query_row(
                "SELECT r.report_date, d.content_revision, s.subject, s.body,
                        s.digest_hash
                 FROM deliveries d
                 JOIN runs r ON r.id=d.run_id
                 JOIN digest_snapshots s
                   ON s.run_id=d.run_id AND s.content_revision=d.content_revision
                 WHERE d.id=?1",
                [delivery.id.as_str()],
                |row| {
                    Ok(DigestSnapshot {
                        run_id: delivery.run_id.clone(),
                        report_date: row.get(0)?,
                        content_revision: row.get(1)?,
                        subject: row.get(2)?,
                        body: row.get(3)?,
                        digest_hash: row.get(4)?,
                    })
                },
            )
            .context("database_read_failed", "unable to read delivery snapshot")
    }

    pub(crate) fn delivery_for_occurrence(&self, occurrence: &str) -> AppResult<Option<Delivery>> {
        self.connection
            .query_row(
                "SELECT id, run_id, kind, occurrence_date, idempotency_key,
                        status, email_id
                 FROM deliveries WHERE kind='scheduled' AND occurrence_date=?1",
                [occurrence],
                |row| {
                    Ok(Delivery {
                        id: row.get(0)?,
                        run_id: row.get(1)?,
                        kind: row.get(2)?,
                        occurrence_date: row.get(3)?,
                        idempotency_key: row.get(4)?,
                        status: row.get(5)?,
                        email_id: row.get(6)?,
                    })
                },
            )
            .optional()
            .context(
                "database_read_failed",
                "unable to inspect scheduled delivery",
            )
    }

    pub(crate) fn finish_delivery(
        &self,
        delivery_id: &str,
        result: Result<&str, &str>,
    ) -> AppResult<()> {
        let (status, email_id, failure) = match result {
            Ok(email_id) => ("accepted", Some(email_id), None),
            Err(failure) => ("failed", None, Some(failure)),
        };
        let changed = self
            .connection
            .execute(
                "UPDATE deliveries SET status=?2, email_id=?3, failure_detail=?4,
                    completed_at=?5 WHERE id=?1 AND status != 'accepted'",
                params![delivery_id, status, email_id, failure, now_unix()],
            )
            .context("database_write_failed", "unable to record email result")?;
        if changed == 1 {
            return Ok(());
        }
        let stored: Option<String> = self
            .connection
            .query_row(
                "SELECT status FROM deliveries WHERE id=?1",
                [delivery_id],
                |row| row.get(0),
            )
            .optional()
            .context("database_read_failed", "unable to verify email result")?;
        match stored.as_deref() {
            Some("accepted") => Ok(()),
            Some(_) => Err(AppError::new(
                "delivery_state_conflict",
                format!("delivery {delivery_id} changed while recording its result"),
            )),
            None => Err(AppError::new(
                "delivery_not_found",
                format!("unknown delivery {delivery_id}"),
            )),
        }
    }
}

fn delivery_for_occurrence_in(
    transaction: &rusqlite::Transaction<'_>,
    occurrence: &str,
) -> AppResult<Option<Delivery>> {
    transaction
        .query_row(
            "SELECT id, run_id, kind, occurrence_date, idempotency_key,
                    status, email_id
             FROM deliveries WHERE kind='scheduled' AND occurrence_date=?1",
            [occurrence],
            decode_delivery,
        )
        .optional()
        .context(
            "database_read_failed",
            "unable to inspect scheduled delivery",
        )
}

fn manual_delivery_for_recovery_in(
    transaction: &rusqlite::Transaction<'_>,
    run: &Run,
) -> AppResult<Option<Delivery>> {
    transaction
        .query_row(
            "SELECT id, run_id, kind, occurrence_date, idempotency_key,
                    status, email_id
             FROM deliveries
             WHERE kind='manual' AND run_id=?1 AND content_revision=?2
               AND status IN ('pending', 'failed')
             ORDER BY created_at DESC LIMIT 1",
            params![run.id, run.content_revision],
            decode_delivery,
        )
        .optional()
        .context(
            "database_read_failed",
            "unable to inspect recoverable manual delivery",
        )
}

fn decode_delivery(row: &rusqlite::Row<'_>) -> rusqlite::Result<Delivery> {
    Ok(Delivery {
        id: row.get(0)?,
        run_id: row.get(1)?,
        kind: row.get(2)?,
        occurrence_date: row.get(3)?,
        idempotency_key: row.get(4)?,
        status: row.get(5)?,
        email_id: row.get(6)?,
    })
}

fn decode_observation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Observation> {
    Ok(Observation {
        id: row.get(0)?,
        session_id: row.get(1)?,
        turn_id: row.get(2)?,
        host_id: row.get(3)?,
        thread_id: row.get(4)?,
        status: row.get(5)?,
        scope_level: row.get(6)?,
        attempt_epoch: row.get(7)?,
        outcome: row.get(8)?,
        file_change_count: row.get(9)?,
        authority_occurred_at: row.get(10)?,
        failure_code: row.get(11)?,
    })
}

fn database_sidecars(path: &Path) -> [PathBuf; 3] {
    ["-wal", "-shm", "-journal"].map(|suffix| {
        let mut value = OsString::from(path.as_os_str());
        value.push(suffix);
        PathBuf::from(value)
    })
}

fn observation_source_was_abandoned(
    transaction: &rusqlite::Transaction<'_>,
    id: &str,
) -> AppResult<bool> {
    transaction
        .query_row(
            "SELECT status='complete' AND outcome='not_eligible'
                    AND failure_code='conversation_source_abandoned'
             FROM observations WHERE id=?1",
            [id],
            |row| row.get(0),
        )
        .context(
            "database_read_failed",
            "unable to verify prior observation source recovery",
        )
}

fn run_operation_lock_path(path: &Path) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(".run.lock");
    PathBuf::from(value)
}

fn observation_operation_lock_path(path: &Path) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(".observe.lock");
    PathBuf::from(value)
}

fn inspect_private_database_file(path: &Path, required: bool) -> AppResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(AppError::new(
                    "database_path_unsafe",
                    format!("database state must be a regular file: {}", path.display()),
                ));
            }
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).context(
                "database_open_failed",
                format!("unable to make {} private", path.display()),
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(AppError::new(
            "database_open_failed",
            format!("database state is missing: {}", path.display()),
        )),
        Err(error) => Err(AppError::new(
            "database_open_failed",
            format!("unable to inspect {}: {error}", path.display()),
        )),
    }
}

fn decode_classification(value: &str) -> AppResult<Vec<Candidate>> {
    serde_json::from_str::<Vec<PersistedCandidate>>(value)
        .context(
            "classification_receipt_invalid",
            "unable to decode persisted classification",
        )
        .map(|candidates| {
            candidates
                .into_iter()
                .map(PersistedCandidate::into_candidate)
                .collect()
        })
}

fn decode_observation_classification(value: &str) -> AppResult<ObservationClassification> {
    serde_json::from_str::<PersistedObservationClassification>(value)
        .context(
            "classification_receipt_invalid",
            "unable to decode persisted observation classification",
        )
        .map(PersistedObservationClassification::into_classification)
}

fn migrate_v2_to_v3(connection: &mut Connection) -> AppResult<()> {
    migrate_v2_to_v3_with_schema(connection, MIGRATION_3)
}

fn migrate_v2_to_v3_with_schema(connection: &mut Connection, schema: &str) -> AppResult<()> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .context(
            "database_schema_failed",
            "unable to lock Decisions schema migration from version 2 to version 3",
        )?;
    transaction.execute_batch(schema).context(
        "database_schema_failed",
        "unable to create the Decisions event stream",
    )?;
    let decision_ids = {
        let mut statement = transaction
            .prepare("SELECT id FROM candidates ORDER BY created_at, id")
            .context(
                "database_schema_failed",
                "unable to prepare historical decision event migration",
            )?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .context(
                "database_schema_failed",
                "unable to read historical decisions for event migration",
            )?
            .collect::<Result<Vec<_>, _>>()
            .context(
                "database_schema_failed",
                "unable to decode historical decisions for event migration",
            )?
    };
    for decision_id in decision_ids {
        append_admission_event(&transaction, &decision_id)?;
    }
    let reviews = {
        let mut statement = transaction
            .prepare(
                "SELECT candidate_id, id, action, reviewed_at, review_source
                 FROM reviews ORDER BY reviewed_at, id",
            )
            .context(
                "database_schema_failed",
                "unable to prepare historical review event migration",
            )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .context(
                "database_schema_failed",
                "unable to read historical reviews for event migration",
            )?
            .collect::<Result<Vec<_>, _>>()
            .context(
                "database_schema_failed",
                "unable to decode historical reviews for event migration",
            )?
    };
    for (decision_id, review_id, action, reviewed_at, review_source) in reviews {
        append_review_event(
            &transaction,
            &decision_id,
            review_id,
            &action,
            reviewed_at,
            &review_source,
        )?;
    }
    transaction
        .execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES(3, ?1)",
            [now_unix()],
        )
        .context(
            "database_schema_failed",
            "unable to record Decisions schema migration version 3",
        )?;
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .context(
            "database_schema_failed",
            "unable to record Decisions schema version 3",
        )?;
    transaction.commit().context(
        "database_schema_failed",
        "unable to commit Decisions schema migration from version 2 to version 3",
    )
}

fn append_admission_event(
    transaction: &rusqlite::Transaction<'_>,
    decision_id: &str,
) -> AppResult<()> {
    let occurred_at: i64 = transaction
        .query_row(
            "SELECT created_at FROM candidates WHERE id=?1",
            [decision_id],
            |row| row.get(0),
        )
        .context(
            "database_read_failed",
            "unable to read admitted decision event time",
        )?;
    let envelope = DecisionEventEnvelope {
        event_id: format!("de_admitted_{decision_id}_v1"),
        event_version: EVENT_ENVELOPE_VERSION,
        event_kind: "decision_admitted".to_owned(),
        occurred_at,
        decision: event_decision(transaction, decision_id, "unreviewed")?,
        review: None,
    };
    insert_decision_event(transaction, decision_id, None, &envelope)
}

fn append_review_event(
    transaction: &rusqlite::Transaction<'_>,
    decision_id: &str,
    review_id: i64,
    action: &str,
    reviewed_at: i64,
    review_source: &str,
) -> AppResult<()> {
    let review_state = match action {
        "confirm" => "confirmed",
        "dismiss" => "dismissed",
        _ => {
            return Err(AppError::new(
                "invalid_review",
                "review must be confirm or dismiss",
            ));
        }
    };
    let envelope = DecisionEventEnvelope {
        event_id: format!("de_review_{review_id}_v1"),
        event_version: EVENT_ENVELOPE_VERSION,
        event_kind: "decision_reviewed".to_owned(),
        occurred_at: reviewed_at,
        decision: event_decision(transaction, decision_id, review_state)?,
        review: Some(DecisionEventReview {
            review_id: format!("r_{review_id}"),
            action: action.to_owned(),
            reviewed_at,
            review_source: review_source.to_owned(),
        }),
    };
    insert_decision_event(transaction, decision_id, Some(review_id), &envelope)
}

fn insert_decision_event(
    transaction: &rusqlite::Transaction<'_>,
    decision_id: &str,
    review_id: Option<i64>,
    envelope: &DecisionEventEnvelope,
) -> AppResult<()> {
    let payload = serde_json::to_string(envelope).map_err(|_error| {
        AppError::new(
            "decision_event_invalid",
            "unable to encode a decision event envelope",
        )
    })?;
    transaction
        .execute(
            "INSERT INTO decision_events(
                event_id, envelope_version, event_kind, decision_id,
                review_id, occurred_at, envelope_json
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                envelope.event_id,
                envelope.event_version,
                envelope.event_kind,
                decision_id,
                review_id,
                envelope.occurred_at,
                payload
            ],
        )
        .context(
            "database_write_failed",
            "unable to append the decision lifecycle event",
        )?;
    Ok(())
}

fn event_decision(
    transaction: &rusqlite::Transaction<'_>,
    decision_id: &str,
    review_state: &str,
) -> AppResult<DecisionEventDecision> {
    let mut decision = transaction
        .query_row(
            "SELECT id, decided_at, timestamp_precision, statement, disposition,
                    confidence, rationale, supersedes_id, authority_start, authority_end
             FROM candidates WHERE id=?1",
            [decision_id],
            |row| {
                Ok(DecisionEventDecision {
                    decision_id: row.get(0)?,
                    decided_at: row.get(1)?,
                    timestamp_precision: row.get(2)?,
                    statement: row.get(3)?,
                    disposition: row.get(4)?,
                    confidence: row.get(5)?,
                    rationale: row.get(6)?,
                    supersedes_decision_id: row.get(7)?,
                    review_state: review_state.to_owned(),
                    authority_span: DecisionEventAuthoritySpan {
                        start: row.get(8)?,
                        end: row.get(9)?,
                    },
                    sources: Vec::new(),
                })
            },
        )
        .context(
            "database_read_failed",
            "unable to read decision lifecycle data",
        )?;
    decision.sources = {
        let mut statement = transaction
            .prepare(
                "SELECT source_role, host_id, thread_id, turn_id, item_id,
                        message_role, occurred_at, timestamp_precision
                 FROM candidate_sources WHERE candidate_id=?1
                 ORDER BY CASE source_role WHEN 'authority' THEN 0 ELSE 1 END,
                          occurred_at, item_id",
            )
            .context(
                "database_read_failed",
                "unable to prepare decision event sources",
            )?;
        statement
            .query_map([decision_id], |row| {
                Ok(StoredSource {
                    source_role: row.get(0)?,
                    host_id: row.get(1)?,
                    thread_id: row.get(2)?,
                    turn_id: row.get(3)?,
                    item_id: row.get(4)?,
                    message_role: row.get(5)?,
                    occurred_at: row.get(6)?,
                    timestamp_precision: row.get(7)?,
                })
            })
            .context(
                "database_read_failed",
                "unable to read decision event sources",
            )?
            .collect::<Result<Vec<_>, _>>()
            .context(
                "database_read_failed",
                "unable to decode decision event sources",
            )?
    };
    Ok(decision)
}

fn event_watermark_sequence(connection: &Connection) -> AppResult<i64> {
    connection
        .query_row(
            "SELECT COALESCE(MAX(sequence), 0) FROM decision_events",
            [],
            |row| row.get(0),
        )
        .context(
            "database_read_failed",
            "unable to read the decision event watermark",
        )
}

fn encode_event_cursor(sequence: i64) -> String {
    let encoded = format!("{sequence:016x}");
    let checksum = hex_digest(&format!(
        "{EVENT_STREAM}\n{EVENT_ENVELOPE_VERSION}\n{encoded}"
    ));
    format!("dc1_{encoded}_{}", &checksum[..16])
}

fn decode_event_cursor(cursor: &str) -> AppResult<i64> {
    let mut parts = cursor.split('_');
    let prefix = parts.next();
    let encoded = parts.next();
    let checksum = parts.next();
    if prefix != Some("dc1")
        || encoded.is_none_or(|value| value.len() != 16)
        || checksum.is_none_or(|value| value.len() != 16)
        || parts.next().is_some()
    {
        return Err(AppError::new(
            "event_cursor_invalid",
            "event cursor is invalid",
        ));
    }
    let encoded = encoded.unwrap_or_default();
    let sequence = i64::from_str_radix(encoded, 16)
        .map_err(|_error| AppError::new("event_cursor_invalid", "event cursor is invalid"))?;
    if encode_event_cursor(sequence) != cursor {
        return Err(AppError::new(
            "event_cursor_invalid",
            "event cursor is invalid",
        ));
    }
    Ok(sequence)
}

#[allow(clippy::too_many_lines)]
fn persist_candidate_in(
    transaction: &rusqlite::Transaction<'_>,
    candidate: &Candidate,
) -> AppResult<bool> {
    let inserted = transaction
        .execute(
            "INSERT INTO candidates(
                id, decided_at, timestamp_precision, statement, disposition,
                confidence, rationale, supersedes_id, authority_start,
                authority_end, created_at
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO NOTHING",
            params![
                candidate.id,
                candidate.decided_at,
                candidate.precision.as_str(),
                candidate.statement,
                candidate.disposition.as_str(),
                candidate.confidence.as_str(),
                candidate.rationale,
                candidate.supersedes_id,
                i64::try_from(candidate.authority_start).map_err(|_| AppError::new(
                    "source_span_invalid",
                    "authority span start exceeds SQLite range"
                ))?,
                i64::try_from(candidate.authority_end).map_err(|_| AppError::new(
                    "source_span_invalid",
                    "authority span end exceeds SQLite range"
                ))?,
                now_unix()
            ],
        )
        .context(
            "database_write_failed",
            "unable to persist decision candidate",
        )?;
    if inserted == 1 {
        insert_source(
            transaction,
            &candidate.id,
            "authority",
            &candidate.authority,
        )?;
        for source in &candidate.context {
            insert_source(transaction, &candidate.id, "context", source)?;
        }
        return Ok(true);
    }
    let semantics_agree: bool = transaction
        .query_row(
            "SELECT decided_at=?2 AND timestamp_precision=?3 AND statement=?4
                    AND disposition=?5 AND confidence=?6 AND rationale IS ?7
                    AND supersedes_id IS ?8 AND authority_start=?9
                    AND authority_end=?10
             FROM candidates WHERE id=?1",
            params![
                candidate.id,
                candidate.decided_at,
                candidate.precision.as_str(),
                candidate.statement,
                candidate.disposition.as_str(),
                candidate.confidence.as_str(),
                candidate.rationale,
                candidate.supersedes_id,
                i64::try_from(candidate.authority_start).map_err(|_| AppError::new(
                    "source_span_invalid",
                    "authority span start exceeds SQLite range"
                ))?,
                i64::try_from(candidate.authority_end).map_err(|_| AppError::new(
                    "source_span_invalid",
                    "authority span end exceeds SQLite range"
                ))?,
            ],
            |row| row.get(0),
        )
        .context(
            "database_read_failed",
            "unable to verify immutable decision candidate semantics",
        )?;
    if !semantics_agree {
        return Err(AppError::new(
            "classification_conflict",
            format!(
                "canonical authority span produced conflicting candidate {}",
                candidate.id
            ),
        ));
    }
    let mut expected_sources = std::iter::once(("authority", &candidate.authority))
        .chain(candidate.context.iter().map(|source| ("context", source)))
        .map(|(source_role, source)| {
            (
                source_role.to_owned(),
                source.host_id.clone(),
                source.thread_id.clone(),
                source.turn_id.clone(),
                source.item_id.clone(),
                source.role.as_str().to_owned(),
                source.occurred_at,
                source.precision.as_str().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    expected_sources.sort();
    let stored_sources = {
        let mut statement = transaction
            .prepare(
                "SELECT source_role, host_id, thread_id, turn_id, item_id,
                        message_role, occurred_at, timestamp_precision
                 FROM candidate_sources WHERE candidate_id=?1
                 ORDER BY source_role, host_id, thread_id, turn_id, item_id",
            )
            .context(
                "database_read_failed",
                "unable to prepare immutable decision sources",
            )?;
        statement
            .query_map([candidate.id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .context(
                "database_read_failed",
                "unable to read immutable decision sources",
            )?
            .collect::<Result<Vec<_>, _>>()
            .context(
                "database_read_failed",
                "unable to decode immutable decision sources",
            )?
    };
    if stored_sources != expected_sources {
        return Err(AppError::new(
            "classification_conflict",
            format!(
                "canonical authority span produced conflicting sources for {}",
                candidate.id
            ),
        ));
    }
    Ok(false)
}

fn insert_source(
    transaction: &rusqlite::Transaction<'_>,
    candidate_id: &str,
    source_role: &str,
    source: &crate::model::SourceMessage,
) -> AppResult<()> {
    transaction
        .execute(
            "INSERT INTO candidate_sources(
                candidate_id, source_role, host_id, thread_id, turn_id, item_id,
                message_role, occurred_at, timestamp_precision
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                candidate_id,
                source_role,
                source.host_id,
                source.thread_id,
                source.turn_id,
                source.item_id,
                source.role.as_str(),
                source.occurred_at,
                source.precision.as_str()
            ],
        )
        .context(
            "database_write_failed",
            "unable to persist candidate source",
        )?;
    Ok(())
}

pub(crate) fn default_database_path() -> AppResult<PathBuf> {
    if let Some(path) = std::env::var_os("DECISIONS_DATABASE") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| AppError::new("home_unavailable", "HOME must be an absolute path"))?;
    Ok(home.join("Library/Application Support/Decisions/decisions.db"))
}

pub(crate) fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

pub(crate) fn hex_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    use crate::model::{
        AuthorityMessageVerdict, AuthorityVerdict, Candidate, Confidence, Disposition, MessageRole,
        ObservationClassification, Precision, SourceMessage,
    };

    use rusqlite::Connection;

    use super::{
        MIGRATION_2, MIGRATION_3, Store, database_sidecars, encode_event_cursor,
        migrate_v2_to_v3_with_schema, now_unix, run_operation_lock_path,
    };

    const V1_FIXTURE: &str = r"
        PRAGMA foreign_keys=ON;
        CREATE TABLE schema_migrations(
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );
        INSERT INTO schema_migrations(version, applied_at) VALUES(1, 1);
        CREATE TABLE runs(
            id TEXT PRIMARY KEY,
            report_date TEXT NOT NULL,
            window_start INTEGER NOT NULL,
            window_end INTEGER NOT NULL,
            source_manifest_hash TEXT NOT NULL,
            status TEXT NOT NULL,
            failure_code TEXT,
            failure_detail TEXT,
            content_revision INTEGER NOT NULL DEFAULT 0,
            started_at INTEGER NOT NULL,
            completed_at INTEGER
        );
        CREATE TABLE candidates(
            id TEXT PRIMARY KEY,
            decided_at INTEGER NOT NULL,
            timestamp_precision TEXT NOT NULL,
            statement TEXT NOT NULL,
            disposition TEXT NOT NULL,
            confidence TEXT NOT NULL,
            rationale TEXT,
            supersedes_id TEXT,
            authority_start INTEGER NOT NULL,
            authority_end INTEGER NOT NULL,
            review_state TEXT NOT NULL DEFAULT 'unreviewed',
            created_at INTEGER NOT NULL
        );
        CREATE TABLE candidate_sources(
            candidate_id TEXT NOT NULL REFERENCES candidates(id) ON DELETE CASCADE,
            source_role TEXT NOT NULL,
            host_id TEXT NOT NULL,
            thread_id TEXT NOT NULL,
            turn_id TEXT NOT NULL,
            item_id TEXT NOT NULL,
            message_role TEXT NOT NULL,
            occurred_at INTEGER NOT NULL,
            timestamp_precision TEXT NOT NULL,
            PRIMARY KEY(candidate_id, source_role, item_id)
        );
        CREATE TABLE reviews(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            candidate_id TEXT NOT NULL REFERENCES candidates(id) ON DELETE CASCADE,
            action TEXT NOT NULL,
            reviewed_at INTEGER NOT NULL,
            review_source TEXT NOT NULL
        );
        INSERT INTO runs(
            id, report_date, window_start, window_end, source_manifest_hash,
            status, started_at
        ) VALUES('legacy', '2026-08-31', 0, 1, 'manifest', 'complete', 1);
        PRAGMA user_version=1;
    ";

    const V2_EVENT_FIXTURE: &str = r"
        PRAGMA foreign_keys=ON;
        CREATE TABLE schema_migrations(
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );
        INSERT INTO schema_migrations(version, applied_at) VALUES(1, 1), (2, 2);
        CREATE TABLE candidates(
            id TEXT PRIMARY KEY,
            decided_at INTEGER NOT NULL,
            timestamp_precision TEXT NOT NULL,
            statement TEXT NOT NULL,
            disposition TEXT NOT NULL,
            confidence TEXT NOT NULL,
            rationale TEXT,
            supersedes_id TEXT,
            authority_start INTEGER NOT NULL,
            authority_end INTEGER NOT NULL,
            review_state TEXT NOT NULL DEFAULT 'unreviewed',
            created_at INTEGER NOT NULL
        );
        CREATE TABLE candidate_sources(
            candidate_id TEXT NOT NULL REFERENCES candidates(id) ON DELETE CASCADE,
            source_role TEXT NOT NULL,
            host_id TEXT NOT NULL,
            thread_id TEXT NOT NULL,
            turn_id TEXT NOT NULL,
            item_id TEXT NOT NULL,
            message_role TEXT NOT NULL,
            occurred_at INTEGER NOT NULL,
            timestamp_precision TEXT NOT NULL,
            PRIMARY KEY(candidate_id, source_role, item_id)
        );
        CREATE TABLE reviews(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            candidate_id TEXT NOT NULL REFERENCES candidates(id) ON DELETE CASCADE,
            action TEXT NOT NULL,
            reviewed_at INTEGER NOT NULL,
            review_source TEXT NOT NULL
        );
        INSERT INTO candidates(
            id, decided_at, timestamp_precision, statement, disposition,
            confidence, rationale, supersedes_id, authority_start, authority_end,
            review_state, created_at
        ) VALUES(
            'd_historical', 10, 'item', 'Keep the event history.', 'adopt',
            'medium', NULL, NULL, 0, 8, 'dismissed', 20
        );
        INSERT INTO candidate_sources(
            candidate_id, source_role, host_id, thread_id, turn_id, item_id,
            message_role, occurred_at, timestamp_precision
        ) VALUES(
            'd_historical', 'authority', 'host', 'thread', 'turn', 'item',
            'user', 10, 'item'
        );
        INSERT INTO reviews(candidate_id, action, reviewed_at, review_source)
        VALUES
            ('d_historical', 'confirm', 30, 'cli'),
            ('d_historical', 'dismiss', 40, 'cli');
        PRAGMA user_version=2;
    ";

    fn candidate() -> Candidate {
        Candidate {
            id: "d_receipt".to_owned(),
            decided_at: 10,
            precision: Precision::Item,
            statement: "Use the durable receipt.".to_owned(),
            disposition: Disposition::Adopt,
            confidence: Confidence::High,
            rationale: Some("Explicitly selected.".to_owned()),
            supersedes_id: None,
            authority_start: 0,
            authority_end: 6,
            authority: SourceMessage {
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
                turn_id: "turn".to_owned(),
                item_id: "item".to_owned(),
                role: MessageRole::User,
                text: "raw transcript must not persist".to_owned(),
                occurred_at: 10,
                precision: Precision::Item,
            },
            context: vec![SourceMessage {
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
                turn_id: "turn".to_owned(),
                item_id: "context".to_owned(),
                role: MessageRole::Assistant,
                text: "raw context must not persist".to_owned(),
                occurred_at: 9,
                precision: Precision::Turn,
            }],
        }
    }

    fn authority(thread_id: &str, turn_id: &str, item_id: &str, occurred_at: i64) -> SourceMessage {
        SourceMessage {
            host_id: "host".to_owned(),
            thread_id: thread_id.to_owned(),
            turn_id: turn_id.to_owned(),
            item_id: item_id.to_owned(),
            role: MessageRole::User,
            text: "Use the selected design.".to_owned(),
            occurred_at,
            precision: Precision::Item,
        }
    }

    #[test]
    fn initializes_current_schema() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let state = directory.path().join("state");
        let database = state.join("decisions.db");
        let store = Store::open(&database)?;
        assert_eq!(store.schema_version()?, 3);
        assert_eq!(fs::metadata(&state)?.permissions().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&database)?.permissions().mode() & 0o777, 0o600);
        Ok(())
    }

    #[test]
    fn observer_activation_is_write_once() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("decisions.db"))?;
        assert_eq!(store.activate_observer(101)?, 101);
        assert_eq!(store.activate_observer(999)?, 101);
        assert_eq!(store.observer_baseline_at()?, Some(101));
        Ok(())
    }

    #[test]
    fn migration_from_v1_is_atomic_and_reaches_the_current_schema()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("decisions.db");
        {
            let connection = Connection::open(&database)?;
            connection.execute_batch(V1_FIXTURE)?;
            let broken = MIGRATION_2.replace(
                "CREATE TABLE authority_verdicts",
                "CREATE TABL authority_verdicts",
            );
            assert!(connection.execute_batch(&broken).is_err());
        }
        {
            let connection = Connection::open(&database)?;
            let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            assert_eq!(version, 1);
            let run_kind_columns: i64 = connection.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('runs') WHERE name='run_kind'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(run_kind_columns, 0);
            let metadata_tables: i64 = connection.query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='table' AND name='product_metadata'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(metadata_tables, 0);
        }
        let store = Store::open(&database)?;
        assert_eq!(store.schema_version()?, 3);
        let legacy_kind: String = store.connection.query_row(
            "SELECT run_kind FROM runs WHERE id='legacy'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(legacy_kind, "legacy_scan");
        Ok(())
    }

    #[test]
    fn migration_v3_is_atomic_backfills_history_and_is_idempotent()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("decisions.db");
        {
            let mut connection = Connection::open(&database)?;
            connection.execute_batch(V2_EVENT_FIXTURE)?;
            let broken = MIGRATION_3.replace(
                "CREATE UNIQUE INDEX decision_events_one_review",
                "CREATE UNIQUE INDE decision_events_one_review",
            );
            assert!(migrate_v2_to_v3_with_schema(&mut connection, &broken).is_err());
            let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            assert_eq!(version, 2);
            let event_tables: i64 = connection.query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='table' AND name='decision_events'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(event_tables, 0);
        }

        let store = Store::open(&database)?;
        assert_eq!(store.schema_version()?, 3);
        let page = store.read_events(&encode_event_cursor(0), 100)?;
        assert_eq!(page.events.len(), 3);
        assert!(!page.has_more);
        assert_eq!(
            page.events[0].event["event_kind"],
            serde_json::json!("decision_admitted")
        );
        assert_eq!(
            page.events[0].event["decision"]["review_state"],
            serde_json::json!("unreviewed")
        );
        assert_eq!(
            page.events[1].event["review"]["action"],
            serde_json::json!("confirm")
        );
        assert_eq!(
            page.events[2].event["review"]["action"],
            serde_json::json!("dismiss")
        );
        let candidate_count: i64 = store.connection.query_row(
            "SELECT COUNT(*) FROM candidates WHERE id='d_historical'",
            [],
            |row| row.get(0),
        )?;
        let review_count: i64 = store.connection.query_row(
            "SELECT COUNT(*) FROM reviews WHERE candidate_id='d_historical'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(candidate_count, 1);
        assert_eq!(review_count, 2);
        drop(store);

        let reopened = Store::open(&database)?;
        let event_count: i64 =
            reopened
                .connection
                .query_row("SELECT COUNT(*) FROM decision_events", [], |row| row.get(0))?;
        assert_eq!(event_count, 3);
        Ok(())
    }

    #[test]
    fn lifecycle_stream_is_ordered_private_and_cursor_idempotent()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let start = store.event_watermark()?.cursor;
        let observation = store.ingest_observation("session", "turn")?;
        let authority = authority("thread", "turn", "item", 10);
        store.bind_observation_source(
            &observation.id,
            "host",
            "thread",
            15,
            "digest",
            1,
            std::slice::from_ref(&authority),
        )?;
        let mut admitted = candidate();
        admitted.authority = authority.clone();
        store.complete_observation(
            &observation.id,
            &ObservationClassification {
                candidates: vec![admitted.clone()],
                authority_verdicts: vec![AuthorityMessageVerdict {
                    authority,
                    verdict: AuthorityVerdict::Decision,
                }],
                needs_context: false,
            },
        )?;
        store.review(&admitted.id, "confirm")?;
        store.review(&admitted.id, "dismiss")?;

        let first = store.read_events(&start, 2)?;
        assert_eq!(first.events.len(), 2);
        assert!(first.has_more);
        assert_eq!(first.next_cursor, first.events[1].cursor);
        assert_eq!(
            first.events[0].event["event_kind"],
            serde_json::json!("decision_admitted")
        );
        assert_eq!(
            first.events[1].event["review"]["action"],
            serde_json::json!("confirm")
        );
        let from_first = store.read_events(&first.events[0].cursor, 1)?;
        assert_eq!(from_first.events[0].event, first.events[1].event);
        let repeated = store.read_events(&start, 2)?;
        assert_eq!(
            serde_json::to_value(&first)?,
            serde_json::to_value(&repeated)?
        );
        let serialized = serde_json::to_string(&store.read_events(&start, 100)?)?;
        assert!(!serialized.contains("raw transcript must not persist"));
        assert!(!serialized.contains("raw context must not persist"));
        assert!(!serialized.contains("working_directory"));
        assert_eq!(
            store
                .read_events("tampered", 1)
                .err()
                .map(|error| error.code),
            Some("event_cursor_invalid")
        );
        assert_eq!(
            store
                .read_events(&encode_event_cursor(999), 1)
                .err()
                .map(|error| error.code),
            Some("event_cursor_ahead")
        );
        Ok(())
    }

    #[test]
    fn event_append_failure_rolls_back_candidate_admission()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let observation = store.ingest_observation("session", "turn")?;
        let authority = authority("thread", "turn", "item", 10);
        store.bind_observation_source(
            &observation.id,
            "host",
            "thread",
            15,
            "digest",
            1,
            std::slice::from_ref(&authority),
        )?;
        store.connection.execute_batch(
            "CREATE TRIGGER reject_decision_event
             BEFORE INSERT ON decision_events
             BEGIN SELECT RAISE(ABORT, 'synthetic event failure'); END;",
        )?;
        let mut admitted = candidate();
        admitted.authority = authority.clone();
        assert!(
            store
                .complete_observation(
                    &observation.id,
                    &ObservationClassification {
                        candidates: vec![admitted.clone()],
                        authority_verdicts: vec![AuthorityMessageVerdict {
                            authority,
                            verdict: AuthorityVerdict::Decision,
                        }],
                        needs_context: false,
                    },
                )
                .is_err()
        );
        let candidates: i64 = store.connection.query_row(
            "SELECT COUNT(*) FROM candidates WHERE id=?1",
            [admitted.id],
            |row| row.get(0),
        )?;
        let verdicts: i64 = store.connection.query_row(
            "SELECT COUNT(*) FROM authority_verdicts WHERE observation_id=?1",
            [observation.id],
            |row| row.get(0),
        )?;
        assert_eq!(candidates, 0);
        assert_eq!(verdicts, 0);
        Ok(())
    }

    #[test]
    fn observation_completion_requires_exact_per_authority_coverage()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        store.activate_observer(10)?;
        let observation = store.ingest_observation("session", "turn")?;
        let authorities = [
            authority("thread", "turn", "user-1", 10),
            authority("thread", "turn", "user-2", 11),
        ];
        store.bind_observation_source(
            &observation.id,
            "host",
            "thread",
            15,
            "digest",
            1,
            &authorities,
        )?;
        let incomplete = ObservationClassification {
            candidates: Vec::new(),
            authority_verdicts: vec![AuthorityMessageVerdict {
                authority: authorities[0].clone(),
                verdict: AuthorityVerdict::NoDecision,
            }],
            needs_context: false,
        };
        assert_eq!(
            store
                .complete_observation(&observation.id, &incomplete)
                .err()
                .map(|error| error.code),
            Some("classification_coverage_invalid")
        );
        let persisted: i64 = store.connection.query_row(
            "SELECT COUNT(*) FROM authority_verdicts WHERE observation_id=?1",
            [observation.id.as_str()],
            |row| row.get(0),
        )?;
        assert_eq!(persisted, 0);
        let complete = ObservationClassification {
            candidates: Vec::new(),
            authority_verdicts: authorities
                .iter()
                .cloned()
                .map(|authority| AuthorityMessageVerdict {
                    authority,
                    verdict: AuthorityVerdict::NoDecision,
                })
                .collect(),
            needs_context: false,
        };
        store.complete_observation(&observation.id, &complete)?;
        let persisted: i64 = store.connection.query_row(
            "SELECT COUNT(*) FROM authority_verdicts WHERE observation_id=?1",
            [observation.id.as_str()],
            |row| row.get(0),
        )?;
        assert_eq!(persisted, 2);
        let projection = store.project_observations("1970-01-01", 10, 20, i64::MAX, i64::MAX)?;
        assert_eq!(projection.observations_covered, 1);
        assert!(store.candidates_for_run(&projection.run.id)?.is_empty());
        Ok(())
    }

    #[test]
    fn observation_rejects_low_confidence_instead_of_silently_dropping_it()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let observation = store.ingest_observation("session", "turn")?;
        let authority = authority("thread", "turn", "item", 10);
        store.bind_observation_source(
            &observation.id,
            "host",
            "thread",
            15,
            "digest",
            1,
            std::slice::from_ref(&authority),
        )?;
        let mut candidate = candidate();
        candidate.id = "d_0123456789abcdefabcd".to_owned();
        candidate.authority = authority.clone();
        candidate.confidence = Confidence::Low;
        let classification = ObservationClassification {
            candidates: vec![candidate],
            authority_verdicts: vec![AuthorityMessageVerdict {
                authority,
                verdict: AuthorityVerdict::Decision,
            }],
            needs_context: false,
        };
        assert_eq!(
            store
                .complete_observation(&observation.id, &classification)
                .err()
                .map(|error| error.code),
            Some("classification_confidence_invalid")
        );
        Ok(())
    }

    #[test]
    fn unresolved_observations_block_projection_and_retry_preserves_attempt_history()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("decisions.db"))?;
        let observation = store.ingest_observation("session", "turn")?;
        store.fail_observation(&observation.id, "conversation_source_failed", "unavailable")?;
        let status = store.observation_status_window(Some((0, 20)))?;
        assert_eq!(status.failures.len(), 1);
        assert_eq!(status.failures[0].id, observation.id);
        assert_eq!(
            status.failures[0].failure_code,
            "conversation_source_failed"
        );
        assert!(!serde_json::to_string(&status)?.contains("unavailable"));
        assert_eq!(
            Store::open(&directory.path().join("decisions.db"))?
                .project_observations("1970-01-01", 0, 20, i64::MAX, i64::MAX)
                .err()
                .map(|error| error.code),
            Some("observation_coverage_incomplete")
        );
        let retried = store.retry_observation(&observation.id)?;
        assert_eq!(retried.status, "queued");
        assert_eq!(retried.attempt_epoch, 1);
        Ok(())
    }

    #[test]
    fn deferred_observation_yields_to_ready_work_and_reenters_in_both_selectors()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(&directory.path().join("decisions.db"))?;
        let deferred = store.ingest_observation("session", "deferred")?;
        store.connection.execute(
            "UPDATE observations SET created_at=10 WHERE id=?1",
            [&deferred.id],
        )?;
        store.defer_observation(&deferred.id, None, 30)?;
        let ready = store.ingest_observation("session", "ready")?;
        store.connection.execute(
            "UPDATE observations SET created_at=20 WHERE id=?1",
            [&ready.id],
        )?;
        let watermark = store.observation_admission_watermark()?;

        assert_eq!(
            store
                .next_observation_before_at(None, 25)?
                .map(|observation| observation.id),
            Some(ready.id.clone())
        );
        assert_eq!(
            store
                .next_observation_for_projection_at(100, watermark, 0, 100, 25)?
                .map(|observation| observation.id),
            Some(ready.id.clone())
        );
        assert_eq!(
            store
                .next_observation_before_at(None, 40)?
                .map(|observation| observation.id),
            Some(ready.id.clone())
        );
        store.mark_observation_not_eligible(&ready.id, "host", "thread", 20, None)?;
        assert!(
            store
                .next_observation_for_projection_at(100, watermark, 0, 100, 25)?
                .is_none()
        );
        assert_eq!(
            store
                .next_observation_before_at(None, 40)?
                .map(|observation| observation.id),
            Some(deferred.id.clone())
        );
        assert_eq!(
            store
                .next_observation_for_projection_at(100, watermark, 0, 100, 40)?
                .map(|observation| observation.id),
            Some(deferred.id)
        );
        Ok(())
    }

    #[test]
    fn processing_observation_precedes_ready_queue_work_in_both_selectors()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let queued = store.ingest_observation("session", "queued")?;
        let processing = store.ingest_observation("session", "processing")?;
        let source = authority("thread", "processing", "processing-user", 5);
        store.bind_observation_source(
            &processing.id,
            "host",
            "thread",
            10,
            "processing-digest",
            1,
            &[source],
        )?;
        store.connection.execute(
            "UPDATE observations SET next_attempt_at=?2 WHERE id=?1",
            rusqlite::params![&processing.id, i64::MAX],
        )?;
        let watermark = store.observation_admission_watermark()?;

        assert_eq!(
            store
                .next_observation_before_at(None, 20)?
                .map(|observation| observation.id),
            Some(processing.id.clone())
        );
        assert_eq!(
            store
                .next_observation_for_projection_at(20, watermark, 0, 20, 20)?
                .map(|observation| observation.id),
            Some(processing.id)
        );
        assert_eq!(store.observation(&queued.id)?.status, "queued");
        Ok(())
    }

    #[test]
    fn unavailable_source_abandonment_is_guarded_audited_and_idempotent()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        assert_eq!(store.activate_observer(7)?, 7);
        let observation = store.ingest_observation("session", "unavailable")?;
        let watermark = store.observation_admission_watermark()?;
        store.defer_observation(&observation.id, None, i64::MAX)?;

        let abandoned = store.abandon_unavailable_observation(&observation.id)?;
        assert_eq!(abandoned.status, "complete");
        assert_eq!(abandoned.outcome.as_deref(), Some("not_eligible"));
        assert_eq!(
            abandoned.failure_code.as_deref(),
            Some("conversation_source_abandoned")
        );
        let completed_at: i64 = store.connection.query_row(
            "SELECT completed_at FROM observations WHERE id=?1",
            [&observation.id],
            |row| row.get(0),
        )?;
        assert_eq!(
            store
                .abandon_unavailable_observation(&observation.id)?
                .failure_code
                .as_deref(),
            Some("conversation_source_abandoned")
        );
        assert_eq!(
            store.connection.query_row(
                "SELECT completed_at FROM observations WHERE id=?1",
                [&observation.id],
                |row| row.get::<_, i64>(0),
            )?,
            completed_at
        );
        assert_eq!(store.observer_baseline_at()?, Some(7));
        for table in [
            "observation_jobs",
            "observation_authority_items",
            "authority_verdicts",
            "observation_candidates",
            "observation_classification_receipts",
            "candidates",
            "decision_events",
        ] {
            let count: i64 = store.connection.query_row(
                &format!("SELECT COUNT(*) FROM {table}"),
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 0, "{table} changed during source abandonment");
        }
        assert_eq!(
            store
                .ingest_reconciled_observation("host", "thread", "unavailable", 10)
                .err()
                .map(|error| error.code),
            Some("observation_source_abandoned_conflict")
        );
        assert!(
            store
                .project_observations("1970-01-01", 0, 20, i64::MAX, watermark)
                .is_ok()
        );

        let (bound, inserted) =
            store.ingest_reconciled_observation("host", "thread", "bound", 10)?;
        assert!(inserted);
        assert_eq!(
            store
                .abandon_unavailable_observation(&bound.id)
                .err()
                .map(|error| error.code),
            Some("observation_source_abandon_unsafe")
        );
        let incomplete = store.ingest_observation("session", "incomplete")?;
        store.defer_observation(&incomplete.id, Some(11), i64::MAX)?;
        assert_eq!(
            store
                .abandon_unavailable_observation(&incomplete.id)
                .err()
                .map(|error| error.code),
            Some("observation_source_abandon_unsafe")
        );
        Ok(())
    }

    #[test]
    fn unavailable_source_abandonment_refuses_changed_or_classified_rows()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let processing = store.ingest_observation("session", "processing")?;
        store.defer_observation(&processing.id, None, i64::MAX)?;
        store.connection.execute(
            "UPDATE observations SET status='processing' WHERE id=?1",
            [&processing.id],
        )?;
        assert_eq!(
            store
                .abandon_unavailable_observation(&processing.id)
                .err()
                .map(|error| error.code),
            Some("observation_source_abandon_invalid")
        );

        let expanded = store.ingest_observation("session", "expanded")?;
        store.defer_observation(&expanded.id, None, i64::MAX)?;
        store.connection.execute(
            "UPDATE observations SET scope_level=1 WHERE id=?1",
            [&expanded.id],
        )?;
        assert_eq!(
            store
                .abandon_unavailable_observation(&expanded.id)
                .err()
                .map(|error| error.code),
            Some("observation_source_abandon_unsafe")
        );

        let classified = store.ingest_observation("session", "classified")?;
        store.defer_observation(&classified.id, None, i64::MAX)?;
        store.connection.execute(
            "INSERT INTO observation_jobs(
                observation_id, scope_level, attempt, nucleus_job_id, status
             ) VALUES(?1, 0, 0, 'job-classified', 'planned')",
            [&classified.id],
        )?;
        assert_eq!(
            store
                .abandon_unavailable_observation(&classified.id)
                .err()
                .map(|error| error.code),
            Some("observation_source_abandon_unsafe")
        );

        let other_complete = store.ingest_observation("session", "other-complete")?;
        store.mark_observation_not_eligible(
            &other_complete.id,
            "host",
            "thread-other-complete",
            12,
            None,
        )?;
        assert_eq!(
            store
                .abandon_unavailable_observation(&other_complete.id)
                .err()
                .map(|error| error.code),
            Some("observation_source_abandon_invalid")
        );
        Ok(())
    }

    #[test]
    fn reconciled_completion_frontier_makes_a_missed_hook_drainable_immediately()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let cutoff = now_unix().saturating_sub(10);
        let (observation, inserted) = store.ingest_reconciled_observation(
            "host",
            "thread",
            "turn",
            cutoff.saturating_sub(1),
        )?;
        assert!(inserted);
        let next = store
            .next_observation_before(Some(cutoff))?
            .ok_or("reconciled observation was not drainable")?;
        assert_eq!(next.id, observation.id);
        Ok(())
    }

    #[test]
    fn hook_first_reconciliation_binds_one_exact_owner_across_a_copied_fork()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let hook = store.ingest_observation("shared-session", "copied-turn")?;
        let (root, root_inserted) =
            store.ingest_reconciled_observation("host", "root", "copied-turn", 10)?;
        assert!(!root_inserted);
        assert_eq!(root.id, hook.id);
        assert_eq!(root.thread_id.as_deref(), Some("root"));

        let (fork_copy, fork_inserted) =
            store.ingest_reconciled_observation("host", "fork", "copied-turn", 10)?;
        assert!(!fork_inserted);
        assert_eq!(fork_copy.id, root.id);
        assert_eq!(fork_copy.thread_id.as_deref(), Some("root"));

        let (fork_only, fork_only_inserted) =
            store.ingest_reconciled_observation("host", "fork", "fork-only-turn", 11)?;
        assert!(fork_only_inserted);
        assert_ne!(fork_only.id, root.id);
        assert_eq!(fork_only.thread_id.as_deref(), Some("fork"));
        Ok(())
    }

    #[test]
    fn projection_uses_source_completion_and_a_stable_admission_frontier()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let crossing = store.ingest_observation("session", "crossing")?;
        let crossing_watermark = store.observation_admission_watermark()?;
        assert_eq!(
            store
                .next_observation_for_projection(10, crossing_watermark, 0, 20)?
                .map(|observation| observation.id),
            Some(crossing.id.clone())
        );
        let crossing_authority = authority("thread", "crossing", "crossing-user", 5);
        store.bind_observation_source(
            &crossing.id,
            "host",
            "thread",
            11,
            "crossing-digest",
            1,
            std::slice::from_ref(&crossing_authority),
        )?;
        assert!(
            store
                .next_observation_for_projection(10, crossing_watermark, 0, 20)?
                .is_none()
        );
        let projection = store.project_observations("1970-01-01", 0, 20, 10, crossing_watermark)?;
        assert_eq!(projection.observations_covered, 0);
        assert_eq!(
            store
                .next_observation_before(None)?
                .map(|observation| observation.id),
            Some(crossing.id)
        );

        let admitted = store.ingest_observation("session", "admitted")?;
        let admitted_watermark = store.observation_admission_watermark()?;
        let admitted_authority = authority("thread", "admitted", "admitted-user", 6);
        store.bind_observation_source(
            &admitted.id,
            "host",
            "thread",
            10,
            "admitted-digest",
            1,
            std::slice::from_ref(&admitted_authority),
        )?;
        assert_eq!(
            store
                .project_observations("1970-01-01", 0, 20, 10, admitted_watermark)
                .err()
                .map(|error| error.code),
            Some("observation_coverage_incomplete")
        );

        let late = store.ingest_observation("session", "late")?;
        store.fail_observation(&late.id, "conversation_source_failed", "unavailable")?;
        store.complete_observation(
            &admitted.id,
            &ObservationClassification {
                candidates: Vec::new(),
                authority_verdicts: vec![AuthorityMessageVerdict {
                    authority: admitted_authority,
                    verdict: AuthorityVerdict::NoDecision,
                }],
                needs_context: false,
            },
        )?;
        assert!(
            store
                .project_observations("1970-01-01", 0, 20, 10, admitted_watermark)
                .is_ok()
        );
        Ok(())
    }

    #[test]
    fn projection_drain_skips_already_scoped_out_of_window_work()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let observation = store.ingest_observation("session", "current-day")?;
        let authority = authority("thread", "current-day", "user", 30);
        store.bind_observation_source(
            &observation.id,
            "host",
            "thread",
            10,
            "digest",
            1,
            std::slice::from_ref(&authority),
        )?;
        let watermark = store.observation_admission_watermark()?;
        assert!(
            store
                .next_observation_for_projection(10, watermark, 0, 20)?
                .is_none()
        );
        assert_eq!(
            store.next_observation_before(None)?.map(|value| value.id),
            Some(observation.id)
        );
        Ok(())
    }

    #[test]
    fn stop_ingest_before_turn_completion_stays_queued_and_is_cutoff_safe()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let observation = store.ingest_observation("session", "in-progress")?;
        let watermark = store.observation_admission_watermark()?;
        store.defer_observation(&observation.id, Some(11), i64::MAX)?;
        assert_eq!(store.observation(&observation.id)?.status, "queued");
        assert!(
            store
                .project_observations("1970-01-01", 0, 20, 10, watermark)
                .is_ok()
        );
        assert_eq!(
            store
                .project_observations("1970-01-01", 0, 20, 12, watermark)
                .err()
                .map(|error| error.code),
            Some("observation_coverage_incomplete")
        );
        Ok(())
    }

    #[test]
    fn reconciled_prebound_source_failure_blocks_projection_without_authorities()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let (observation, inserted) =
            store.ingest_reconciled_observation("host", "thread", "turn", 10)?;
        assert!(inserted);
        let watermark = store.observation_admission_watermark()?;
        store.fail_observation(&observation.id, "conversation_source_failed", "unavailable")?;
        assert_eq!(
            store
                .project_observations("1970-01-01", 0, 20, 10, watermark)
                .err()
                .map(|error| error.code),
            Some("observation_coverage_incomplete")
        );
        Ok(())
    }

    #[test]
    fn scheduled_observer_lock_waits_for_the_serial_worker()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("decisions.db");
        let store = Store::open(&database)?;
        let active = store.lock_observation_processing()?;
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || -> Result<(), String> {
            let waiting = Store::open(&database).map_err(|error| error.to_string())?;
            started_tx.send(()).map_err(|error| error.to_string())?;
            let _lock = waiting
                .wait_for_observation_processing()
                .map_err(|error| error.to_string())?;
            acquired_tx.send(()).map_err(|error| error.to_string())?;
            Ok(())
        });
        started_rx.recv()?;
        assert!(acquired_rx.try_recv().is_err());
        drop(active);
        acquired_rx.recv()?;
        worker.join().map_err(|_| "lock worker panicked")??;
        Ok(())
    }

    #[test]
    fn refuses_a_symlink_database() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("target");
        fs::write(&target, [])?;
        let database = directory.path().join("decisions.db");
        symlink(&target, &database)?;
        let error = Store::open(&database).err().ok_or("expected unsafe path")?;
        assert_eq!(error.code, "database_path_unsafe");
        Ok(())
    }

    #[test]
    fn leaves_existing_custom_parent_mode_unchanged() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let state = directory.path().join("shared");
        fs::create_dir(&state)?;
        fs::set_permissions(&state, fs::Permissions::from_mode(0o755))?;
        let _store = Store::open(&state.join("decisions.db"))?;
        assert_eq!(fs::metadata(&state)?.permissions().mode() & 0o777, 0o755);
        Ok(())
    }

    #[test]
    fn secures_existing_wal_in_custom_parent() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let state = directory.path().join("shared");
        fs::create_dir(&state)?;
        fs::set_permissions(&state, fs::Permissions::from_mode(0o755))?;
        let database = state.join("decisions.db");
        let first = Store::open(&database)?;
        first
            .connection
            .execute("CREATE TABLE sidecar_probe(value INTEGER)", [])?;
        let wal = database_sidecars(&database)[0].clone();
        fs::set_permissions(&wal, fs::Permissions::from_mode(0o644))?;
        let _second = Store::open(&database)?;
        assert_eq!(fs::metadata(&wal)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(&state)?.permissions().mode() & 0o777, 0o755);
        Ok(())
    }

    #[test]
    fn refuses_a_dangling_sidecar_symlink() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("decisions.db");
        let _store = Store::open(&database)?;
        let journal = database_sidecars(&database)[2].clone();
        symlink(directory.path().join("missing"), &journal)?;
        let error = Store::open(&database).err().ok_or("expected unsafe path")?;
        assert_eq!(error.code, "database_path_unsafe");
        Ok(())
    }

    #[test]
    fn refuses_a_dangling_database_symlink() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("decisions.db");
        symlink(directory.path().join("missing"), &database)?;
        let error = Store::open(&database).err().ok_or("expected unsafe path")?;
        assert_eq!(error.code, "database_path_unsafe");
        Ok(())
    }

    #[test]
    fn manual_retry_reuses_ambiguous_delivery_but_new_send_follows_acceptance()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("state/decisions.db"))?;
        let run = store.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        store.complete_run(&run.id, &[])?;
        store.connection.execute(
            "UPDATE runs SET run_kind='observation_projection' WHERE id=?1",
            [run.id.as_str()],
        )?;
        let run = store.latest_complete_run("2026-08-31")?;
        let _snapshot = store.snapshot(&run, "Subject", "Body")?;
        let first = store.begin_delivery(&run, None)?;
        store.finish_delivery(&first.id, Err("ambiguous transport"))?;
        let retry = store.begin_delivery(&run, None)?;
        assert_eq!(retry.id, first.id);
        assert_eq!(retry.idempotency_key, first.idempotency_key);
        store.finish_delivery(&retry.id, Ok("email_1"))?;
        let next = store.begin_delivery(&run, None)?;
        assert_ne!(next.id, first.id);
        assert_ne!(next.idempotency_key, first.idempotency_key);
        Ok(())
    }

    #[test]
    fn resumes_matching_build_and_requires_failure_before_new_attempt()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("state/decisions.db"))?;
        let first = store.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        let resumed = store.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        assert_eq!(resumed.id, first.id);
        let conflict = store.begin_or_resume_run("2026-08-31", 0, 10, "changed");
        assert_eq!(
            conflict.err().map(|error| error.code),
            Some("building_run_source_changed")
        );
        store.fail_run(&first.id, "explicit_new_attempt", "test policy")?;
        let next = store.begin_or_resume_run("2026-08-31", 0, 10, "changed")?;
        assert_ne!(next.id, first.id);
        Ok(())
    }

    #[test]
    fn run_admission_is_atomic_across_connections() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("state/decisions.db");
        let mut first = Store::open(&database)?;
        let mut second = Store::open(&database)?;
        let first_run = first.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        let second_run = second.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        assert_eq!(first_run.id, second_run.id);
        let active: i64 = first.connection.query_row(
            "SELECT COUNT(*) FROM runs
             WHERE report_date='2026-08-31' AND status IN ('building', 'abandoning')",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(active, 1);
        Ok(())
    }

    #[test]
    fn run_operation_lock_fences_admission_from_abandonment()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("state/decisions.db");
        let mut builder = Store::open(&database)?;
        let mut abandoner = Store::open(&database)?;
        let run = builder.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        builder.plan_job(&run.id, "thread", "job")?;

        let admission_lock = builder.lock_run_operations()?;
        assert_eq!(
            fs::metadata(run_operation_lock_path(&database))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        builder.begin_job_admission("job")?;
        let busy = abandoner.lock_run_operations();
        assert_eq!(
            busy.err().map(|error| error.code),
            Some("run_operation_busy")
        );
        drop(admission_lock);

        let _abandonment_lock = abandoner.lock_run_operations()?;
        let (abandoning, jobs) = abandoner.prepare_abandon("2026-08-31")?;
        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].admitted);
        assert_eq!(jobs[0].status, "submitted");
        abandoner.restore_build_after_unresolved_admission(&abandoning.id, &jobs)?;
        let resumed = abandoner.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        assert_eq!(resumed.id, run.id);
        abandoner.plan_job(&resumed.id, "thread", "job")?;
        assert_eq!(abandoner.job_status("job")?, "submitted");
        Ok(())
    }

    #[test]
    fn run_operation_lock_refuses_a_dangling_symlink() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("state/decisions.db");
        let store = Store::open(&database)?;
        let lock_path = run_operation_lock_path(&database);
        symlink(directory.path().join("missing"), &lock_path)?;
        let error = store
            .lock_run_operations()
            .err()
            .ok_or("expected unsafe run-operation lock")?;
        assert_eq!(error.code, "database_path_unsafe");
        Ok(())
    }

    #[test]
    fn unresolved_admission_restore_refuses_a_changed_snapshot()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("state/decisions.db"))?;
        let run = store.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        store.plan_job(&run.id, "thread", "job")?;
        store.begin_job_admission("job")?;
        let (abandoning, jobs) = store.prepare_abandon("2026-08-31")?;
        let _ = store.mark_job("job", "complete", None)?;
        let restore = store.restore_build_after_unresolved_admission(&abandoning.id, &jobs);
        assert_eq!(
            restore.err().map(|error| error.code),
            Some("abandonment_job_set_changed")
        );
        let active = store.prepare_abandon("2026-08-31")?.0;
        assert_eq!(active.status, "abandoning");
        Ok(())
    }

    #[test]
    fn first_durable_receipt_or_failure_wins_terminal_race()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("state/decisions.db");
        let mut first = Store::open(&database)?;
        let second = Store::open(&database)?;
        let run = first.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;

        first.plan_job(&run.id, "thread-success-first", "job-success-first")?;
        first.begin_job_admission("job-success-first")?;
        first.persist_classification_receipt(
            "job-success-first",
            "call",
            r#"{"accepted":true,"candidate_count":1}"#,
            false,
            Some(&[candidate()]),
        )?;
        assert!(!second.mark_job("job-success-first", "failed", Some("stale terminal"))?);
        assert_eq!(second.job_status("job-success-first")?, "complete");

        first.plan_job(&run.id, "thread-failure-first", "job-failure-first")?;
        first.begin_job_admission("job-failure-first")?;
        assert!(second.mark_job("job-failure-first", "failed", Some("stale terminal"))?);
        let late_receipt = first.persist_classification_receipt(
            "job-failure-first",
            "call",
            r#"{"accepted":true,"candidate_count":1}"#,
            false,
            Some(&[candidate()]),
        );
        assert_eq!(
            late_receipt.err().map(|error| error.code),
            Some("classification_receipt_late")
        );
        assert_eq!(second.job_status("job-failure-first")?, "failed");
        assert!(!second.mark_job("job-failure-first", "failed", Some("late terminal"))?);
        assert!(
            second
                .persisted_classification("job-failure-first")?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn abandonment_blocks_work_until_correlations_are_reconciled()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("state/decisions.db"))?;
        let run = store.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        store.plan_job(&run.id, "thread", "job")?;
        let (abandoning, jobs) = store.prepare_abandon("2026-08-31")?;
        assert_eq!(abandoning.id, run.id);
        assert_eq!(abandoning.status, "abandoning");
        assert_eq!(jobs.len(), 1);
        assert!(!jobs[0].admitted);
        assert!(store.plan_job(&run.id, "another", "another-job").is_err());
        assert_eq!(
            store
                .complete_run(&run.id, &[])
                .err()
                .map(|error| error.code),
            Some("run_state_conflict")
        );
        store.finish_abandon(&run.id, &jobs)?;
        let next = store.begin_or_resume_run("2026-08-31", 0, 10, "changed")?;
        assert_ne!(next.id, run.id);
        Ok(())
    }

    #[test]
    fn delivery_admission_and_acceptance_are_monotonic_across_connections()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("state/decisions.db");
        let mut first = Store::open(&database)?;
        let run = first.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
        first.complete_run(&run.id, &[])?;
        first.connection.execute(
            "UPDATE runs SET run_kind='observation_projection' WHERE id=?1",
            [run.id.as_str()],
        )?;
        let run = first.latest_complete_run("2026-08-31")?;
        let _snapshot = first.snapshot(&run, "Subject", "Body")?;
        let mut second = Store::open(&database)?;
        let first_delivery = first.begin_delivery(&run, None)?;
        let second_delivery = second.begin_delivery(&run, None)?;
        assert_eq!(first_delivery.id, second_delivery.id);
        first.finish_delivery(&first_delivery.id, Ok("email_1"))?;
        second.finish_delivery(&second_delivery.id, Err("late failure"))?;
        let stored: (String, Option<String>, Option<String>) = first.connection.query_row(
            "SELECT status, email_id, failure_detail FROM deliveries WHERE id=?1",
            [first_delivery.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(stored.0, "accepted");
        assert_eq!(stored.1.as_deref(), Some("email_1"));
        assert!(stored.2.is_none());
        Ok(())
    }

    #[test]
    fn restart_replays_exact_receipt_and_recovers_text_free_classification()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("state/decisions.db");
        {
            let mut store = Store::open(&database)?;
            let run = store.begin_or_resume_run("2026-08-31", 0, 10, "manifest")?;
            store.plan_job(&run.id, "thread", "job")?;
            store.persist_job_request_digest("job", "request-digest")?;
            let receipt = store.persist_classification_receipt(
                "job",
                "call",
                r#"{"accepted":true,"candidate_count":1}"#,
                false,
                Some(&[candidate()]),
            )?;
            assert_eq!(
                receipt.result_json,
                r#"{"accepted":true,"candidate_count":1}"#
            );
        }
        let store = Store::open(&database)?;
        store.persist_job_request_digest("job", "request-digest")?;
        let replay = store
            .classification_receipt("job", "call")?
            .ok_or("missing durable receipt")?;
        assert_eq!(
            replay.result_json,
            r#"{"accepted":true,"candidate_count":1}"#
        );
        let candidates = store
            .persisted_classification("job")?
            .ok_or("missing durable classification")?;
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].authority.text.is_empty());
        assert!(candidates[0].context[0].text.is_empty());
        let conflict = store.persist_job_request_digest("job", "different-digest");
        assert_eq!(
            conflict.err().map(|error| error.code),
            Some("job_request_conflict")
        );
        Ok(())
    }
}
