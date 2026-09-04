//! Codex app-server adapter for Nucleus.
//!
//! This crate deliberately exposes Nucleus's small invocation domain instead of
//! Codex command-line arguments. The adapter owns the translation to a concrete,
//! inspected Codex installation.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::future;
use std::io::{self, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use base64::Engine as _;
use fs2::FileExt as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tempfile::TempDir;
use thiserror::Error;
use tokio::io::{
    AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, AsyncWriteExt as _, BufReader,
};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

pub use nucleus_core::{BuiltinToolsV1, WorkspaceAccess};

const MAX_PROTOCOL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_MODEL_CATALOG_BYTES: usize = 4 * 1024 * 1024;
const MAX_ACCOUNT_PROTOCOL_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ACCOUNT_STDERR_BYTES: usize = 64 * 1024;
const MAX_AUTH_BYTES: u64 = 4 * 1024 * 1024;
const STDERR_CHUNK_BYTES: usize = 8 * 1024;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);
const CANONICAL_AUTH_REFRESH_TIMEOUT: Duration = Duration::from_secs(8);
const AUTH_REFRESH_SHUTDOWN_GRACE: Duration = Duration::from_millis(250);
// Codex's external-auth bridge waits ten seconds for a host refresh response.
// Leave time for Nucleus to return a bounded protocol error before that bridge
// gives up independently.
const EXTERNAL_AUTH_RESPONSE_TIMEOUT: Duration = Duration::from_millis(9_500);
const AUTH_LOCK_NAME: &str = ".nucleus-auth.lock";
const AUTH_SESSION_LOCK_NAME: &str = ".nucleus-auth-sessions.lock";
const AUTH_STAGING_PREFIX: &str = ".nucleus-auth-operation.";
const AUTH_GENERATION_PREFIX: &str = ".nucleus-auth-generation.";
const AUTH_CONFIG: &str = "cli_auth_credentials_store = \"file\"\n";

/// The exact Codex CLI release whose app-server contract this adapter proves.
/// Supporting another release requires reviewing and updating the protocol
/// semantic checks below.
pub const SUPPORTED_CODEX_VERSION: &str = "0.146.0";

const DISABLED_FEATURES: &[&str] = &[
    "apps",
    "auth_elicitation",
    "browser_use",
    "browser_use_external",
    "browser_use_full_cdp_access",
    "computer_use",
    "deferred_executor",
    "enable_mcp_apps",
    "goals",
    "image_generation",
    "in_app_browser",
    "multi_agent",
    "multi_agent_v2",
    "plugins",
    "remote_plugin",
    "request_permissions_tool",
    "skill_mcp_dependency_install",
    "skill_search",
    "token_budget",
    "tool_call_mcp_elicitation",
    "tool_suggest",
];

const CONFIG_OVERRIDES: &[&str] = &[
    "agents.enabled=false",
    "cli_auth_credentials_store=\"file\"",
    "include_apps_instructions=false",
    "include_collaboration_mode_instructions=false",
    "orchestrator.mcp.enabled=false",
    "orchestrator.skills.enabled=false",
    "skills.bundled.enabled=false",
    "skills.include_instructions=false",
    "tools.experimental_request_user_input.enabled=false",
    "tools.update_plan.enabled=false",
];

/// The directions in which raw app-server protocol records travel.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolDirection {
    ToHarness,
    FromHarness,
}

/// One requester-owned dynamic tool exposed to Codex.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// The stable settings accepted by the Codex adapter.
#[derive(Debug, Clone)]
pub struct CodexRunSpec {
    pub instructions: String,
    pub developer_instructions: Option<String>,
    pub prompt: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub working_directory: PathBuf,
    pub workspace_access: WorkspaceAccess,
    pub builtin_tools: BuiltinToolsV1,
    pub timeout: Duration,
    pub tools: Vec<DynamicTool>,
    /// Exact requester process environment supplied through a memory-only
    /// launch context. `None` inherits the Nucleus daemon environment.
    pub launch_environment: Option<BTreeMap<String, String>>,
}

/// Exact installed-harness identity and the capabilities used for validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessInspection {
    pub harness: String,
    pub version: String,
    pub executable: PathBuf,
    pub models: Vec<ModelCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapability {
    pub model: String,
    pub default_reasoning_effort: Option<String>,
    pub reasoning_efforts: Vec<String>,
    pub supports_local_execution: bool,
    pub supports_web_search: bool,
}

/// Opaque external schema generated by the exact Codex binary.
#[derive(Debug, Clone)]
pub struct GeneratedSchema {
    pub media_type: &'static str,
    pub bytes: Vec<u8>,
}

/// Events are ordered as observed by the adapter. Protocol records retain their
/// exact JSONL bytes, including the line terminator.
#[derive(Debug)]
pub enum CodexEvent {
    Protocol {
        direction: ProtocolDirection,
        bytes: Vec<u8>,
    },
    Stderr(Vec<u8>),
    ToolCall(PendingToolCall),
}

#[derive(Debug)]
pub struct PendingToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
    pub reply: oneshot::Sender<ToolResult>,
}

/// The requester service returns this value through the Nucleus mailbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub success: bool,
    /// JSON or prose presented to the model as one input-text content item.
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOutcome {
    pub thread_id: String,
    pub turn_id: String,
    pub final_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSnapshot {
    pub rate_limits: Value,
    pub usage: Option<Value>,
    pub usage_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationReadiness {
    pub configured: bool,
    pub authenticated: bool,
    pub detail: Option<String>,
}

#[derive(Clone)]
enum WorkerAuthentication {
    ApiKey(String),
    ManagedChatgpt(ManagedChatgptCredentials),
}

// Deliberately has no `Debug` implementation: these values are carried only
// in memory between the authoritative home and one external-auth app-server.
#[derive(Clone)]
struct ManagedChatgptCredentials {
    access_token: String,
    account_id: String,
    // Codex accepts this as optional and otherwise derives it from JWT claims.
    plan_type: Option<String>,
}

impl ManagedChatgptCredentials {
    fn login_params(&self) -> Value {
        json!({
            "type": "chatgptAuthTokens",
            "accessToken": self.access_token,
            "chatgptAccountId": self.account_id,
            "chatgptPlanType": self.plan_type,
        })
    }

    fn refresh_response(&self) -> Value {
        json!({
            "accessToken": self.access_token,
            "chatgptAccountId": self.account_id,
            "chatgptPlanType": self.plan_type,
        })
    }
}

struct ManagedAuthSession {
    harness: CodexHarness,
    credentials: ManagedChatgptCredentials,
    auth_session: AuthSessionLease,
}

#[derive(Default)]
struct SensitiveProtocolState {
    request_ids: BTreeSet<i64>,
    values: Vec<Vec<u8>>,
}

#[derive(Debug)]
struct AuthOperationSupervisor {
    state: StdMutex<AuthOperationState>,
    active: watch::Sender<usize>,
}

#[derive(Debug, Default)]
struct AuthOperationState {
    closing: bool,
    active: usize,
}

impl Default for AuthOperationSupervisor {
    fn default() -> Self {
        let (active, _) = watch::channel(0);
        Self {
            state: StdMutex::new(AuthOperationState::default()),
            active,
        }
    }
}

impl AuthOperationSupervisor {
    fn begin(self: &Arc<Self>) -> Result<AuthOperationActivity, CodexError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closing {
            return Err(CodexError::Authentication(
                "authentication operations are shutting down".to_owned(),
            ));
        }
        state.active = state.active.saturating_add(1);
        self.active.send_replace(state.active);
        drop(state);
        Ok(AuthOperationActivity {
            supervisor: Arc::clone(self),
        })
    }

    fn close(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closing = true;
    }

    async fn wait_for_idle(&self) {
        let mut active = self.active.subscribe();
        loop {
            if *active.borrow_and_update() == 0 {
                return;
            }
            if active.changed().await.is_err() {
                return;
            }
        }
    }
}

struct AuthOperationActivity {
    supervisor: Arc<AuthOperationSupervisor>,
}

impl Drop for AuthOperationActivity {
    fn drop(&mut self) {
        let mut state = self
            .supervisor
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.active = state.active.saturating_sub(1);
        self.supervisor.active.send_replace(state.active);
    }
}

#[derive(Debug, Error)]
pub enum CodexError {
    #[error("Codex harness inspection failed: {0}")]
    Inspection(String),
    #[error("unsupported invocation setting {setting}: {reason}")]
    UnsupportedSetting { setting: String, reason: String },
    #[error("could not prepare isolated Codex state: {0}")]
    Preparation(#[source] io::Error),
    #[error("Codex authentication is unavailable: {0}")]
    Authentication(String),
    #[error("Nucleus-owned Codex authentication is currently in use")]
    AuthenticationBusy,
    #[error("could not start Codex app-server: {0}")]
    Spawn(#[source] io::Error),
    #[error("Codex app-server protocol failed: {0}")]
    Protocol(String),
    #[error("Codex app-server exited unsuccessfully ({status})")]
    HarnessFailure { status: std::process::ExitStatus },
    #[error("the durable event consumer disconnected")]
    EventConsumerDisconnected,
    #[error("job cancelled")]
    Cancelled,
    #[error("job exceeded its timeout")]
    TimedOut,
    #[error("Codex turn ended with status {status}: {detail}")]
    TurnFailed { status: String, detail: String },
}

#[derive(Debug, Clone)]
pub struct CodexHarness {
    executable: PathBuf,
    codex_home: Option<PathBuf>,
    credential_gate: Arc<tokio::sync::Mutex<()>>,
    auth_session_gate: Arc<tokio::sync::RwLock<()>>,
    auth_operation_supervisor: Arc<AuthOperationSupervisor>,
}

impl Default for CodexHarness {
    fn default() -> Self {
        Self::new("codex")
    }
}

impl CodexHarness {
    #[must_use]
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            codex_home: default_codex_home(),
            credential_gate: Arc::new(tokio::sync::Mutex::new(())),
            auth_session_gate: Arc::new(tokio::sync::RwLock::new(())),
            auth_operation_supervisor: Arc::new(AuthOperationSupervisor::default()),
        }
    }

    /// Construct an adapter whose sole persistent authentication authority is
    /// the supplied Nucleus-owned Codex home.
    #[must_use]
    pub fn with_codex_home(executable: impl Into<PathBuf>, codex_home: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            codex_home: Some(codex_home.into()),
            credential_gate: Arc::new(tokio::sync::Mutex::new(())),
            auth_session_gate: Arc::new(tokio::sync::RwLock::new(())),
            auth_operation_supervisor: Arc::new(AuthOperationSupervisor::default()),
        }
    }

    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    #[must_use]
    pub fn codex_home(&self) -> Option<&Path> {
        self.codex_home.as_deref()
    }

    /// Wait until all supervised authentication operations started by this
    /// harness and its clones have completed.
    pub async fn wait_for_auth_idle(&self) {
        self.auth_operation_supervisor.wait_for_idle().await;
    }

    /// Permanently reject new supervised account and token-refresh operations.
    /// Daemon shutdown closes this gate before draining the active count.
    pub fn close_auth_operations(&self) {
        self.auth_operation_supervisor.close();
    }

    /// Inspect the Nucleus-owned credential files without contacting Codex.
    #[must_use]
    pub fn authentication_readiness(&self) -> AuthenticationReadiness {
        let Some(home) = self.codex_home.as_deref() else {
            return AuthenticationReadiness {
                configured: false,
                authenticated: false,
                detail: Some("no Nucleus Codex home is configured".to_owned()),
            };
        };
        match validate_credential_home(home, true) {
            Ok(()) => AuthenticationReadiness {
                configured: true,
                authenticated: true,
                detail: None,
            },
            Err(error) => AuthenticationReadiness {
                configured: home.join("config.toml").is_file(),
                authenticated: false,
                detail: Some(error.to_string()),
            },
        }
    }

    /// Read authenticated account limits, and optionally account usage,
    /// without starting a model turn. The credential lease is held for the
    /// complete app-server session. Account reads share the job-session
    /// barrier, so an attended login cannot replace the active account while
    /// either operation is using it.
    ///
    /// # Errors
    ///
    /// Returns an authentication, spawn, protocol, or timeout error.
    pub async fn read_account_snapshot(
        &self,
        include_usage: bool,
        lease_wait: Duration,
        timeout: Duration,
    ) -> Result<AccountSnapshot, CodexError> {
        let deadline = tokio::time::Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| unsupported("timeout", "cannot represent account deadline"))?;
        let (session, lease) = self.acquire_account_auth(lease_wait).await?;
        let home = self.codex_home.clone().ok_or_else(|| {
            CodexError::Authentication("no Nucleus Codex home is configured".to_owned())
        })?;

        // Account handlers may proactively refresh near-expiry managed tokens.
        // Never give that cancelable child the authoritative home: Codex's file
        // backend writes auth.json in place. A cancelled or timed-out account
        // request can only damage this private staging copy.
        let canonical_auth = read_valid_auth_file(&home.join("auth.json"))?;
        let temporary = create_auth_staging_directory(&home)?;
        let staging_home = temporary.path().join("codex-home");
        prepare_staging_auth_home(&staging_home, Some(&canonical_auth)).await?;
        let harness = self.clone();
        let activity = self.auth_operation_supervisor.begin()?;
        // Once Codex can proactively rotate a refresh token, this task must run
        // through staged validation and promotion even if the HTTP requester
        // disconnects or cancels its future.
        let account = tokio::spawn(async move {
            let _activity = activity;
            let _session = session;
            let _lease = lease;
            let _temporary = temporary;
            let result = harness
                .read_account_snapshot_from_home(&staging_home, include_usage, deadline)
                .await;
            match read_valid_auth_file(&staging_home.join("auth.json")) {
                Ok(staged_auth) => {
                    promote_account_auth_if_advanced(&home, &canonical_auth, &staged_auth)?;
                }
                Err(error) if result.is_ok() => return Err(error),
                Err(_) => {}
            }
            result
        });
        account.await.map_err(|_| {
            CodexError::Authentication("account authentication supervisor failed".to_owned())
        })?
    }

    async fn read_account_snapshot_from_home(
        &self,
        home: &Path,
        include_usage: bool,
        deadline: tokio::time::Instant,
    ) -> Result<AccountSnapshot, CodexError> {
        let mut command = self.command();
        for feature in DISABLED_FEATURES {
            command.args(["--disable", feature]);
        }
        for setting in CONFIG_OVERRIDES {
            command.args(["-c", setting]);
        }
        command
            .args(["app-server", "--stdio"])
            .env("CODEX_HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.as_std_mut().process_group(0);
        let mut child = command.spawn().map_err(CodexError::Spawn)?;
        let group = child.id();
        let mut guard = ProcessGroupGuard::new(group);
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CodexError::Protocol("account app-server omitted stdin".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CodexError::Protocol("account app-server omitted stdout".to_owned()))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| CodexError::Protocol("account app-server omitted stderr".to_owned()))?;
        let stderr_task = tokio::spawn(async move { read_bounded_stderr(&mut stderr).await });
        let mut protocol = AccountProtocol::new(stdin, stdout, deadline);
        let result = async {
            protocol
                .request(
                    0,
                    "initialize",
                    Some(json!({
                        "clientInfo": {
                            "name": "nucleus",
                            "title": "Nucleus",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    })),
                )
                .await?;
            protocol.notify("initialized", Some(json!({}))).await?;
            let rate_limits = protocol
                .request(1, "account/rateLimits/read", Some(json!({})))
                .await?;
            let (usage, usage_error) = if include_usage {
                match protocol
                    .request(2, "account/usage/read", Some(json!({})))
                    .await
                {
                    Ok(value) => (Some(value), None),
                    Err(error) => (None, Some(error.to_string())),
                }
            } else {
                (None, None)
            };
            Ok(AccountSnapshot {
                rate_limits,
                usage,
                usage_error,
            })
        }
        .await;
        terminate_process_group(&mut child, &mut guard).await;
        let _ = stderr_task.await;
        result
    }

    /// Run an attended Codex login through a private staging copy of the
    /// Nucleus-owned credential home, then atomically promote a successful
    /// validated result.
    /// Login owns the cross-process session barrier exclusively and therefore
    /// cannot switch or revoke the account while a job is active. It also uses
    /// the canonical credential lease, so it cannot race token refresh or an
    /// account read.
    ///
    /// # Errors
    ///
    /// Returns an authentication, spawn, or wait error.
    pub async fn login(&self, device_auth: bool) -> Result<std::process::ExitStatus, CodexError> {
        let _session = self.acquire_login_session().await?;
        let _lease = self.acquire_credential_lease().await?;
        let home = self.codex_home.as_deref().ok_or_else(|| {
            CodexError::Authentication("no Nucleus Codex home is configured".to_owned())
        })?;
        let auth_path = home.join("auth.json");
        let canonical_auth = match fs::symlink_metadata(&auth_path) {
            Ok(_) => Some(read_valid_auth_file(&auth_path)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(CodexError::Authentication(format!(
                    "could not inspect {}: {error}",
                    auth_path.display()
                )));
            }
        };
        let temporary = create_auth_staging_directory(home)?;
        let staging_home = temporary.path().join("codex-home");
        prepare_staging_auth_home(&staging_home, canonical_auth.as_deref()).await?;
        let mut command = self.command();
        command.arg("login");
        if device_auth {
            command.arg("--device-auth");
        }
        command
            .env("CODEX_HOME", &staging_home)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        command.as_std_mut().process_group(0);
        let mut child = command.spawn().map_err(CodexError::Spawn)?;
        let guard = ProcessGroupGuard::new(child.id());
        let status = child.wait().await.map_err(CodexError::Spawn)?;
        // The direct login process has exited; synchronously kill any
        // descendants before validating the staged generation.
        drop(guard);
        if status.success() {
            let staged_auth = read_valid_auth_file(&staging_home.join("auth.json"))?;
            promote_auth_document(home, &staged_auth)?;
        }
        Ok(status)
    }

    async fn acquire_account_auth(
        &self,
        wait: Duration,
    ) -> Result<(AuthSessionLease, CredentialLease), CodexError> {
        if wait.is_zero() {
            let session = self.try_acquire_auth_session().await?;
            let credential = self.try_acquire_credential_lease().await?;
            return Ok((session, credential));
        }
        let deadline = tokio::time::Instant::now()
            .checked_add(wait)
            .ok_or(CodexError::AuthenticationBusy)?;
        let session = tokio::time::timeout_at(deadline, self.acquire_auth_session())
            .await
            .map_err(|_| CodexError::AuthenticationBusy)??;
        let credential = tokio::time::timeout_at(deadline, self.acquire_credential_lease())
            .await
            .map_err(|_| CodexError::AuthenticationBusy)??;
        Ok((session, credential))
    }

    async fn acquire_auth_session(&self) -> Result<AuthSessionLease, CodexError> {
        let gate = Arc::clone(&self.auth_session_gate).read_owned().await;
        let home = self.codex_home.clone().ok_or_else(|| {
            CodexError::Authentication("no Nucleus Codex home is configured".to_owned())
        })?;
        let file = tokio::task::spawn_blocking(move || open_auth_session_shared(&home))
            .await
            .map_err(|error| {
                CodexError::Authentication(format!("auth session worker failed: {error}"))
            })??;
        Ok(AuthSessionLease {
            _inner: Arc::new(AuthSessionLeaseInner {
                _gate: gate,
                _file: file,
            }),
        })
    }

    async fn try_acquire_auth_session(&self) -> Result<AuthSessionLease, CodexError> {
        let gate = Arc::clone(&self.auth_session_gate)
            .try_read_owned()
            .map_err(|_| CodexError::AuthenticationBusy)?;
        let home = self.codex_home.clone().ok_or_else(|| {
            CodexError::Authentication("no Nucleus Codex home is configured".to_owned())
        })?;
        let file = tokio::task::spawn_blocking(move || try_open_auth_session_shared(&home))
            .await
            .map_err(|error| {
                CodexError::Authentication(format!("auth session worker failed: {error}"))
            })??;
        Ok(AuthSessionLease {
            _inner: Arc::new(AuthSessionLeaseInner {
                _gate: gate,
                _file: file,
            }),
        })
    }

    async fn acquire_login_session(&self) -> Result<LoginSessionLease, CodexError> {
        let gate = Arc::clone(&self.auth_session_gate).write_owned().await;
        let home = self.codex_home.clone().ok_or_else(|| {
            CodexError::Authentication("no Nucleus Codex home is configured".to_owned())
        })?;
        let file = tokio::task::spawn_blocking(move || open_auth_session_exclusive(&home))
            .await
            .map_err(|error| {
                CodexError::Authentication(format!("login session worker failed: {error}"))
            })??;
        Ok(LoginSessionLease {
            _gate: gate,
            _file: file,
        })
    }

    async fn acquire_credential_lease(&self) -> Result<CredentialLease, CodexError> {
        let gate = Arc::clone(&self.credential_gate).lock_owned().await;
        let home = self.codex_home.clone().ok_or_else(|| {
            CodexError::Authentication("no Nucleus Codex home is configured".to_owned())
        })?;
        let file = tokio::task::spawn_blocking(move || open_credential_lease(&home))
            .await
            .map_err(|error| {
                CodexError::Authentication(format!("credential lease worker failed: {error}"))
            })??;
        Ok(CredentialLease {
            _gate: gate,
            _file: file,
        })
    }

    async fn try_acquire_credential_lease(&self) -> Result<CredentialLease, CodexError> {
        let gate = Arc::clone(&self.credential_gate)
            .try_lock_owned()
            .map_err(|_| CodexError::AuthenticationBusy)?;
        let home = self.codex_home.clone().ok_or_else(|| {
            CodexError::Authentication("no Nucleus Codex home is configured".to_owned())
        })?;
        let file = tokio::task::spawn_blocking(move || try_open_credential_lease(&home))
            .await
            .map_err(|error| {
                CodexError::Authentication(format!("credential lease worker failed: {error}"))
            })??;
        Ok(CredentialLease {
            _gate: gate,
            _file: file,
        })
    }

    /// Return a fresh external-auth snapshot for a worker that received a 401.
    /// The rejected access token is the worker's credential generation. Under
    /// the canonical lease, a changed generation is reused; an unchanged
    /// generation is advanced by Codex's managed refresh flow exactly once.
    async fn refresh_managed_auth(
        &self,
        rejected: &ManagedChatgptCredentials,
        auth_session: AuthSessionLease,
    ) -> Result<ManagedChatgptCredentials, CodexError> {
        let harness = self.clone();
        let rejected = rejected.clone();
        let activity = self.auth_operation_supervisor.begin()?;
        // Dropping the awaiting job detaches this bounded task rather than
        // interrupting credential persistence. The task also retains the
        // job's shared session lease, so attended login remains excluded.
        let refresh = tokio::spawn(async move {
            let _activity = activity;
            let _auth_session = auth_session;
            harness.refresh_managed_auth_supervised(&rejected).await
        });
        refresh.await.map_err(|_| {
            CodexError::Authentication("managed authentication supervisor failed".to_owned())
        })?
    }

    async fn refresh_managed_auth_supervised(
        &self,
        rejected: &ManagedChatgptCredentials,
    ) -> Result<ManagedChatgptCredentials, CodexError> {
        let _lease = self.acquire_credential_lease().await?;
        let home = self.codex_home.as_deref().ok_or_else(|| {
            CodexError::Authentication("no Nucleus Codex home is configured".to_owned())
        })?;
        let canonical_auth = read_valid_auth_file(&home.join("auth.json"))?;
        let current = match worker_authentication_from_document(&canonical_auth)? {
            WorkerAuthentication::ManagedChatgpt(credentials) => credentials,
            WorkerAuthentication::ApiKey(_) => {
                return Err(CodexError::Authentication(
                    "the authoritative authentication mode changed during the job".to_owned(),
                ));
            }
        };
        if current.account_id != rejected.account_id {
            return Err(CodexError::Authentication(
                "the authoritative ChatGPT account changed during the job".to_owned(),
            ));
        }
        if current.access_token != rejected.access_token {
            return Ok(current);
        }

        let temporary = create_auth_staging_directory(home)?;
        let staging_home = temporary.path().join("codex-home");
        prepare_staging_auth_home(&staging_home, Some(&canonical_auth)).await?;
        let refresh_result = self.force_managed_auth_refresh(&staging_home).await;
        let refreshed_auth = match read_valid_auth_file(&staging_home.join("auth.json")) {
            Ok(refreshed_auth) => refreshed_auth,
            Err(error) if refresh_result.is_ok() => return Err(error),
            Err(_) => {
                return refresh_result.and(Err(CodexError::Authentication(
                    "managed Codex token refresh did not leave a valid generation".to_owned(),
                )));
            }
        };
        let refreshed = match worker_authentication_from_document(&refreshed_auth)? {
            WorkerAuthentication::ManagedChatgpt(credentials) => credentials,
            WorkerAuthentication::ApiKey(_) => {
                return Err(CodexError::Authentication(
                    "managed Codex authentication changed modes during refresh".to_owned(),
                ));
            }
        };
        if refreshed.access_token == rejected.access_token {
            return Err(CodexError::Authentication(
                "managed Codex authentication did not advance the rejected credential".to_owned(),
            ));
        }
        if refreshed.account_id != rejected.account_id {
            return Err(CodexError::Authentication(
                "managed Codex authentication changed ChatGPT accounts during refresh".to_owned(),
            ));
        }
        promote_auth_document(home, &refreshed_auth)?;
        refresh_result?;
        Ok(refreshed)
    }

    async fn force_managed_auth_refresh(&self, home: &Path) -> Result<(), CodexError> {
        let deadline = tokio::time::Instant::now()
            .checked_add(CANONICAL_AUTH_REFRESH_TIMEOUT)
            .ok_or_else(|| {
                CodexError::Authentication("could not represent authentication deadline".to_owned())
            })?;
        let mut command = self.command();
        for feature in DISABLED_FEATURES {
            command.args(["--disable", feature]);
        }
        for setting in CONFIG_OVERRIDES {
            command.args(["-c", setting]);
        }
        command
            .args(["app-server", "--stdio"])
            .env("CODEX_HOME", home)
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_ACCESS_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.as_std_mut().process_group(0);
        let mut child = command.spawn().map_err(|_| {
            CodexError::Authentication("could not start managed Codex token refresh".to_owned())
        })?;
        let group = child.id();
        let mut guard = ProcessGroupGuard::new(group);
        let stdin = child.stdin.take().ok_or_else(|| {
            CodexError::Authentication("managed Codex token refresh omitted stdin".to_owned())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            CodexError::Authentication("managed Codex token refresh omitted stdout".to_owned())
        })?;
        let mut stderr = child.stderr.take().ok_or_else(|| {
            CodexError::Authentication("managed Codex token refresh omitted stderr".to_owned())
        })?;
        // Drain diagnostics to keep the child unblocked, but never propagate
        // their contents: the canonical process has access to refresh tokens.
        let stderr_task = tokio::spawn(async move { read_bounded_stderr(&mut stderr).await });
        let mut protocol = AccountProtocol::new(stdin, stdout, deadline);
        let result = async {
            protocol
                .request(
                    0,
                    "initialize",
                    Some(json!({
                        "clientInfo": {
                            "name": "nucleus-auth-broker",
                            "title": "Nucleus authentication broker",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    })),
                )
                .await?;
            protocol.notify("initialized", Some(json!({}))).await?;
            protocol
                .request(1, "account/read", Some(json!({ "refreshToken": true })))
                .await?;
            Ok::<(), CodexError>(())
        }
        .await;
        terminate_process_group_with_grace(&mut child, &mut guard, AUTH_REFRESH_SHUTDOWN_GRACE)
            .await;
        let _ = stderr_task.await;
        result.map_err(|_| {
            CodexError::Authentication("managed Codex token refresh failed".to_owned())
        })
    }

    /// Inspect the exact binary used to execute jobs. Validation never assumes
    /// that a model or effort supported by another Codex version is available.
    ///
    /// # Errors
    ///
    /// Returns an inspection error when the executable cannot be run or its
    /// version/model-catalog output is unsuccessful or malformed.
    pub async fn inspect(&self) -> Result<HarnessInspection, CodexError> {
        let version = self.read_version().await?;

        let catalog_output = self
            .command()
            .args(["debug", "models", "--bundled"])
            .output()
            .await
            .map_err(|error| CodexError::Inspection(error.to_string()))?;
        if !catalog_output.status.success() {
            return Err(CodexError::Inspection(diagnostic(
                "could not read Codex's bundled model catalog",
                &catalog_output.stderr,
            )));
        }
        if catalog_output.stdout.len() > MAX_MODEL_CATALOG_BYTES {
            return Err(CodexError::Inspection(format!(
                "Codex's bundled model catalog exceeded {MAX_MODEL_CATALOG_BYTES} bytes"
            )));
        }
        let catalog: Value = serde_json::from_slice(&catalog_output.stdout)
            .map_err(|error| CodexError::Inspection(format!("invalid model catalog: {error}")))?;
        let entries = catalog
            .get("models")
            .and_then(Value::as_array)
            .ok_or_else(|| CodexError::Inspection("model catalog omitted models".to_owned()))?;
        let mut models = Vec::with_capacity(entries.len());
        for entry in entries {
            let Some(model) = entry.get("slug").and_then(Value::as_str) else {
                continue;
            };
            let reasoning_efforts = entry
                .get("supported_reasoning_levels")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|level| level.get("effort").and_then(Value::as_str))
                .map(str::to_owned)
                .collect();
            let default_reasoning_effort = entry
                .get("default_reasoning_level")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let supports_local_execution = entry
                .get("shell_type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind != "disabled");
            let supports_web_search = entry
                .get("supports_search_tool")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            models.push(ModelCapability {
                model: model.to_owned(),
                default_reasoning_effort,
                reasoning_efforts,
                supports_local_execution,
                supports_web_search,
            });
        }
        if models.is_empty() {
            return Err(CodexError::Inspection(
                "model catalog contained no usable models".to_owned(),
            ));
        }
        Ok(HarnessInspection {
            harness: "codex".to_owned(),
            version,
            executable: self.executable.clone(),
            models,
        })
    }

    async fn read_version(&self) -> Result<String, CodexError> {
        let output = self
            .command()
            .arg("--version")
            .output()
            .await
            .map_err(|error| CodexError::Inspection(error.to_string()))?;
        if !output.status.success() {
            return Err(CodexError::Inspection(diagnostic(
                "codex --version failed",
                &output.stderr,
            )));
        }
        let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let version = line.strip_prefix("codex-cli ").unwrap_or(&line).to_owned();
        if version.is_empty() {
            Err(CodexError::Inspection(
                "codex --version returned an empty version".to_owned(),
            ))
        } else {
            Ok(version)
        }
    }

    /// Ask the installed harness to emit its protocol schema. The resulting
    /// document is stored opaquely by Nucleus and referenced by each JSONL row.
    ///
    /// # Errors
    ///
    /// Returns an error when schema generation fails or the generated bundle
    /// cannot be read.
    pub async fn generate_protocol_schema(&self) -> Result<GeneratedSchema, CodexError> {
        let directory = TempDir::new().map_err(CodexError::Preparation)?;
        let output = self
            .command()
            .args([
                "app-server",
                "generate-json-schema",
                "--experimental",
                "--out",
            ])
            .arg(directory.path())
            .output()
            .await
            .map_err(|error| CodexError::Inspection(error.to_string()))?;
        if !output.status.success() {
            return Err(CodexError::Inspection(diagnostic(
                "Codex schema generation failed",
                &output.stderr,
            )));
        }
        let path = directory
            .path()
            .join("codex_app_server_protocol.schemas.json");
        let bytes = tokio::fs::read(path)
            .await
            .map_err(CodexError::Preparation)?;
        Ok(GeneratedSchema {
            media_type: "application/schema+json",
            bytes,
        })
    }

    /// Validate the complete portable run specification against one inspected
    /// Codex installation.
    ///
    /// # Errors
    ///
    /// Returns [`CodexError::UnsupportedSetting`] for every setting the exact
    /// inspected harness cannot enforce.
    pub fn validate(
        &self,
        inspection: &HarnessInspection,
        spec: &CodexRunSpec,
    ) -> Result<(), CodexError> {
        self.validate_inspection_identity(inspection)?;
        if !spec.working_directory.is_absolute() {
            return Err(unsupported("workingDirectory", "must be an absolute path"));
        }
        if !spec.working_directory.is_dir() {
            return Err(unsupported(
                "workingDirectory",
                "must name an existing directory",
            ));
        }
        if spec.timeout.is_zero() {
            return Err(unsupported("timeoutSeconds", "must be greater than zero"));
        }
        if std::time::Instant::now()
            .checked_add(spec.timeout)
            .is_none()
        {
            return Err(unsupported(
                "timeoutSeconds",
                "is too large for this platform's monotonic clock",
            ));
        }
        if spec.builtin_tools.local_execution
            && matches!(spec.workspace_access, WorkspaceAccess::None)
        {
            return Err(unsupported(
                "builtinTools.localExecution",
                "the Codex adapter cannot guarantee local execution with no filesystem access",
            ));
        }
        let model = inspection
            .models
            .iter()
            .find(|candidate| candidate.model == spec.model)
            .ok_or_else(|| {
                unsupported(
                    "model",
                    format!(
                        "{} is not in Codex {}'s bundled catalog",
                        spec.model, inspection.version
                    ),
                )
            })?;
        if let Some(requested_effort) = &spec.reasoning_effort
            && !model
                .reasoning_efforts
                .iter()
                .any(|effort| effort == requested_effort)
        {
            return Err(unsupported(
                "reasoningEffort",
                format!(
                    "{} does not support {:?} in Codex {}",
                    spec.model, requested_effort, inspection.version
                ),
            ));
        }
        if spec.builtin_tools.local_execution && !model.supports_local_execution {
            return Err(unsupported(
                "builtinTools.localExecution",
                format!(
                    "{} does not support local execution in Codex {}",
                    spec.model, inspection.version
                ),
            ));
        }
        if spec.builtin_tools.web_search && !model.supports_web_search {
            return Err(unsupported(
                "builtinTools.webSearch",
                format!(
                    "{} does not support web search in Codex {}",
                    spec.model, inspection.version
                ),
            ));
        }
        validate_tools(&spec.tools)
    }

    /// Verify that the generated schema for the inspected binary contains the
    /// exact app-server semantics used by the v1 adapter.
    ///
    /// This is intentionally structural rather than a schema digest allowlist:
    /// unrelated additions to Codex's generated bundle do not break admission,
    /// while removal or renaming of anything Nucleus sends or consumes does.
    ///
    /// # Errors
    ///
    /// Returns an unsupported-setting error for an unbound harness version or
    /// an app-server schema missing any required v1 protocol semantic.
    pub fn validate_protocol_schema(
        &self,
        inspection: &HarnessInspection,
        schema: &GeneratedSchema,
    ) -> Result<(), CodexError> {
        self.validate_inspection_identity(inspection)?;
        let document: Value = serde_json::from_slice(&schema.bytes).map_err(|error| {
            incompatible_protocol(format!("generated schema is not JSON: {error}"))
        })?;
        verify_protocol_semantics(&document)
    }

    fn validate_inspection_identity(
        &self,
        inspection: &HarnessInspection,
    ) -> Result<(), CodexError> {
        if inspection.harness != "codex" {
            return Err(unsupported(
                "harness",
                "the Codex adapter only accepts codex",
            ));
        }
        if inspection.executable != self.executable {
            return Err(unsupported(
                "harness",
                "the inspected executable does not match this adapter instance",
            ));
        }
        if inspection.version != SUPPORTED_CODEX_VERSION {
            return Err(unsupported(
                "harness.version",
                format!(
                    "Codex {} is not bound to this adapter; supported version is {SUPPORTED_CODEX_VERSION}",
                    inspection.version
                ),
            ));
        }
        Ok(())
    }

    async fn write_model_catalog(
        &self,
        directory: &Path,
        codex_home: &Path,
        spec: &CodexRunSpec,
    ) -> Result<PathBuf, CodexError> {
        let mut command = self.command();
        apply_launch_environment(&mut command, spec);
        command
            .args(["debug", "models", "--bundled"])
            .env("CODEX_HOME", codex_home)
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let output = command
            .output()
            .await
            .map_err(|error| CodexError::Inspection(error.to_string()))?;
        if !output.status.success() {
            return Err(CodexError::Inspection(diagnostic(
                "could not read Codex's bundled model catalog",
                &output.stderr,
            )));
        }
        if output.stdout.len() > MAX_MODEL_CATALOG_BYTES {
            return Err(CodexError::Inspection(format!(
                "Codex's bundled model catalog exceeded {MAX_MODEL_CATALOG_BYTES} bytes"
            )));
        }
        let catalog = configured_model_catalog(&output.stdout, spec)?;
        let path = directory.join("models.json");
        tokio::fs::write(
            &path,
            serde_json::to_vec(&catalog).map_err(|error| {
                CodexError::Protocol(format!("could not encode model catalog: {error}"))
            })?,
        )
        .await
        .map_err(CodexError::Preparation)?;
        Ok(path)
    }

    /// Run one ephemeral Codex thread. The caller must durably consume every
    /// event; disconnecting the event channel fails the run.
    ///
    /// # Errors
    ///
    /// Returns an error for validation, preparation, protocol, tool-mailbox,
    /// cancellation, timeout, or harness-turn failure. The harness process group
    /// is cleaned up before ordinary return and by a drop guard if aborted.
    #[allow(clippy::too_many_lines)]
    pub async fn run(
        &self,
        inspection: &HarnessInspection,
        spec: CodexRunSpec,
        events: mpsc::Sender<CodexEvent>,
        mut cancelled: watch::Receiver<bool>,
    ) -> Result<CodexOutcome, CodexError> {
        self.validate(inspection, &spec)?;
        let deadline = tokio::time::Instant::now()
            .checked_add(spec.timeout)
            .ok_or_else(|| unsupported("timeoutSeconds", "cannot represent deadline"))?;
        let auth_session = {
            let session = self.acquire_auth_session();
            tokio::pin!(session);
            let cancellation = wait_for_cancellation(&mut cancelled);
            tokio::pin!(cancellation);
            tokio::select! {
                result = &mut session => result?,
                () = &mut cancellation => return Err(CodexError::Cancelled),
                () = tokio::time::sleep_until(deadline) => return Err(CodexError::TimedOut),
            }
        };
        let worker_authentication = {
            let lease = self.acquire_credential_lease();
            tokio::pin!(lease);
            let cancellation = wait_for_cancellation(&mut cancelled);
            tokio::pin!(cancellation);
            let _lease = tokio::select! {
                result = &mut lease => result?,
                () = &mut cancellation => return Err(CodexError::Cancelled),
                () = tokio::time::sleep_until(deadline) => return Err(CodexError::TimedOut),
            };
            let persistent_home = self.codex_home.as_deref().ok_or_else(|| {
                CodexError::Authentication("no Nucleus Codex home is configured".to_owned())
            })?;
            read_worker_authentication(persistent_home)?
        };
        let temporary = TempDir::new().map_err(CodexError::Preparation)?;
        let codex_home = temporary.path().join("codex-home");
        tokio::fs::create_dir(&codex_home)
            .await
            .map_err(CodexError::Preparation)?;
        prepare_isolated_codex_home(&codex_home, &worker_authentication).await?;
        let catalog_path = {
            let preparation = async {
                let current_version = self.read_version().await?;
                if current_version != inspection.version {
                    return Err(CodexError::Inspection(format!(
                        "Codex changed from version {} to {current_version} after admission",
                        inspection.version
                    )));
                }
                self.write_model_catalog(temporary.path(), &codex_home, &spec)
                    .await
            };
            tokio::pin!(preparation);
            let cancellation = wait_for_cancellation(&mut cancelled);
            tokio::pin!(cancellation);
            tokio::select! {
                result = &mut preparation => result?,
                () = &mut cancellation => return Err(CodexError::Cancelled),
                () = tokio::time::sleep_until(deadline) => return Err(CodexError::TimedOut),
            }
        };

        let effective_cwd = match spec.workspace_access {
            WorkspaceAccess::None => {
                let path = temporary.path().join("workspace");
                tokio::fs::create_dir(&path)
                    .await
                    .map_err(CodexError::Preparation)?;
                path
            }
            WorkspaceAccess::ReadOnly | WorkspaceAccess::ReadWrite => {
                spec.working_directory.clone()
            }
        };

        let mut command =
            self.app_server_command(&spec, &effective_cwd, &codex_home, &catalog_path)?;

        let sensitive_values = match &worker_authentication {
            WorkerAuthentication::ApiKey(api_key) => vec![api_key.as_bytes().to_vec()],
            WorkerAuthentication::ManagedChatgpt(credentials) => {
                vec![credentials.access_token.as_bytes().to_vec()]
            }
        };
        let managed_auth = match worker_authentication {
            WorkerAuthentication::ApiKey(_) => None,
            WorkerAuthentication::ManagedChatgpt(credentials) => Some(ManagedAuthSession {
                harness: self.clone(),
                credentials,
                auth_session: auth_session.clone(),
            }),
        };

        let managed_worker = managed_auth.is_some();
        let mut child = command.spawn().map_err(CodexError::Spawn)?;
        let group = child.id();
        // Tokio's `kill_on_drop` targets only the direct child. The guard also
        // kills its process group if this future is aborted mid-run.
        let mut process_group = ProcessGroupGuard::new(group);
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CodexError::Protocol("app-server did not expose stdin".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CodexError::Protocol("app-server did not expose stdout".to_owned()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| CodexError::Protocol("app-server did not expose stderr".to_owned()))?;

        let sensitive_protocol = Arc::new(StdMutex::new(SensitiveProtocolState {
            request_ids: BTreeSet::new(),
            values: sensitive_values,
        }));
        let (lines_tx, lines_rx) = mpsc::channel(64);
        let stdout_task = tokio::spawn(read_protocol_lines(
            stdout,
            lines_tx,
            events.clone(),
            Arc::clone(&sensitive_protocol),
        ));
        let stderr_task = if managed_worker {
            tokio::spawn(discard_stderr(stderr))
        } else {
            tokio::spawn(read_stderr(stderr, events.clone()))
        };
        let result = {
            let protocol = ProtocolClient {
                stdin,
                lines: lines_rx,
                events: events.clone(),
                next_request_id: 0,
                final_message: String::new(),
                active_thread_id: None,
                active_turn_id: None,
                allowed_tools: spec.tools.iter().map(|tool| tool.name.clone()).collect(),
                seen_tool_calls: BTreeSet::new(),
                managed_auth,
                sensitive_protocol,
            }
            .run(&spec, &effective_cwd);
            tokio::pin!(protocol);

            let timeout = tokio::time::sleep_until(deadline);
            tokio::pin!(timeout);
            let cancellation = wait_for_cancellation(&mut cancelled);
            tokio::pin!(cancellation);

            tokio::select! {
                result = &mut protocol => result,
                () = &mut cancellation => Err(CodexError::Cancelled),
                () = &mut timeout => Err(CodexError::TimedOut),
            }
        };

        // A clean stdout EOF is ambiguous until the direct child status is
        // known. Give an already-closing child a brief chance to report its
        // status so an unsuccessful harness exit is not mislabeled as malformed
        // JSON-RPC.
        let observed_exit = if matches!(
            &result,
            Err(CodexError::Protocol(message)) if message == "app-server stdout closed"
        ) {
            match tokio::time::timeout(Duration::from_millis(250), child.wait()).await {
                Ok(Ok(status)) => Some(status),
                Ok(Err(error)) => {
                    return Err(CodexError::Protocol(format!(
                        "could not read app-server exit status: {error}"
                    )));
                }
                Err(_) => None,
            }
        } else {
            child.try_wait().map_err(|error| {
                CodexError::Protocol(format!("could not read app-server exit status: {error}"))
            })?
        };

        terminate_process_group(&mut child, &mut process_group).await;
        let stdout_result = join_reader(stdout_task, "stdout").await;
        let stderr_result = join_reader(stderr_task, "stderr").await;
        drop(events);

        stdout_result?;
        stderr_result?;
        match (result, observed_exit) {
            (Err(CodexError::Protocol(message)), Some(status))
                if message == "app-server stdout closed" && !status.success() =>
            {
                Err(CodexError::HarnessFailure { status })
            }
            (result, _) => result,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .env_remove("CODEX_EXEC_SERVER_URL")
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_ACCESS_TOKEN")
            .kill_on_drop(true);
        command
    }

    fn app_server_command(
        &self,
        spec: &CodexRunSpec,
        effective_cwd: &Path,
        codex_home: &Path,
        catalog_path: &Path,
    ) -> Result<Command, CodexError> {
        let mut command = self.command();
        apply_launch_environment(&mut command, spec);
        for feature in DISABLED_FEATURES {
            command.args(["--disable", feature]);
        }
        for setting in CONFIG_OVERRIDES {
            command.args(["-c", setting]);
        }
        for feature in disabled_features(spec) {
            command.args(["--disable", feature]);
        }
        for setting in runtime_config(spec) {
            command.args(["-c", &setting]);
        }
        let catalog_setting = format!(
            "model_catalog_json={}",
            serde_json::to_string(&catalog_path.display().to_string()).map_err(|error| {
                CodexError::Protocol(format!("could not encode model catalog path: {error}"))
            })?
        );
        command
            .args(["-c", &catalog_setting, "app-server", "--stdio"])
            .current_dir(effective_cwd)
            .env("CODEX_HOME", codex_home)
            .env_remove("CODEX_EXEC_SERVER_URL")
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_ACCESS_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command.as_std_mut().process_group(0);
        Ok(command)
    }
}

struct ProtocolClient {
    stdin: ChildStdin,
    lines: mpsc::Receiver<io::Result<Vec<u8>>>,
    events: mpsc::Sender<CodexEvent>,
    next_request_id: i64,
    final_message: String,
    active_thread_id: Option<String>,
    active_turn_id: Option<String>,
    allowed_tools: BTreeSet<String>,
    seen_tool_calls: BTreeSet<String>,
    managed_auth: Option<ManagedAuthSession>,
    sensitive_protocol: Arc<StdMutex<SensitiveProtocolState>>,
}

impl ProtocolClient {
    #[allow(clippy::too_many_lines)]
    async fn run(
        mut self,
        spec: &CodexRunSpec,
        effective_cwd: &Path,
    ) -> Result<CodexOutcome, CodexError> {
        self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "nucleus",
                    "title": "Nucleus",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": { "experimentalApi": true }
            }),
        )
        .await?;
        self.notify("initialized", None).await?;
        if let Some(auth) = &self.managed_auth {
            let params = auth.credentials.login_params();
            let response = self
                .request_sensitive("account/login/start", params)
                .await?;
            if response.get("type").and_then(Value::as_str) != Some("chatgptAuthTokens") {
                return Err(CodexError::Authentication(
                    "Codex did not accept Nucleus-managed ChatGPT authentication".to_owned(),
                ));
            }
        }
        self.ensure_no_mcp_servers().await?;

        let dynamic_tools = dynamic_tool_specs(&spec.tools);
        let sandbox = match spec.workspace_access {
            WorkspaceAccess::None | WorkspaceAccess::ReadOnly => "read-only",
            WorkspaceAccess::ReadWrite => "workspace-write",
        };
        let mut thread_params = json!({
            "model": spec.model,
            "cwd": effective_cwd.display().to_string(),
            "approvalPolicy": "never",
            "sandbox": sandbox,
            "baseInstructions": spec.instructions,
            "ephemeral": true,
            "experimentalRawEvents": true,
            "dynamicTools": dynamic_tools
        });
        if let Some(instructions) = &spec.developer_instructions {
            thread_params["developerInstructions"] = json!(instructions);
        }
        if matches!(spec.workspace_access, WorkspaceAccess::None) {
            thread_params["environments"] = json!([]);
        }
        let thread = self.request("thread/start", thread_params).await?;
        let thread_id = required_string(&thread, "/thread/id", "thread/start response")?;
        self.active_thread_id = Some(thread_id.clone());
        let mut turn_params = json!({
            "threadId": thread_id,
            "input": [{ "type": "text", "text": spec.prompt, "textElements": [] }]
        });
        if let Some(effort) = &spec.reasoning_effort {
            turn_params["effort"] = json!(effort);
        }
        if matches!(spec.workspace_access, WorkspaceAccess::None) {
            turn_params["environments"] = json!([]);
        }
        let turn = self.request("turn/start", turn_params).await?;
        let turn_id = required_string(&turn, "/turn/id", "turn/start response")?;
        self.active_turn_id = Some(turn_id.clone());

        loop {
            let message = self.receive().await?;
            if self.handle_server_request(&message).await? {
                continue;
            }
            self.record_agent_message(&message);
            if message.get("method").and_then(Value::as_str) != Some("turn/completed") {
                continue;
            }
            if message.pointer("/params/threadId").and_then(Value::as_str)
                != Some(thread_id.as_str())
                || message.pointer("/params/turn/id").and_then(Value::as_str)
                    != Some(turn_id.as_str())
            {
                continue;
            }
            let Some(status) = message
                .pointer("/params/turn/status")
                .and_then(Value::as_str)
            else {
                return Err(CodexError::Protocol(
                    "turn/completed omitted turn status".to_owned(),
                ));
            };
            if status == "completed" {
                return Ok(CodexOutcome {
                    thread_id,
                    turn_id,
                    final_message: self.final_message,
                });
            }
            let detail = message
                .pointer("/params/turn/error/message")
                .and_then(Value::as_str)
                .unwrap_or("no error detail was provided")
                .to_owned();
            return Err(CodexError::TurnFailed {
                status: status.to_owned(),
                detail,
            });
        }
    }

    async fn ensure_no_mcp_servers(&mut self) -> Result<(), CodexError> {
        let response = self
            .request(
                "mcpServerStatus/list",
                json!({
                    "cursor": null,
                    "limit": null,
                    "detail": "toolsAndAuthOnly",
                    "threadId": null
                }),
            )
            .await?;
        let servers = response
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                CodexError::Protocol("mcpServerStatus/list response omitted data".to_owned())
            })?;
        if servers.is_empty() {
            Ok(())
        } else {
            Err(CodexError::Protocol(
                "isolated app-server unexpectedly loaded an MCP server".to_owned(),
            ))
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, CodexError> {
        self.request_with_error_policy(method, params, false).await
    }

    // Authentication requests contain access tokens. A harness rejection is
    // reported without copying its free-form error detail into diagnostics.
    async fn request_sensitive(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, CodexError> {
        self.request_with_error_policy(method, params, true).await
    }

    async fn request_with_error_policy(
        &mut self,
        method: &str,
        params: Value,
        sensitive: bool,
    ) -> Result<Value, CodexError> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        if sensitive {
            self.sensitive_protocol
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .request_ids
                .insert(id);
            self.write_sensitive(&request).await?;
        } else {
            self.write(&request).await?;
        }
        loop {
            let message = self.receive().await?;
            if self.handle_server_request(&message).await? {
                continue;
            }
            self.record_agent_message(&message);
            if message.get("id") != Some(&json!(id)) {
                continue;
            }
            if let Some(error) = message.get("error") {
                if sensitive {
                    return Err(CodexError::Authentication(format!(
                        "{method} was rejected by Codex"
                    )));
                }
                let detail = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown JSON-RPC error");
                return Err(CodexError::Protocol(format!("{method} failed: {detail}")));
            }
            return message
                .get("result")
                .cloned()
                .ok_or_else(|| CodexError::Protocol(format!("{method} response omitted result")));
        }
    }

    async fn notify(&mut self, method: &str, params: Option<Value>) -> Result<(), CodexError> {
        let mut message = json!({ "jsonrpc": "2.0", "method": method });
        if let Some(params) = params {
            message["params"] = params;
        }
        self.write(&message).await
    }

    async fn receive(&mut self) -> Result<Value, CodexError> {
        let bytes = self
            .lines
            .recv()
            .await
            .ok_or_else(|| CodexError::Protocol("app-server stdout closed".to_owned()))?
            .map_err(|error| CodexError::Protocol(error.to_string()))?;
        serde_json::from_slice(&bytes).map_err(|error| {
            CodexError::Protocol(format!("app-server emitted invalid JSON-RPC: {error}"))
        })
    }

    async fn write(&mut self, message: &Value) -> Result<(), CodexError> {
        let mut bytes = serde_json::to_vec(message)
            .map_err(|error| CodexError::Protocol(format!("could not encode JSON-RPC: {error}")))?;
        bytes.push(b'\n');
        self.events
            .send(CodexEvent::Protocol {
                direction: ProtocolDirection::ToHarness,
                bytes: bytes.clone(),
            })
            .await
            .map_err(|_| CodexError::EventConsumerDisconnected)?;
        self.stdin
            .write_all(&bytes)
            .await
            .map_err(|error| CodexError::Protocol(format!("could not write JSON-RPC: {error}")))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| CodexError::Protocol(format!("could not flush JSON-RPC: {error}")))
    }

    async fn write_sensitive(&mut self, message: &Value) -> Result<(), CodexError> {
        let mut bytes = serde_json::to_vec(message)
            .map_err(|_| CodexError::Protocol("could not encode sensitive JSON-RPC".to_owned()))?;
        bytes.push(b'\n');
        self.stdin
            .write_all(&bytes)
            .await
            .map_err(|_| CodexError::Protocol("could not write sensitive JSON-RPC".to_owned()))?;
        self.stdin
            .flush()
            .await
            .map_err(|_| CodexError::Protocol("could not flush sensitive JSON-RPC".to_owned()))
    }

    async fn handle_server_request(&mut self, message: &Value) -> Result<bool, CodexError> {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(false);
        };
        let Some(id) = message.get("id").cloned() else {
            return Ok(false);
        };
        if method == "account/chatgptAuthTokens/refresh" {
            return self.handle_auth_refresh(id, message).await;
        }
        if method != "item/tool/call" {
            self.write(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": "unsupported server request" }
            }))
            .await?;
            return Err(CodexError::Protocol(
                "app-server requested an unsupported operation".to_owned(),
            ));
        }

        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        if params
            .get("namespace")
            .is_some_and(|value| !value.is_null())
        {
            return Err(CodexError::Protocol(
                "dynamic tool call unexpectedly included a namespace".to_owned(),
            ));
        }
        let call_id = required_string(&params, "/callId", "tool call")?;
        let name = required_string(&params, "/tool", "tool call")?;
        let thread_id = required_string(&params, "/threadId", "tool call")?;
        let turn_id = required_string(&params, "/turnId", "tool call")?;
        if self.active_thread_id.as_deref() != Some(thread_id.as_str())
            || self.active_turn_id.as_deref() != Some(turn_id.as_str())
        {
            return Err(CodexError::Protocol(
                "dynamic tool call did not belong to the active thread and turn".to_owned(),
            ));
        }
        if !self.allowed_tools.contains(&name) {
            return Err(CodexError::Protocol(format!(
                "app-server requested undeclared dynamic tool {name:?}"
            )));
        }
        if !self.seen_tool_calls.insert(call_id.clone()) {
            return Err(CodexError::Protocol(format!(
                "app-server repeated dynamic tool call id {call_id:?}"
            )));
        }
        let arguments = params
            .get("arguments")
            .cloned()
            .ok_or_else(|| CodexError::Protocol("tool call omitted arguments".to_owned()))?;
        if !arguments.is_object() {
            return Err(CodexError::Protocol(
                "tool call arguments were not an object".to_owned(),
            ));
        }
        let (reply, result) = oneshot::channel();
        self.events
            .send(CodexEvent::ToolCall(PendingToolCall {
                call_id,
                name,
                arguments,
                reply,
            }))
            .await
            .map_err(|_| CodexError::EventConsumerDisconnected)?;
        let result = result.await.map_err(|_| {
            CodexError::Protocol("tool call result producer disconnected".to_owned())
        })?;
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "contentItems": [{ "type": "inputText", "text": result.content }],
                "success": result.success
            }
        }))
        .await?;
        Ok(true)
    }

    async fn handle_auth_refresh(
        &mut self,
        id: Value,
        message: &Value,
    ) -> Result<bool, CodexError> {
        let Some(session) = &self.managed_auth else {
            self.write(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32600, "message": "external authentication is not active" }
            }))
            .await?;
            return Err(CodexError::Protocol(
                "app-server requested external authentication refresh unexpectedly".to_owned(),
            ));
        };
        let params = message.get("params").ok_or_else(|| {
            CodexError::Protocol("authentication refresh omitted params".to_owned())
        })?;
        if params.get("reason").and_then(Value::as_str) != Some("unauthorized") {
            return Err(CodexError::Protocol(
                "authentication refresh used an unsupported reason".to_owned(),
            ));
        }
        if let Some(previous_account_id) = params.get("previousAccountId")
            && !previous_account_id.is_null()
            && previous_account_id.as_str() != Some(session.credentials.account_id.as_str())
        {
            return Err(CodexError::Authentication(
                "Codex requested refresh for a different ChatGPT account".to_owned(),
            ));
        }
        let harness = session.harness.clone();
        let rejected = session.credentials.clone();
        let auth_session = session.auth_session.clone();
        let Ok(Ok(refreshed)) = tokio::time::timeout(
            EXTERNAL_AUTH_RESPONSE_TIMEOUT,
            harness.refresh_managed_auth(&rejected, auth_session),
        )
        .await
        else {
            self.write(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32603,
                    "message": "Nucleus managed authentication refresh failed"
                }
            }))
            .await?;
            return Err(CodexError::Authentication(
                "managed ChatGPT authentication refresh failed".to_owned(),
            ));
        };
        let response = refreshed.refresh_response();
        {
            let mut sensitive = self
                .sensitive_protocol
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let access_token = refreshed.access_token.as_bytes();
            if !sensitive
                .values
                .iter()
                .any(|known| known.as_slice() == access_token)
            {
                sensitive.values.push(access_token.to_vec());
            }
        }
        self.write_sensitive(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": response
        }))
        .await?;
        if let Some(session) = &mut self.managed_auth {
            session.credentials = refreshed;
        }
        Ok(true)
    }

    fn record_agent_message(&mut self, message: &Value) {
        let (Some(thread_id), Some(turn_id)) = (
            self.active_thread_id.as_deref(),
            self.active_turn_id.as_deref(),
        ) else {
            return;
        };
        let item = match message.get("method").and_then(Value::as_str) {
            Some("item/completed")
                if message.pointer("/params/threadId").and_then(Value::as_str)
                    == Some(thread_id)
                    && message.pointer("/params/turnId").and_then(Value::as_str)
                        == Some(turn_id) =>
            {
                message.pointer("/params/item")
            }
            Some("turn/completed")
                if message.pointer("/params/threadId").and_then(Value::as_str)
                    == Some(thread_id)
                    && message.pointer("/params/turn/id").and_then(Value::as_str)
                        == Some(turn_id) =>
            {
                message
                    .pointer("/params/turn/items")
                    .and_then(Value::as_array)
                    .and_then(|items| {
                        items.iter().rev().find(|item| {
                            item.get("type").and_then(Value::as_str) == Some("agentMessage")
                        })
                    })
            }
            _ => None,
        };
        if let Some(text) = item
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("agentMessage"))
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
        {
            text.clone_into(&mut self.final_message);
        }
    }
}

#[allow(clippy::too_many_lines)]
fn verify_protocol_semantics(document: &Value) -> Result<(), CodexError> {
    for (definition, method, params_ref) in [
        (
            "ClientRequest",
            "initialize",
            "#/definitions/InitializeParams",
        ),
        (
            "ClientRequest",
            "thread/start",
            "#/definitions/v2/ThreadStartParams",
        ),
        (
            "ClientRequest",
            "turn/start",
            "#/definitions/v2/TurnStartParams",
        ),
        (
            "ClientRequest",
            "mcpServerStatus/list",
            "#/definitions/v2/ListMcpServerStatusParams",
        ),
        (
            "ClientRequest",
            "account/login/start",
            "#/definitions/v2/LoginAccountParams",
        ),
        (
            "ClientRequest",
            "account/read",
            "#/definitions/v2/GetAccountParams",
        ),
        (
            "ServerRequest",
            "item/tool/call",
            "#/definitions/DynamicToolCallParams",
        ),
        (
            "ServerRequest",
            "account/chatgptAuthTokens/refresh",
            "#/definitions/ChatgptAuthTokensRefreshParams",
        ),
        (
            "ServerNotification",
            "item/completed",
            "#/definitions/v2/ItemCompletedNotification",
        ),
        (
            "ServerNotification",
            "turn/completed",
            "#/definitions/v2/TurnCompletedNotification",
        ),
        (
            "ServerNotification",
            "thread/tokenUsage/updated",
            "#/definitions/v2/ThreadTokenUsageUpdatedNotification",
        ),
    ] {
        require_method(document, definition, method, params_ref)?;
    }

    let thread_start = protocol_definition(document, &["v2", "ThreadStartParams"])?;
    require_properties(
        thread_start,
        "v2/ThreadStartParams",
        &[
            "approvalPolicy",
            "baseInstructions",
            "cwd",
            "developerInstructions",
            "dynamicTools",
            "ephemeral",
            "environments",
            "experimentalRawEvents",
            "model",
            "sandbox",
        ],
    )?;
    require_enum_value(
        protocol_definition(document, &["v2", "AskForApproval"])?,
        "v2/AskForApproval",
        "never",
    )?;
    let sandbox = protocol_definition(document, &["v2", "SandboxMode"])?;
    require_enum_value(sandbox, "v2/SandboxMode", "read-only")?;
    require_enum_value(sandbox, "v2/SandboxMode", "workspace-write")?;

    let turn_start = protocol_definition(document, &["v2", "TurnStartParams"])?;
    require_properties(
        turn_start,
        "v2/TurnStartParams",
        &["effort", "environments", "input", "threadId"],
    )?;
    require_required(turn_start, "v2/TurnStartParams", &["input", "threadId"])?;

    let login = protocol_definition(document, &["v2", "LoginAccountParams"])?;
    let external_login = login
        .get("oneOf")
        .and_then(Value::as_array)
        .and_then(|variants| {
            variants.iter().find(|variant| {
                contains_enum_value(
                    variant.pointer("/properties/type").unwrap_or(&Value::Null),
                    "chatgptAuthTokens",
                )
            })
        })
        .ok_or_else(|| {
            incompatible_protocol("v2/LoginAccountParams omitted the chatgptAuthTokens variant")
        })?;
    require_properties(
        external_login,
        "v2/LoginAccountParams chatgptAuthTokens variant",
        &["accessToken", "chatgptAccountId", "chatgptPlanType", "type"],
    )?;
    require_required(
        external_login,
        "v2/LoginAccountParams chatgptAuthTokens variant",
        &["accessToken", "chatgptAccountId", "type"],
    )?;
    require_properties(
        protocol_definition(document, &["v2", "GetAccountParams"])?,
        "v2/GetAccountParams",
        &["refreshToken"],
    )?;
    let refresh_params = protocol_definition(document, &["ChatgptAuthTokensRefreshParams"])?;
    require_properties(
        refresh_params,
        "ChatgptAuthTokensRefreshParams",
        &["previousAccountId", "reason"],
    )?;
    require_required(
        refresh_params,
        "ChatgptAuthTokensRefreshParams",
        &["reason"],
    )?;
    require_enum_value(
        protocol_definition(document, &["ChatgptAuthTokensRefreshReason"])?,
        "ChatgptAuthTokensRefreshReason",
        "unauthorized",
    )?;
    let refresh_response = protocol_definition(document, &["ChatgptAuthTokensRefreshResponse"])?;
    require_properties(
        refresh_response,
        "ChatgptAuthTokensRefreshResponse",
        &["accessToken", "chatgptAccountId", "chatgptPlanType"],
    )?;
    require_required(
        refresh_response,
        "ChatgptAuthTokensRefreshResponse",
        &["accessToken", "chatgptAccountId"],
    )?;

    let dynamic_tool = protocol_definition(document, &["v2", "DynamicToolSpec"])?;
    let function_tool = dynamic_tool
        .get("oneOf")
        .and_then(Value::as_array)
        .and_then(|variants| {
            variants.iter().find(|variant| {
                contains_enum_value(
                    variant.pointer("/properties/type").unwrap_or(&Value::Null),
                    "function",
                )
            })
        })
        .ok_or_else(|| {
            incompatible_protocol("v2/DynamicToolSpec omitted the function tool variant")
        })?;
    require_required(
        function_tool,
        "v2/DynamicToolSpec function variant",
        &["description", "inputSchema", "name", "type"],
    )?;

    require_required(
        protocol_definition(document, &["DynamicToolCallParams"])?,
        "DynamicToolCallParams",
        &["arguments", "callId", "threadId", "tool", "turnId"],
    )?;
    require_required(
        protocol_definition(document, &["v2", "ThreadStartResponse"])?,
        "v2/ThreadStartResponse",
        &["thread"],
    )?;
    require_required(
        protocol_definition(document, &["v2", "TurnStartResponse"])?,
        "v2/TurnStartResponse",
        &["turn"],
    )?;
    require_required(
        protocol_definition(document, &["v2", "Thread"])?,
        "v2/Thread",
        &["id"],
    )?;
    require_required(
        protocol_definition(document, &["v2", "Turn"])?,
        "v2/Turn",
        &["id", "status"],
    )?;
    require_enum_value(
        protocol_definition(document, &["v2", "TurnStatus"])?,
        "v2/TurnStatus",
        "completed",
    )?;
    require_required(
        protocol_definition(document, &["v2", "ItemCompletedNotification"])?,
        "v2/ItemCompletedNotification",
        &["item", "threadId", "turnId"],
    )?;
    require_required(
        protocol_definition(document, &["v2", "TurnCompletedNotification"])?,
        "v2/TurnCompletedNotification",
        &["threadId", "turn"],
    )?;
    require_required(
        protocol_definition(document, &["v2", "ThreadTokenUsageUpdatedNotification"])?,
        "v2/ThreadTokenUsageUpdatedNotification",
        &["threadId", "tokenUsage", "turnId"],
    )?;
    require_required(
        protocol_definition(document, &["v2", "RawResponseCompletedNotification"])?,
        "v2/RawResponseCompletedNotification",
        &["responseId", "threadId", "turnId"],
    )
}

fn protocol_definition<'a>(document: &'a Value, path: &[&str]) -> Result<&'a Value, CodexError> {
    let mut value = document
        .get("definitions")
        .ok_or_else(|| incompatible_protocol("generated schema omitted the definitions object"))?;
    for segment in path {
        value = value.get(*segment).ok_or_else(|| {
            incompatible_protocol(format!(
                "generated schema omitted definition {}",
                path.join("/")
            ))
        })?;
    }
    Ok(value)
}

fn require_method(
    document: &Value,
    definition: &str,
    method: &str,
    params_ref: &str,
) -> Result<(), CodexError> {
    let variants = protocol_definition(document, &[definition])?
        .get("oneOf")
        .and_then(Value::as_array)
        .ok_or_else(|| incompatible_protocol(format!("{definition} omitted oneOf")))?;
    let variant = variants
        .iter()
        .find(|variant| {
            variant
                .pointer("/properties/method/enum")
                .and_then(Value::as_array)
                .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(method)))
        })
        .ok_or_else(|| incompatible_protocol(format!("{definition} omitted method {method}")))?;
    let found_ref = variant
        .pointer("/properties/params/$ref")
        .and_then(Value::as_str);
    if found_ref == Some(params_ref) {
        Ok(())
    } else {
        Err(incompatible_protocol(format!(
            "{definition} method {method} parameters must reference {params_ref}"
        )))
    }
}

fn require_properties(definition: &Value, name: &str, fields: &[&str]) -> Result<(), CodexError> {
    let properties = definition
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| incompatible_protocol(format!("{name} omitted properties")))?;
    for field in fields {
        if !properties.contains_key(*field) {
            return Err(incompatible_protocol(format!(
                "{name} omitted property {field}"
            )));
        }
    }
    Ok(())
}

fn require_required(definition: &Value, name: &str, fields: &[&str]) -> Result<(), CodexError> {
    let required = definition
        .get("required")
        .and_then(Value::as_array)
        .ok_or_else(|| incompatible_protocol(format!("{name} omitted required")))?;
    for field in fields {
        if !required
            .iter()
            .any(|required_field| required_field.as_str() == Some(field))
        {
            return Err(incompatible_protocol(format!(
                "{name} does not require {field}"
            )));
        }
    }
    Ok(())
}

fn require_enum_value(definition: &Value, name: &str, value: &str) -> Result<(), CodexError> {
    if contains_enum_value(definition, value) {
        Ok(())
    } else {
        Err(incompatible_protocol(format!(
            "{name} omitted enum value {value}"
        )))
    }
}

fn contains_enum_value(value: &Value, expected: &str) -> bool {
    match value {
        Value::Array(values) => values
            .iter()
            .any(|value| contains_enum_value(value, expected)),
        Value::Object(object) => {
            object
                .get("enum")
                .and_then(Value::as_array)
                .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(expected)))
                || object
                    .values()
                    .any(|value| contains_enum_value(value, expected))
        }
        _ => false,
    }
}

fn incompatible_protocol(reason: impl Into<String>) -> CodexError {
    unsupported("harness.protocolSchema", reason)
}

fn validate_tools(tools: &[DynamicTool]) -> Result<(), CodexError> {
    let mut names = BTreeSet::new();
    for tool in tools {
        if tool.name.is_empty()
            || tool.name.len() > 128
            || !tool
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err(unsupported(
                "toolset",
                format!("tool name {:?} is invalid", tool.name),
            ));
        }
        if !names.insert(&tool.name) {
            return Err(unsupported(
                "toolset",
                format!("tool name {:?} is duplicated", tool.name),
            ));
        }
        if tool.description.is_empty() {
            return Err(unsupported(
                "toolset",
                format!("tool {:?} has no description", tool.name),
            ));
        }
        if !tool.input_schema.is_object() {
            return Err(unsupported(
                "toolset",
                format!("tool {:?} input schema is not an object", tool.name),
            ));
        }
    }
    Ok(())
}

fn dynamic_tool_specs(tools: &[DynamicTool]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema,
                "deferLoading": false
            })
        })
        .collect()
}

fn disabled_features(spec: &CodexRunSpec) -> Vec<&'static str> {
    let mut features = Vec::new();
    if !spec.builtin_tools.local_execution {
        features.extend(["code_mode", "code_mode_host", "code_mode_only"]);
    }
    if !spec.builtin_tools.web_search {
        features.push("standalone_web_search");
    }
    features
}

fn runtime_config(spec: &CodexRunSpec) -> Vec<String> {
    let local = spec.builtin_tools.local_execution;
    let mut settings = vec![
        format!("include_environment_context={local}"),
        format!("include_permissions_instructions={local}"),
        if spec.builtin_tools.web_search {
            "web_search=\"live\"".to_owned()
        } else {
            "web_search=\"disabled\"".to_owned()
        },
    ];
    if local {
        settings.push("features.code_mode_host=true".to_owned());
        settings.push("shell_environment_policy.inherit=\"all\"".to_owned());
        if matches!(spec.workspace_access, WorkspaceAccess::ReadOnly) {
            settings.push("sandbox_permissions=[\"disk-full-read-access\"]".to_owned());
        }
    }
    settings
}

fn configured_model_catalog(bytes: &[u8], spec: &CodexRunSpec) -> Result<Value, CodexError> {
    let mut catalog: Value = serde_json::from_slice(bytes)
        .map_err(|error| CodexError::Inspection(format!("invalid model catalog: {error}")))?;
    let models = catalog
        .get_mut("models")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| CodexError::Inspection("model catalog omitted models".to_owned()))?;
    let index = models
        .iter()
        .position(|model| model.get("slug").and_then(Value::as_str) == Some(spec.model.as_str()))
        .ok_or_else(|| {
            unsupported(
                "model",
                format!("{} is not in the bundled model catalog", spec.model),
            )
        })?;
    let mut model = models.remove(index);
    let object = model.as_object_mut().ok_or_else(|| {
        CodexError::Inspection(format!("the {} catalog entry is not an object", spec.model))
    })?;
    let supports_local_execution = object
        .get("shell_type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "disabled");
    if spec.builtin_tools.local_execution && !supports_local_execution {
        return Err(unsupported(
            "builtinTools.localExecution",
            format!("{} does not support local execution", spec.model),
        ));
    }
    let supports_web_search = object
        .get("supports_search_tool")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if spec.builtin_tools.web_search && !supports_web_search {
        return Err(unsupported(
            "builtinTools.webSearch",
            format!("{} does not support web search", spec.model),
        ));
    }

    object.insert("multi_agent_version".to_owned(), json!("disabled"));
    object.insert("supports_parallel_tool_calls".to_owned(), json!(false));
    object.insert("include_skills_usage_instructions".to_owned(), json!(false));
    object.insert("experimental_supported_tools".to_owned(), json!([]));
    object.insert("input_modalities".to_owned(), json!(["text"]));
    object.insert("base_instructions".to_owned(), json!(spec.instructions));
    object.insert("model_messages".to_owned(), Value::Null);
    if !spec.builtin_tools.local_execution {
        object.insert("tool_mode".to_owned(), json!("direct"));
        object.insert("shell_type".to_owned(), json!("disabled"));
    }
    if !spec.builtin_tools.web_search {
        object.insert("supports_search_tool".to_owned(), json!(false));
    }
    if !(spec.builtin_tools.local_execution
        && matches!(spec.workspace_access, WorkspaceAccess::ReadWrite))
    {
        object.insert("apply_patch_tool_type".to_owned(), Value::Null);
    }
    *models = vec![model];
    Ok(catalog)
}

async fn read_protocol_lines(
    stdout: tokio::process::ChildStdout,
    sender: mpsc::Sender<io::Result<Vec<u8>>>,
    events: mpsc::Sender<CodexEvent>,
    sensitive_protocol: Arc<StdMutex<SensitiveProtocolState>>,
) -> Result<(), CodexError> {
    read_protocol_lines_with_limit(
        stdout,
        sender,
        events,
        sensitive_protocol,
        MAX_PROTOCOL_BYTES,
    )
    .await
}

async fn read_protocol_lines_with_limit(
    stdout: impl AsyncRead + Unpin,
    sender: mpsc::Sender<io::Result<Vec<u8>>>,
    events: mpsc::Sender<CodexEvent>,
    sensitive_protocol: Arc<StdMutex<SensitiveProtocolState>>,
    max_protocol_bytes: u64,
) -> Result<(), CodexError> {
    let mut reader = BufReader::new(stdout);
    let mut total = 0_u64;
    let mut protocol_consumer_connected = true;
    let mut protocol_limit_exceeded = false;
    loop {
        let mut line = Vec::new();
        let count = reader
            .read_until(b'\n', &mut line)
            .await
            .map_err(|error| CodexError::Protocol(error.to_string()))?;
        if count == 0 {
            return Ok(());
        }
        total = total.saturating_add(count as u64);
        let protocol_line = (protocol_consumer_connected
            && !protocol_limit_exceeded
            && total <= max_protocol_bytes)
            .then(|| line.clone());
        if !is_sensitive_protocol_record(&line, &sensitive_protocol) {
            events
                .send(CodexEvent::Protocol {
                    direction: ProtocolDirection::FromHarness,
                    bytes: line,
                })
                .await
                .map_err(|_| CodexError::EventConsumerDisconnected)?;
        }
        if !protocol_limit_exceeded && total > max_protocol_bytes {
            protocol_limit_exceeded = true;
            if protocol_consumer_connected
                && sender
                    .send(Err(io::Error::other(
                        "app-server exceeded the protocol output limit",
                    )))
                    .await
                    .is_err()
            {
                protocol_consumer_connected = false;
            }
            continue;
        }
        if let Some(line) = protocol_line
            && sender.send(Ok(line)).await.is_err()
        {
            protocol_consumer_connected = false;
        }
    }
}

fn is_sensitive_protocol_record(
    bytes: &[u8],
    sensitive_protocol: &StdMutex<SensitiveProtocolState>,
) -> bool {
    let mut sensitive = sensitive_protocol
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if sensitive
        .values
        .iter()
        .any(|value| !value.is_empty() && bytes.windows(value.len()).any(|window| window == value))
    {
        return true;
    }
    let Ok(message) = serde_json::from_slice::<Value>(bytes) else {
        // While a sensitive request is pending, an unparseable response cannot
        // be attributed safely and therefore cannot become a public event.
        return !sensitive.request_ids.is_empty();
    };
    if message.get("method").and_then(Value::as_str) == Some("account/chatgptAuthTokens/refresh") {
        return true;
    }
    // JSON-RPC client and server request IDs occupy independent spaces. A
    // normal server request may reuse an outbound sensitive request ID and is
    // not that request's response.
    if message.get("method").is_some() {
        return false;
    }
    let Some(id) = message.get("id").and_then(Value::as_i64) else {
        return false;
    };
    sensitive.request_ids.remove(&id)
}

async fn read_stderr(
    mut stderr: tokio::process::ChildStderr,
    sender: mpsc::Sender<CodexEvent>,
) -> Result<(), CodexError> {
    let mut buffer = vec![0_u8; STDERR_CHUNK_BYTES];
    loop {
        let count = stderr
            .read(&mut buffer)
            .await
            .map_err(|error| CodexError::Protocol(format!("could not read stderr: {error}")))?;
        if count == 0 {
            return Ok(());
        }
        sender
            .send(CodexEvent::Stderr(buffer[..count].to_vec()))
            .await
            .map_err(|_| CodexError::EventConsumerDisconnected)?;
    }
}

async fn discard_stderr(mut stderr: tokio::process::ChildStderr) -> Result<(), CodexError> {
    let mut buffer = vec![0_u8; STDERR_CHUNK_BYTES];
    loop {
        let count = stderr
            .read(&mut buffer)
            .await
            .map_err(|error| CodexError::Protocol(error.to_string()))?;
        if count == 0 {
            return Ok(());
        }
    }
}

async fn read_bounded_stderr(stderr: &mut tokio::process::ChildStderr) -> io::Result<Vec<u8>> {
    let mut tail = Vec::new();
    let mut buffer = vec![0_u8; STDERR_CHUNK_BYTES];
    loop {
        let count = stderr.read(&mut buffer).await?;
        if count == 0 {
            return Ok(tail);
        }
        retain_tail(&mut tail, &buffer[..count], MAX_ACCOUNT_STDERR_BYTES);
    }
}

fn retain_tail(tail: &mut Vec<u8>, bytes: &[u8], limit: usize) {
    if bytes.len() >= limit {
        tail.clear();
        tail.extend_from_slice(&bytes[bytes.len() - limit..]);
        return;
    }
    let excess = tail.len().saturating_add(bytes.len()).saturating_sub(limit);
    if excess > 0 {
        tail.drain(..excess);
    }
    tail.extend_from_slice(bytes);
}

async fn wait_for_cancellation(cancelled: &mut watch::Receiver<bool>) {
    loop {
        if *cancelled.borrow() {
            return;
        }
        if cancelled.changed().await.is_err() {
            // Losing the producer is not a cancellation request. Continue to
            // rely on the harness result and the wall-clock deadline.
            future::pending::<()>().await;
        }
    }
}

async fn terminate_process_group(child: &mut Child, guard: &mut ProcessGroupGuard) {
    terminate_process_group_with_grace(child, guard, SHUTDOWN_GRACE).await;
}

async fn terminate_process_group_with_grace(
    child: &mut Child,
    guard: &mut ProcessGroupGuard,
    grace: Duration,
) {
    if let Some(group) = guard.group {
        signal_process_group(group, "-TERM").await;
    } else {
        let _ = child.start_kill();
    }
    let needs_wait = !matches!(tokio::time::timeout(grace, child.wait()).await, Ok(Ok(_)));
    // The direct child can exit while a grandchild ignores TERM, so signal the
    // full group once more before disarming the drop guard.
    if let Some(group) = guard.group {
        signal_process_group(group, "-KILL").await;
    }
    if needs_wait {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    guard.disarm();
}

async fn signal_process_group(group: u32, signal: &str) {
    let _ = Command::new("/bin/kill")
        .args([OsStr::new(signal), OsStr::new("--")])
        .arg(format!("-{group}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

/// Last-resort cleanup when an async run is dropped or aborted.
struct ProcessGroupGuard {
    group: Option<u32>,
    armed: bool,
}

impl ProcessGroupGuard {
    const fn new(group: Option<u32>) -> Self {
        Self { group, armed: true }
    }

    const fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        let Some(group) = self.group.filter(|_| self.armed) else {
            return;
        };
        let _ = StdCommand::new("/bin/kill")
            .args([OsStr::new("-KILL"), OsStr::new("--")])
            .arg(format!("-{group}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

async fn join_reader(
    task: JoinHandle<Result<(), CodexError>>,
    stream: &str,
) -> Result<(), CodexError> {
    task.await
        .map_err(|error| CodexError::Protocol(format!("{stream} reader task failed: {error}")))?
}

struct CredentialLease {
    _gate: tokio::sync::OwnedMutexGuard<()>,
    _file: File,
}

#[derive(Clone)]
struct AuthSessionLease {
    _inner: Arc<AuthSessionLeaseInner>,
}

struct AuthSessionLeaseInner {
    _gate: tokio::sync::OwnedRwLockReadGuard<()>,
    _file: File,
}

struct LoginSessionLease {
    _gate: tokio::sync::OwnedRwLockWriteGuard<()>,
    _file: File,
}

fn default_codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".codex"))
        })
}

fn validate_credential_home(home: &Path, require_auth: bool) -> Result<(), CodexError> {
    let metadata = fs::symlink_metadata(home).map_err(|error| {
        CodexError::Authentication(format!("could not inspect {}: {error}", home.display()))
    })?;
    if !metadata.file_type().is_dir() || metadata.permissions().mode() & 0o777 != 0o700 {
        return Err(CodexError::Authentication(format!(
            "Codex home must be a private regular directory: {}",
            home.display()
        )));
    }
    let config = home.join("config.toml");
    let metadata = fs::symlink_metadata(&config).map_err(|error| {
        CodexError::Authentication(format!("could not inspect {}: {error}", config.display()))
    })?;
    if !metadata.file_type().is_file()
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() > 64 * 1024
    {
        return Err(CodexError::Authentication(format!(
            "Codex config must be a private regular file no larger than 65536 bytes: {}",
            config.display()
        )));
    }
    let document = fs::read_to_string(&config).map_err(|error| {
        CodexError::Authentication(format!("could not read {}: {error}", config.display()))
    })?;
    if document.trim() != AUTH_CONFIG.trim() {
        return Err(CodexError::Authentication(format!(
            "Codex config may contain only cli_auth_credentials_store = \"file\": {}",
            config.display()
        )));
    }
    if require_auth {
        validate_auth_file(&home.join("auth.json"))?;
    }
    Ok(())
}

fn validate_auth_file(path: &Path) -> Result<(), CodexError> {
    read_valid_auth_file(path).map(|_| ())
}

fn read_valid_auth_file(path: &Path) -> Result<Vec<u8>, CodexError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        CodexError::Authentication(format!("could not inspect {}: {error}", path.display()))
    })?;
    if !metadata.file_type().is_file()
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() == 0
        || metadata.len() > MAX_AUTH_BYTES
    {
        return Err(CodexError::Authentication(format!(
            "auth.json must be a nonempty mode-0600 regular file no larger than {MAX_AUTH_BYTES} bytes: {}",
            path.display()
        )));
    }
    let bytes = fs::read(path).map_err(|error| {
        CodexError::Authentication(format!("could not read {}: {error}", path.display()))
    })?;
    if bytes.is_empty() || u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_AUTH_BYTES {
        return Err(CodexError::Authentication(format!(
            "auth.json must be nonempty and no larger than {MAX_AUTH_BYTES} bytes: {}",
            path.display()
        )));
    }
    validate_auth_document(&bytes)?;
    Ok(bytes)
}

/// Validate the stable credential shape Codex writes to `auth.json`.
///
/// File-backed authentication contains either a selected nonempty API key or a
/// managed `ChatGPT` token bundle with enough account identity to construct the
/// exact external-auth worker handshake. Other fields remain intentionally
/// unconstrained so Codex can evolve unrelated credential metadata without
/// requiring a Nucleus release.
///
/// # Errors
///
/// Returns [`CodexError::Authentication`] when the document is not a JSON
/// object containing usable file-backed credentials for the exact adapter.
pub fn validate_auth_document(bytes: &[u8]) -> Result<(), CodexError> {
    worker_authentication_from_document(bytes).map(|_| ())
}

fn open_credential_lease(home: &Path) -> Result<File, CodexError> {
    let (file, path) = open_credential_lock(home)?;
    file.lock_exclusive().map_err(|error| {
        CodexError::Authentication(format!("could not lock {}: {error}", path.display()))
    })?;
    Ok(file)
}

fn try_open_credential_lease(home: &Path) -> Result<File, CodexError> {
    let (file, path) = open_credential_lock(home)?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            Err(CodexError::AuthenticationBusy)
        }
        Err(error) => Err(CodexError::Authentication(format!(
            "could not lock {}: {error}",
            path.display()
        ))),
    }
}

fn open_credential_lock(home: &Path) -> Result<(File, PathBuf), CodexError> {
    open_private_lock(home, AUTH_LOCK_NAME)
}

fn open_auth_session_shared(home: &Path) -> Result<File, CodexError> {
    let (file, path) = open_private_lock(home, AUTH_SESSION_LOCK_NAME)?;
    fs2::FileExt::lock_shared(&file).map_err(|error| {
        CodexError::Authentication(format!("could not lock {}: {error}", path.display()))
    })?;
    Ok(file)
}

fn try_open_auth_session_shared(home: &Path) -> Result<File, CodexError> {
    let (file, path) = open_private_lock(home, AUTH_SESSION_LOCK_NAME)?;
    match fs2::FileExt::try_lock_shared(&file) {
        Ok(()) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            Err(CodexError::AuthenticationBusy)
        }
        Err(error) => Err(CodexError::Authentication(format!(
            "could not lock {}: {error}",
            path.display()
        ))),
    }
}

fn open_auth_session_exclusive(home: &Path) -> Result<File, CodexError> {
    let (file, path) = open_private_lock(home, AUTH_SESSION_LOCK_NAME)?;
    file.lock_exclusive().map_err(|error| {
        CodexError::Authentication(format!("could not lock {}: {error}", path.display()))
    })?;
    Ok(file)
}

fn open_private_lock(home: &Path, name: &str) -> Result<(File, PathBuf), CodexError> {
    validate_credential_home(home, false)?;
    let path = home.join(name);
    if let Ok(metadata) = fs::symlink_metadata(&path)
        && !metadata.file_type().is_file()
    {
        return Err(CodexError::Authentication(format!(
            "credential lease path is not a regular file: {}",
            path.display()
        )));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
        .map_err(|error| {
            CodexError::Authentication(format!("could not open {}: {error}", path.display()))
        })?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|error| {
            CodexError::Authentication(format!("could not secure {}: {error}", path.display()))
        })?;
    Ok((file, path))
}

fn read_worker_authentication(home: &Path) -> Result<WorkerAuthentication, CodexError> {
    validate_credential_home(home, true)?;
    let bytes = read_valid_auth_file(&home.join("auth.json"))?;
    worker_authentication_from_document(&bytes)
}

fn worker_authentication_from_document(bytes: &[u8]) -> Result<WorkerAuthentication, CodexError> {
    let document: Value = serde_json::from_slice(bytes)
        .map_err(|_| CodexError::Authentication("auth.json is not valid JSON".to_owned()))?;
    let object = document.as_object().ok_or_else(|| {
        CodexError::Authentication("auth.json must contain a JSON object".to_owned())
    })?;
    let api_key = object
        .get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let declared_mode = match object.get("auth_mode") {
        Some(Value::String(mode)) => Some(mode.as_str()),
        Some(Value::Null) | None => None,
        Some(_) => {
            return Err(CodexError::Authentication(
                "auth.json auth_mode must be a string".to_owned(),
            ));
        }
    };
    let use_api_key = match declared_mode {
        Some("apikey") => true,
        Some("chatgpt") => false,
        None => api_key.is_some(),
        Some(_) => {
            return Err(CodexError::Authentication(
                "Nucleus supports file-backed API key or managed ChatGPT authentication".to_owned(),
            ));
        }
    };
    if use_api_key {
        return api_key
            .map(|key| WorkerAuthentication::ApiKey(key.to_owned()))
            .ok_or_else(|| {
                CodexError::Authentication(
                    "API-key authentication is missing its credential".to_owned(),
                )
            });
    }

    let tokens = object
        .get("tokens")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CodexError::Authentication(
                "managed ChatGPT authentication is missing tokens".to_owned(),
            )
        })?;
    let access_token = tokens
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            CodexError::Authentication(
                "managed ChatGPT authentication is missing its access token".to_owned(),
            )
        })?
        .to_owned();
    if jwt_claims(&access_token).is_none() {
        return Err(CodexError::Authentication(
            "managed ChatGPT authentication has an invalid access token".to_owned(),
        ));
    }
    if tokens
        .get("refresh_token")
        .and_then(Value::as_str)
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err(CodexError::Authentication(
            "managed ChatGPT authentication is missing its refresh token".to_owned(),
        ));
    }
    let id_token = tokens.get("id_token").and_then(Value::as_str);
    let account_id = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| id_token.and_then(|token| chatgpt_claim(token, "chatgpt_account_id")))
        .or_else(|| chatgpt_claim(&access_token, "chatgpt_account_id"))
        .ok_or_else(|| {
            CodexError::Authentication(
                "managed ChatGPT authentication is missing its account identifier".to_owned(),
            )
        })?;
    let plan_type = id_token
        .and_then(|token| chatgpt_claim(token, "chatgpt_plan_type"))
        .or_else(|| chatgpt_claim(&access_token, "chatgpt_plan_type"));
    Ok(WorkerAuthentication::ManagedChatgpt(
        ManagedChatgptCredentials {
            access_token,
            account_id,
            plan_type,
        },
    ))
}

fn chatgpt_claim(token: &str, name: &str) -> Option<String> {
    let claims = jwt_claims(token)?;
    claims
        .get("https://api.openai.com/auth")?
        .get(name)?
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn jwt_claims(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let header = parts.next()?;
    let payload = parts.next()?;
    let signature = parts.next()?;
    if header.is_empty() || payload.is_empty() || signature.is_empty() {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&bytes).ok().filter(Value::is_object)
}

fn create_auth_staging_directory(home: &Path) -> Result<TempDir, CodexError> {
    // A daemon crash can bypass TempDir's Drop cleanup. Because the credential
    // lease is cross-process, a new owner can safely remove only prior private
    // staging directories before creating its own. An orphaned Codex process
    // may keep an unlinked staging file open, but it can never reach auth.json.
    for entry in fs::read_dir(home).map_err(CodexError::Preparation)? {
        let entry = entry.map_err(CodexError::Preparation)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let file_type = entry.file_type().map_err(CodexError::Preparation)?;
        if name.starts_with(AUTH_STAGING_PREFIX) && file_type.is_dir() {
            fs::remove_dir_all(entry.path()).map_err(CodexError::Preparation)?;
        } else if name.starts_with(AUTH_GENERATION_PREFIX) && file_type.is_file() {
            fs::remove_file(entry.path()).map_err(CodexError::Preparation)?;
        }
    }
    let temporary = tempfile::Builder::new()
        .prefix(AUTH_STAGING_PREFIX)
        .tempdir_in(home)
        .map_err(CodexError::Preparation)?;
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700))
        .map_err(CodexError::Preparation)?;
    Ok(temporary)
}

async fn prepare_staging_auth_home(
    staging_home: &Path,
    canonical_auth: Option<&[u8]>,
) -> Result<(), CodexError> {
    tokio::fs::create_dir(staging_home)
        .await
        .map_err(CodexError::Preparation)?;
    tokio::fs::set_permissions(staging_home, fs::Permissions::from_mode(0o700))
        .await
        .map_err(CodexError::Preparation)?;
    write_private_file(&staging_home.join("config.toml"), AUTH_CONFIG.as_bytes()).await?;
    if let Some(canonical_auth) = canonical_auth {
        write_private_file(&staging_home.join("auth.json"), canonical_auth).await?;
    }
    Ok(())
}

fn promote_account_auth_if_advanced(
    home: &Path,
    canonical_auth: &[u8],
    staged_auth: &[u8],
) -> Result<(), CodexError> {
    let canonical = worker_authentication_from_document(canonical_auth)?;
    let staged = worker_authentication_from_document(staged_auth)?;
    match (canonical, staged) {
        (WorkerAuthentication::ApiKey(canonical), WorkerAuthentication::ApiKey(staged)) => {
            if canonical != staged {
                return Err(CodexError::Authentication(
                    "Codex changed API-key authentication during an account read".to_owned(),
                ));
            }
            Ok(())
        }
        (
            WorkerAuthentication::ManagedChatgpt(canonical),
            WorkerAuthentication::ManagedChatgpt(staged),
        ) => {
            if canonical.account_id != staged.account_id {
                return Err(CodexError::Authentication(
                    "Codex changed ChatGPT accounts during an account read".to_owned(),
                ));
            }
            if canonical_auth == staged_auth {
                return Ok(());
            }
            promote_auth_document(home, staged_auth)
        }
        _ => Err(CodexError::Authentication(
            "Codex changed authentication modes during an account read".to_owned(),
        )),
    }
}

fn promote_auth_document(home: &Path, bytes: &[u8]) -> Result<(), CodexError> {
    let auth_path = home.join("auth.json");
    let mut replacement = tempfile::Builder::new()
        .prefix(AUTH_GENERATION_PREFIX)
        .tempfile_in(home)
        .map_err(|error| {
            CodexError::Authentication(format!(
                "could not stage managed authentication in {}: {error}",
                home.display()
            ))
        })?;
    replacement
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .and_then(|()| replacement.write_all(bytes))
        .and_then(|()| replacement.flush())
        .and_then(|()| replacement.as_file().sync_all())
        .map_err(|error| {
            CodexError::Authentication(format!(
                "could not persist managed authentication in {}: {error}",
                home.display()
            ))
        })?;
    replacement.persist(&auth_path).map_err(|error| {
        CodexError::Authentication(format!(
            "could not promote managed authentication at {}: {}",
            auth_path.display(),
            error.error
        ))
    })?;
    File::open(home)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            CodexError::Authentication(format!(
                "could not sync managed authentication directory {}: {error}",
                home.display()
            ))
        })
}

async fn prepare_isolated_codex_home(
    isolated_home: &Path,
    authentication: &WorkerAuthentication,
) -> Result<(), CodexError> {
    tokio::fs::set_permissions(isolated_home, fs::Permissions::from_mode(0o700))
        .await
        .map_err(CodexError::Preparation)?;
    write_private_file(&isolated_home.join("config.toml"), AUTH_CONFIG.as_bytes()).await?;
    if let WorkerAuthentication::ApiKey(api_key) = authentication {
        let bytes = serde_json::to_vec(&json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": api_key,
        }))
        .map_err(|error| {
            CodexError::Authentication(format!("could not prepare API-key authentication: {error}"))
        })?;
        write_private_file(&isolated_home.join("auth.json"), &bytes).await?;
    }
    Ok(())
}

async fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), CodexError> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut file = options.open(path).await.map_err(CodexError::Preparation)?;
    file.write_all(bytes)
        .await
        .map_err(CodexError::Preparation)?;
    file.flush().await.map_err(CodexError::Preparation)
}

fn apply_launch_environment(command: &mut Command, spec: &CodexRunSpec) {
    if let Some(environment) = &spec.launch_environment {
        command.env_clear();
        command.envs(environment);
    }
    command
        .env_remove("CODEX_EXEC_SERVER_URL")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN");
}

struct AccountProtocol {
    stdin: ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    deadline: tokio::time::Instant,
    total_bytes: u64,
}

impl AccountProtocol {
    fn new(
        stdin: ChildStdin,
        stdout: tokio::process::ChildStdout,
        deadline: tokio::time::Instant,
    ) -> Self {
        Self {
            stdin,
            stdout: BufReader::new(stdout),
            deadline,
            total_bytes: 0,
        }
    }

    async fn request(
        &mut self,
        id: i64,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, CodexError> {
        let mut message = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if let Some(params) = params {
            message["params"] = params;
        }
        self.write(&message).await?;
        loop {
            let message = self.read().await?;
            if message.get("id") != Some(&json!(id)) {
                continue;
            }
            if message.get("error").is_some() {
                return Err(CodexError::Authentication(format!(
                    "{method} was rejected by Codex"
                )));
            }
            return message
                .get("result")
                .cloned()
                .ok_or_else(|| CodexError::Protocol(format!("{method} response omitted result")));
        }
    }

    async fn notify(&mut self, method: &str, params: Option<Value>) -> Result<(), CodexError> {
        let mut message = json!({ "jsonrpc": "2.0", "method": method });
        if let Some(params) = params {
            message["params"] = params;
        }
        self.write(&message).await
    }

    async fn write(&mut self, message: &Value) -> Result<(), CodexError> {
        let mut bytes = serde_json::to_vec(message)
            .map_err(|error| CodexError::Protocol(format!("could not encode JSON-RPC: {error}")))?;
        bytes.push(b'\n');
        tokio::time::timeout_at(self.deadline, self.stdin.write_all(&bytes))
            .await
            .map_err(|_| CodexError::TimedOut)?
            .map_err(|error| CodexError::Protocol(format!("could not write JSON-RPC: {error}")))?;
        tokio::time::timeout_at(self.deadline, self.stdin.flush())
            .await
            .map_err(|_| CodexError::TimedOut)?
            .map_err(|error| CodexError::Protocol(format!("could not flush JSON-RPC: {error}")))
    }

    async fn read(&mut self) -> Result<Value, CodexError> {
        let mut line = Vec::new();
        let count =
            tokio::time::timeout_at(self.deadline, self.stdout.read_until(b'\n', &mut line))
                .await
                .map_err(|_| CodexError::TimedOut)?
                .map_err(|error| {
                    CodexError::Protocol(format!("could not read JSON-RPC: {error}"))
                })?;
        if count == 0 {
            return Err(CodexError::Protocol(
                "account app-server stdout closed".to_owned(),
            ));
        }
        self.total_bytes = self.total_bytes.saturating_add(count as u64);
        if self.total_bytes > MAX_ACCOUNT_PROTOCOL_BYTES {
            return Err(CodexError::Protocol(
                "account app-server exceeded the protocol output limit".to_owned(),
            ));
        }
        serde_json::from_slice(&line).map_err(|error| {
            CodexError::Protocol(format!(
                "account app-server emitted invalid JSON-RPC: {error}"
            ))
        })
    }
}

fn required_string(value: &Value, pointer: &str, context: &str) -> Result<String, CodexError> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| CodexError::Protocol(format!("{context} omitted {pointer}")))
}

fn unsupported(setting: &str, reason: impl Into<String>) -> CodexError {
    CodexError::UnsupportedSetting {
        setting: setting.to_owned(),
        reason: reason.into(),
    }
}

fn diagnostic(prefix: &str, stderr: &[u8]) -> String {
    let detail = String::from_utf8_lossy(stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BuiltinToolsV1, CodexError, CodexEvent, CodexHarness, CodexRunSpec, DynamicTool,
        GeneratedSchema, HarnessInspection, ModelCapability, ProtocolDirection,
        SUPPORTED_CODEX_VERSION, ToolResult, WorkerAuthentication, WorkspaceAccess,
        configured_model_catalog, disabled_features, dynamic_tool_specs,
        prepare_isolated_codex_home, read_protocol_lines_with_limit, read_worker_authentication,
        runtime_config, validate_auth_document,
    };
    use serde_json::{Value, json};
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::Duration;
    use tokio::io::{AsyncWriteExt as _, duplex};
    use tokio::sync::{mpsc, watch};

    #[tokio::test]
    async fn protocol_limit_fails_consumer_but_keeps_ordered_capture_draining()
    -> Result<(), Box<dyn std::error::Error>> {
        let (reader, mut writer) = duplex(1_024);
        let (protocol_tx, mut protocol_rx) = mpsc::channel(4);
        let (events_tx, mut events_rx) = mpsc::channel(4);
        let reader_task = tokio::spawn(read_protocol_lines_with_limit(
            reader,
            protocol_tx,
            events_tx,
            Arc::new(StdMutex::new(super::SensitiveProtocolState::default())),
            4,
        ));
        let records = [
            b"a\n".to_vec(),
            b"bb\n".to_vec(),
            b"c\n".to_vec(),
            b"later\n".to_vec(),
        ];
        for record in &records {
            writer.write_all(record).await?;
        }
        writer.shutdown().await?;

        reader_task.await??;

        assert_eq!(
            protocol_rx
                .recv()
                .await
                .ok_or("protocol consumer closed before its first line")??,
            b"a\n"
        );
        let limit_error = protocol_rx
            .recv()
            .await
            .ok_or("protocol consumer did not receive the limit failure")?
            .err()
            .ok_or("limit-crossing line unexpectedly reached the protocol consumer")?;
        assert!(limit_error.to_string().contains("protocol output limit"));
        assert!(protocol_rx.recv().await.is_none());

        let mut captured = Vec::new();
        while let Some(event) = events_rx.recv().await {
            let CodexEvent::Protocol { direction, bytes } = event else {
                panic!("stdout reader emitted a non-protocol event");
            };
            assert_eq!(direction, ProtocolDirection::FromHarness);
            captured.push(bytes);
        }
        assert_eq!(captured, records);
        Ok(())
    }

    #[test]
    fn sensitive_response_ids_do_not_hide_same_id_server_requests() {
        let sensitive = StdMutex::new(super::SensitiveProtocolState {
            request_ids: BTreeSet::from([1]),
            values: vec![b"bearer-secret".to_vec()],
        });
        assert!(!super::is_sensitive_protocol_record(
            br#"{"id":1,"method":"item/tool/call","params":{}}"#,
            &sensitive
        ));
        assert!(super::is_sensitive_protocol_record(
            b"malformed bearer-secret response\n",
            &sensitive
        ));
        assert!(super::is_sensitive_protocol_record(
            b"malformed response while auth is pending\n",
            &sensitive
        ));
        assert!(super::is_sensitive_protocol_record(
            br#"{"id":1,"result":{}}"#,
            &sensitive
        ));
        assert!(!super::is_sensitive_protocol_record(
            br#"{"id":1,"result":{}}"#,
            &sensitive
        ));
        assert!(super::is_sensitive_protocol_record(
            br#"{"id":20,"method":"account/chatgptAuthTokens/refresh","params":{}}"#,
            &sensitive
        ));
    }

    fn inspection() -> HarnessInspection {
        HarnessInspection {
            harness: "codex".to_owned(),
            version: SUPPORTED_CODEX_VERSION.to_owned(),
            executable: "codex".into(),
            models: vec![ModelCapability {
                model: "example-model".to_owned(),
                default_reasoning_effort: Some("medium".to_owned()),
                reasoning_efforts: vec!["medium".to_owned()],
                supports_local_execution: true,
                supports_web_search: true,
            }],
        }
    }

    #[test]
    fn refresh_protocol_and_cleanup_fit_the_external_bridge_deadline() {
        assert!(
            super::CANONICAL_AUTH_REFRESH_TIMEOUT + super::AUTH_REFRESH_SHUTDOWN_GRACE
                < super::EXTERNAL_AUTH_RESPONSE_TIMEOUT
        );
    }

    const INITIAL_ACCESS_TOKEN: &str = "header.e30.signature-initial-secret";
    const REFRESHED_ACCESS_TOKEN: &str = "header.e30.signature-refreshed-secret";
    const INITIAL_REFRESH_TOKEN: &str = "initial-refresh-token-secret";
    const REFRESHED_REFRESH_TOKEN: &str = "refreshed-refresh-token-secret";
    const CHATGPT_ACCOUNT_ID: &str = "account-1";

    fn managed_auth_document(access_token: &str, refresh_token: &str) -> String {
        serde_json::to_string(&json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "access_token": access_token,
                "refresh_token": refresh_token,
                "account_id": CHATGPT_ACCOUNT_ID,
            }
        }))
        .unwrap_or_else(|error| panic!("encode managed auth fixture: {error}"))
    }

    fn write_test_codex_home(path: &Path, auth: &str) -> std::io::Result<()> {
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        fs::write(path.join("config.toml"), super::AUTH_CONFIG)?;
        fs::set_permissions(path.join("config.toml"), fs::Permissions::from_mode(0o600))?;
        fs::write(path.join("auth.json"), auth)?;
        fs::set_permissions(path.join("auth.json"), fs::Permissions::from_mode(0o600))
    }

    fn method_schema(method: &str, params_ref: &str) -> Value {
        json!({
            "properties": {
                "method": { "enum": [method] },
                "params": { "$ref": params_ref }
            }
        })
    }

    fn compatible_protocol_schema() -> GeneratedSchema {
        let document = json!({
            "definitions": {
                "ClientRequest": { "oneOf": [
                    method_schema("initialize", "#/definitions/InitializeParams"),
                    method_schema("thread/start", "#/definitions/v2/ThreadStartParams"),
                    method_schema("turn/start", "#/definitions/v2/TurnStartParams"),
                    method_schema(
                        "mcpServerStatus/list",
                        "#/definitions/v2/ListMcpServerStatusParams"
                    ),
                    method_schema(
                        "account/login/start",
                        "#/definitions/v2/LoginAccountParams"
                    ),
                    method_schema("account/read", "#/definitions/v2/GetAccountParams")
                ] },
                "ServerRequest": { "oneOf": [
                    method_schema("item/tool/call", "#/definitions/DynamicToolCallParams"),
                    method_schema(
                        "account/chatgptAuthTokens/refresh",
                        "#/definitions/ChatgptAuthTokensRefreshParams"
                    )
                ] },
                "ServerNotification": { "oneOf": [
                    method_schema(
                        "item/completed",
                        "#/definitions/v2/ItemCompletedNotification"
                    ),
                    method_schema(
                        "turn/completed",
                        "#/definitions/v2/TurnCompletedNotification"
                    ),
                    method_schema(
                        "thread/tokenUsage/updated",
                        "#/definitions/v2/ThreadTokenUsageUpdatedNotification"
                    )
                ] },
                "DynamicToolCallParams": {
                    "required": ["arguments", "callId", "threadId", "tool", "turnId"]
                },
                "ChatgptAuthTokensRefreshParams": {
                    "required": ["reason"],
                    "properties": { "previousAccountId": {}, "reason": {} }
                },
                "ChatgptAuthTokensRefreshReason": { "enum": ["unauthorized"] },
                "ChatgptAuthTokensRefreshResponse": {
                    "required": ["accessToken", "chatgptAccountId"],
                    "properties": { "accessToken": {}, "chatgptAccountId": {}, "chatgptPlanType": {} }
                },
                "v2": {
                    "ThreadStartParams": { "properties": {
                        "approvalPolicy": {}, "baseInstructions": {}, "cwd": {},
                        "developerInstructions": {}, "dynamicTools": {}, "ephemeral": {},
                        "environments": {}, "experimentalRawEvents": {}, "model": {}, "sandbox": {}
                    } },
                    "AskForApproval": { "enum": ["never"] },
                    "SandboxMode": { "enum": ["read-only", "workspace-write"] },
                    "TurnStartParams": {
                        "required": ["input", "threadId"],
                        "properties": { "effort": {}, "environments": {}, "input": {}, "threadId": {} }
                    },
                    "LoginAccountParams": { "oneOf": [{
                        "required": ["accessToken", "chatgptAccountId", "type"],
                        "properties": {
                            "accessToken": {}, "chatgptAccountId": {},
                            "chatgptPlanType": {},
                            "type": { "enum": ["chatgptAuthTokens"] }
                        }
                    }] },
                    "GetAccountParams": { "properties": { "refreshToken": {} } },
                    "DynamicToolSpec": { "oneOf": [{
                        "required": ["description", "inputSchema", "name", "type"],
                        "properties": { "type": { "enum": ["function"] } }
                    }] },
                    "ThreadStartResponse": { "required": ["thread"] },
                    "TurnStartResponse": { "required": ["turn"] },
                    "Thread": { "required": ["id"] },
                    "Turn": { "required": ["id", "status"] },
                    "TurnStatus": { "enum": ["completed", "failed"] },
                    "ItemCompletedNotification": {
                        "required": ["item", "threadId", "turnId"]
                    },
                    "TurnCompletedNotification": { "required": ["threadId", "turn"] }
                    ,"ThreadTokenUsageUpdatedNotification": {
                        "required": ["threadId", "tokenUsage", "turnId"]
                    },
                    "RawResponseCompletedNotification": {
                        "required": ["responseId", "threadId", "turnId"]
                    }
                }
            }
        });
        GeneratedSchema {
            media_type: "application/schema+json",
            bytes: serde_json::to_vec(&document)
                .unwrap_or_else(|error| panic!("encode compatible protocol schema: {error}")),
        }
    }

    #[test]
    fn validation_is_against_inspected_capabilities() {
        let directory = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("create temporary directory: {error}"));
        let mut spec = CodexRunSpec {
            instructions: "Follow the requester contract.".to_owned(),
            developer_instructions: None,
            prompt: "work".to_owned(),
            model: "example-model".to_owned(),
            reasoning_effort: Some("medium".to_owned()),
            working_directory: directory.path().to_path_buf(),
            workspace_access: WorkspaceAccess::ReadOnly,
            builtin_tools: BuiltinToolsV1 {
                local_execution: false,
                web_search: false,
            },
            timeout: Duration::from_secs(30),
            tools: Vec::new(),
            launch_environment: None,
        };
        let harness = CodexHarness::new("codex");
        harness
            .validate(&inspection(), &spec)
            .unwrap_or_else(|error| panic!("validate supported invocation: {error}"));
        spec.reasoning_effort = Some("ultra".to_owned());
        let error = match harness.validate(&inspection(), &spec) {
            Ok(()) => panic!("unsupported reasoning effort must fail validation"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("reasoningEffort"));

        let mut unknown_version = inspection();
        unknown_version.version = "0.147.0".to_owned();
        let error = match harness.validate(&unknown_version, &spec) {
            Ok(()) => panic!("unknown Codex version must fail validation"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("harness.version"));
        assert!(error.to_string().contains(SUPPORTED_CODEX_VERSION));
    }

    #[test]
    fn protocol_schema_is_bound_to_required_v1_semantics() {
        let harness = CodexHarness::new("codex");
        let schema = compatible_protocol_schema();
        harness
            .validate_protocol_schema(&inspection(), &schema)
            .unwrap_or_else(|error| panic!("validate supported protocol schema: {error}"));

        let mut incompatible: Value = serde_json::from_slice(&schema.bytes)
            .unwrap_or_else(|error| panic!("decode protocol schema: {error}"));
        incompatible
            .pointer_mut("/definitions/v2/ThreadStartParams/properties")
            .and_then(Value::as_object_mut)
            .unwrap_or_else(|| panic!("protocol schema must contain thread start properties"))
            .remove("dynamicTools");
        let incompatible = GeneratedSchema {
            media_type: "application/schema+json",
            bytes: serde_json::to_vec(&incompatible)
                .unwrap_or_else(|error| panic!("encode incompatible schema: {error}")),
        };
        let error = match harness.validate_protocol_schema(&inspection(), &incompatible) {
            Ok(()) => panic!("schema without dynamic tools must fail validation"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("harness.protocolSchema"));
        assert!(error.to_string().contains("dynamicTools"));
    }

    #[test]
    fn protocol_schema_binds_managed_chatgpt_authentication() {
        let harness = CodexHarness::new("codex");
        let schema = compatible_protocol_schema();
        let document: Value = serde_json::from_slice(&schema.bytes)
            .unwrap_or_else(|error| panic!("decode protocol schema: {error}"));
        let assert_rejected = |document: Value, expected: &str| {
            let incompatible = GeneratedSchema {
                media_type: "application/schema+json",
                bytes: serde_json::to_vec(&document)
                    .unwrap_or_else(|error| panic!("encode incompatible schema: {error}")),
            };
            let error = match harness.validate_protocol_schema(&inspection(), &incompatible) {
                Ok(()) => panic!("managed authentication schema omission must fail validation"),
                Err(error) => error,
            };
            assert!(
                error.to_string().contains(expected),
                "unexpected validation error: {error}"
            );
        };

        for (pointer, method) in [
            ("/definitions/ClientRequest/oneOf", "account/login/start"),
            ("/definitions/ClientRequest/oneOf", "account/read"),
            (
                "/definitions/ServerRequest/oneOf",
                "account/chatgptAuthTokens/refresh",
            ),
        ] {
            let mut missing_method = document.clone();
            missing_method
                .pointer_mut(pointer)
                .and_then(Value::as_array_mut)
                .unwrap_or_else(|| panic!("schema must contain {pointer}"))
                .retain(|variant| {
                    variant
                        .pointer("/properties/method/enum/0")
                        .and_then(Value::as_str)
                        != Some(method)
                });
            assert_rejected(missing_method, method);
        }

        for (pointer, property) in [
            (
                "/definitions/v2/LoginAccountParams/oneOf/0/properties",
                "chatgptPlanType",
            ),
            (
                "/definitions/v2/GetAccountParams/properties",
                "refreshToken",
            ),
            (
                "/definitions/ChatgptAuthTokensRefreshResponse/properties",
                "chatgptPlanType",
            ),
        ] {
            let mut missing_property = document.clone();
            missing_property
                .pointer_mut(pointer)
                .and_then(Value::as_object_mut)
                .unwrap_or_else(|| panic!("schema must contain {pointer}"))
                .remove(property);
            assert_rejected(missing_property, property);
        }

        let mut missing_unauthorized_reason = document;
        missing_unauthorized_reason["definitions"]["ChatgptAuthTokensRefreshReason"]["enum"] =
            json!(["expired"]);
        assert_rejected(missing_unauthorized_reason, "unauthorized");
    }

    #[test]
    fn tools_translate_to_codex_without_exposing_argv() {
        let tools = dynamic_tool_specs(&[DynamicTool {
            name: "create_todo".to_owned(),
            description: "Create one todo".to_owned(),
            input_schema: json!({"type": "object"}),
        }]);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "create_todo");
        assert_eq!(tools[0]["deferLoading"], false);
    }

    #[test]
    fn catalog_enforces_each_builtin_tool_setting() {
        let bytes = serde_json::to_vec(&json!({
            "models": [{
                "slug": "example-model",
                "tool_mode": "code_mode_only",
                "shell_type": "shell_command",
                "supports_search_tool": true,
                "apply_patch_tool_type": "freeform",
                "multi_agent_version": "v2"
            }, { "slug": "other-model" }]
        }))
        .unwrap_or_else(|error| panic!("encode catalog: {error}"));
        let directory = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("create temporary directory: {error}"));
        let mut spec = CodexRunSpec {
            instructions: "Follow the requester contract.".to_owned(),
            developer_instructions: None,
            prompt: "work".to_owned(),
            model: "example-model".to_owned(),
            reasoning_effort: Some("medium".to_owned()),
            working_directory: directory.path().to_path_buf(),
            workspace_access: WorkspaceAccess::ReadOnly,
            builtin_tools: BuiltinToolsV1 {
                local_execution: false,
                web_search: false,
            },
            timeout: Duration::from_secs(30),
            tools: Vec::new(),
            launch_environment: None,
        };

        let restricted = configured_model_catalog(&bytes, &spec)
            .unwrap_or_else(|error| panic!("configure restricted catalog: {error}"));
        let model = &restricted["models"][0];
        assert_eq!(restricted["models"].as_array().map(Vec::len), Some(1));
        assert_eq!(model["tool_mode"], "direct");
        assert_eq!(model["shell_type"], "disabled");
        assert_eq!(model["supports_search_tool"], false);
        assert!(model["apply_patch_tool_type"].is_null());
        assert_eq!(model["multi_agent_version"], "disabled");
        assert_eq!(
            model["base_instructions"].as_str(),
            Some(spec.instructions.as_str())
        );
        assert!(model["model_messages"].is_null());

        spec.builtin_tools = BuiltinToolsV1 {
            local_execution: true,
            web_search: true,
        };
        spec.workspace_access = WorkspaceAccess::ReadWrite;
        let enabled = configured_model_catalog(&bytes, &spec)
            .unwrap_or_else(|error| panic!("configure enabled catalog: {error}"));
        let model = &enabled["models"][0];
        assert_eq!(model["tool_mode"], "code_mode_only");
        assert_eq!(model["shell_type"], "shell_command");
        assert_eq!(model["supports_search_tool"], true);
        assert_eq!(model["apply_patch_tool_type"], "freeform");
    }

    #[test]
    fn runtime_flags_are_derived_from_builtin_tool_policy() {
        let directory = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("create temporary directory: {error}"));
        let mut spec = CodexRunSpec {
            instructions: "Follow the requester contract.".to_owned(),
            developer_instructions: None,
            prompt: "work".to_owned(),
            model: "example-model".to_owned(),
            reasoning_effort: None,
            working_directory: directory.path().to_path_buf(),
            workspace_access: WorkspaceAccess::ReadOnly,
            builtin_tools: BuiltinToolsV1 {
                local_execution: false,
                web_search: false,
            },
            timeout: Duration::from_secs(30),
            tools: Vec::new(),
            launch_environment: None,
        };
        assert!(disabled_features(&spec).contains(&"code_mode"));
        assert!(disabled_features(&spec).contains(&"standalone_web_search"));
        assert!(runtime_config(&spec).contains(&"web_search=\"disabled\"".to_owned()));

        spec.builtin_tools = BuiltinToolsV1 {
            local_execution: true,
            web_search: true,
        };
        assert!(!disabled_features(&spec).contains(&"code_mode"));
        assert!(!disabled_features(&spec).contains(&"standalone_web_search"));
        let settings = runtime_config(&spec);
        assert!(settings.contains(&"web_search=\"live\"".to_owned()));
        assert!(settings.contains(&"features.code_mode_host=true".to_owned()));
        assert!(settings.contains(&"sandbox_permissions=[\"disk-full-read-access\"]".to_owned()));
    }

    #[test]
    fn authentication_document_requires_usable_file_credentials() {
        for invalid in [
            b"".as_slice(),
            b"{}".as_slice(),
            b"[]".as_slice(),
            br#"{"tokens":"#.as_slice(),
            br#"{"OPENAI_API_KEY":"  "}"#.as_slice(),
            br#"{"tokens":{"access_token":"access","refresh_token":""}}"#.as_slice(),
            br#"{"tokens":{"access_token":"access","refresh_token":"refresh"}}"#.as_slice(),
            br#"{"tokens":{"access_token":"access","refresh_token":"refresh","account_id":"account"}}"#.as_slice(),
            br#"{"auth_mode":"agentIdentity","OPENAI_API_KEY":"key"}"#.as_slice(),
        ] {
            assert!(
                validate_auth_document(invalid).is_err(),
                "invalid authentication document was accepted: {}",
                String::from_utf8_lossy(invalid)
            );
        }
        validate_auth_document(br#"{"OPENAI_API_KEY":"key"}"#)
            .unwrap_or_else(|error| panic!("validate API-key authentication: {error}"));
        validate_auth_document(
            br#"{"tokens":{"access_token":"header.e30.signature","refresh_token":"refresh","account_id":"account"}}"#,
        )
        .unwrap_or_else(|error| panic!("validate token authentication: {error}"));
    }

    #[test]
    fn managed_authentication_uses_id_token_identity_without_exposing_refresh_token() {
        use base64::Engine as _;

        let claims = json!({
            "https://api.openai.com/auth": {
                "chatgpt_account_id": CHATGPT_ACCOUNT_ID,
                "chatgpt_plan_type": "pro"
            }
        });
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&claims)
                .unwrap_or_else(|error| panic!("encode JWT claims: {error}")),
        );
        let id_token = format!("header.{payload}.signature");
        let document = serde_json::to_vec(&json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": id_token,
                "access_token": INITIAL_ACCESS_TOKEN,
                "refresh_token": INITIAL_REFRESH_TOKEN
            }
        }))
        .unwrap_or_else(|error| panic!("encode authentication: {error}"));

        let WorkerAuthentication::ManagedChatgpt(credentials) =
            super::worker_authentication_from_document(&document)
                .unwrap_or_else(|error| panic!("parse managed authentication: {error}"))
        else {
            panic!("managed authentication was classified as an API key");
        };
        assert_eq!(credentials.access_token, INITIAL_ACCESS_TOKEN);
        assert_eq!(credentials.account_id, CHATGPT_ACCOUNT_ID);
        assert_eq!(credentials.plan_type.as_deref(), Some("pro"));
    }

    #[tokio::test]
    async fn sensitive_authentication_rejection_is_not_emitted_or_echoed()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("managed-fake-codex");
        write_managed_fake_codex(&executable)?;
        fs::write(directory.path().join("reject-sensitive-login"), b"")?;
        let codex_home = directory.path().join("codex-home");
        write_test_codex_home(
            &codex_home,
            &managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN),
        )?;
        let harness = CodexHarness::with_codex_home(&executable, &codex_home);
        let inspection = harness.inspect().await?;
        let spec = CodexRunSpec {
            instructions: "Return the managed result.".to_owned(),
            developer_instructions: None,
            prompt: "work".to_owned(),
            model: "example-model".to_owned(),
            reasoning_effort: Some("medium".to_owned()),
            working_directory: directory.path().to_path_buf(),
            workspace_access: WorkspaceAccess::None,
            builtin_tools: BuiltinToolsV1 {
                local_execution: false,
                web_search: false,
            },
            timeout: Duration::from_secs(5),
            tools: Vec::new(),
            launch_environment: None,
        };
        let (events_tx, mut events_rx) = mpsc::channel(64);
        let event_collector = tokio::spawn(async move {
            let mut bytes = Vec::new();
            while let Some(event) = events_rx.recv().await {
                match event {
                    CodexEvent::Protocol { bytes: record, .. } | CodexEvent::Stderr(record) => {
                        bytes.extend(record);
                    }
                    CodexEvent::ToolCall(_) => panic!("managed fixture emitted a tool call"),
                }
            }
            bytes
        });
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let Err(error) = harness.run(&inspection, spec, events_tx, cancel_rx).await else {
            panic!("sensitive authentication rejection must fail");
        };
        assert!(!error.to_string().contains(INITIAL_ACCESS_TOKEN));
        let event_bytes = event_collector.await?;
        assert!(!String::from_utf8_lossy(&event_bytes).contains(INITIAL_ACCESS_TOKEN));
        Ok(())
    }

    #[tokio::test]
    async fn managed_worker_home_contains_no_persistent_authentication()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let persistent_home = directory.path().join("persistent-home");
        let isolated_home = directory.path().join("isolated-home");
        write_test_codex_home(
            &persistent_home,
            &managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN),
        )?;
        fs::create_dir(&isolated_home)?;
        let authentication = read_worker_authentication(&persistent_home)?;

        prepare_isolated_codex_home(&isolated_home, &authentication).await?;

        assert!(isolated_home.join("config.toml").is_file());
        assert!(!isolated_home.join("auth.json").exists());
        Ok(())
    }

    #[tokio::test]
    async fn isolated_worker_authentication_never_replaces_authoritative_authentication()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let persistent_home = directory.path().join("persistent-home");
        let isolated_home = directory.path().join("isolated-home");
        fs::create_dir(&persistent_home)?;
        fs::create_dir(&isolated_home)?;
        let authoritative = br#"{"OPENAI_API_KEY":"authoritative"}"#;
        fs::write(persistent_home.join("auth.json"), authoritative)?;
        fs::set_permissions(
            persistent_home.join("auth.json"),
            fs::Permissions::from_mode(0o600),
        )?;
        prepare_isolated_codex_home(
            &isolated_home,
            &WorkerAuthentication::ApiKey("authoritative".to_owned()),
        )
        .await?;
        fs::write(
            isolated_home.join("auth.json"),
            br#"{"OPENAI_API_KEY":"worker-write"}"#,
        )?;

        assert_eq!(fs::read(persistent_home.join("auth.json"))?, authoritative);
        Ok(())
    }

    #[tokio::test]
    async fn nonblocking_account_lease_reports_authentication_busy()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let codex_home = directory.path().join("codex-home");
        fs::create_dir(&codex_home)?;
        fs::set_permissions(&codex_home, fs::Permissions::from_mode(0o700))?;
        fs::write(
            codex_home.join("config.toml"),
            "cli_auth_credentials_store = \"file\"\n",
        )?;
        fs::set_permissions(
            codex_home.join("config.toml"),
            fs::Permissions::from_mode(0o600),
        )?;
        fs::write(
            codex_home.join("auth.json"),
            r#"{"OPENAI_API_KEY":"credential"}"#,
        )?;
        fs::set_permissions(
            codex_home.join("auth.json"),
            fs::Permissions::from_mode(0o600),
        )?;
        let harness = CodexHarness::with_codex_home("unused-codex", codex_home);
        let _held = harness.acquire_credential_lease().await?;
        let Err(error) = harness.try_acquire_credential_lease().await else {
            panic!("nonblocking lease unexpectedly succeeded");
        };
        assert!(matches!(error, CodexError::AuthenticationBusy));
        Ok(())
    }

    #[tokio::test]
    async fn launch_environment_is_a_full_snapshot_with_nucleus_overrides()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("environment-codex");
        let capture = directory.path().join("environment.txt");
        fs::write(
            &executable,
            r#"#!/bin/sh
set -eu
env > "$CAPTURE_PATH"
printf '%s\n' '{"models":[{"slug":"example-model","shell_type":"shell_command","supports_search_tool":true}]}'
"#,
        )?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
        let codex_home = directory.path().join("isolated-home");
        fs::create_dir(&codex_home)?;
        let mut environment = BTreeMap::new();
        environment.insert("CALLER_ONLY".to_owned(), "present".to_owned());
        environment.insert("CAPTURE_PATH".to_owned(), capture.display().to_string());
        environment.insert("CODEX_HOME".to_owned(), "/attacker/home".to_owned());
        environment.insert(
            "CODEX_EXEC_SERVER_URL".to_owned(),
            "https://attacker.invalid".to_owned(),
        );
        environment.insert("OPENAI_API_KEY".to_owned(), "requester-api-key".to_owned());
        environment.insert(
            "CODEX_ACCESS_TOKEN".to_owned(),
            "requester-access-token".to_owned(),
        );
        let spec = CodexRunSpec {
            instructions: "contract".to_owned(),
            developer_instructions: None,
            prompt: "work".to_owned(),
            model: "example-model".to_owned(),
            reasoning_effort: None,
            working_directory: directory.path().to_path_buf(),
            workspace_access: WorkspaceAccess::ReadOnly,
            builtin_tools: BuiltinToolsV1 {
                local_execution: true,
                web_search: true,
            },
            timeout: Duration::from_secs(5),
            tools: Vec::new(),
            launch_environment: Some(environment),
        };
        let harness = CodexHarness::new(&executable);
        harness
            .write_model_catalog(directory.path(), &codex_home, &spec)
            .await?;

        let captured = fs::read_to_string(capture)?;
        assert!(captured.lines().any(|line| line == "CALLER_ONLY=present"));
        assert!(!captured.lines().any(|line| line.starts_with("HOME=")));
        assert!(
            !captured
                .lines()
                .any(|line| line.starts_with("CODEX_EXEC_SERVER_URL="))
        );
        assert!(
            !captured
                .lines()
                .any(|line| line.starts_with("OPENAI_API_KEY="))
        );
        assert!(
            !captured
                .lines()
                .any(|line| line.starts_with("CODEX_ACCESS_TOKEN="))
        );
        assert!(
            captured
                .lines()
                .any(|line| { line == format!("CODEX_HOME={}", codex_home.display()) })
        );
        Ok(())
    }

    #[tokio::test]
    async fn job_sessions_overlap_and_exclude_attended_login_across_harnesses()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("managed-fake-codex");
        write_managed_fake_codex(&executable)?;
        let codex_home = directory.path().join("codex-home");
        write_test_codex_home(
            &codex_home,
            &managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN),
        )?;
        let first = CodexHarness::with_codex_home(&executable, &codex_home);
        // A separate harness has separate in-memory gates, so this also proves
        // the shared/exclusive file-lock boundary used across processes.
        let second = CodexHarness::with_codex_home(&executable, &codex_home);
        let first_job = first.acquire_auth_session().await?;
        let second_job = second.try_acquire_auth_session().await?;
        let login_harness = CodexHarness::with_codex_home(&executable, &codex_home);
        let login = tokio::spawn(async move { login_harness.login(false).await });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!directory.path().join("login-started").exists());
        assert!(!login.is_finished());

        drop(first_job);
        drop(second_job);
        let status = tokio::time::timeout(Duration::from_secs(2), login).await???;
        assert!(status.success());
        assert!(directory.path().join("login-started").is_file());
        assert_ne!(
            fs::read_to_string(directory.path().join("login-home"))?,
            codex_home.display().to_string()
        );
        Ok(())
    }

    #[tokio::test]
    async fn account_snapshot_promotes_a_safe_proactive_refresh_from_staging()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("managed-fake-codex");
        write_managed_fake_codex(&executable)?;
        fs::write(directory.path().join("proactive-account-refresh"), b"")?;
        let codex_home = directory.path().join("codex-home");
        write_test_codex_home(
            &codex_home,
            &managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN),
        )?;
        let harness = CodexHarness::with_codex_home(&executable, &codex_home);

        let snapshot = harness
            .read_account_snapshot(true, Duration::from_secs(1), Duration::from_secs(3))
            .await?;
        assert_eq!(snapshot.rate_limits["limitId"], "primary");
        assert_eq!(
            snapshot
                .usage
                .as_ref()
                .and_then(|usage| usage.get("planType")),
            Some(&json!("pro"))
        );
        let staging_home = fs::read_to_string(directory.path().join("account-read-home"))?;
        assert_ne!(staging_home, codex_home.display().to_string());
        assert!(!Path::new(&staging_home).exists());
        let WorkerAuthentication::ManagedChatgpt(refreshed) =
            read_worker_authentication(&codex_home)?
        else {
            panic!("account refresh changed authentication modes");
        };
        assert_eq!(refreshed.access_token, REFRESHED_ACCESS_TOKEN);
        assert_eq!(refreshed.account_id, CHATGPT_ACCOUNT_ID);
        Ok(())
    }

    #[tokio::test]
    async fn account_request_cancellation_cannot_strand_a_rotated_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("managed-fake-codex");
        write_managed_fake_codex(&executable)?;
        fs::write(directory.path().join("slow-account-refresh"), b"")?;
        let codex_home = directory.path().join("codex-home");
        write_test_codex_home(
            &codex_home,
            &managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN),
        )?;
        let harness = CodexHarness::with_codex_home(&executable, &codex_home);
        let account_harness = harness.clone();
        let account = tokio::spawn(async move {
            account_harness
                .read_account_snapshot(false, Duration::from_secs(1), Duration::from_secs(3))
                .await
        });

        let started = directory.path().join("account-read-started");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while !started.is_file() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "proactive account refresh did not start"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let staging_home = fs::read_to_string(directory.path().join("account-read-home"))?;
        assert_ne!(staging_home, codex_home.display().to_string());
        let WorkerAuthentication::ManagedChatgpt(before_promotion) =
            read_worker_authentication(&codex_home)?
        else {
            panic!("canonical authentication changed modes");
        };
        assert_eq!(before_promotion.access_token, INITIAL_ACCESS_TOKEN);

        account.abort();
        let Err(cancellation) = account.await else {
            panic!("account request completed before cancellation");
        };
        assert!(cancellation.is_cancelled());
        tokio::time::timeout(Duration::from_secs(4), harness.wait_for_auth_idle()).await?;

        let WorkerAuthentication::ManagedChatgpt(refreshed) =
            read_worker_authentication(&codex_home)?
        else {
            panic!("account refresh changed authentication modes");
        };
        assert_eq!(refreshed.access_token, REFRESHED_ACCESS_TOKEN);
        assert_eq!(refreshed.account_id, CHATGPT_ACCOUNT_ID);
        assert!(!Path::new(&staging_home).exists());
        Ok(())
    }

    #[tokio::test]
    async fn account_request_timeout_still_promotes_a_complete_rotated_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("managed-fake-codex");
        write_managed_fake_codex(&executable)?;
        fs::write(directory.path().join("timeout-account-refresh"), b"")?;
        let codex_home = directory.path().join("codex-home");
        write_test_codex_home(
            &codex_home,
            &managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN),
        )?;
        let harness = CodexHarness::with_codex_home(&executable, &codex_home);

        let Err(error) = harness
            .read_account_snapshot(false, Duration::from_secs(1), Duration::from_secs(3))
            .await
        else {
            panic!("stalled account request must time out");
        };
        assert!(matches!(error, CodexError::TimedOut));
        let WorkerAuthentication::ManagedChatgpt(refreshed) =
            read_worker_authentication(&codex_home)?
        else {
            panic!("account refresh changed authentication modes");
        };
        assert_eq!(refreshed.access_token, REFRESHED_ACCESS_TOKEN);
        assert_eq!(refreshed.account_id, CHATGPT_ACCOUNT_ID);
        let staging_home = fs::read_to_string(directory.path().join("account-read-home"))?;
        assert!(!Path::new(&staging_home).exists());
        Ok(())
    }

    #[test]
    fn account_promotion_preserves_a_refresh_token_only_rotation()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let codex_home = directory.path().join("codex-home");
        let canonical = managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN);
        let staged = managed_auth_document(INITIAL_ACCESS_TOKEN, REFRESHED_REFRESH_TOKEN);
        write_test_codex_home(&codex_home, &canonical)?;

        super::promote_account_auth_if_advanced(
            &codex_home,
            canonical.as_bytes(),
            staged.as_bytes(),
        )?;
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(codex_home.join("auth.json"))?)?,
            serde_json::from_str::<Value>(&staged)?
        );
        Ok(())
    }

    #[tokio::test]
    async fn attended_login_promotes_only_from_its_private_staging_home()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("managed-fake-codex");
        write_managed_fake_codex(&executable)?;
        fs::write(directory.path().join("login-refresh"), b"")?;
        let codex_home = directory.path().join("codex-home");
        write_test_codex_home(
            &codex_home,
            &managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN),
        )?;
        let harness = CodexHarness::with_codex_home(&executable, &codex_home);

        assert!(harness.login(false).await?.success());
        let staging_home = fs::read_to_string(directory.path().join("login-home"))?;
        assert_ne!(staging_home, codex_home.display().to_string());
        assert!(!Path::new(&staging_home).exists());
        let WorkerAuthentication::ManagedChatgpt(refreshed) =
            read_worker_authentication(&codex_home)?
        else {
            panic!("login changed authentication modes");
        };
        assert_eq!(refreshed.access_token, REFRESHED_ACCESS_TOKEN);
        assert_eq!(refreshed.account_id, CHATGPT_ACCOUNT_ID);
        Ok(())
    }

    #[tokio::test]
    async fn attended_login_cancellation_cannot_damage_canonical_authentication()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("managed-fake-codex");
        write_managed_fake_codex(&executable)?;
        fs::write(directory.path().join("slow-login"), b"")?;
        let codex_home = directory.path().join("codex-home");
        let initial = managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN);
        write_test_codex_home(&codex_home, &initial)?;
        let harness = CodexHarness::with_codex_home(&executable, &codex_home);
        let login = tokio::spawn(async move { harness.login(false).await });

        let started = directory.path().join("login-started");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while !started.is_file() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "attended login did not start"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let staging_home = fs::read_to_string(directory.path().join("login-home"))?;
        assert_ne!(staging_home, codex_home.display().to_string());
        login.abort();
        let Err(cancellation) = login.await else {
            panic!("attended login completed before cancellation");
        };
        assert!(cancellation.is_cancelled());
        assert!(!Path::new(&staging_home).exists());
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(codex_home.join("auth.json"))?)?,
            serde_json::from_str::<Value>(&initial)?
        );
        Ok(())
    }

    #[tokio::test]
    async fn closed_auth_operation_gate_rejects_new_account_and_refresh_children()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("managed-fake-codex");
        write_managed_fake_codex(&executable)?;
        let codex_home = directory.path().join("codex-home");
        write_test_codex_home(
            &codex_home,
            &managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN),
        )?;
        let harness = CodexHarness::with_codex_home(&executable, &codex_home);
        let WorkerAuthentication::ManagedChatgpt(rejected) =
            read_worker_authentication(&codex_home)?
        else {
            panic!("managed authentication was classified as an API key");
        };
        harness.close_auth_operations();

        let Err(account_error) = harness
            .read_account_snapshot(false, Duration::from_secs(1), Duration::from_secs(3))
            .await
        else {
            panic!("closed supervisor started an account child");
        };
        assert!(account_error.to_string().contains("shutting down"));
        let auth_session = harness.acquire_auth_session().await?;
        let Err(refresh_error) = harness.refresh_managed_auth(&rejected, auth_session).await else {
            panic!("closed supervisor started a refresh child");
        };
        assert!(refresh_error.to_string().contains("shutting down"));
        assert!(!directory.path().join("account-read-home").exists());
        assert!(!directory.path().join("refresh-count").exists());
        Ok(())
    }

    #[tokio::test]
    async fn changed_generation_from_another_account_is_never_adopted()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let codex_home = directory.path().join("codex-home");
        write_test_codex_home(
            &codex_home,
            &managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN),
        )?;
        let WorkerAuthentication::ManagedChatgpt(rejected) =
            read_worker_authentication(&codex_home)?
        else {
            panic!("managed authentication was classified as an API key");
        };
        fs::write(
            codex_home.join("auth.json"),
            json!({
                "auth_mode": "chatgpt",
                "tokens": {
                    "access_token": REFRESHED_ACCESS_TOKEN,
                    "refresh_token": REFRESHED_REFRESH_TOKEN,
                    "account_id": "different-account",
                }
            })
            .to_string(),
        )?;
        let harness = CodexHarness::with_codex_home("unused-codex", &codex_home);
        let auth_session = harness.acquire_auth_session().await?;

        let Err(error) = harness.refresh_managed_auth(&rejected, auth_session).await else {
            panic!("a different account generation must be rejected");
        };
        assert!(error.to_string().contains("account changed"));
        Ok(())
    }

    #[tokio::test]
    async fn managed_refresh_subprocess_helper() -> Result<(), Box<dyn std::error::Error>> {
        let Some(codex_home) = std::env::var_os("NUCLEUS_TEST_REFRESH_HOME") else {
            return Ok(());
        };
        let executable = std::env::var_os("NUCLEUS_TEST_REFRESH_EXECUTABLE")
            .ok_or("refresh helper omitted executable")?;
        let WorkerAuthentication::ManagedChatgpt(rejected) =
            super::worker_authentication_from_document(
                managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN).as_bytes(),
            )?
        else {
            panic!("managed authentication was classified as an API key");
        };
        let harness = CodexHarness::with_codex_home(executable, codex_home);
        let auth_session = harness.acquire_auth_session().await?;
        let refreshed = harness
            .refresh_managed_auth(&rejected, auth_session)
            .await?;
        assert_eq!(refreshed.access_token, REFRESHED_ACCESS_TOKEN);
        assert_eq!(refreshed.account_id, CHATGPT_ACCOUNT_ID);
        Ok(())
    }

    #[tokio::test]
    async fn eight_simultaneous_rejections_force_one_canonical_refresh()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("managed-fake-codex");
        write_managed_fake_codex(&executable)?;
        let codex_home = directory.path().join("codex-home");
        write_test_codex_home(
            &codex_home,
            &managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN),
        )?;
        let test_executable = std::env::current_exe()?;
        let mut refreshes = Vec::new();
        for _ in 0..8 {
            refreshes.push(
                tokio::process::Command::new(&test_executable)
                    .args([
                        "--exact",
                        "tests::managed_refresh_subprocess_helper",
                        "--nocapture",
                    ])
                    .env("NUCLEUS_TEST_REFRESH_HOME", &codex_home)
                    .env("NUCLEUS_TEST_REFRESH_EXECUTABLE", &executable)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?,
            );
        }
        for mut refresh in refreshes {
            assert!(refresh.wait().await?.success());
        }
        assert_eq!(
            fs::read_to_string(directory.path().join("refresh-count"))?
                .lines()
                .count(),
            1
        );
        assert_eq!(
            serde_json::from_str::<Value>(&fs::read_to_string(codex_home.join("auth.json"))?)?,
            serde_json::from_str::<Value>(&managed_auth_document(
                REFRESHED_ACCESS_TOKEN,
                REFRESHED_REFRESH_TOKEN,
            ))?
        );
        Ok(())
    }

    #[tokio::test]
    async fn managed_worker_refresh_is_end_to_end_and_secrets_never_become_events()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("managed-fake-codex");
        write_managed_fake_codex(&executable)?;
        let codex_home = directory.path().join("codex-home");
        write_test_codex_home(
            &codex_home,
            &managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN),
        )?;
        let harness = CodexHarness::with_codex_home(&executable, &codex_home);
        let inspection = harness.inspect().await?;
        let spec = CodexRunSpec {
            instructions: "Return the managed result.".to_owned(),
            developer_instructions: None,
            prompt: "work".to_owned(),
            model: "example-model".to_owned(),
            reasoning_effort: Some("medium".to_owned()),
            working_directory: directory.path().to_path_buf(),
            workspace_access: WorkspaceAccess::None,
            builtin_tools: BuiltinToolsV1 {
                local_execution: false,
                web_search: false,
            },
            timeout: Duration::from_secs(5),
            tools: Vec::new(),
            launch_environment: None,
        };
        let (events_tx, mut events_rx) = mpsc::channel(64);
        let event_collector = tokio::spawn(async move {
            let mut bytes = Vec::new();
            while let Some(event) = events_rx.recv().await {
                match event {
                    CodexEvent::Protocol { bytes: record, .. } | CodexEvent::Stderr(record) => {
                        bytes.extend(record);
                    }
                    CodexEvent::ToolCall(_) => panic!("managed fixture emitted a tool call"),
                }
            }
            bytes
        });
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let outcome = harness.run(&inspection, spec, events_tx, cancel_rx).await?;
        assert_eq!(outcome.final_message, "managed");
        let event_bytes = event_collector.await?;
        assert!(
            !String::from_utf8_lossy(&event_bytes).contains("account/chatgptAuthTokens/refresh"),
            "authentication refresh request became a Codex event"
        );
        for secret in [
            INITIAL_ACCESS_TOKEN,
            REFRESHED_ACCESS_TOKEN,
            INITIAL_REFRESH_TOKEN,
            REFRESHED_REFRESH_TOKEN,
        ] {
            assert!(
                !String::from_utf8_lossy(&event_bytes).contains(secret),
                "authentication material escaped through a Codex event"
            );
        }
        assert_eq!(
            fs::read_to_string(directory.path().join("refresh-count"))?
                .lines()
                .count(),
            1
        );
        assert_eq!(
            serde_json::from_str::<Value>(&fs::read_to_string(codex_home.join("auth.json"))?)?,
            serde_json::from_str::<Value>(&managed_auth_document(
                REFRESHED_ACCESS_TOKEN,
                REFRESHED_REFRESH_TOKEN,
            ))?
        );
        Ok(())
    }

    #[tokio::test]
    async fn worker_cancellation_cannot_interrupt_canonical_refresh()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("managed-fake-codex");
        write_managed_fake_codex(&executable)?;
        fs::write(directory.path().join("slow-refresh"), b"")?;
        let codex_home = directory.path().join("codex-home");
        write_test_codex_home(
            &codex_home,
            &managed_auth_document(INITIAL_ACCESS_TOKEN, INITIAL_REFRESH_TOKEN),
        )?;
        let harness = CodexHarness::with_codex_home(&executable, &codex_home);
        let inspection = harness.inspect().await?;
        let spec = CodexRunSpec {
            instructions: "Return the managed result.".to_owned(),
            developer_instructions: None,
            prompt: "work".to_owned(),
            model: "example-model".to_owned(),
            reasoning_effort: Some("medium".to_owned()),
            working_directory: directory.path().to_path_buf(),
            workspace_access: WorkspaceAccess::None,
            builtin_tools: BuiltinToolsV1 {
                local_execution: false,
                web_search: false,
            },
            timeout: Duration::from_secs(5),
            tools: Vec::new(),
            launch_environment: None,
        };
        let (events_tx, mut events_rx) = mpsc::channel(64);
        let event_drain = tokio::spawn(async move { while events_rx.recv().await.is_some() {} });
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let worker_harness = harness.clone();
        let worker = tokio::spawn(async move {
            worker_harness
                .run(&inspection, spec, events_tx, cancel_rx)
                .await
        });

        let refresh_started = directory.path().join("refresh-started");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while !refresh_started.is_file() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "managed refresh did not start"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        cancel_tx.send(true)?;
        let result = tokio::time::timeout(Duration::from_secs(2), worker).await??;
        assert!(matches!(result, Err(CodexError::Cancelled)));

        tokio::time::timeout(Duration::from_secs(3), harness.wait_for_auth_idle()).await?;
        event_drain.await?;
        let WorkerAuthentication::ManagedChatgpt(refreshed) =
            read_worker_authentication(&codex_home)?
        else {
            panic!("refreshed authentication changed modes");
        };
        assert_eq!(refreshed.access_token, REFRESHED_ACCESS_TOKEN);
        assert_eq!(refreshed.account_id, CHATGPT_ACCOUNT_ID);
        assert_eq!(
            fs::read_to_string(directory.path().join("refresh-count"))?
                .lines()
                .count(),
            1
        );
        Ok(())
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn fake_app_server_preserves_protocol_and_cleans_descendants()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("fake-codex");
        let descendant_pid = directory.path().join("descendant.pid");
        write_fake_codex(&executable, &descendant_pid)?;
        let codex_home = directory.path().join("codex-home");
        fs::create_dir(&codex_home)?;
        fs::set_permissions(&codex_home, fs::Permissions::from_mode(0o700))?;
        fs::write(
            codex_home.join("config.toml"),
            "cli_auth_credentials_store = \"file\"\n",
        )?;
        fs::set_permissions(
            codex_home.join("config.toml"),
            fs::Permissions::from_mode(0o600),
        )?;
        fs::write(
            codex_home.join("auth.json"),
            r#"{"OPENAI_API_KEY":"credential"}"#,
        )?;
        fs::set_permissions(
            codex_home.join("auth.json"),
            fs::Permissions::from_mode(0o600),
        )?;
        let harness = CodexHarness::with_codex_home(&executable, &codex_home);
        let inspection = harness.inspect().await?;
        assert_eq!(inspection.version, SUPPORTED_CODEX_VERSION);
        assert!(inspection.models[0].supports_local_execution);
        assert!(inspection.models[0].supports_web_search);
        let spec = CodexRunSpec {
            instructions: "Use only create_todo and call it once.".to_owned(),
            developer_instructions: Some("Call the supplied tool exactly once.".to_owned()),
            prompt: "Create one todo".to_owned(),
            model: "example-model".to_owned(),
            reasoning_effort: Some("medium".to_owned()),
            working_directory: directory.path().to_path_buf(),
            workspace_access: WorkspaceAccess::None,
            builtin_tools: BuiltinToolsV1 {
                local_execution: false,
                web_search: false,
            },
            timeout: Duration::from_secs(5),
            tools: vec![DynamicTool {
                name: "create_todo".to_owned(),
                description: "Create one todo".to_owned(),
                input_schema: json!({"type": "object"}),
            }],
            launch_environment: None,
        };
        let (events_tx, mut events_rx) = mpsc::channel(32);
        let event_collector = tokio::spawn(async move {
            let mut protocol = Vec::new();
            let mut saw_stderr = false;
            while let Some(event) = events_rx.recv().await {
                match event {
                    CodexEvent::Protocol { bytes, .. } => protocol.push(bytes),
                    CodexEvent::Stderr(bytes) => saw_stderr |= !bytes.is_empty(),
                    CodexEvent::ToolCall(call) => {
                        assert_eq!(call.call_id, "call-1");
                        assert_eq!(call.name, "create_todo");
                        assert_eq!(call.arguments["title"], "Actionable");
                        call.reply
                            .send(ToolResult {
                                success: true,
                                content: r#"{"id":"t1"}"#.to_owned(),
                            })
                            .unwrap_or_else(|_| panic!("adapter must await the tool result"));
                    }
                }
            }
            (protocol, saw_stderr)
        });
        // A requester dropping the cancellation handle is not cancellation.
        let (cancel_tx, cancel_rx) = watch::channel(false);
        drop(cancel_tx);
        let outcome = harness.run(&inspection, spec, events_tx, cancel_rx).await?;
        assert_eq!(outcome.thread_id, "thread-1");
        assert_eq!(outcome.turn_id, "turn-1");
        assert_eq!(outcome.final_message, "created");
        let (records, saw_stderr) = event_collector.await?;
        assert!(saw_stderr);
        assert!(records.iter().all(|record| record.ends_with(b"\n")));
        assert!(records.iter().any(|record| {
            std::str::from_utf8(record)
                .is_ok_and(|line| line.contains("\"method\":\"item/tool/call\""))
        }));
        assert!(records.iter().any(|record| {
            std::str::from_utf8(record).is_ok_and(|line| {
                line.contains("\"baseInstructions\":\"Use only create_todo and call it once.\"")
            })
        }));
        assert!(records.iter().any(|record| {
            std::str::from_utf8(record).is_ok_and(|line| {
                line.contains("\"developerInstructions\":\"Call the supplied tool exactly once.\"")
                    && line.contains("\"experimentalRawEvents\":true")
                    && line.contains("\"environments\":[]")
            })
        }));
        assert!(records.iter().any(|record| {
            std::str::from_utf8(record).is_ok_and(|line| line.contains("\\\"id\\\":\\\"t1\\\""))
        }));
        assert_eq!(
            fs::read_to_string(codex_home.join("auth.json"))?,
            r#"{"OPENAI_API_KEY":"credential"}"#
        );

        let failed_spec = CodexRunSpec {
            instructions: "contract".to_owned(),
            developer_instructions: None,
            prompt: "FAIL_AFTER_THREAD".to_owned(),
            model: "example-model".to_owned(),
            reasoning_effort: Some("medium".to_owned()),
            working_directory: directory.path().to_path_buf(),
            workspace_access: WorkspaceAccess::None,
            builtin_tools: BuiltinToolsV1 {
                local_execution: false,
                web_search: false,
            },
            timeout: Duration::from_secs(5),
            tools: Vec::new(),
            launch_environment: None,
        };
        let (failed_events, mut failed_events_rx) = mpsc::channel(32);
        let drain = tokio::spawn(async move { while failed_events_rx.recv().await.is_some() {} });
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let Err(failure) = harness
            .run(&inspection, failed_spec, failed_events, cancel_rx)
            .await
        else {
            panic!("nonzero app-server exit should fail");
        };
        assert!(
            matches!(failure, CodexError::HarnessFailure { .. }),
            "unexpected failure: {failure:?}"
        );
        drain.await?;

        let pid: u32 = fs::read_to_string(&descendant_pid)?.trim().parse()?;
        for _ in 0..20 {
            if !process_exists(pid) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("descendant process {pid} survived adapter cleanup");
    }

    fn write_managed_fake_codex(path: &Path) -> std::io::Result<()> {
        fs::write(
            path,
            r#"#!/bin/sh
set -eu
SCRIPT_DIR=${0%/*}
if [ "${1:-}" = "--version" ]; then
  printf '%s\n' 'codex-cli 0.146.0'
  exit 0
fi
if [ "${1:-}" = "debug" ]; then
  printf '%s\n' '{"models":[{"slug":"example-model","default_reasoning_level":"medium","supported_reasoning_levels":[{"effort":"medium"}],"tool_mode":"code_mode_only","shell_type":"shell_command","supports_search_tool":true,"apply_patch_tool_type":"freeform"}]}'
  exit 0
fi
if [ "${1:-}" = "login" ]; then
  printf '%s' "$CODEX_HOME" > "$SCRIPT_DIR/login-home"
  if [ -f "$SCRIPT_DIR/slow-login" ]; then
    printf '%s' '{"auth_mode":' > "$CODEX_HOME/auth.json"
    printf '%s' 'started' > "$SCRIPT_DIR/login-started"
    sleep 5
    exit 48
  fi
  if [ -f "$SCRIPT_DIR/login-refresh" ]; then
    printf '%s' '{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"access_token":"header.e30.signature-refreshed-secret","refresh_token":"refreshed-refresh-token-secret","account_id":"account-1"}}' > "$CODEX_HOME/auth.json"
  fi
  printf '%s' 'started' > "$SCRIPT_DIR/login-started"
  exit 0
fi
if [ -f "$CODEX_HOME/auth.json" ]; then
  IFS= read -r initialize
  printf '%s\n' '{"id":0,"result":{}}'
  IFS= read -r initialized
  IFS= read -r account_request
  case "$account_request" in
    *'"method":"account/read"'*'"refreshToken":true'*)
      printf '%s\n' 'refresh' >> "$SCRIPT_DIR/refresh-count"
      if [ -f "$SCRIPT_DIR/slow-refresh" ]; then
        printf '%s' 'started' > "$SCRIPT_DIR/refresh-started"
        sleep 1
      fi
      printf '%s' '{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"access_token":"header.e30.signature-refreshed-secret","refresh_token":"refreshed-refresh-token-secret","account_id":"account-1"}}' > "$CODEX_HOME/auth.json"
      printf '%s\n' '{"id":1,"result":{"account":{"type":"chatgpt"},"requiresOpenaiAuth":true}}'
      exit 0
      ;;
    *'"method":"account/rateLimits/read"'*)
      printf '%s' "$CODEX_HOME" > "$SCRIPT_DIR/account-read-home"
      if [ -f "$SCRIPT_DIR/slow-account-refresh" ]; then
        printf '%s' '{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"access_token":"header.e30.signature-refreshed-secret","refresh_token":"refreshed-refresh-token-secret","account_id":"account-1"}}' > "$CODEX_HOME/auth.json"
        printf '%s' 'started' > "$SCRIPT_DIR/account-read-started"
        sleep 1
      fi
      if [ -f "$SCRIPT_DIR/timeout-account-refresh" ]; then
        printf '%s' '{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"access_token":"header.e30.signature-refreshed-secret","refresh_token":"refreshed-refresh-token-secret","account_id":"account-1"}}' > "$CODEX_HOME/auth.json"
        printf '%s' 'started' > "$SCRIPT_DIR/account-read-started"
        sleep 5
      fi
      if [ -f "$SCRIPT_DIR/proactive-account-refresh" ]; then
        printf '%s' '{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"access_token":"header.e30.signature-refreshed-secret","refresh_token":"refreshed-refresh-token-secret","account_id":"account-1"}}' > "$CODEX_HOME/auth.json"
      fi
      printf '%s\n' '{"id":1,"result":{"limitId":"primary","usedPercent":1}}'
      if IFS= read -r usage_request; then
        case "$usage_request" in
          *'"method":"account/usage/read"'*)
            printf '%s\n' '{"id":2,"result":{"planType":"pro"}}'
            ;;
        esac
      fi
      exit 0
      ;;
    *) exit 40 ;;
  esac
fi

IFS= read -r initialize
printf '%s\n' '{"id":0,"result":{}}'
IFS= read -r initialized
IFS= read -r login
case "$login" in
  *'"method":"account/login/start"'*) ;;
  *) exit 41 ;;
esac
case "$login" in
  *'"accessToken":"header.e30.signature-initial-secret"'*) ;;
  *) exit 42 ;;
esac
case "$login" in
  *'"chatgptAccountId":"account-1"'*) ;;
  *) exit 43 ;;
esac
if [ -f "$SCRIPT_DIR/reject-sensitive-login" ]; then
  printf '%s\n' '{"id":1,"error":{"message":"rejected header.e30.signature-initial-secret"}}'
  printf '%s\n' 'diagnostic header.e30.signature-initial-secret' >&2
  exit 46
fi
printf '%s\n' '{"id":1,"result":{"type":"chatgptAuthTokens"}}'
printf '%s\n' '{"method":"account/updated","params":{"authMode":"chatgptAuthTokens","planType":"pro"}}'
IFS= read -r inventory
printf '%s\n' '{"id":2,"result":{"data":[],"nextCursor":null}}'
IFS= read -r thread
printf '%s\n' '{"id":3,"result":{"thread":{"id":"thread-managed"}}}'
IFS= read -r turn
printf '%s\n' '{"id":4,"result":{"turn":{"id":"turn-managed"}}}'
printf '%s\n' '{"id":20,"method":"account/chatgptAuthTokens/refresh","params":{"reason":"unauthorized","previousAccountId":"account-1"}}'
IFS= read -r refresh_result
case "$refresh_result" in
  *'"accessToken":"header.e30.signature-refreshed-secret"'*) ;;
  *) exit 44 ;;
esac
case "$refresh_result" in
  *'"chatgptAccountId":"account-1"'*) ;;
  *) exit 45 ;;
esac
printf '%s\n' '{"method":"item/completed","params":{"threadId":"thread-managed","turnId":"turn-managed","item":{"type":"agentMessage","text":"managed"}}}'
printf '%s\n' '{"method":"turn/completed","params":{"threadId":"thread-managed","turn":{"id":"turn-managed","status":"completed","items":[]}}}'
"#,
        )?;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions)
    }

    fn write_fake_codex(path: &Path, descendant_pid: &Path) -> std::io::Result<()> {
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '%s\n' 'codex-cli 0.146.0'
  exit 0
fi
if [ "$1" = "debug" ]; then
  printf '%s\n' '{{"models":[{{"slug":"example-model","default_reasoning_level":"medium","supported_reasoning_levels":[{{"effort":"medium"}}],"tool_mode":"code_mode_only","shell_type":"shell_command","supports_search_tool":true,"apply_patch_tool_type":"freeform"}}]}}'
  exit 0
fi
case "$*" in
  *'model_catalog_json='*) ;;
  *) exit 20 ;;
esac
case "$*" in
  *'web_search="disabled"'*) ;;
  *) exit 21 ;;
esac
printf '%s\n' 'diagnostic' >&2
IFS= read -r initialize
printf '%s\n' '{{"id":0,"result":{{}}}}'
IFS= read -r initialized
IFS= read -r inventory
printf '%s\n' '{{"id":1,"result":{{"data":[],"nextCursor":null}}}}'
IFS= read -r thread
printf '%s\n' '{{"id":2,"result":{{"thread":{{"id":"thread-1"}}}}}}'
  IFS= read -r turn
  printf '%s\n' '{{"id":3,"result":{{"turn":{{"id":"turn-1"}}}}}}'
  case "$turn" in
    *FAIL_AFTER_THREAD*) exit 19 ;;
  esac
  (trap '' TERM; sleep 300) &
  printf '%s\n' "$!" > '{pidfile}'
  printf '%s\n' '{{"id":20,"method":"item/tool/call","params":{{"threadId":"thread-1","turnId":"turn-1","callId":"call-1","namespace":null,"tool":"create_todo","arguments":{{"title":"Actionable"}}}}}}'
IFS= read -r tool_result
case "$tool_result" in
  *'"success":true'*) ;;
  *) exit 22 ;;
esac
  printf '%s\n' '{{"method":"item/completed","params":{{"threadId":"thread-1","turnId":"turn-1","item":{{"type":"agentMessage","text":"created"}}}}}}'
  printf '%s\n' '{{"method":"turn/completed","params":{{"threadId":"thread-other","turn":{{"id":"turn-other","status":"completed","items":[{{"type":"agentMessage","text":"wrong-turn"}}]}}}}}}'
  printf '%s\n' '{{"method":"turn/completed","params":{{"threadId":"thread-1","turn":{{"id":"turn-1","status":"completed","items":[]}}}}}}'
  printf '%s' '{{"OPENAI_API_KEY":"refreshed"}}' > "$CODEX_HOME/auth.json"
  wait
"#,
            pidfile = descendant_pid.display(),
        );
        fs::write(path, script)?;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions)
    }

    fn process_exists(pid: u32) -> bool {
        Command::new("/bin/kill")
            .args(["-0", "--", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}
