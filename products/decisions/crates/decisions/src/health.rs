use decisions::api::{HealthReport, WorkerState};
use rusqlite::{OptionalExtension as _, params};

use crate::error::{AppResult, Context as _};
use crate::store::Store;

impl Store {
    pub(crate) fn worker_started(&self, now: i64) -> AppResult<()> {
        self.connection.execute(
            "INSERT INTO observer_worker(singleton, started_at, running_since) VALUES(1, ?1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET started_at=?1, running_since=?1", [now],
        ).context("database_write_failed", "cannot record worker start")?;
        Ok(())
    }

    pub(crate) fn worker_finished(
        &self,
        now: i64,
        worked: bool,
        error: Option<&str>,
    ) -> AppResult<()> {
        self.connection
            .execute(
                "UPDATE observer_worker SET finished_at=?1, running_since=NULL,
                idle_since=CASE WHEN ?2 IS NOT NULL THEN NULL
                    WHEN ?3 OR idle_since IS NULL THEN ?1 ELSE idle_since END,
                error_since=CASE WHEN ?2 IS NULL THEN NULL
                    WHEN error_code=?2 THEN error_since ELSE ?1 END,
                error_code=?2 WHERE singleton=1",
                params![now, error, worked],
            )
            .context("database_write_failed", "cannot record worker completion")?;
        Ok(())
    }

    pub(crate) fn worker_health(&self, now: i64, max_idle_seconds: i64) -> AppResult<HealthReport> {
        // Hold the lock through the read when idle. A live owner can finish while
        // we read; that is a normal successive observation, not a process census.
        let lock = match self.lock_observation_processing() {
            Ok(lock) => Some(lock),
            Err(error) if error.code == "observation_busy" => None,
            Err(error) => return Err(error),
        };
        let running = lock.is_none();
        let record = self
            .connection
            .query_row(
                "SELECT started_at, finished_at, running_since, idle_since, error_code, error_since
             FROM observer_worker WHERE singleton=1",
                [],
                |row| {
                    Ok(WorkerRecord {
                        started: row.get(0)?,
                        finished: row.get(1)?,
                        running: row.get(2)?,
                        idle: row.get(3)?,
                        error: row.get(4)?,
                        error_since: row.get(5)?,
                    })
                },
            )
            .optional()
            .context("database_read_failed", "cannot read worker activity")?;
        let mut report = HealthReport {
            ok: false,
            state: WorkerState::Unobserved,
            checked_at: now,
            state_since: None,
            state_duration_seconds: None,
            last_started_at: record.as_ref().map(|row| row.started),
            last_finished_at: record.as_ref().and_then(|row| row.finished),
            max_idle_seconds,
            error_code: None,
        };
        if running {
            report.ok = true;
            report.state = WorkerState::Working;
            report.state_since = record.as_ref().and_then(|row| row.running);
        } else if let Some(row) = record {
            if row.running.is_some() {
                report.state = WorkerState::Interrupted;
                // The run's start is known; its unrecorded exit time is not.
            } else if row.error.is_some() {
                report.state = WorkerState::Error;
                report.state_since = row.error_since;
                report.error_code = row.error;
            } else if let Some(finished) = row.finished {
                if now.saturating_sub(finished) > max_idle_seconds {
                    report.state = WorkerState::Stale;
                    report.state_since = Some(finished.saturating_add(max_idle_seconds));
                } else {
                    report.ok = true;
                    report.state = WorkerState::Idle;
                    report.state_since = row.idle;
                }
            }
        }
        report.state_duration_seconds = report
            .state_since
            .map(|since| now.saturating_sub(since).max(0));
        Ok(report)
    }
}

struct WorkerRecord {
    started: i64,
    finished: Option<i64>,
    running: Option<i64>,
    idle: Option<i64>,
    error: Option<String>,
    error_since: Option<i64>,
}
