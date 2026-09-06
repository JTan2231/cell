//! Clockwork-owned activation definitions and public command results.
//! Definition decoding checks structure; registration also validates local artifacts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub key: String,
    pub release_id: String,
    pub release_root: String,
    pub authority: Authority,
    pub overlap: OverlapPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub arguments: Vec<String>,
    pub cwd: String,
    pub schedule: Schedule,
    pub launch: LaunchImage,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    pub output: Output,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Authority {
    CurrentUserBackground,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OverlapPolicy {
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Schedule {
    Interval {
        seconds: u64,
        #[serde(default)]
        run_at_load: bool,
    },
    LocalCalendar {
        hour: u8,
        minute: u8,
        #[serde(default)]
        run_at_load: bool,
    },
}

impl Schedule {
    #[must_use]
    pub fn run_at_load(&self) -> bool {
        match self {
            Self::Interval { run_at_load, .. } | Self::LocalCalendar { run_at_load, .. } => {
                *run_at_load
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum LaunchImage {
    Direct {
        program: String,
        sha256: String,
    },
    Interpreted {
        interpreter: String,
        interpreter_sha256: String,
        script: String,
        script_sha256: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefinitionRecord {
    pub digest: String,
    pub key: String,
    pub registered_at: i64,
    pub manifest: Manifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefinitionSummary {
    pub digest: String,
    pub key: String,
    pub registered_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingRecord {
    pub key: String,
    pub definition_digest: Option<String>,
    pub enabled: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivationState {
    StartFailed,
    Running,
    Exited,
    Signaled,
    TimedOut,
    SkippedOverlap,
    Lost,
}

impl ActivationState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StartFailed => "start_failed",
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Signaled => "signaled",
            Self::TimedOut => "timed_out",
            Self::SkippedOverlap => "skipped_overlap",
            Self::Lost => "lost",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "start_failed" => Some(Self::StartFailed),
            "running" => Some(Self::Running),
            "exited" => Some(Self::Exited),
            "signaled" => Some(Self::Signaled),
            "timed_out" => Some(Self::TimedOut),
            "skipped_overlap" => Some(Self::SkippedOverlap),
            "lost" => Some(Self::Lost),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Manual,
    Launchd,
}

impl Trigger {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Launchd => "launchd",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationRecord {
    pub id: String,
    pub key: String,
    pub definition_digest: String,
    pub trigger: Trigger,
    pub state: ActivationState,
    pub admitted_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub broker_pid: Option<u32>,
    pub child_pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub detail: Option<String>,
}

/// The existing successful CLI envelope. The wire format is intentionally unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Success<T> {
    pub ok: bool,
    pub data: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Failure {
    pub ok: bool,
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub database: std::path::PathBuf,
    pub state_root: std::path::PathBuf,
    pub sqlite: String,
    pub recovered_lost_activations: usize,
    pub pending_binding_transitions: Vec<String>,
    pub clockwork_binary: std::path::PathBuf,
    pub launchctl: std::path::PathBuf,
}

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Decode Clockwork's existing success/error envelope.
///
/// # Errors
/// Returns the provider error or rejects a malformed envelope.
pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| Error(e.to_string()))?;
    match value.get("ok").and_then(serde_json::Value::as_bool) {
        Some(true) => serde_json::from_value::<Success<T>>(value)
            .map(|response| response.data)
            .map_err(|e| Error(e.to_string())),
        Some(false) => {
            let response: Failure =
                serde_json::from_value(value).map_err(|e| Error(e.to_string()))?;
            Err(Error(format!(
                "{}: {}",
                response.error.code, response.error.message
            )))
        }
        None => Err(Error("invalid Clockwork response envelope".to_owned())),
    }
}

impl Manifest {
    /// Decode the strict schema-one TOML structure. Registration separately verifies artifacts.
    ///
    /// # Errors
    /// Returns an error for malformed TOML or an unsupported schema version.
    pub fn from_toml(source: &str) -> Result<Self, Error> {
        let manifest: Self = toml::from_str(source).map_err(|e| Error(e.to_string()))?;
        if manifest.schema_version != 1 {
            return Err(Error(format!(
                "unsupported Clockwork manifest schema {}",
                manifest.schema_version
            )));
        }
        Ok(manifest)
    }

    /// Encode a definition for the existing registration command.
    ///
    /// # Errors
    /// Returns a TOML serialization error.
    pub fn to_toml(&self) -> Result<String, Error> {
        toml::to_string(self).map_err(|e| Error(e.to_string()))
    }

    /// Compute the same immutable definition digest used by the provider.
    ///
    /// # Errors
    /// Returns a serialization error.
    pub fn digest(&self) -> Result<String, Error> {
        use sha2::{Digest as _, Sha256};
        let canonical = serde_json::to_vec(self).map_err(|e| Error(e.to_string()))?;
        Ok(hex::encode(Sha256::digest(canonical)))
    }
}

/// A client for an explicitly selected Clockwork executable.
/// Each method has the same effects as its corresponding CLI operation.
#[derive(Debug, Clone)]
pub struct Client {
    executable: std::path::PathBuf,
    state_root: Option<std::path::PathBuf>,
}

#[allow(clippy::missing_errors_doc)]
impl Client {
    #[must_use]
    pub fn new(executable: impl Into<std::path::PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            state_root: None,
        }
    }

    /// Isolate Clockwork-owned paths, as with the CLI's installation-test option.
    #[must_use]
    pub fn with_state_root(mut self, state_root: impl Into<std::path::PathBuf>) -> Self {
        self.state_root = Some(state_root.into());
        self
    }

    fn invoke<T: serde::de::DeserializeOwned>(
        &self,
        arguments: &[&std::ffi::OsStr],
    ) -> Result<T, Error> {
        let mut command = std::process::Command::new(&self.executable);
        command.arg("--json");
        if let Some(state_root) = &self.state_root {
            command.arg("--state-root").arg(state_root);
        }
        let output = command
            .args(arguments)
            .output()
            .map_err(|e| Error(format!("unable to invoke Clockwork: {e}")))?;
        if output.status.success() {
            decode(&output.stdout)
        } else {
            match decode::<T>(&output.stderr) {
                Err(error) => Err(error),
                Ok(_) => Err(Error(format!("Clockwork exited with {}", output.status))),
            }
        }
    }

    pub fn register(&self, file: &std::path::Path) -> Result<DefinitionRecord, Error> {
        self.invoke(&["definition".as_ref(), "register".as_ref(), file.as_os_str()])
    }
    pub fn definitions(&self) -> Result<Vec<DefinitionSummary>, Error> {
        self.invoke(&["definition".as_ref(), "list".as_ref()])
    }
    pub fn definition(&self, digest: &str) -> Result<DefinitionRecord, Error> {
        self.invoke(&["definition".as_ref(), "show".as_ref(), digest.as_ref()])
    }
    pub fn switch(&self, key: &str, digest: &str) -> Result<BindingRecord, Error> {
        self.invoke(&[
            "binding".as_ref(),
            "switch".as_ref(),
            key.as_ref(),
            digest.as_ref(),
        ])
    }
    pub fn disable(&self, key: &str, selection: Option<&str>) -> Result<BindingRecord, Error> {
        let mut args: Vec<&std::ffi::OsStr> =
            vec!["binding".as_ref(), "disable".as_ref(), key.as_ref()];
        if let Some(digest) = selection {
            args.push("--select".as_ref());
            args.push(digest.as_ref());
        }
        self.invoke(&args)
    }
    pub fn bindings(&self) -> Result<Vec<BindingRecord>, Error> {
        self.invoke(&["binding".as_ref(), "list".as_ref()])
    }
    pub fn binding(&self, key: &str) -> Result<BindingRecord, Error> {
        self.invoke(&["binding".as_ref(), "show".as_ref(), key.as_ref()])
    }
    pub fn run(&self, key: &str) -> Result<ActivationRecord, Error> {
        self.invoke(&["run".as_ref(), key.as_ref()])
    }
    pub fn history(&self, key: Option<&str>, limit: usize) -> Result<Vec<ActivationRecord>, Error> {
        let limit = limit.to_string();
        let mut args = vec!["history".as_ref(), "--limit".as_ref(), limit.as_ref()];
        if let Some(key) = key {
            args.push(key.as_ref());
        }
        self.invoke(&args)
    }
    pub fn doctor(&self) -> Result<DoctorReport, Error> {
        self.invoke(&["doctor".as_ref()])
    }
}
