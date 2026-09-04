use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::contracts::ManagedSubmission;
use crate::error::{AppError, AppResult};
use crate::git::{scopes_overlap, validate_scopes};
use crate::model::{
    AttemptState, AttemptView, CandidateView, DelegationSubmission, Disposition, DocumentView,
    GateResult, GateSpec, HandoffOutcome, NewRun, OpaqueMarkdown, PacketState, PacketView,
    PathScope, Role, RunState, RunView, sha256_hex,
};

const SCHEMA: &str = r"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS runs (
  id TEXT PRIMARY KEY,
  request_key TEXT UNIQUE,
  repository TEXT NOT NULL,
  source_commit TEXT NOT NULL,
  state TEXT NOT NULL,
  contract_set_sha256 TEXT NOT NULL,
  input_bundle_sha256 TEXT NOT NULL,
  brief_document_id TEXT NOT NULL,
  terminology_document_id TEXT NOT NULL,
  remediation_limit INTEGER NOT NULL CHECK(remediation_limit BETWEEN 0 AND 8),
  final_candidate_id TEXT,
  final_ref TEXT,
  cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK(cancel_requested IN (0,1)),
  detail TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS documents (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id),
  kind TEXT NOT NULL,
  subject_id TEXT,
  ordinal INTEGER NOT NULL,
  markdown TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  UNIQUE(run_id, kind, subject_id, ordinal)
) STRICT;

CREATE TABLE IF NOT EXISTS packets (
  run_id TEXT NOT NULL REFERENCES runs(id),
  packet_key TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  state TEXT NOT NULL,
  contract_ids_json TEXT NOT NULL,
  depends_on_json TEXT NOT NULL,
  path_scopes_json TEXT NOT NULL,
  plan_document_id TEXT NOT NULL REFERENCES documents(id),
  current_candidate_id TEXT,
  remediation_round INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(run_id, packet_key),
  UNIQUE(run_id, ordinal)
) STRICT;

CREATE TABLE IF NOT EXISTS attempts (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id),
  role TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  round INTEGER NOT NULL,
  targeted INTEGER NOT NULL CHECK(targeted IN (0,1)),
  state TEXT NOT NULL,
  nucleus_job_id TEXT NOT NULL UNIQUE,
  request_bytes BLOB NOT NULL,
  request_sha256 TEXT NOT NULL,
  toolset_name TEXT NOT NULL,
  workspace_path TEXT NOT NULL,
  base_commit TEXT,
  allowed_scopes_json TEXT NOT NULL,
  admitted INTEGER NOT NULL DEFAULT 0 CHECK(admitted IN (0,1)),
  tool_after INTEGER NOT NULL DEFAULT 0,
  domain_document_id TEXT REFERENCES documents(id),
  disposition TEXT,
  predecessor_attempt_id TEXT REFERENCES attempts(id),
  detail TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(run_id, role, subject_id, round, id)
) STRICT;
CREATE INDEX IF NOT EXISTS attempts_subject_idx
  ON attempts(run_id, role, subject_id, round, created_at);

CREATE TABLE IF NOT EXISTS candidates (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id),
  subject_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  round INTEGER NOT NULL,
  base_commit TEXT NOT NULL,
  commit_oid TEXT NOT NULL,
  ref_name TEXT NOT NULL UNIQUE,
  handoff_document_id TEXT NOT NULL REFERENCES documents(id),
  attempt_id TEXT NOT NULL UNIQUE REFERENCES attempts(id),
  created_at INTEGER NOT NULL,
  UNIQUE(run_id, subject_id, kind, round)
) STRICT;

CREATE TABLE IF NOT EXISTS gate_specs (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id),
  ordinal INTEGER NOT NULL,
  name TEXT NOT NULL,
  command TEXT NOT NULL,
  UNIQUE(run_id, ordinal),
  UNIQUE(run_id, name)
) STRICT;

CREATE TABLE IF NOT EXISTS gate_results (
  id TEXT PRIMARY KEY,
  gate_id TEXT NOT NULL REFERENCES gate_specs(id),
  candidate_id TEXT NOT NULL REFERENCES candidates(id),
  round INTEGER NOT NULL,
  exit_code INTEGER NOT NULL,
  output TEXT NOT NULL,
  output_truncated INTEGER NOT NULL CHECK(output_truncated IN (0,1)),
  created_at INTEGER NOT NULL,
  UNIQUE(gate_id, candidate_id)
) STRICT;

CREATE TABLE IF NOT EXISTS tool_receipts (
  job_id TEXT NOT NULL,
  call_id TEXT NOT NULL,
  arguments_sha256 TEXT NOT NULL,
  result_schema_id TEXT NOT NULL,
  result_json TEXT NOT NULL,
  is_error INTEGER NOT NULL CHECK(is_error IN (0,1)),
  created_at INTEGER NOT NULL,
  PRIMARY KEY(job_id, call_id)
) STRICT;
";

#[derive(Clone, Debug)]
pub struct Store {
    path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct NewAttempt<'a> {
    pub run_id: &'a str,
    pub role: Role,
    pub subject_id: &'a str,
    pub round: u32,
    pub targeted: bool,
    pub nucleus_job_id: &'a str,
    pub request_bytes: &'a [u8],
    pub request_sha256: &'a str,
    pub toolset_name: &'a str,
    pub workspace_path: &'a str,
    pub base_commit: Option<&'a str>,
    pub allowed_scopes: &'a [PathScope],
    pub predecessor_attempt_id: Option<&'a str>,
}

#[derive(Clone, Debug)]
pub struct Receipt {
    pub arguments_sha256: String,
    pub result_schema_id: String,
    pub result_json: String,
    pub is_error: bool,
}

impl Store {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn initialize(&self) -> AppResult<()> {
        let parent = self.path.parent().ok_or_else(|| {
            AppError::new(
                "database_path_invalid",
                "database path has no parent directory",
            )
        })?;
        let parent_existed = parent.exists();
        fs::create_dir_all(parent)?;
        if !parent_existed {
            set_directory_private(parent)?;
        }
        if self
            .path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(AppError::new(
                "database_symlink_refused",
                "Vizier database must not be a symbolic link",
            ));
        }
        let connection = self.connect_unchecked()?;
        connection.execute_batch(SCHEMA)?;
        connection.execute(
            "INSERT INTO meta(key, value) VALUES('schema_version', '1') ON CONFLICT(key) DO NOTHING",
            [],
        )?;
        drop(connection);
        self.secure_files()?;
        Ok(())
    }

    pub fn check_ready(&self) -> AppResult<()> {
        if !self.path.is_file() {
            return Err(AppError::new(
                "database_not_initialized",
                format!("run `vizier --database {} init` first", self.path.display()),
            ));
        }
        let connection = self.connect()?;
        let version: String = connection.query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )?;
        if version != "1" {
            return Err(AppError::new(
                "database_schema_unsupported",
                format!("unsupported Vizier schema version {version}"),
            ));
        }
        Ok(())
    }

    pub fn create_run(&self, input: &NewRun) -> AppResult<RunView> {
        if input.contracts.is_empty() {
            return Err(AppError::new(
                "contract_units_empty",
                "at least one ordered contract unit is required",
            ));
        }
        let mut connection = self.connect()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let input_digest = input_bundle_digest(input)?;
        if let Some(key) = &input.request_key
            && let Some(run) = run_by_request_key(&transaction, key)?
        {
            if run.input_bundle_sha256 != input_digest {
                return Err(AppError::new(
                    "request_key_conflict",
                    "request key already belongs to a different exact Vizier input bundle",
                ));
            }
            transaction.commit()?;
            return Ok(run);
        }
        let active: i64 = transaction.query_row(
            "SELECT count(*) FROM runs WHERE state NOT IN ('succeeded','needs_attention','cancelled')",
            [],
            |row| row.get(0),
        )?;
        if active != 0 {
            return Err(AppError::new(
                "run_already_active",
                "Vizier v0.1 permits one active run; resume, cancel, or resolve it first",
            ));
        }
        let now = now();
        let brief_id = document_id();
        let terminology_id = document_id();
        let contract_digest = contract_set_digest(&input.contracts);
        transaction.execute(
            "INSERT INTO runs(id,request_key,repository,source_commit,state,contract_set_sha256,input_bundle_sha256,brief_document_id,terminology_document_id,remediation_limit,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                input.id,
                input.request_key,
                input.repository,
                input.source_commit,
                RunState::Queued.as_str(),
                contract_digest,
                input_digest,
                brief_id,
                terminology_id,
                input.remediation_limit,
                now,
                now,
            ],
        )?;
        insert_document(
            &transaction,
            &brief_id,
            &input.id,
            "brief",
            None,
            0,
            &input.brief,
            now,
        )?;
        insert_document(
            &transaction,
            &terminology_id,
            &input.id,
            "terminology",
            None,
            0,
            &input.terminology,
            now,
        )?;
        for (ordinal, (unit_id, markdown)) in input.contracts.iter().enumerate() {
            let ordinal = checked_ordinal(ordinal, "contract unit")?;
            insert_document(
                &transaction,
                &document_id(),
                &input.id,
                "contract_unit",
                Some(unit_id),
                ordinal,
                markdown,
                now,
            )?;
        }
        for (ordinal, (name, command)) in input.gates.iter().enumerate() {
            let ordinal = checked_ordinal(ordinal, "gate")?;
            transaction.execute(
                "INSERT INTO gate_specs(id,run_id,ordinal,name,command) VALUES(?,?,?,?,?)",
                params![
                    format!("gate-{}", Uuid::now_v7()),
                    input.id,
                    ordinal,
                    name,
                    command,
                ],
            )?;
        }
        transaction.commit()?;
        self.run(&input.id)
    }

    pub fn run(&self, run_id: &str) -> AppResult<RunView> {
        let connection = self.connect()?;
        query_run(&connection, "WHERE id=?", params![run_id])?.ok_or_else(|| {
            AppError::new(
                "run_not_found",
                format!("Vizier run {run_id} does not exist"),
            )
        })
    }

    pub fn list_runs(&self) -> AppResult<Vec<RunView>> {
        let connection = self.connect()?;
        let mut statement =
            connection.prepare(&format!("{} ORDER BY created_at DESC", run_select("")))?;
        let values = statement
            .query_map([], row_run)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn set_run_state(
        &self,
        run_id: &str,
        state: RunState,
        detail: Option<&str>,
    ) -> AppResult<()> {
        let connection = self.connect()?;
        let count = connection.execute(
            "UPDATE runs SET state=?,detail=?,updated_at=? WHERE id=?",
            params![state.as_str(), detail, now(), run_id],
        )?;
        require_changed(count, "run_not_found", "run does not exist")
    }

    pub fn request_cancel(&self, run_id: &str) -> AppResult<()> {
        let connection = self.connect()?;
        let count = connection.execute(
            "UPDATE runs SET cancel_requested=1,state='cancelled',detail='cancellation requested',updated_at=? WHERE id=? AND state NOT IN ('succeeded','cancelled')",
            params![now(), run_id],
        )?;
        if count == 0 {
            let run = self.run(run_id)?;
            if run.state == RunState::Succeeded {
                return Err(AppError::new(
                    "run_already_succeeded",
                    "a succeeded run cannot be cancelled",
                ));
            }
        }
        Ok(())
    }

    pub fn finish_run(&self, run_id: &str, candidate_id: &str, final_ref: &str) -> AppResult<()> {
        let connection = self.connect()?;
        let count = connection.execute(
            "UPDATE runs SET state='succeeded',final_candidate_id=?,final_ref=?,detail=NULL,updated_at=? WHERE id=? AND cancel_requested=0",
            params![candidate_id, final_ref, now(), run_id],
        )?;
        require_changed(
            count,
            "run_cancelled",
            "run was cancelled before completion",
        )
    }

    pub fn documents(&self, run_id: &str, kind: &str) -> AppResult<Vec<DocumentView>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT id,run_id,kind,subject_id,ordinal,markdown,sha256,created_at FROM documents WHERE run_id=? AND kind=? ORDER BY ordinal,id",
        )?;
        let rows = statement
            .query_map(params![run_id, kind], row_document)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn document_for_subject(
        &self,
        run_id: &str,
        kind: &str,
        subject_id: &str,
        ordinal: u32,
    ) -> AppResult<Option<DocumentView>> {
        let connection = self.connect()?;
        connection
            .query_row(
                "SELECT id,run_id,kind,subject_id,ordinal,markdown,sha256,created_at FROM documents WHERE run_id=? AND kind=? AND subject_id=? AND ordinal=?",
                params![run_id, kind, subject_id, ordinal],
                row_document,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn document(&self, document_id: &str) -> AppResult<DocumentView> {
        let connection = self.connect()?;
        connection.query_row(
            "SELECT id,run_id,kind,subject_id,ordinal,markdown,sha256,created_at FROM documents WHERE id=?",
            params![document_id],
            row_document,
        ).optional()?.ok_or_else(|| AppError::new("document_not_found", "document does not exist"))
    }

    pub fn record_document(
        &self,
        run_id: &str,
        kind: &str,
        subject_id: Option<&str>,
        ordinal: u32,
        markdown: &OpaqueMarkdown,
    ) -> AppResult<DocumentView> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = transaction.query_row(
            "SELECT id,run_id,kind,subject_id,ordinal,markdown,sha256,created_at FROM documents WHERE run_id=? AND kind=? AND subject_id IS ? AND ordinal=?",
            params![run_id, kind, subject_id, ordinal],
            row_document,
        ).optional()? {
            if existing.sha256 != markdown.sha256() || existing.markdown.as_bytes() != markdown.as_bytes() {
                return Err(AppError::new("document_conflict", "an immutable Markdown document already exists with different bytes"));
            }
            transaction.commit()?;
            return Ok(existing);
        }
        let id = document_id();
        insert_document(
            &transaction,
            &id,
            run_id,
            kind,
            subject_id,
            ordinal,
            markdown,
            now(),
        )?;
        transaction.commit()?;
        self.document(&id)
    }

    pub fn record_delegation(
        &self,
        run_id: &str,
        submission: &DelegationSubmission,
    ) -> AppResult<()> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: i64 = transaction.query_row(
            "SELECT count(*) FROM packets WHERE run_id=?",
            params![run_id],
            |row| row.get(0),
        )?;
        if existing != 0 {
            return Err(AppError::new(
                "delegation_already_recorded",
                "the run already has an immutable delegation plan",
            ));
        }
        let overview = OpaqueMarkdown::from_text(submission.overview_markdown.clone())?;
        let overview_id = document_id();
        insert_document(
            &transaction,
            &overview_id,
            run_id,
            "delegation_plan",
            None,
            0,
            &overview,
            now(),
        )?;
        for (ordinal, packet) in submission.packets.iter().enumerate() {
            let ordinal = checked_ordinal(ordinal, "packet")?;
            let markdown = OpaqueMarkdown::from_text(packet.plan_markdown.clone())?;
            let document_id = document_id();
            insert_document(
                &transaction,
                &document_id,
                run_id,
                "packet_plan",
                Some(&packet.packet_key),
                0,
                &markdown,
                now(),
            )?;
            transaction.execute(
                "INSERT INTO packets(run_id,packet_key,ordinal,state,contract_ids_json,depends_on_json,path_scopes_json,plan_document_id) VALUES(?,?,?,?,?,?,?,?)",
                params![
                    run_id,
                    packet.packet_key,
                    ordinal,
                    PacketState::Planned.as_str(),
                    serde_json::to_string(&packet.contract_unit_ids)?,
                    serde_json::to_string(&packet.depends_on)?,
                    serde_json::to_string(&packet.path_scopes)?,
                    document_id,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn packets(&self, run_id: &str) -> AppResult<Vec<PacketView>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT run_id,packet_key,ordinal,state,contract_ids_json,depends_on_json,path_scopes_json,plan_document_id,current_candidate_id,remediation_round FROM packets WHERE run_id=? ORDER BY ordinal",
        )?;
        let mut rows = statement.query(params![run_id])?;
        let mut values = Vec::new();
        while let Some(row) = rows.next()? {
            values.push(row_packet(row)?);
        }
        Ok(values)
    }

    pub fn packet(&self, run_id: &str, key: &str) -> AppResult<PacketView> {
        let connection = self.connect()?;
        connection.query_row(
            "SELECT run_id,packet_key,ordinal,state,contract_ids_json,depends_on_json,path_scopes_json,plan_document_id,current_candidate_id,remediation_round FROM packets WHERE run_id=? AND packet_key=?",
            params![run_id, key],
            row_packet,
        ).optional()?.ok_or_else(|| AppError::new("packet_not_found", format!("packet {key} does not exist")))
    }

    pub fn set_packet_state(
        &self,
        run_id: &str,
        key: &str,
        state: PacketState,
        candidate_id: Option<&str>,
        round: u32,
    ) -> AppResult<()> {
        let connection = self.connect()?;
        let count = connection.execute(
            "UPDATE packets SET state=?,current_candidate_id=COALESCE(?,current_candidate_id),remediation_round=? WHERE run_id=? AND packet_key=?",
            params![state.as_str(), candidate_id, round, run_id, key],
        )?;
        require_changed(count, "packet_not_found", "packet does not exist")
    }

    pub fn create_attempt(&self, attempt: &NewAttempt<'_>) -> AppResult<AttemptView> {
        let id = format!("attempt-{}", Uuid::now_v7());
        let now = now();
        let connection = self.connect()?;
        connection.execute(
            "INSERT INTO attempts(id,run_id,role,subject_id,round,targeted,state,nucleus_job_id,request_bytes,request_sha256,toolset_name,workspace_path,base_commit,allowed_scopes_json,predecessor_attempt_id,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                id,
                attempt.run_id,
                attempt.role.as_str(),
                attempt.subject_id,
                attempt.round,
                i64::from(attempt.targeted),
                AttemptState::Prepared.as_str(),
                attempt.nucleus_job_id,
                attempt.request_bytes,
                attempt.request_sha256,
                attempt.toolset_name,
                attempt.workspace_path,
                attempt.base_commit,
                serde_json::to_string(attempt.allowed_scopes)?,
                attempt.predecessor_attempt_id,
                now,
                now,
            ],
        )?;
        self.attempt(&id)
    }

    pub fn attempt(&self, attempt_id: &str) -> AppResult<AttemptView> {
        let connection = self.connect()?;
        connection
            .query_row(
                &attempt_select("WHERE id=?"),
                params![attempt_id],
                row_attempt,
            )
            .optional()?
            .ok_or_else(|| {
                AppError::new(
                    "attempt_not_found",
                    format!("attempt {attempt_id} does not exist"),
                )
            })
    }

    pub fn latest_attempt(
        &self,
        run_id: &str,
        role: Role,
        subject_id: &str,
        round: u32,
    ) -> AppResult<Option<AttemptView>> {
        let connection = self.connect()?;
        connection.query_row(
            &attempt_select("WHERE run_id=? AND role=? AND subject_id=? AND round=? ORDER BY created_at DESC,id DESC LIMIT 1"),
            params![run_id, role.as_str(), subject_id, round],
            row_attempt,
        ).optional().map_err(Into::into)
    }

    pub fn active_attempts(&self, run_id: &str) -> AppResult<Vec<AttemptView>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(&attempt_select(
            "WHERE run_id=? AND state IN ('prepared','admitted','running') ORDER BY created_at",
        ))?;
        Ok(statement
            .query_map(params![run_id], row_attempt)?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn attempts(&self, run_id: &str) -> AppResult<Vec<AttemptView>> {
        let connection = self.connect()?;
        let mut statement =
            connection.prepare(&attempt_select("WHERE run_id=? ORDER BY created_at,id"))?;
        Ok(statement
            .query_map(params![run_id], row_attempt)?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn mark_attempt_admitted(&self, attempt_id: &str, request_sha256: &str) -> AppResult<()> {
        let connection = self.connect()?;
        let count = connection.execute(
            "UPDATE attempts SET admitted=1,state='admitted',updated_at=? WHERE id=? AND request_sha256=?",
            params![now(), attempt_id, request_sha256],
        )?;
        require_changed(
            count,
            "attempt_conflict",
            "attempt request digest changed before admission",
        )
    }

    pub fn set_attempt_runtime(
        &self,
        attempt_id: &str,
        state: AttemptState,
        detail: Option<&str>,
    ) -> AppResult<()> {
        let connection = self.connect()?;
        let count = connection.execute(
            "UPDATE attempts SET state=?,detail=?,updated_at=? WHERE id=?",
            params![state.as_str(), detail, now(), attempt_id],
        )?;
        require_changed(count, "attempt_not_found", "attempt does not exist")
    }

    pub fn advance_tool_after(&self, attempt_id: &str, sequence: u64) -> AppResult<()> {
        let connection = self.connect()?;
        connection.execute(
            "UPDATE attempts SET tool_after=max(tool_after,?),updated_at=? WHERE id=?",
            params![
                i64::try_from(sequence).unwrap_or(i64::MAX),
                now(),
                attempt_id
            ],
        )?;
        Ok(())
    }

    pub fn bind_attempt_result(
        &self,
        attempt_id: &str,
        document_id: &str,
        disposition: Option<Disposition>,
    ) -> AppResult<()> {
        let connection = self.connect()?;
        let count = connection.execute(
            "UPDATE attempts SET domain_document_id=?,disposition=?,updated_at=? WHERE id=? AND domain_document_id IS NULL",
            params![document_id, disposition.map(Disposition::as_str), now(), attempt_id],
        )?;
        if count == 0 {
            let existing = self.attempt(attempt_id)?;
            if existing.domain_document_id.as_deref() != Some(document_id)
                || existing.disposition != disposition
            {
                return Err(AppError::new(
                    "attempt_result_conflict",
                    "attempt already committed a different domain result",
                ));
            }
        }
        Ok(())
    }

    pub fn receipt(&self, job_id: &str, call_id: &str) -> AppResult<Option<Receipt>> {
        let connection = self.connect()?;
        connection.query_row(
            "SELECT arguments_sha256,result_schema_id,result_json,is_error FROM tool_receipts WHERE job_id=? AND call_id=?",
            params![job_id, call_id],
            |row| Ok(Receipt {
                arguments_sha256: row.get(0)?,
                result_schema_id: row.get(1)?,
                result_json: row.get(2)?,
                is_error: row.get::<_, i64>(3)? != 0,
            }),
        ).optional().map_err(Into::into)
    }

    pub fn record_receipt(
        &self,
        job_id: &str,
        call_id: &str,
        receipt: &Receipt,
    ) -> AppResult<Receipt> {
        let connection = self.connect()?;
        connection.execute(
            "INSERT INTO tool_receipts(job_id,call_id,arguments_sha256,result_schema_id,result_json,is_error,created_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(job_id,call_id) DO NOTHING",
            params![job_id, call_id, receipt.arguments_sha256, receipt.result_schema_id, receipt.result_json, i64::from(receipt.is_error), now()],
        )?;
        let persisted = self.receipt(job_id, call_id)?.ok_or_else(|| {
            AppError::new(
                "tool_receipt_missing",
                "tool receipt was not readable after its durable insert",
            )
        })?;
        if persisted.arguments_sha256 != receipt.arguments_sha256
            || persisted.result_schema_id != receipt.result_schema_id
            || persisted.result_json != receipt.result_json
            || persisted.is_error != receipt.is_error
        {
            return Err(AppError::new(
                "tool_replay_conflict",
                "a repeated tool call changed its arguments or result",
            ));
        }
        Ok(persisted)
    }

    pub fn commit_managed_submission(
        &self,
        attempt_id: &str,
        job_id: &str,
        call_id: &str,
        arguments_sha256: &str,
        result_schema_id: &str,
        submission: &ManagedSubmission,
    ) -> AppResult<Receipt> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(receipt) = transaction.query_row(
            "SELECT arguments_sha256,result_schema_id,result_json,is_error FROM tool_receipts WHERE job_id=? AND call_id=?",
            params![job_id,call_id],
            |row| Ok(Receipt { arguments_sha256: row.get(0)?, result_schema_id: row.get(1)?, result_json: row.get(2)?, is_error: row.get::<_,i64>(3)? != 0 }),
        ).optional()? {
            if receipt.arguments_sha256 != arguments_sha256 {
                return Err(AppError::new("tool_replay_conflict", "a repeated tool call changed its exact arguments"));
            }
            transaction.commit()?;
            return Ok(receipt);
        }
        let attempt = transaction
            .query_row(
                &attempt_select("WHERE id=?"),
                params![attempt_id],
                row_attempt,
            )
            .optional()?
            .ok_or_else(|| AppError::new("attempt_not_found", "attempt does not exist"))?;
        if attempt.nucleus_job_id != job_id {
            return Err(AppError::new(
                "tool_job_mismatch",
                "tool call is not bound to this attempt's Nucleus job",
            ));
        }
        if attempt.domain_document_id.is_some() {
            return Err(AppError::new(
                "attempt_result_already_committed",
                "attempt already accepted one terminal submission",
            ));
        }
        validate_attempt_current_state(&transaction, &attempt)?;
        let (document_id, kind, status, disposition) = match submission {
            ManagedSubmission::UnitPlan(markdown) => {
                if attempt.role != Role::Planner {
                    return Err(AppError::new(
                        "tool_role_mismatch",
                        "only a planner may submit a unit plan",
                    ));
                }
                let markdown = OpaqueMarkdown::from_text(markdown.clone())?;
                let id = document_id();
                insert_document(
                    &transaction,
                    &id,
                    &attempt.run_id,
                    "unit_plan",
                    Some(&attempt.subject_id),
                    attempt.round,
                    &markdown,
                    now(),
                )?;
                (id, "unit_plan", "recorded", None)
            }
            ManagedSubmission::Delegation(value) => {
                if attempt.role != Role::Assembler {
                    return Err(AppError::new(
                        "tool_role_mismatch",
                        "only an assembler may submit a delegation plan",
                    ));
                }
                validate_delegation(&transaction, &attempt.run_id, attempt.round, value)?;
                let overview = OpaqueMarkdown::from_text(value.overview_markdown.clone())?;
                let id = document_id();
                insert_document(
                    &transaction,
                    &id,
                    &attempt.run_id,
                    "delegation_plan",
                    None,
                    attempt.round,
                    &overview,
                    now(),
                )?;
                if attempt.round > 0 {
                    transaction.execute(
                        "DELETE FROM packets WHERE run_id=?",
                        params![attempt.run_id],
                    )?;
                }
                insert_packets(&transaction, &attempt.run_id, attempt.round, value)?;
                (id, "delegation_plan", "recorded", None)
            }
            ManagedSubmission::Handoff(value) => {
                if !attempt.role.is_writer() {
                    return Err(AppError::new(
                        "tool_role_mismatch",
                        "only an implementor or integrator may submit a handoff",
                    ));
                }
                let markdown = OpaqueMarkdown::from_text(value.markdown.clone())?;
                let id = document_id();
                let kind = if attempt.role == Role::Integrator {
                    "integration_handoff"
                } else {
                    "implementation_handoff"
                };
                insert_document(
                    &transaction,
                    &id,
                    &attempt.run_id,
                    kind,
                    Some(&attempt.subject_id),
                    attempt.round,
                    &markdown,
                    now(),
                )?;
                let disposition =
                    (value.outcome == HandoffOutcome::Blocked).then_some(Disposition::Blocked);
                (
                    id,
                    kind,
                    if disposition.is_some() {
                        "blocked"
                    } else {
                        "ready"
                    },
                    disposition,
                )
            }
            ManagedSubmission::Review(value) => {
                if !matches!(
                    attempt.role,
                    Role::PlanReviewer | Role::PacketReviewer | Role::IntegratedReviewer
                ) {
                    return Err(AppError::new(
                        "tool_role_mismatch",
                        "only an independent reviewer may submit a review",
                    ));
                }
                validate_review(&transaction, &attempt, value)?;
                let markdown = OpaqueMarkdown::from_text(value.markdown.clone())?;
                let id = document_id();
                let kind = match attempt.role {
                    Role::PlanReviewer => "plan_review",
                    Role::PacketReviewer => "packet_review",
                    Role::IntegratedReviewer => "integrated_review",
                    _ => unreachable!(),
                };
                insert_document(
                    &transaction,
                    &id,
                    &attempt.run_id,
                    kind,
                    Some(&attempt.subject_id),
                    attempt.round,
                    &markdown,
                    now(),
                )?;
                (
                    id,
                    kind,
                    value.disposition.as_str(),
                    Some(value.disposition),
                )
            }
        };
        transaction.execute(
            "UPDATE attempts SET domain_document_id=?,disposition=?,updated_at=? WHERE id=? AND domain_document_id IS NULL",
            params![document_id,disposition.map(Disposition::as_str),now(),attempt.id],
        )?;
        let result_json = serde_json::json!({"ok":true,"recorded":{"kind":kind,"id":document_id,"status":status}}).to_string();
        transaction.execute(
            "INSERT INTO tool_receipts(job_id,call_id,arguments_sha256,result_schema_id,result_json,is_error,created_at) VALUES(?,?,?,?,?,0,?)",
            params![job_id,call_id,arguments_sha256,result_schema_id,result_json,now()],
        )?;
        transaction.commit()?;
        Ok(Receipt {
            arguments_sha256: arguments_sha256.to_owned(),
            result_schema_id: result_schema_id.to_owned(),
            result_json,
            is_error: false,
        })
    }

    pub fn record_managed_error(
        &self,
        job_id: &str,
        call_id: &str,
        arguments_sha256: &str,
        result_schema_id: &str,
        code: &str,
        message: &str,
    ) -> AppResult<Receipt> {
        let receipt = Receipt {
            arguments_sha256: arguments_sha256.to_owned(),
            result_schema_id: result_schema_id.to_owned(),
            result_json: serde_json::json!({"error":{"code":code,"message":message}}).to_string(),
            is_error: true,
        };
        self.record_receipt(job_id, call_id, &receipt)
    }

    pub fn record_candidate(
        &self,
        run_id: &str,
        subject_id: &str,
        kind: &str,
        round: u32,
        base_commit: &str,
        commit_oid: &str,
        ref_name: &str,
        handoff_document_id: &str,
        attempt_id: &str,
    ) -> AppResult<CandidateView> {
        let id = format!("candidate-{}", Uuid::now_v7());
        let connection = self.connect()?;
        connection.execute(
            "INSERT INTO candidates(id,run_id,subject_id,kind,round,base_commit,commit_oid,ref_name,handoff_document_id,attempt_id,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(attempt_id) DO NOTHING",
            params![id,run_id,subject_id,kind,round,base_commit,commit_oid,ref_name,handoff_document_id,attempt_id,now()],
        )?;
        self.candidate_for_attempt(attempt_id)?
            .ok_or_else(|| AppError::new("candidate_record_failed", "candidate was not recorded"))
    }

    pub fn candidate_for_attempt(&self, attempt_id: &str) -> AppResult<Option<CandidateView>> {
        let connection = self.connect()?;
        connection
            .query_row(
                &candidate_select("WHERE attempt_id=?"),
                params![attempt_id],
                row_candidate,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn is_current_resultless_leaf(&self, attempt: &AttemptView) -> AppResult<bool> {
        let connection = self.connect()?;
        let current: Option<String> = connection
            .query_row(
                "SELECT id FROM attempts WHERE run_id=? AND role=? AND subject_id=? ORDER BY created_at DESC,id DESC LIMIT 1",
                params![attempt.run_id, attempt.role.as_str(), attempt.subject_id],
                |row| row.get(0),
            )
            .optional()?;
        let has_successor: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM attempts WHERE predecessor_attempt_id=?)",
            params![attempt.id],
            |row| row.get(0),
        )?;
        Ok(attempt.domain_document_id.is_none()
            && current.as_deref() == Some(attempt.id.as_str())
            && !has_successor)
    }

    pub fn candidate(&self, candidate_id: &str) -> AppResult<CandidateView> {
        let connection = self.connect()?;
        connection
            .query_row(
                &candidate_select("WHERE id=?"),
                params![candidate_id],
                row_candidate,
            )
            .optional()?
            .ok_or_else(|| AppError::new("candidate_not_found", "candidate does not exist"))
    }

    pub fn latest_candidate(
        &self,
        run_id: &str,
        subject_id: &str,
        kind: &str,
    ) -> AppResult<Option<CandidateView>> {
        let connection = self.connect()?;
        connection.query_row(
            &candidate_select("WHERE run_id=? AND subject_id=? AND kind=? ORDER BY round DESC,created_at DESC LIMIT 1"),
            params![run_id,subject_id,kind],
            row_candidate,
        ).optional().map_err(Into::into)
    }

    pub fn gates(&self, run_id: &str) -> AppResult<Vec<GateSpec>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT id,run_id,ordinal,name,command FROM gate_specs WHERE run_id=? ORDER BY ordinal",
        )?;
        Ok(statement
            .query_map(params![run_id], |row| {
                Ok(GateSpec {
                    id: row.get(0)?,
                    run_id: row.get(1)?,
                    ordinal: row.get(2)?,
                    name: row.get(3)?,
                    command: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn gate_result(&self, gate_id: &str, candidate_id: &str) -> AppResult<Option<GateResult>> {
        let connection = self.connect()?;
        connection.query_row(
            "SELECT id,gate_id,candidate_id,round,exit_code,output,output_truncated,created_at FROM gate_results WHERE gate_id=? AND candidate_id=?",
            params![gate_id,candidate_id],
            row_gate_result,
        ).optional().map_err(Into::into)
    }

    pub fn record_gate_result(&self, result: &GateResult) -> AppResult<()> {
        let connection = self.connect()?;
        connection.execute(
            "INSERT INTO gate_results(id,gate_id,candidate_id,round,exit_code,output,output_truncated,created_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(gate_id,candidate_id) DO NOTHING",
            params![result.id,result.gate_id,result.candidate_id,result.round,result.exit_code,result.output,i64::from(result.output_truncated),result.created_at],
        )?;
        Ok(())
    }

    pub fn secure_files(&self) -> AppResult<()> {
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.path.as_os_str().to_owned();
            path.push(suffix);
            let path = PathBuf::from(path);
            if path.exists() {
                set_file_private(&path)?;
            }
        }
        Ok(())
    }

    fn connect(&self) -> AppResult<Connection> {
        if !self.path.is_file() {
            return Err(AppError::new(
                "database_not_initialized",
                "Vizier database is not initialized",
            ));
        }
        if self
            .path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(AppError::new(
                "database_symlink_refused",
                "Vizier database must not be a symbolic link",
            ));
        }
        self.connect_unchecked()
    }

    fn connect_unchecked(&self) -> AppResult<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        Ok(connection)
    }
}

fn insert_document(
    connection: &Connection,
    id: &str,
    run_id: &str,
    kind: &str,
    subject_id: Option<&str>,
    ordinal: u32,
    markdown: &OpaqueMarkdown,
    created_at: i64,
) -> AppResult<()> {
    connection.execute(
        "INSERT INTO documents(id,run_id,kind,subject_id,ordinal,markdown,sha256,created_at) VALUES(?,?,?,?,?,?,?,?)",
        params![id,run_id,kind,subject_id,ordinal,markdown.as_str(),markdown.sha256(),created_at],
    )?;
    Ok(())
}

fn insert_packets(
    transaction: &Connection,
    run_id: &str,
    revision: u32,
    submission: &DelegationSubmission,
) -> AppResult<()> {
    for (ordinal, packet) in submission.packets.iter().enumerate() {
        let ordinal = checked_ordinal(ordinal, "packet")?;
        let markdown = OpaqueMarkdown::from_text(packet.plan_markdown.clone())?;
        let plan_id = document_id();
        insert_document(
            transaction,
            &plan_id,
            run_id,
            "packet_plan",
            Some(&packet.packet_key),
            revision,
            &markdown,
            now(),
        )?;
        transaction.execute(
            "INSERT INTO packets(run_id,packet_key,ordinal,state,contract_ids_json,depends_on_json,path_scopes_json,plan_document_id) VALUES(?,?,?,?,?,?,?,?)",
            params![run_id,packet.packet_key,ordinal,PacketState::Planned.as_str(),serde_json::to_string(&packet.contract_unit_ids)?,serde_json::to_string(&packet.depends_on)?,serde_json::to_string(&packet.path_scopes)?,plan_id],
        )?;
    }
    Ok(())
}

fn validate_delegation(
    transaction: &Connection,
    run_id: &str,
    revision: u32,
    submission: &DelegationSubmission,
) -> AppResult<()> {
    let existing: i64 = transaction.query_row(
        "SELECT count(*) FROM packets WHERE run_id=?",
        params![run_id],
        |row| row.get(0),
    )?;
    if revision == 0 && existing != 0 {
        return Err(AppError::new(
            "delegation_already_recorded",
            "the run already has an immutable delegation plan",
        ));
    }
    if revision > 0 && existing == 0 {
        return Err(AppError::new(
            "delegation_revision_basis_missing",
            "a delegation revision requires the current predecessor manifest",
        ));
    }
    if revision > 0 {
        let advanced: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM packets WHERE run_id=? AND (state!='planned' OR current_candidate_id IS NOT NULL OR remediation_round!=0))",
            params![run_id],
            |row| row.get(0),
        )?;
        if advanced {
            return Err(AppError::new(
                "delegation_revision_too_late",
                "a delegation plan cannot be replaced after packet implementation begins",
            ));
        }
    }
    let mut statement = transaction.prepare(
        "SELECT subject_id FROM documents WHERE run_id=? AND kind='contract_unit' ORDER BY ordinal",
    )?;
    let contract_ids = statement
        .query_map(params![run_id], |row| row.get::<_, String>(0))?
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    let keys = submission
        .packets
        .iter()
        .map(|packet| packet.packet_key.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if keys.len() != submission.packets.len() {
        return Err(AppError::new(
            "delegation_invalid",
            "packet keys must be unique",
        ));
    }
    let mut covered = std::collections::BTreeSet::new();
    for packet in &submission.packets {
        validate_mechanical_id(&packet.packet_key, "packet key")?;
        validate_scopes(&packet.path_scopes)?;
        if packet.contract_unit_ids.is_empty() {
            return Err(AppError::new(
                "delegation_invalid",
                "every packet must cover at least one contract unit",
            ));
        }
        for id in &packet.contract_unit_ids {
            if !contract_ids.contains(id) {
                return Err(AppError::new(
                    "delegation_invalid",
                    format!(
                        "packet {} cites unknown contract unit {id}",
                        packet.packet_key
                    ),
                ));
            }
            covered.insert(id.clone());
        }
        for dependency in &packet.depends_on {
            if dependency == &packet.packet_key || !keys.contains(dependency.as_str()) {
                return Err(AppError::new(
                    "delegation_invalid",
                    format!(
                        "packet {} has invalid dependency {dependency}",
                        packet.packet_key
                    ),
                ));
            }
        }
    }
    if covered != contract_ids {
        return Err(AppError::new(
            "delegation_incomplete",
            "the packet manifest must cover every exact contract unit",
        ));
    }
    for packet in &submission.packets {
        if reaches(
            &submission.packets,
            &packet.packet_key,
            &packet.packet_key,
            &mut std::collections::BTreeSet::new(),
            true,
        ) {
            return Err(AppError::new(
                "delegation_cycle",
                "packet dependencies must form a DAG",
            ));
        }
    }
    for (index, left) in submission.packets.iter().enumerate() {
        for right in submission.packets.iter().skip(index + 1) {
            if scopes_overlap(&left.path_scopes, &right.path_scopes)
                && !reaches(
                    &submission.packets,
                    &left.packet_key,
                    &right.packet_key,
                    &mut std::collections::BTreeSet::new(),
                    false,
                )
                && !reaches(
                    &submission.packets,
                    &right.packet_key,
                    &left.packet_key,
                    &mut std::collections::BTreeSet::new(),
                    false,
                )
            {
                return Err(AppError::new(
                    "delegation_overlap",
                    format!(
                        "packets {} and {} have overlapping write scopes without an ordering dependency",
                        left.packet_key, right.packet_key
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn reaches(
    packets: &[crate::model::PacketSubmission],
    from: &str,
    target: &str,
    seen: &mut std::collections::BTreeSet<String>,
    require_edge: bool,
) -> bool {
    if !seen.insert(from.to_owned()) {
        return false;
    }
    let Some(packet) = packets.iter().find(|packet| packet.packet_key == from) else {
        return false;
    };
    for dependency in &packet.depends_on {
        if dependency == target || reaches(packets, dependency, target, seen, false) {
            return true;
        }
    }
    !require_edge && from == target
}

fn validate_review(
    transaction: &Connection,
    attempt: &AttemptView,
    review: &crate::model::ReviewSubmission,
) -> AppResult<()> {
    let packet_keys = transaction
        .prepare("SELECT packet_key FROM packets WHERE run_id=?")?
        .query_map(params![attempt.run_id], |row| row.get::<_, String>(0))?
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    let contract_ids = transaction
        .prepare("SELECT subject_id FROM documents WHERE run_id=? AND kind='contract_unit'")?
        .query_map(params![attempt.run_id], |row| row.get::<_, String>(0))?
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    for key in &review.affected_packet_keys {
        if !packet_keys.contains(key) {
            return Err(AppError::new(
                "review_scope_invalid",
                format!("review cites unknown packet {key}"),
            ));
        }
    }
    for id in &review.contract_unit_ids {
        if !contract_ids.contains(id) {
            return Err(AppError::new(
                "review_scope_invalid",
                format!("review cites unknown contract unit {id}"),
            ));
        }
    }
    if attempt.role == Role::PacketReviewer
        && review
            .affected_packet_keys
            .iter()
            .any(|key| key != &attempt.subject_id)
    {
        return Err(AppError::new(
            "review_scope_invalid",
            "a packet review cannot route changes into another packet",
        ));
    }
    Ok(())
}

fn validate_attempt_current_state(
    transaction: &Connection,
    attempt: &AttemptView,
) -> AppResult<()> {
    if attempt.state.is_terminal() {
        return Err(AppError::new(
            "attempt_not_current",
            "terminal attempts cannot commit a new domain result",
        ));
    }
    let (state, cancelled): (String, i64) = transaction.query_row(
        "SELECT state,cancel_requested FROM runs WHERE id=?",
        params![attempt.run_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if cancelled != 0
        || matches!(
            state.as_str(),
            "succeeded" | "needs_attention" | "cancelled"
        )
    {
        return Err(AppError::new(
            "run_not_current",
            "the owning run no longer permits domain submissions",
        ));
    }
    let allowed = match attempt.role {
        Role::Planner => state == RunState::Planning.as_str(),
        Role::Assembler => state == RunState::Assembling.as_str(),
        Role::PlanReviewer => state == RunState::PlanReview.as_str(),
        Role::Implementor | Role::PacketReviewer => {
            matches!(state.as_str(), "implementing" | "packet_review")
        }
        Role::Integrator => state == RunState::Integrating.as_str(),
        Role::IntegratedReviewer => state == RunState::FinalReview.as_str(),
    };
    if !allowed {
        return Err(AppError::new(
            "attempt_phase_conflict",
            format!(
                "role {} cannot submit while run is in {state}",
                attempt.role.as_str()
            ),
        ));
    }
    if matches!(attempt.role, Role::Implementor | Role::PacketReviewer) {
        let packet_state: Option<String> = transaction
            .query_row(
                "SELECT state FROM packets WHERE run_id=? AND packet_key=?",
                params![attempt.run_id, attempt.subject_id],
                |row| row.get(0),
            )
            .optional()?;
        let expected = if attempt.role == Role::Implementor {
            "implementing"
        } else {
            "reviewing"
        };
        if packet_state.as_deref() != Some(expected) {
            return Err(AppError::new(
                "attempt_subject_state_conflict",
                format!(
                    "packet {} is not in the expected {expected} state",
                    attempt.subject_id
                ),
            ));
        }
    }
    Ok(())
}

fn validate_mechanical_id(value: &str, label: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(AppError::new(
            "delegation_invalid",
            format!("{label} {value:?} is not a bounded mechanical identity"),
        ));
    }
    Ok(())
}

fn query_run<P: rusqlite::Params>(
    connection: &Connection,
    clause: &str,
    params: P,
) -> AppResult<Option<RunView>> {
    connection
        .query_row(&run_select(clause), params, row_run)
        .optional()
        .map_err(Into::into)
}

fn run_by_request_key(connection: &Connection, key: &str) -> AppResult<Option<RunView>> {
    query_run(connection, "WHERE request_key=?", params![key])
}

fn run_select(clause: &str) -> String {
    format!(
        "SELECT id,repository,source_commit,state,contract_set_sha256,input_bundle_sha256,remediation_limit,final_candidate_id,final_ref,cancel_requested,detail,created_at,updated_at FROM runs {clause}"
    )
}

fn row_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunView> {
    let state: String = row.get(3)?;
    let state = RunState::parse(&state).ok_or_else(|| invalid_enum_value(3, "run state"))?;
    Ok(RunView {
        id: row.get(0)?,
        repository: row.get(1)?,
        source_commit: row.get(2)?,
        state,
        contract_set_sha256: row.get(4)?,
        input_bundle_sha256: row.get(5)?,
        remediation_limit: row.get(6)?,
        final_candidate_id: row.get(7)?,
        final_ref: row.get(8)?,
        cancel_requested: row.get::<_, i64>(9)? != 0,
        detail: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn row_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentView> {
    let text: String = row.get(5)?;
    let markdown = OpaqueMarkdown::from_text(text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(DocumentView {
        id: row.get(0)?,
        run_id: row.get(1)?,
        kind: row.get(2)?,
        subject_id: row.get(3)?,
        ordinal: row.get(4)?,
        markdown,
        sha256: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn row_packet(row: &rusqlite::Row<'_>) -> rusqlite::Result<PacketView> {
    let state: String = row.get(3)?;
    let contracts: String = row.get(4)?;
    let dependencies: String = row.get(5)?;
    let scopes: String = row.get(6)?;
    let state = PacketState::parse(&state).ok_or_else(|| invalid_enum_value(3, "packet state"))?;
    Ok(PacketView {
        run_id: row.get(0)?,
        key: row.get(1)?,
        ordinal: row.get(2)?,
        state,
        contract_unit_ids: serde_json::from_str(&contracts).map_err(json_sql_error)?,
        depends_on: serde_json::from_str(&dependencies).map_err(json_sql_error)?,
        path_scopes: serde_json::from_str(&scopes).map_err(json_sql_error)?,
        plan_document_id: row.get(7)?,
        current_candidate_id: row.get(8)?,
        remediation_round: row.get(9)?,
    })
}

fn attempt_select(clause: &str) -> String {
    format!(
        "SELECT id,run_id,role,subject_id,round,targeted,state,nucleus_job_id,request_bytes,request_sha256,toolset_name,workspace_path,base_commit,allowed_scopes_json,admitted,tool_after,domain_document_id,disposition,predecessor_attempt_id,detail,created_at,updated_at FROM attempts {clause}"
    )
}

fn row_attempt(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttemptView> {
    let role: String = row.get(2)?;
    let state: String = row.get(6)?;
    let scopes: String = row.get(13)?;
    let disposition: Option<String> = row.get(17)?;
    let role = Role::parse(&role).ok_or_else(|| invalid_enum_value(2, "attempt role"))?;
    let state =
        AttemptState::parse(&state).ok_or_else(|| invalid_enum_value(6, "attempt state"))?;
    Ok(AttemptView {
        id: row.get(0)?,
        run_id: row.get(1)?,
        role,
        subject_id: row.get(3)?,
        round: row.get(4)?,
        targeted: row.get::<_, i64>(5)? != 0,
        state,
        nucleus_job_id: row.get(7)?,
        request_bytes: row.get(8)?,
        request_sha256: row.get(9)?,
        toolset_name: row.get(10)?,
        workspace_path: row.get(11)?,
        base_commit: row.get(12)?,
        allowed_scopes: serde_json::from_str(&scopes).map_err(json_sql_error)?,
        admitted: row.get::<_, i64>(14)? != 0,
        tool_after: u64::try_from(row.get::<_, i64>(15)?).unwrap_or_default(),
        domain_document_id: row.get(16)?,
        disposition: disposition.as_deref().and_then(Disposition::parse),
        predecessor_attempt_id: row.get(18)?,
        detail: row.get(19)?,
        created_at: row.get(20)?,
        updated_at: row.get(21)?,
    })
}

fn candidate_select(clause: &str) -> String {
    format!(
        "SELECT id,run_id,subject_id,kind,round,base_commit,commit_oid,ref_name,handoff_document_id,attempt_id,created_at FROM candidates {clause}"
    )
}

fn row_candidate(row: &rusqlite::Row<'_>) -> rusqlite::Result<CandidateView> {
    Ok(CandidateView {
        id: row.get(0)?,
        run_id: row.get(1)?,
        subject_id: row.get(2)?,
        kind: row.get(3)?,
        round: row.get(4)?,
        base_commit: row.get(5)?,
        commit_oid: row.get(6)?,
        ref_name: row.get(7)?,
        handoff_document_id: row.get(8)?,
        attempt_id: row.get(9)?,
        created_at: row.get(10)?,
    })
}

fn row_gate_result(row: &rusqlite::Row<'_>) -> rusqlite::Result<GateResult> {
    Ok(GateResult {
        id: row.get(0)?,
        gate_id: row.get(1)?,
        candidate_id: row.get(2)?,
        round: row.get(3)?,
        exit_code: row.get(4)?,
        output: row.get(5)?,
        output_truncated: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
    })
}

fn json_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn invalid_enum_value(column: usize, label: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        Box::new(AppError::new(
            "database_value_invalid",
            format!("stored {label} is not recognized"),
        )),
    )
}

fn checked_ordinal(value: usize, label: &str) -> AppResult<u32> {
    u32::try_from(value).map_err(|_| {
        AppError::new(
            "ordinal_out_of_range",
            format!("{label} ordinal exceeds the supported range"),
        )
    })
}

fn document_id() -> String {
    format!("document-{}", Uuid::now_v7())
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn require_changed(count: usize, code: &'static str, message: &'static str) -> AppResult<()> {
    if count == 0 {
        Err(AppError::new(code, message))
    } else {
        Ok(())
    }
}

fn contract_set_digest(contracts: &[(String, OpaqueMarkdown)]) -> String {
    let mut bytes = Vec::new();
    for (id, markdown) in contracts {
        bytes.extend_from_slice(&(id.len() as u64).to_be_bytes());
        bytes.extend_from_slice(id.as_bytes());
        bytes.extend_from_slice(markdown.sha256().as_bytes());
    }
    sha256_hex(&bytes)
}

fn input_bundle_digest(input: &NewRun) -> AppResult<String> {
    #[derive(serde::Serialize)]
    struct DigestInput<'a> {
        repository: &'a str,
        source_commit: &'a str,
        brief_sha256: String,
        terminology_sha256: String,
        contracts: Vec<(&'a str, String)>,
        gates: &'a [(String, String)],
        remediation_limit: u32,
    }
    let value = DigestInput {
        repository: &input.repository,
        source_commit: &input.source_commit,
        brief_sha256: input.brief.sha256(),
        terminology_sha256: input.terminology.sha256(),
        contracts: input
            .contracts
            .iter()
            .map(|(id, markdown)| (id.as_str(), markdown.sha256()))
            .collect(),
        gates: &input.gates,
        remediation_limit: input.remediation_limit,
    };
    Ok(sha256_hex(&serde_json::to_vec(&value)?))
}

#[cfg(unix)]
fn set_directory_private(path: &Path) -> AppResult<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_directory_private(_path: &Path) -> AppResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_file_private(path: &Path) -> AppResult<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_private(_path: &Path) -> AppResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{NewAttempt, Receipt, Store};
    use crate::contracts::ManagedSubmission;
    use crate::error::{AppError, AppResult};
    use crate::model::{NewRun, OpaqueMarkdown, Role};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn markdown(value: &str) -> TestResult<OpaqueMarkdown> {
        Ok(OpaqueMarkdown::from_text(value)?)
    }

    fn require_error<T>(result: AppResult<T>, message: &str) -> TestResult<AppError> {
        match result {
            Err(error) => Ok(error),
            Ok(_) => Err(message.to_owned().into()),
        }
    }

    #[test]
    fn stores_exact_documents_and_replay_safe_receipts() -> TestResult {
        let directory = tempfile::tempdir()?;
        let store = Store::new(directory.path().join("vizier.db"));
        store.initialize()?;
        let run = store.create_run(&NewRun {
            id: "run-test".to_owned(),
            request_key: None,
            repository: "/tmp/repo".to_owned(),
            source_commit: "abc".to_owned(),
            brief: markdown("# Brief\r\n")?,
            terminology: markdown("# Terms\n")?,
            contracts: vec![("c1".to_owned(), markdown("# C1  \n")?)],
            gates: vec![("ci".to_owned(), "true".to_owned())],
            remediation_limit: 1,
        })?;
        assert_eq!(run.contract_set_sha256.len(), 64);
        let documents = store.documents("run-test", "contract_unit")?;
        let contract = documents
            .first()
            .ok_or("expected one stored contract document")?;
        assert_eq!(contract.markdown.as_bytes(), b"# C1  \n");

        let attempt = store.create_attempt(&NewAttempt {
            run_id: "run-test",
            role: Role::Planner,
            subject_id: "c1",
            round: 0,
            targeted: false,
            nucleus_job_id: "job-test",
            request_bytes: b"{}",
            request_sha256: "digest",
            toolset_name: "unit-plan",
            workspace_path: "/tmp/repo",
            base_commit: None,
            allowed_scopes: &[],
            predecessor_attempt_id: None,
        })?;
        assert_eq!(attempt.role, Role::Planner);
        let guarded = require_error(
            store.commit_managed_submission(
                &attempt.id,
                "job-test",
                "guarded-call",
                "guarded-args",
                "vizier.tool.result.v1",
                &ManagedSubmission::UnitPlan("# Plan\n".to_owned()),
            ),
            "queued run unexpectedly accepted a planner result",
        )?;
        assert_eq!(guarded.code(), "attempt_phase_conflict");
        store.set_run_state("run-test", crate::model::RunState::Planning, None)?;
        store.commit_managed_submission(
            &attempt.id,
            "job-test",
            "current-call",
            "current-args",
            "vizier.tool.result.v1",
            &ManagedSubmission::UnitPlan("# Plan\n".to_owned()),
        )?;
        let receipt = Receipt {
            arguments_sha256: "args".to_owned(),
            result_schema_id: "result".to_owned(),
            result_json: "{\"ok\":true}".to_owned(),
            is_error: false,
        };
        store.record_receipt("job-test", "call-1", &receipt)?;
        store.record_receipt("job-test", "call-1", &receipt)?;
        let conflict = Receipt {
            arguments_sha256: "changed".to_owned(),
            ..receipt
        };
        assert!(
            store
                .record_receipt("job-test", "call-1", &conflict)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn request_keys_require_the_same_exact_input_bundle() -> TestResult {
        let directory = tempfile::tempdir()?;
        let store = Store::new(directory.path().join("vizier.db"));
        store.initialize()?;
        let input = NewRun {
            id: "run-first".to_owned(),
            request_key: Some("caller-key".to_owned()),
            repository: "/tmp/repo".to_owned(),
            source_commit: "abc".to_owned(),
            brief: markdown("# Brief\n")?,
            terminology: markdown("# Terms\n")?,
            contracts: vec![("c1".to_owned(), markdown("# C1\n")?)],
            gates: vec![("ci".to_owned(), "true".to_owned())],
            remediation_limit: 1,
        };
        let first = store.create_run(&input)?;
        let mut replay = input.clone();
        replay.id = "run-replay-ignored".to_owned();
        assert_eq!(store.create_run(&replay)?.id, first.id);
        replay.brief = markdown("# Changed brief\n")?;
        let error = require_error(
            store.create_run(&replay),
            "changed request-key reuse unexpectedly succeeded",
        )?;
        assert_eq!(error.code(), "request_key_conflict");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn initialization_does_not_chmod_an_existing_parent() -> TestResult {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir()?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755))?;
        let before = std::fs::metadata(directory.path())?.permissions().mode() & 0o777;
        Store::new(directory.path().join("vizier.db")).initialize()?;
        let after = std::fs::metadata(directory.path())?.permissions().mode() & 0o777;
        assert_eq!(after, before);
        Ok(())
    }
}
