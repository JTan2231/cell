//! Krisis-owned supported CLI input and output types and client.
#![allow(clippy::missing_errors_doc)]

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::ffi::OsString;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub use krisis_api::{account, lifecycle};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub host_id: Option<String>,
    pub thread_id: Option<String>,
    pub status: String,
    pub scope_level: i64,
    pub attempt_epoch: i64,
    pub outcome: Option<String>,
    pub file_change_count: i64,
    pub authority_occurred_at: Option<i64>,
    pub failure_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationStatus {
    pub failures_has_more: bool,
    pub observer_baseline_at: Option<i64>,
    pub queued: usize,
    pub processing: usize,
    pub complete: usize,
    pub failed: usize,
    pub accounts_pending_annals: usize,
    pub accounts_accepted_by_annals: usize,
    pub failures: Vec<ObservationFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationFailure {
    pub id: String,
    pub failure_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCandidate {
    pub id: String,
    pub run_id: String,
    pub decided_at: i64,
    pub timestamp_precision: String,
    pub statement: String,
    pub disposition: String,
    pub confidence: String,
    pub rationale: Option<String>,
    pub supersedes_id: Option<String>,
    pub authority_start: i64,
    pub authority_end: i64,
    pub review_state: String,
    pub sources: Vec<krisis_api::lifecycle::DecisionEventSource>,
}

#[derive(Debug, Clone)]
pub struct AnnalsTarget {
    pub binary: PathBuf,
    pub config: PathBuf,
    pub expected_library_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopHookInput {
    pub session_id: String,
    pub turn_id: String,
    pub hook_event_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookReceipt {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationReceipt {
    pub observer_baseline_at: i64,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessResult {
    pub observation_id: String,
    pub status: String,
    pub scope_level: i64,
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationProcess {
    pub processed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<ProcessResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountDelivery {
    pub processed: bool,
    pub kind: String,
    pub account_id: String,
    pub library_id: String,
    pub job_id: String,
    pub accepted_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProcessOutput {
    Account(AccountDelivery),
    Observation(ObservationProcess),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileResult {
    pub threads_scanned: usize,
    pub activities_scanned: usize,
    pub observations_enqueued: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub schema_version: i64,
    pub observer_baseline_at: Option<i64>,
    pub observer: ObservationStatus,
    pub conversation_source: conversations::DoctorReport,
    pub nucleus: String,
    pub annals_library_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("unable to invoke Krisis")]
    Io(#[from] std::io::Error),
    #[error("Krisis returned invalid JSON")]
    Json(#[from] serde_json::Error),
    #[error("Krisis command failed: {0}")]
    Failed(String),
}

/// Runs only explicit public commands; the provider owns all durable state.
#[derive(Debug, Clone)]
pub struct Client {
    binary: PathBuf,
    database: Option<PathBuf>,
    annals: Option<AnnalsTarget>,
}

impl Client {
    #[must_use]
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            database: None,
            annals: None,
        }
    }

    #[must_use]
    pub fn with_database(mut self, database: impl Into<PathBuf>) -> Self {
        self.database = Some(database.into());
        self
    }

    #[must_use]
    pub fn with_annals(mut self, annals: AnnalsTarget) -> Self {
        self.annals = Some(annals);
        self
    }

    fn json<T: DeserializeOwned>(
        &self,
        arguments: &[OsString],
        stdin: Option<Vec<u8>>,
    ) -> Result<T, ClientError> {
        let mut command = Command::new(&self.binary);
        if let Some(database) = &self.database {
            command.arg("--database").arg(database);
        }
        if let Some(annals) = &self.annals {
            command
                .arg("--annals-binary")
                .arg(&annals.binary)
                .arg("--annals-config")
                .arg(&annals.config)
                .arg("--annals-library-id")
                .arg(&annals.expected_library_id);
        }
        command
            .arg("--json")
            .args(arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = if let Some(input) = stdin {
            let mut child = command.stdin(Stdio::piped()).spawn()?;
            if let Some(mut pipe) = child.stdin.take()
                && let Err(error) = pipe.write_all(&input)
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ClientError::Io(error));
            }
            child.wait_with_output()?
        } else {
            command.output()?
        };
        if !output.status.success() {
            return Err(ClientError::Failed(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    pub fn activate(&self, at: Option<i64>) -> Result<ActivationReceipt, ClientError> {
        let mut arguments = vec!["observe".into(), "activate".into()];
        if let Some(at) = at {
            arguments.extend(["--at".into(), at.to_string().into()]);
        }
        self.json(&arguments, None)
    }

    pub fn ingest(&self, hook: &StopHookInput) -> Result<HookReceipt, ClientError> {
        self.json(
            &["observe".into(), "ingest".into()],
            Some(serde_json::to_vec(hook)?),
        )
    }

    pub fn process(&self) -> Result<ProcessOutput, ClientError> {
        self.json(&["observe".into(), "process".into()], None)
    }

    pub fn status(&self, date: Option<&str>) -> Result<ObservationStatus, ClientError> {
        self.json(&dated("status", date), None)
    }

    pub fn status_limit(
        &self,
        date: Option<&str>,
        limit: usize,
    ) -> Result<ObservationStatus, ClientError> {
        let mut args = dated("status", date);
        args.extend(["--limit".into(), limit.to_string().into()]);
        self.json(&args, None)
    }

    pub fn reconcile(&self, date: Option<&str>) -> Result<ReconcileResult, ClientError> {
        self.json(&dated("reconcile", date), None)
    }

    /// The caller must have proved the exact queued source permanently unavailable.
    pub fn abandon_unavailable(&self, observation_id: &str) -> Result<Observation, ClientError> {
        self.json(
            &[
                "observe".into(),
                "abandon".into(),
                observation_id.into(),
                "--source-unavailable".into(),
            ],
            None,
        )
    }

    pub fn retry(&self, observation_id: &str) -> Result<Observation, ClientError> {
        self.json(
            &["observe".into(), "retry".into(), observation_id.into()],
            None,
        )
    }

    /// Read one retained legacy candidate; this does not reinterpret it as an account.
    pub fn show(&self, decision_id: &str) -> Result<StoredCandidate, ClientError> {
        self.json(&["show".into(), decision_id.into()], None)
    }

    pub fn doctor(&self) -> Result<DoctorReport, ClientError> {
        self.json(&["doctor".into()], None)
    }
}

fn dated(operation: &str, date: Option<&str>) -> Vec<OsString> {
    let mut arguments = vec!["observe".into(), operation.into()];
    if let Some(date) = date {
        arguments.extend(["--date".into(), date.into()]);
    }
    arguments
}
