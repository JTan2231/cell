use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::config::UsageConfig;
use crate::types::{ThreadTokenUsage, TokenUsageBreakdown};

const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = r"
CREATE TABLE runs (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    model_run_token          TEXT UNIQUE,
    annals_model_run_id      INTEGER,
    delivery_id              INTEGER,
    inbox_job_id             TEXT,
    attempt                  INTEGER,
    work_id                  INTEGER,
    work_label               TEXT,
    source_name              TEXT,
    base_revision            INTEGER,
    model                    TEXT,
    reasoning_effort         TEXT,
    observer_version         TEXT NOT NULL,
    codex_version            TEXT,
    thread_id                TEXT,
    turn_id                  TEXT,
    status                   TEXT NOT NULL,
    coverage                 TEXT NOT NULL,
    started_at_ms            INTEGER NOT NULL,
    completed_at_ms          INTEGER,
    input_tokens             INTEGER,
    cached_input_tokens      INTEGER,
    cache_write_input_tokens INTEGER,
    output_tokens            INTEGER,
    reasoning_output_tokens  INTEGER,
    total_tokens             INTEGER,
    model_context_window     INTEGER,
    exact_stream_complete    INTEGER NOT NULL DEFAULT 1
                                 CHECK (exact_stream_complete IN (0, 1)),
    error                    TEXT
);

CREATE INDEX runs_by_delivery ON runs(delivery_id, id);
CREATE INDEX runs_by_started ON runs(started_at_ms DESC, id DESC);

CREATE TABLE token_snapshots (
    run_id                    INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    sequence                  INTEGER NOT NULL,
    observed_at_ms            INTEGER NOT NULL,
    thread_id                 TEXT NOT NULL,
    turn_id                   TEXT NOT NULL,
    last_input_tokens         INTEGER NOT NULL,
    last_cached_input_tokens  INTEGER NOT NULL,
    last_cache_write_tokens   INTEGER NOT NULL,
    last_output_tokens        INTEGER NOT NULL,
    last_reasoning_tokens     INTEGER NOT NULL,
    last_total_tokens         INTEGER NOT NULL,
    total_input_tokens        INTEGER NOT NULL,
    total_cached_input_tokens INTEGER NOT NULL,
    total_cache_write_tokens  INTEGER NOT NULL,
    total_output_tokens       INTEGER NOT NULL,
    total_reasoning_tokens    INTEGER NOT NULL,
    total_tokens              INTEGER NOT NULL,
    model_context_window      INTEGER,
    PRIMARY KEY(run_id, sequence)
) WITHOUT ROWID;

CREATE TABLE response_usages (
    run_id                   INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    sequence                 INTEGER NOT NULL,
    observed_at_ms           INTEGER NOT NULL,
    response_id              TEXT NOT NULL,
    thread_id                TEXT NOT NULL,
    turn_id                  TEXT NOT NULL,
    input_tokens             INTEGER NOT NULL,
    cached_input_tokens      INTEGER NOT NULL,
    cache_write_input_tokens INTEGER NOT NULL,
    output_tokens            INTEGER NOT NULL,
    reasoning_output_tokens  INTEGER NOT NULL,
    total_tokens             INTEGER NOT NULL,
    PRIMARY KEY(run_id, sequence),
    UNIQUE(run_id, response_id)
) WITHOUT ROWID;

CREATE TABLE quota_snapshots (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id         INTEGER REFERENCES runs(id) ON DELETE SET NULL,
    observed_at_ms INTEGER NOT NULL,
    source         TEXT NOT NULL,
    payload         TEXT NOT NULL CHECK (json_valid(payload))
);

CREATE INDEX quota_snapshots_by_observed
    ON quota_snapshots(observed_at_ms DESC, id DESC);
";

#[derive(Debug)]
pub(crate) struct UsageDatabase {
    connection: Connection,
}

#[derive(Debug, Clone)]
pub(crate) struct RunIdentity {
    pub(crate) model_run_token: Option<String>,
    pub(crate) annals_model_run_id: Option<i64>,
    pub(crate) delivery_id: Option<i64>,
    pub(crate) inbox_job_id: Option<String>,
    pub(crate) attempt: Option<i64>,
    pub(crate) work_id: Option<i64>,
    pub(crate) work_label: Option<String>,
    pub(crate) source_name: Option<String>,
    pub(crate) base_revision: Option<i64>,
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredRun {
    pub(crate) id: i64,
    pub(crate) model_run_token: Option<String>,
    pub(crate) annals_model_run_id: Option<i64>,
    pub(crate) delivery_id: Option<i64>,
    pub(crate) inbox_job_id: Option<String>,
    pub(crate) attempt: Option<i64>,
    pub(crate) work_id: Option<i64>,
    pub(crate) work_label: Option<String>,
    pub(crate) source_name: Option<String>,
    pub(crate) base_revision: Option<i64>,
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) codex_version: Option<String>,
    pub(crate) thread_id: Option<String>,
    pub(crate) turn_id: Option<String>,
    pub(crate) status: String,
    pub(crate) coverage: String,
    pub(crate) started_at_ms: i64,
    pub(crate) completed_at_ms: Option<i64>,
    pub(crate) usage: Option<TokenUsageBreakdown>,
    pub(crate) model_context_window: Option<i64>,
    pub(crate) exact_response_stream_complete: bool,
    pub(crate) error: Option<String>,
    pub(crate) response_count: i64,
    pub(crate) responses: Vec<StoredResponseUsage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredResponseUsage {
    pub(crate) sequence: i64,
    pub(crate) observed_at_ms: i64,
    pub(crate) response_id: String,
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
    pub(crate) usage: TokenUsageBreakdown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredQuotaSnapshot {
    pub(crate) observed_at_ms: i64,
    pub(crate) source: String,
    pub(crate) snapshot: Value,
}

impl UsageDatabase {
    pub(crate) fn open(path: &Path) -> Result<Self, DatabaseError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|source| DatabaseError::CreateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let connection = Connection::open(path).map_err(|source| DatabaseError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match version {
            0 => {
                connection.execute_batch(SCHEMA)?;
                connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            }
            SCHEMA_VERSION => {}
            version => return Err(DatabaseError::UnsupportedSchema(version)),
        }
        Ok(Self { connection })
    }

    pub(crate) fn begin_run(
        &mut self,
        identity: &RunIdentity,
        codex_version: Option<&str>,
    ) -> Result<i64, DatabaseError> {
        self.connection.execute(
            "INSERT INTO runs(\
                 model_run_token, annals_model_run_id, delivery_id, inbox_job_id, attempt, \
                 work_id, work_label, source_name, base_revision, model, reasoning_effort, \
                 observer_version, codex_version, status, coverage, started_at_ms\
             ) VALUES(\
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, \
                 'running', 'pending', ?14\
             )",
            params![
                identity.model_run_token,
                identity.annals_model_run_id,
                identity.delivery_id,
                identity.inbox_job_id,
                identity.attempt,
                identity.work_id,
                identity.work_label,
                identity.source_name,
                identity.base_revision,
                identity.model,
                identity.reasoning_effort,
                env!("CARGO_PKG_VERSION"),
                codex_version,
                now_millis()?
            ],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    pub(crate) fn bind_thread(
        &self,
        run_id: i64,
        thread_id: &str,
        turn_id: Option<&str>,
    ) -> Result<(), DatabaseError> {
        self.connection.execute(
            "UPDATE runs SET thread_id = ?1, turn_id = COALESCE(?2, turn_id) WHERE id = ?3",
            params![thread_id, turn_id, run_id],
        )?;
        Ok(())
    }

    pub(crate) fn record_token_usage(
        &self,
        run_id: i64,
        thread_id: &str,
        turn_id: &str,
        usage: &ThreadTokenUsage,
    ) -> Result<(), DatabaseError> {
        let sequence = next_sequence(&self.connection, "token_snapshots", run_id)?;
        let observed_at = now_millis()?;
        let last = usage.last;
        let total = usage.total;
        self.connection.execute(
            "INSERT INTO token_snapshots(\
                 run_id, sequence, observed_at_ms, thread_id, turn_id, \
                 last_input_tokens, last_cached_input_tokens, last_cache_write_tokens, \
                 last_output_tokens, last_reasoning_tokens, last_total_tokens, \
                 total_input_tokens, total_cached_input_tokens, total_cache_write_tokens, \
                 total_output_tokens, total_reasoning_tokens, total_tokens, model_context_window\
             ) VALUES(\
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, \
                 ?15, ?16, ?17, ?18\
             )",
            params![
                run_id,
                sequence,
                observed_at,
                thread_id,
                turn_id,
                last.input_tokens,
                last.cached_input_tokens,
                last.cache_write_input_tokens,
                last.output_tokens,
                last.reasoning_output_tokens,
                last.total_tokens,
                total.input_tokens,
                total.cached_input_tokens,
                total.cache_write_input_tokens,
                total.output_tokens,
                total.reasoning_output_tokens,
                total.total_tokens,
                usage.model_context_window
            ],
        )?;
        let coverage = if last.is_consistent() && total.is_consistent() {
            "cumulative"
        } else {
            "invalid"
        };
        self.connection.execute(
            "UPDATE runs SET \
                 thread_id = ?1, turn_id = ?2, \
                 coverage = CASE \
                     WHEN coverage = 'invalid' OR ?3 = 'invalid' THEN 'invalid' \
                     ELSE 'cumulative' END, \
                 error = CASE \
                     WHEN ?3 = 'invalid' AND error IS NULL THEN \
                         'an upstream cumulative token snapshot was inconsistent' \
                     ELSE error END, \
                 input_tokens = ?4, cached_input_tokens = ?5, \
                 cache_write_input_tokens = ?6, output_tokens = ?7, \
                 reasoning_output_tokens = ?8, total_tokens = ?9, \
                 model_context_window = ?10 \
             WHERE id = ?11",
            params![
                thread_id,
                turn_id,
                coverage,
                total.input_tokens,
                total.cached_input_tokens,
                total.cache_write_input_tokens,
                total.output_tokens,
                total.reasoning_output_tokens,
                total.total_tokens,
                usage.model_context_window,
                run_id
            ],
        )?;
        Ok(())
    }

    pub(crate) fn record_response_usage(
        &self,
        run_id: i64,
        response_id: &str,
        thread_id: &str,
        turn_id: &str,
        usage: TokenUsageBreakdown,
    ) -> Result<(), DatabaseError> {
        if !usage.is_consistent() {
            self.record_exact_gap(
                run_id,
                "an upstream response reported inconsistent token usage",
            )?;
            return Ok(());
        }
        let sequence = next_sequence(&self.connection, "response_usages", run_id)?;
        self.connection.execute(
            "INSERT OR IGNORE INTO response_usages(\
                 run_id, sequence, observed_at_ms, response_id, thread_id, turn_id, \
                 input_tokens, cached_input_tokens, cache_write_input_tokens, \
                 output_tokens, reasoning_output_tokens, total_tokens\
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                run_id,
                sequence,
                now_millis()?,
                response_id,
                thread_id,
                turn_id,
                usage.input_tokens,
                usage.cached_input_tokens,
                usage.cache_write_input_tokens,
                usage.output_tokens,
                usage.reasoning_output_tokens,
                usage.total_tokens
            ],
        )?;
        Ok(())
    }

    pub(crate) fn record_quota_snapshot(
        &self,
        run_id: Option<i64>,
        source: &str,
        payload: &Value,
    ) -> Result<(), DatabaseError> {
        self.connection.execute(
            "INSERT INTO quota_snapshots(run_id, observed_at_ms, source, payload) \
             VALUES(?1, ?2, ?3, ?4)",
            params![
                run_id,
                now_millis()?,
                source,
                serde_json::to_string(payload)?
            ],
        )?;
        Ok(())
    }

    pub(crate) fn complete_run(
        &self,
        run_id: i64,
        status: &str,
        thread_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> Result<(), DatabaseError> {
        let response_totals = self.connection.query_row(
            "SELECT COUNT(*), \
                    COALESCE(SUM(input_tokens), 0), \
                    COALESCE(SUM(cached_input_tokens), 0), \
                    COALESCE(SUM(cache_write_input_tokens), 0), \
                    COALESCE(SUM(output_tokens), 0), \
                    COALESCE(SUM(reasoning_output_tokens), 0), \
                    COALESCE(SUM(total_tokens), 0) \
             FROM response_usages WHERE run_id = ?1",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    TokenUsageBreakdown {
                        input_tokens: row.get(1)?,
                        cached_input_tokens: row.get(2)?,
                        cache_write_input_tokens: row.get(3)?,
                        output_tokens: row.get(4)?,
                        reasoning_output_tokens: row.get(5)?,
                        total_tokens: row.get(6)?,
                    },
                ))
            },
        )?;
        let (response_count, exact) = response_totals;
        let (coverage, exact_stream_complete, cumulative) = self.connection.query_row(
            "SELECT coverage, exact_stream_complete, input_tokens, cached_input_tokens, \
                        cache_write_input_tokens, output_tokens, \
                        reasoning_output_tokens, total_tokens \
                 FROM runs WHERE id = ?1",
            [run_id],
            |row| {
                let input_tokens: Option<i64> = row.get(2)?;
                let cumulative = if let Some(input_tokens) = input_tokens {
                    Some(TokenUsageBreakdown {
                        input_tokens,
                        cached_input_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                        cache_write_input_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                        output_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                        reasoning_output_tokens: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                        total_tokens: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                    })
                } else {
                    None
                };
                Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?, cumulative))
            },
        )?;

        let exact_available = exact_stream_complete && response_count > 0 && exact.is_consistent();
        let cumulative_available = coverage == "cumulative" && cumulative.is_some();
        if exact_available
            && (!cumulative_available || cumulative.is_some_and(|cumulative| cumulative == exact))
        {
            self.connection.execute(
                "UPDATE runs SET \
                     input_tokens = ?1, cached_input_tokens = ?2, \
                     cache_write_input_tokens = ?3, output_tokens = ?4, \
                     reasoning_output_tokens = ?5, total_tokens = ?6, \
                     coverage = 'exact' \
                 WHERE id = ?7",
                params![
                    exact.input_tokens,
                    exact.cached_input_tokens,
                    exact.cache_write_input_tokens,
                    exact.output_tokens,
                    exact.reasoning_output_tokens,
                    exact.total_tokens,
                    run_id
                ],
            )?;
        } else if cumulative_available {
            self.connection.execute(
                "UPDATE runs SET coverage = 'cumulative' WHERE id = ?1",
                [run_id],
            )?;
            if exact_stream_complete && response_count == 0 {
                self.record_exact_gap(run_id, "no exact response usage events were observed")?;
            } else if exact_stream_complete {
                self.record_exact_gap(
                    run_id,
                    "exact response totals differed from the final cumulative snapshot",
                )?;
            }
        } else {
            self.connection.execute(
                "UPDATE runs SET coverage = 'gap', exact_stream_complete = 0, \
                     error = COALESCE(error, 'no usable terminal token telemetry was observed') \
                 WHERE id = ?1",
                [run_id],
            )?;
        }
        self.connection.execute(
            "UPDATE runs SET \
                 status = ?1, thread_id = COALESCE(?2, thread_id), \
                 turn_id = COALESCE(?3, turn_id), completed_at_ms = ?4, \
                 coverage = CASE WHEN coverage = 'pending' THEN 'gap' ELSE coverage END \
             WHERE id = ?5",
            params![status, thread_id, turn_id, now_millis()?, run_id],
        )?;
        Ok(())
    }

    pub(crate) fn finish_incomplete_run(
        &self,
        run_id: i64,
        status: &str,
    ) -> Result<(), DatabaseError> {
        self.connection.execute(
            "UPDATE runs SET status = ?1, completed_at_ms = ?2, coverage = 'gap', \
                 exact_stream_complete = 0, \
                 error = COALESCE(error, 'the proxy ended before turn completion') \
             WHERE id = ?3",
            params![status, now_millis()?, run_id],
        )?;
        Ok(())
    }

    pub(crate) fn record_exact_gap(&self, run_id: i64, warning: &str) -> Result<(), DatabaseError> {
        self.connection.execute(
            "UPDATE runs SET exact_stream_complete = 0, \
                 error = CASE WHEN error IS NULL THEN ?1 ELSE error || '; ' || ?1 END \
             WHERE id = ?2",
            params![warning, run_id],
        )?;
        Ok(())
    }

    pub(crate) fn runs(&self, limit: usize) -> Result<Vec<StoredRun>, DatabaseError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut runs = {
            let mut statement = self.connection.prepare(
                "SELECT \
                 r.id, r.model_run_token, r.annals_model_run_id, r.delivery_id, \
                 r.inbox_job_id, r.attempt, r.work_id, r.work_label, r.source_name, \
                 r.base_revision, r.model, r.reasoning_effort, r.codex_version, \
                 r.thread_id, r.turn_id, r.status, r.coverage, r.started_at_ms, \
                 r.completed_at_ms, r.input_tokens, r.cached_input_tokens, \
                 r.cache_write_input_tokens, r.output_tokens, \
                 r.reasoning_output_tokens, r.total_tokens, r.model_context_window, \
                 r.exact_stream_complete, r.error \
             FROM runs AS r \
             ORDER BY r.started_at_ms DESC, r.id DESC LIMIT ?1",
            )?;
            let rows = statement.query_map([limit], stored_run_from_row)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for run in &mut runs {
            run.responses = self.response_usages(run.id)?;
            run.response_count = i64::try_from(run.responses.len()).unwrap_or(i64::MAX);
        }
        Ok(runs)
    }

    fn response_usages(&self, run_id: i64) -> Result<Vec<StoredResponseUsage>, DatabaseError> {
        let mut statement = self.connection.prepare(
            "SELECT sequence, observed_at_ms, response_id, thread_id, turn_id, \
                    input_tokens, cached_input_tokens, cache_write_input_tokens, \
                    output_tokens, reasoning_output_tokens, total_tokens \
             FROM response_usages WHERE run_id = ?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map([run_id], |row| {
            Ok(StoredResponseUsage {
                sequence: row.get(0)?,
                observed_at_ms: row.get(1)?,
                response_id: row.get(2)?,
                thread_id: row.get(3)?,
                turn_id: row.get(4)?,
                usage: TokenUsageBreakdown {
                    input_tokens: row.get(5)?,
                    cached_input_tokens: row.get(6)?,
                    cache_write_input_tokens: row.get(7)?,
                    output_tokens: row.get(8)?,
                    reasoning_output_tokens: row.get(9)?,
                    total_tokens: row.get(10)?,
                },
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub(crate) fn latest_account_snapshot(
        &self,
    ) -> Result<Option<StoredQuotaSnapshot>, DatabaseError> {
        let row: Option<(i64, String, String)> = self
            .connection
            .query_row(
                "SELECT observed_at_ms, source, payload FROM quota_snapshots \
                 WHERE source = 'account/rateLimits/read' \
                 ORDER BY observed_at_ms DESC, id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        row.map(|(observed_at_ms, source, payload)| {
            Ok(StoredQuotaSnapshot {
                observed_at_ms,
                source,
                snapshot: serde_json::from_str(&payload)?,
            })
        })
        .transpose()
    }
}

impl RunIdentity {
    pub(crate) fn resolve(
        config: &UsageConfig,
        token: Option<&str>,
    ) -> Result<Self, DatabaseError> {
        let Some(token) = token else {
            return Ok(Self::unattributed());
        };
        let connection = Connection::open_with_flags(
            &config.library,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let model_run = connection
            .query_row(
                "SELECT m.id, m.work_id, w.label, m.base_revision, m.model, \
                        m.reasoning_effort \
                 FROM model_runs AS m JOIN works AS w ON w.id = m.work_id \
                 WHERE m.token = ?1",
                [token],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((model_run_id, work_id, work_label, base_revision, model, effort)) = model_run
        else {
            return Ok(Self {
                model_run_token: Some(token.to_owned()),
                ..Self::unattributed()
            });
        };
        let receipt = receipt_for_token(&config.spool, token)?;
        let (delivery_id, inbox_job_id, attempt) = receipt.map_or((None, None, None), |receipt| {
            (
                receipt.ingestion_id,
                Some(receipt.id),
                Some(i64::from(receipt.attempts)),
            )
        });
        let source_name = delivery_id
            .map(|delivery_id| {
                connection
                    .query_row(
                        "SELECT source_name FROM ingestions WHERE id = ?1",
                        [delivery_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
            })
            .transpose()?
            .flatten();
        Ok(Self {
            model_run_token: Some(token.to_owned()),
            annals_model_run_id: Some(model_run_id),
            delivery_id,
            inbox_job_id,
            attempt,
            work_id: Some(work_id),
            work_label: Some(work_label),
            source_name,
            base_revision: Some(base_revision),
            model: Some(model),
            reasoning_effort: Some(effort),
        })
    }

    pub(crate) fn unattributed() -> Self {
        Self {
            model_run_token: None,
            annals_model_run_id: None,
            delivery_id: None,
            inbox_job_id: None,
            attempt: None,
            work_id: None,
            work_label: None,
            source_name: None,
            base_revision: None,
            model: None,
            reasoning_effort: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct JobReceipt {
    id: String,
    attempts: u32,
    ingestion_id: Option<i64>,
    model_run_token: Option<String>,
}

fn receipt_for_token(spool: &Path, token: &str) -> Result<Option<JobReceipt>, DatabaseError> {
    let processing = spool.join("processing");
    let entries = match fs::read_dir(&processing) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(DatabaseError::ReadDirectory {
                path: processing,
                source,
            });
        }
    };
    for entry in entries {
        let path = entry?.path().join("job.json");
        let document = match fs::read_to_string(&path) {
            Ok(document) => document,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(DatabaseError::ReadReceipt { path, source }),
        };
        let receipt: JobReceipt = serde_json::from_str(&document)?;
        if receipt.model_run_token.as_deref() == Some(token) {
            return Ok(Some(receipt));
        }
    }
    Ok(None)
}

fn stored_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredRun> {
    let input_tokens: Option<i64> = row.get(19)?;
    let usage = if let Some(input_tokens) = input_tokens {
        Some(TokenUsageBreakdown {
            input_tokens,
            cached_input_tokens: row.get::<_, Option<i64>>(20)?.unwrap_or(0),
            cache_write_input_tokens: row.get::<_, Option<i64>>(21)?.unwrap_or(0),
            output_tokens: row.get::<_, Option<i64>>(22)?.unwrap_or(0),
            reasoning_output_tokens: row.get::<_, Option<i64>>(23)?.unwrap_or(0),
            total_tokens: row.get::<_, Option<i64>>(24)?.unwrap_or(0),
        })
    } else {
        None
    };
    Ok(StoredRun {
        id: row.get(0)?,
        model_run_token: row.get(1)?,
        annals_model_run_id: row.get(2)?,
        delivery_id: row.get(3)?,
        inbox_job_id: row.get(4)?,
        attempt: row.get(5)?,
        work_id: row.get(6)?,
        work_label: row.get(7)?,
        source_name: row.get(8)?,
        base_revision: row.get(9)?,
        model: row.get(10)?,
        reasoning_effort: row.get(11)?,
        codex_version: row.get(12)?,
        thread_id: row.get(13)?,
        turn_id: row.get(14)?,
        status: row.get(15)?,
        coverage: row.get(16)?,
        started_at_ms: row.get(17)?,
        completed_at_ms: row.get(18)?,
        usage,
        model_context_window: row.get(25)?,
        exact_response_stream_complete: row.get(26)?,
        error: row.get(27)?,
        response_count: 0,
        responses: Vec::new(),
    })
}

fn next_sequence(connection: &Connection, table: &str, run_id: i64) -> Result<i64, DatabaseError> {
    let sql = format!("SELECT COALESCE(MAX(sequence), -1) + 1 FROM {table} WHERE run_id = ?1");
    connection
        .query_row(&sql, [run_id], |row| row.get(0))
        .map_err(Into::into)
}

pub(crate) fn now_millis() -> Result<i64, DatabaseError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(DatabaseError::Clock)?;
    i64::try_from(duration.as_millis()).map_err(|_| DatabaseError::ClockOverflow)
}

#[derive(Debug, Error)]
pub(crate) enum DatabaseError {
    #[error("unable to create telemetry directory {path}: {source}")]
    CreateDirectory {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unable to open telemetry database {path}: {source}")]
    Open {
        path: std::path::PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("unsupported telemetry database schema version {0}")]
    UnsupportedSchema(i64),
    #[error("unable to read inbox directory {path}: {source}")]
    ReadDirectory {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unable to read job receipt {path}: {source}")]
    ReadReceipt {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("system clock is before the Unix epoch: {0}")]
    Clock(std::time::SystemTimeError),
    #[error("system time does not fit in SQLite integer milliseconds")]
    ClockOverflow,
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::{RunIdentity, UsageDatabase};
    use crate::types::{ThreadTokenUsage, TokenUsageBreakdown};

    #[test]
    fn latest_cumulative_snapshot_becomes_the_run_total() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let mut database = UsageDatabase::open(&directory.path().join("usage.db"))?;
        let identity = RunIdentity::unattributed();
        let run_id = database.begin_run(&identity, Some("codex-cli test"))?;
        let first = breakdown(100, 20);
        database.record_token_usage(
            run_id,
            "thread",
            "turn",
            &ThreadTokenUsage {
                last: first,
                total: first,
                model_context_window: Some(1000),
            },
        )?;
        let second = breakdown(250, 50);
        database.record_token_usage(
            run_id,
            "thread",
            "turn",
            &ThreadTokenUsage {
                last: breakdown(150, 30),
                total: second,
                model_context_window: Some(1000),
            },
        )?;
        database.complete_run(run_id, "completed", Some("thread"), Some("turn"))?;

        let runs = database.runs(10)?;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].usage, Some(second));
        assert_eq!(runs[0].coverage, "cumulative");
        Ok(())
    }

    #[test]
    fn exact_responses_are_exposed_and_reconciled() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut database = UsageDatabase::open(&directory.path().join("usage.db"))?;
        let run_id = database.begin_run(&RunIdentity::unattributed(), None)?;
        let first = breakdown(100, 20);
        let second = breakdown(150, 30);
        database.record_response_usage(run_id, "response-1", "thread", "turn", first)?;
        database.record_response_usage(run_id, "response-2", "thread", "turn", second)?;
        database.record_token_usage(
            run_id,
            "thread",
            "turn",
            &ThreadTokenUsage {
                last: second,
                total: breakdown(250, 50),
                model_context_window: Some(1_000),
            },
        )?;
        database.complete_run(run_id, "completed", Some("thread"), Some("turn"))?;

        let run = &database.runs(1)?[0];
        assert_eq!(run.coverage, "exact");
        assert_eq!(run.usage, Some(breakdown(250, 50)));
        assert_eq!(run.response_count, 2);
        assert_eq!(run.responses[0].response_id, "response-1");
        assert_eq!(run.responses[1].usage, second);
        Ok(())
    }

    #[test]
    fn an_exact_stream_gap_uses_a_consistent_cumulative_fallback()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut database = UsageDatabase::open(&directory.path().join("usage.db"))?;
        let run_id = database.begin_run(&RunIdentity::unattributed(), None)?;
        database.record_exact_gap(run_id, "one response omitted usage")?;
        let cumulative = breakdown(250, 50);
        database.record_token_usage(
            run_id,
            "thread",
            "turn",
            &ThreadTokenUsage {
                last: cumulative,
                total: cumulative,
                model_context_window: None,
            },
        )?;
        database.complete_run(run_id, "completed", Some("thread"), Some("turn"))?;

        let run = &database.runs(1)?[0];
        assert_eq!(run.coverage, "cumulative");
        assert_eq!(run.usage, Some(cumulative));
        assert!(!run.exact_response_stream_complete);
        assert_eq!(run.error.as_deref(), Some("one response omitted usage"));
        Ok(())
    }

    #[test]
    fn a_cumulative_mismatch_is_preferred_and_disclosed() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let mut database = UsageDatabase::open(&directory.path().join("usage.db"))?;
        let run_id = database.begin_run(&RunIdentity::unattributed(), None)?;
        database.record_response_usage(
            run_id,
            "response-1",
            "thread",
            "turn",
            breakdown(100, 20),
        )?;
        let cumulative = breakdown(250, 50);
        database.record_token_usage(
            run_id,
            "thread",
            "turn",
            &ThreadTokenUsage {
                last: cumulative,
                total: cumulative,
                model_context_window: None,
            },
        )?;
        database.complete_run(run_id, "completed", Some("thread"), Some("turn"))?;

        let run = &database.runs(1)?[0];
        assert_eq!(run.coverage, "cumulative");
        assert_eq!(run.usage, Some(cumulative));
        assert_eq!(run.response_count, 1);
        assert_eq!(
            run.error.as_deref(),
            Some("exact response totals differed from the final cumulative snapshot")
        );
        Ok(())
    }

    #[test]
    fn an_incomplete_process_is_never_promoted_to_exact() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let mut database = UsageDatabase::open(&directory.path().join("usage.db"))?;
        let run_id = database.begin_run(&RunIdentity::unattributed(), None)?;
        database.record_response_usage(
            run_id,
            "response-1",
            "thread",
            "turn",
            breakdown(100, 20),
        )?;
        database.finish_incomplete_run(run_id, "codex-exit-1")?;

        let run = &database.runs(1)?[0];
        assert_eq!(run.coverage, "gap");
        assert_eq!(run.status, "codex-exit-1");
        assert!(run.usage.is_none());
        assert!(!run.exact_response_stream_complete);
        assert_eq!(
            run.error.as_deref(),
            Some("the proxy ended before turn completion")
        );
        Ok(())
    }

    fn breakdown(input: i64, output: i64) -> TokenUsageBreakdown {
        TokenUsageBreakdown {
            input_tokens: input,
            cached_input_tokens: input / 2,
            cache_write_input_tokens: 0,
            output_tokens: output,
            reasoning_output_tokens: output / 2,
            total_tokens: input + output,
        }
    }
}
