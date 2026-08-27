//! Per-user Nucleus coordinator and Unix-socket HTTP API.

mod schemas;

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::rejection::QueryRejection;
use axum::extract::{DefaultBodyLimit, Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use nucleus_codex::{
    CodexError, CodexEvent, CodexHarness, CodexRunSpec, DynamicTool, GeneratedSchema,
    HarnessInspection, ProtocolDirection, ToolResult,
};
use nucleus_core::{
    AbsolutePath, AccountSnapshotQueryV1, AccountSnapshotV1, AttemptId, AttemptOutputV1,
    AttemptState, AttemptTerminalReason, AttemptV1, AuthenticationReadinessV1, CancelJobResponseV1,
    ErrorResponseV1, HarnessCapability, HarnessIdentity, HealthResponseV1, JobAcceptedV1, JobId,
    JobRequestV1, JobState, JobSummaryV1, JobV1, LaunchContextAcceptedV1, LaunchContextId,
    LaunchContextRegistrationV1, LifecycleEventKind, LifecycleEventV1, ListJobsQueryV1,
    ListJobsResponseV1, LogRecordV1, LogSchemaV1, LogStream, LogsQueryV1, LogsResponseV1,
    PROTOCOL_VERSION_V1, PendingToolCallV1, ReasoningEffort, RegisteredToolsetV1, Requester,
    SchemaId, ToolCallId, ToolCallState, ToolCallV1, ToolCallsQueryV1, ToolCallsResponseV1,
    ToolResultV1, ToolsetDefinitionsV1, ToolsetRegistrationV1, sha256_digest,
};
use nucleus_store::{
    AttemptRecord, AttemptState as StoreAttemptState, JobRecord, JobState as StoreJobState,
    LogRecord, LogSchemaRecord, NewAttempt, NewJob, NewLogRecord, NewLogSchema, NewPendingToolCall,
    NewToolResult, NewToolset, PendingToolCallRecord, Store, StoreError,
    ToolCallState as StoreToolCallState, ToolsetRecord,
};
use serde_json::value::{RawValue, to_raw_value};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::net::UnixListener;
use tokio::sync::{Mutex, broadcast, oneshot, watch};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::schemas::{
    BYTES_ID, INTERNAL_SCHEMAS, JOB_REQUEST_ID, LIFECYCLE_ID, TOOLSET_DEFINITIONS_ID,
};

const ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const MAX_PAGE: usize = 1_000;
const DEFAULT_PAGE: usize = 100;
const LOG_FOLLOW_WAIT: Duration = Duration::from_secs(25);
const STARTUP_INSPECTION_TIMEOUT: Duration = Duration::from_secs(3);
const ADMISSION_INSPECTION_TIMEOUT: Duration = Duration::from_secs(10);
const ACCOUNT_TIMEOUT: Duration = Duration::from_secs(30);
const LAUNCH_CONTEXT_TTL: Duration = Duration::from_secs(120);

type ToolReplyKey = (String, String);

#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub socket: PathBuf,
    pub database: PathBuf,
    pub codex: PathBuf,
    pub codex_home: PathBuf,
}

#[derive(Debug, Clone)]
pub struct StandardPaths {
    pub state_dir: PathBuf,
    pub socket: PathBuf,
    pub database: PathBuf,
    pub codex_home: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("HOME is unavailable or is not an absolute path")]
    MissingHome,
    #[error("no Codex executable was found; pass --codex or set NUCLEUS_CODEX")]
    CodexNotFound,
    #[error("Codex path is not an executable file: {0}")]
    InvalidCodex(PathBuf),
    #[error("unable to prepare {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("a Nucleus daemon is already listening on {0}")]
    AlreadyRunning(PathBuf),
    #[error("socket path exists and is not a Unix socket: {0}")]
    SocketPathOccupied(PathBuf),
}

#[derive(Clone)]
pub struct AppState {
    store: Arc<Mutex<Store>>,
    codex: CodexHarness,
    cancellations: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
    tool_replies: Arc<Mutex<HashMap<ToolReplyKey, oneshot::Sender<ToolResult>>>>,
    changes: broadcast::Sender<String>,
    mailbox_changes: broadcast::Sender<String>,
    launch_contexts: Arc<Mutex<HashMap<String, EphemeralLaunchContext>>>,
}

#[derive(Clone)]
struct EphemeralLaunchContext {
    requester: Requester,
    environment: BTreeMap<String, String>,
    expires: tokio::time::Instant,
    bound_job_id: Option<String>,
}

impl AppState {
    /// Build state, seed Nucleus-owned schemas, and reconcile interrupted work.
    ///
    /// # Errors
    ///
    /// Returns an error when schema seeding or durable recovery fails.
    pub async fn new(store: Store, codex: CodexHarness) -> Result<Self, DaemonError> {
        let (changes, _) = broadcast::channel(1_024);
        let (mailbox_changes, _) = broadcast::channel(256);
        let state = Self {
            store: Arc::new(Mutex::new(store)),
            codex,
            cancellations: Arc::new(Mutex::new(HashMap::new())),
            tool_replies: Arc::new(Mutex::new(HashMap::new())),
            changes,
            mailbox_changes,
            launch_contexts: Arc::new(Mutex::new(HashMap::new())),
        };
        state.seed_internal_schemas().await?;
        state.recover_interrupted_work().await?;
        Ok(state)
    }

    async fn seed_internal_schemas(&self) -> Result<(), DaemonError> {
        let created_at = now();
        let mut store = self.store.lock().await;
        for schema in INTERNAL_SCHEMAS {
            store.put_log_schema(NewLogSchema {
                id: schema.id.to_owned(),
                name: schema.name.to_owned(),
                version: "1".to_owned(),
                media_type: "application/schema+json".to_owned(),
                producer: "nucleus".to_owned(),
                producer_version: None,
                schema_bytes: schema.document.as_bytes().to_vec(),
                created_at: created_at.clone(),
            })?;
        }
        Ok(())
    }

    async fn recover_interrupted_work(&self) -> Result<(), DaemonError> {
        let recovered_at = now();
        let mut store = self.store.lock().await;
        let attempts = store.unfinished_attempts()?;
        for attempt in attempts {
            store.transition_attempt_with_message(
                &attempt.id,
                StoreAttemptState::Lost,
                &recovered_at,
                Some("lost"),
                Some("nucleusd restarted while the attempt was in progress"),
            )?;
            append_lifecycle_to_store(
                &mut store,
                &attempt.job_id,
                Some(&attempt.id),
                LifecycleEventKind::AttemptLost,
                Some("nucleusd restarted while the attempt was in progress"),
                None,
            )?;
            append_lifecycle_to_store(
                &mut store,
                &attempt.job_id,
                Some(&attempt.id),
                LifecycleEventKind::JobFailed,
                Some("nucleusd restarted while the attempt was in progress"),
                None,
            )?;
        }
        let jobs = store.list_jobs()?;
        for job in jobs
            .into_iter()
            .filter(|job| job.state == StoreJobState::Accepted && job.current_attempt_id.is_none())
        {
            store.finish_job(&job.id, StoreJobState::Failed, &recovered_at, Some("lost"))?;
            append_lifecycle_to_store(
                &mut store,
                &job.id,
                None,
                LifecycleEventKind::JobFailed,
                Some("nucleusd restarted before the attempt was created"),
                None,
            )?;
        }
        Ok(())
    }

    fn notify(&self, job_id: &str) {
        let _ = self.changes.send(job_id.to_owned());
    }

    fn notify_mailbox(&self, job_id: &str) {
        let _ = self.mailbox_changes.send(job_id.to_owned());
    }

    async fn shutdown_jobs(&self) {
        let senders = self
            .cancellations
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for sender in senders {
            let _ = sender.send(true);
        }
    }

    async fn wait_for_jobs(&self, timeout: Duration) {
        let deadline = tokio::time::Instant::now() + timeout;
        while !self.cancellations.lock().await.is_empty() && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// Resolve the standard per-user state paths.
///
/// # Errors
///
/// Returns an error when `HOME` is absent or is not absolute.
pub fn standard_paths() -> Result<StandardPaths, DaemonError> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or(DaemonError::MissingHome)?;
    let state_dir = home.join("Library/Application Support/Nucleus");
    Ok(StandardPaths {
        socket: state_dir.join("nucleus.sock"),
        database: state_dir.join("nucleus.db"),
        codex_home: state_dir.join("codex-home"),
        state_dir,
    })
}

/// Resolve and canonicalize the exact Codex executable.
///
/// # Errors
///
/// Returns an error when no executable is found or the selected path is invalid.
pub fn resolve_codex(explicit: Option<PathBuf>) -> Result<PathBuf, DaemonError> {
    let candidate = explicit
        .or_else(|| env::var_os("NUCLEUS_CODEX").map(PathBuf::from))
        .or_else(|| executable_on_path("codex"))
        .or_else(|| {
            ["/opt/homebrew/bin/codex", "/usr/local/bin/codex"]
                .into_iter()
                .map(PathBuf::from)
                .find(|path| path.is_file())
        })
        .ok_or(DaemonError::CodexNotFound)?;
    if !is_executable(&candidate) {
        return Err(DaemonError::InvalidCodex(candidate));
    }
    fs::canonicalize(&candidate).map_err(|source| DaemonError::Io {
        path: candidate,
        source,
    })
}

/// Open durable state and serve until SIGINT or SIGTERM.
///
/// # Errors
///
/// Returns an error when state, socket setup, recovery, or serving fails.
pub async fn serve(config: ServeConfig) -> Result<(), DaemonError> {
    prepare_parent(&config.database)?;
    prepare_parent(&config.socket)?;
    let store = Store::open(&config.database)?;
    secure_store_files(&config.database)?;
    let state = AppState::new(
        store,
        CodexHarness::with_codex_home(&config.codex, &config.codex_home),
    )
    .await?;
    let listener = bind_socket(&config.socket).await?;
    info!(socket = %config.socket.display(), database = %config.database.display(), "nucleusd ready");
    let shutdown_state = state.clone();
    let completion_state = state.clone();
    let result = axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown_state.shutdown_jobs().await;
        })
        .await;
    completion_state.wait_for_jobs(Duration::from_secs(4)).await;
    result.map_err(|source| DaemonError::Io {
        path: config.socket,
        source,
    })
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/account", get(account_snapshot))
        .route("/v1/launch-contexts", post(register_launch_context))
        .route("/v1/jobs", post(submit_job).get(list_jobs))
        .route("/v1/jobs/{job_id}", get(get_job))
        .route("/v1/jobs/{job_id}/cancel", post(cancel_job))
        .route("/v1/jobs/{job_id}/logs", get(get_logs))
        .route("/v1/jobs/{job_id}/tool-calls", get(get_pending_tool_calls))
        .route(
            "/v1/jobs/{job_id}/tool-calls/{call_id}/result",
            post(post_tool_result),
        )
        .route("/v1/schemas", post(register_schema))
        .route("/v1/schemas/{schema_id}", get(get_schema))
        .route("/v1/toolsets", post(register_toolset))
        .route("/v1/toolsets/{provider}/{name}/{version}", get(get_toolset))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Json<HealthResponseV1> {
    let checked_at = now();
    let auth = state.codex.authentication_readiness();
    let readiness = tokio::time::timeout(STARTUP_INSPECTION_TIMEOUT, async {
        let inspection = state.codex.inspect().await?;
        let schema = state.codex.generate_protocol_schema().await?;
        state.codex.validate_protocol_schema(&inspection, &schema)?;
        Ok::<_, CodexError>(inspection)
    })
    .await;
    let (harness, executable, detail) = match readiness {
        Ok(Ok(inspection)) => (
            Some(HarnessIdentity {
                harness: "codex".into(),
                harness_version: inspection.version,
                adapter_version: ADAPTER_VERSION.to_owned(),
            }),
            Some(AbsolutePath::new(inspection.executable)),
            None,
        ),
        Ok(Err(error)) => (None, None, Some(error.to_string())),
        Err(_) => (
            None,
            None,
            Some("Codex readiness inspection timed out".to_owned()),
        ),
    };
    let accepting_jobs = harness.is_some() && auth.configured && auth.authenticated;
    Json(HealthResponseV1 {
        version: PROTOCOL_VERSION_V1,
        status: if accepting_jobs { "ok" } else { "degraded" }.to_owned(),
        daemon_version: ADAPTER_VERSION.to_owned(),
        accepting_jobs,
        checked_at,
        supported_protocol_versions: vec![PROTOCOL_VERSION_V1],
        harness,
        harness_executable: executable,
        capabilities: vec![
            HarnessCapability::ExactModel,
            HarnessCapability::ReasoningEffort,
            HarnessCapability::WorkspaceNone,
            HarnessCapability::WorkspaceReadOnly,
            HarnessCapability::WorkspaceReadWrite,
            HarnessCapability::BuiltinLocalExecution,
            HarnessCapability::BuiltinWebSearch,
            HarnessCapability::DynamicClientTools,
            HarnessCapability::RawJsonlInput,
            HarnessCapability::RawJsonlOutput,
            HarnessCapability::DeveloperInstructions,
            HarnessCapability::ExplicitEmptyEnvironments,
            HarnessCapability::ExperimentalRawEvents,
            HarnessCapability::PersistentFileAuthentication,
        ],
        authentication: AuthenticationReadinessV1 {
            codex_home: AbsolutePath::new(
                state
                    .codex
                    .codex_home()
                    .unwrap_or_else(|| Path::new("/unconfigured")),
            ),
            configured: auth.configured,
            authenticated: auth.authenticated,
            detail: auth.detail,
        },
        detail,
    })
}

async fn register_launch_context(
    State(state): State<AppState>,
    payload: Result<Json<LaunchContextRegistrationV1>, JsonRejection>,
) -> Result<(StatusCode, Json<LaunchContextAcceptedV1>), ApiError> {
    let Json(registration) = payload.map_err(ApiError::invalid_json)?;
    registration.validate().map_err(ApiError::validation)?;
    let id = format!("launch_{}", Uuid::now_v7());
    let expires = tokio::time::Instant::now() + LAUNCH_CONTEXT_TTL;
    let expires_at = (OffsetDateTime::now_utc()
        + time::Duration::seconds(LAUNCH_CONTEXT_TTL.as_secs().cast_signed()))
    .format(&Rfc3339)
    .unwrap_or_else(|error| panic!("current timestamp must format: {error}"));
    let environment = registration
        .environment
        .into_iter()
        .map(|variable| (variable.name, variable.value))
        .collect();
    let mut contexts = state.launch_contexts.lock().await;
    let now = tokio::time::Instant::now();
    contexts.retain(|_, context| context.expires > now);
    contexts.insert(
        id.clone(),
        EphemeralLaunchContext {
            requester: registration.requester,
            environment,
            expires,
            bound_job_id: None,
        },
    );
    drop(contexts);
    schedule_launch_context_expiry(Arc::clone(&state.launch_contexts), id.clone(), expires);
    Ok((
        StatusCode::CREATED,
        Json(LaunchContextAcceptedV1 {
            version: PROTOCOL_VERSION_V1,
            id: LaunchContextId::new(id),
            expires_at,
        }),
    ))
}

fn schedule_launch_context_expiry(
    contexts: Arc<Mutex<HashMap<String, EphemeralLaunchContext>>>,
    id: String,
    expires: tokio::time::Instant,
) {
    tokio::spawn(async move {
        tokio::time::sleep_until(expires).await;
        let mut contexts = contexts.lock().await;
        if contexts
            .get(&id)
            .is_some_and(|context| context.expires == expires)
        {
            contexts.remove(&id);
        }
    });
}

async fn account_snapshot(
    State(state): State<AppState>,
    query: Result<Query<AccountSnapshotQueryV1>, QueryRejection>,
) -> Result<Json<AccountSnapshotV1>, ApiError> {
    let Query(query) = query.map_err(ApiError::invalid_query)?;
    query.validate().map_err(ApiError::validation)?;
    let inspection = tokio::time::timeout(ADMISSION_INSPECTION_TIMEOUT, state.codex.inspect())
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "harness_inspection_timed_out",
                "Codex capability inspection timed out",
            )
        })?
        .map_err(ApiError::harness_unavailable)?;
    let snapshot = state
        .codex
        .read_account_snapshot(
            query.include_usage,
            Duration::from_secs(u64::from(query.wait_seconds)),
            ACCOUNT_TIMEOUT,
        )
        .await
        .map_err(|error| {
            let code = if matches!(error, CodexError::AuthenticationBusy) {
                "authentication_busy"
            } else {
                "model_auth_unavailable"
            };
            ApiError::new(StatusCode::SERVICE_UNAVAILABLE, code, error.to_string())
        })?;
    Ok(Json(AccountSnapshotV1 {
        version: PROTOCOL_VERSION_V1,
        observed_at: now(),
        harness: HarnessIdentity {
            harness: "codex".into(),
            harness_version: inspection.version,
            adapter_version: ADAPTER_VERSION.to_owned(),
        },
        rate_limits: snapshot.rate_limits,
        usage: snapshot.usage,
        usage_error: snapshot.usage_error,
    }))
}

#[allow(clippy::too_many_lines)]
async fn submit_job(
    State(state): State<AppState>,
    payload: Result<Json<JobRequestV1>, JsonRejection>,
) -> Result<(StatusCode, Json<JobAcceptedV1>), ApiError> {
    let Json(request) = payload.map_err(ApiError::invalid_json)?;
    request.validate().map_err(ApiError::validation)?;
    if request.invocation.harness.as_str() != "codex" {
        return Err(ApiError::unprocessable(
            "unsupported_harness",
            "v1 has no adapter for the requested harness",
        ));
    }

    if let Some(existing) = exact_existing_job(&state, &request).await? {
        return accepted_response(&state, existing, StatusCode::OK).await;
    }

    if let Some(parent) = &request.parent
        && state
            .store
            .lock()
            .await
            .get_job(parent.as_str())
            .map_err(ApiError::store)?
            .is_none()
    {
        return Err(ApiError::not_found("parent job", parent.as_str()));
    }
    let launch_environment = match resolve_launch_environment(&state, &request).await? {
        LaunchEnvironmentResolution::Environment(environment) => environment,
        LaunchEnvironmentResolution::Existing(existing) => {
            return accepted_response(&state, *existing, StatusCode::OK).await;
        }
    };
    let definitions = load_toolset_definitions(&state, &request).await?;
    let dynamic_tools = dynamic_tools(&definitions)?;
    let inspection = tokio::time::timeout(ADMISSION_INSPECTION_TIMEOUT, state.codex.inspect())
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "harness_inspection_timed_out",
                "Codex capability inspection timed out",
            )
        })?
        .map_err(ApiError::harness_unavailable)?;
    let spec = codex_spec(&request, dynamic_tools.clone(), launch_environment.clone());
    state
        .codex
        .validate(&inspection, &spec)
        .map_err(ApiError::harness_compatibility)?;
    let generated_schema = tokio::time::timeout(
        ADMISSION_INSPECTION_TIMEOUT,
        state.codex.generate_protocol_schema(),
    )
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "schema_generation_timed_out",
            "Codex protocol schema generation timed out",
        )
    })?
    .map_err(ApiError::harness_unavailable)?;
    state
        .codex
        .validate_protocol_schema(&inspection, &generated_schema)
        .map_err(ApiError::harness_compatibility)?;
    let protocol_schema_id = register_codex_schema(&state, &inspection, generated_schema).await?;

    let created_at = now();
    let request_bytes = serde_json::to_vec(&request).map_err(ApiError::encoding)?;
    let admission = {
        let mut store = state.store.lock().await;
        store
            .admit_job(NewJob {
                id: request.id.to_string(),
                label: request.label.clone(),
                requester_program: request.requester.program.clone(),
                requester_id: request.requester.id.clone(),
                parent_job_id: request.parent.as_ref().map(ToString::to_string),
                request_schema_id: JOB_REQUEST_ID.to_owned(),
                request_bytes,
                created_at: created_at.clone(),
            })
            .map_err(ApiError::store)?
    };
    if !admission.was_created() {
        return accepted_response(&state, admission.into_inner(), StatusCode::OK).await;
    }
    let job = admission.into_inner();
    let attempt_id = format!("attempt_{}", Uuid::now_v7());
    let attempt = {
        let mut store = state.store.lock().await;
        append_lifecycle_to_store(
            &mut store,
            &job.id,
            None,
            LifecycleEventKind::JobAccepted,
            Some("job admitted after stable and harness-specific validation"),
            None,
        )?;
        let attempt = store
            .create_attempt(NewAttempt {
                id: attempt_id.clone(),
                job_id: job.id.clone(),
                ordinal: 1,
                harness: "codex".to_owned(),
                harness_version: inspection.version.clone(),
                adapter_version: ADAPTER_VERSION.to_owned(),
                created_at: now(),
            })
            .map_err(ApiError::store)?;
        append_lifecycle_to_store(
            &mut store,
            &job.id,
            Some(&attempt_id),
            LifecycleEventKind::AttemptCreated,
            None,
            None,
        )?;
        append_lifecycle_to_store(
            &mut store,
            &job.id,
            Some(&attempt_id),
            LifecycleEventKind::HarnessValidated,
            Some(&format!("Codex {} validated", inspection.version)),
            to_raw_value(&json!({
                "harness": "codex",
                "harnessVersion": inspection.version,
                "adapterVersion": ADAPTER_VERSION,
                "executable": inspection.executable,
                "protocolSchemaId": protocol_schema_id
            }))
            .ok(),
        )?;
        attempt
    };
    if let Some(context) = &request.invocation.launch_context {
        state.launch_contexts.lock().await.remove(context.as_str());
    }
    let cancel_rx = register_cancellation_watch(&state, &job.id).await?;
    let run_state = state.clone();
    let run_request = request.clone();
    let run_inspection = inspection.clone();
    let run_attempt_id = attempt_id.clone();
    let run_definitions = definitions.clone();
    let run_environment = launch_environment;
    tokio::spawn(async move {
        run_job(
            run_state,
            run_request,
            run_attempt_id,
            run_inspection,
            protocol_schema_id,
            run_definitions,
            run_environment,
            cancel_rx,
        )
        .await;
    });
    state.notify(&job.id);
    let cursor = last_log_sequence(&state, &job.id).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(JobAcceptedV1 {
            version: PROTOCOL_VERSION_V1,
            job_id: JobId::new(job.id),
            state: JobState::Accepted,
            request_digest: sha256_digest(&job.request_bytes),
            attempt: Some(attempt_to_core(attempt, None)),
            log_cursor: cursor,
        }),
    ))
}

async fn register_cancellation_watch(
    state: &AppState,
    job_id: &str,
) -> Result<watch::Receiver<bool>, ApiError> {
    let (sender, receiver) = watch::channel(false);
    state
        .cancellations
        .lock()
        .await
        .insert(job_id.to_owned(), sender.clone());

    // A cancellation can commit after attempt admission but before the sender
    // is visible. Once the sender is installed, reread the durable source of
    // truth. A later cancellation sees the sender; an earlier one is observed
    // here, so the run cannot start with a fresh false receiver.
    let cancellation_requested = state
        .store
        .lock()
        .await
        .get_job(job_id)
        .map_err(ApiError::store)?
        .ok_or_else(|| ApiError::not_found("job", job_id))?
        .cancellation_requested_at
        .is_some();
    if cancellation_requested {
        sender.send_replace(true);
    }
    Ok(receiver)
}

async fn get_job(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<JobV1>, ApiError> {
    let store = state.store.lock().await;
    let job = store
        .get_job(&job_id)
        .map_err(ApiError::store)?
        .ok_or_else(|| ApiError::not_found("job", &job_id))?;
    let attempts = store.list_attempts(&job_id).map_err(ApiError::store)?;
    let outputs = attempt_outputs(&store, &job_id)?;
    Ok(Json(job_to_core(job, attempts, &outputs)?))
}

async fn list_jobs(
    State(state): State<AppState>,
    query: Result<Query<ListJobsQueryV1>, QueryRejection>,
) -> Result<Json<ListJobsResponseV1>, ApiError> {
    let Query(query) = query.map_err(ApiError::invalid_query)?;
    query.validate().map_err(ApiError::validation)?;
    let store = state.store.lock().await;
    let records = if let (Some(program), Some(id)) = (&query.requester_program, &query.requester_id)
    {
        store
            .list_jobs_by_requester(program, id)
            .map_err(ApiError::store)?
    } else {
        store.list_jobs().map_err(ApiError::store)?
    };
    let mut records = records
        .into_iter()
        .filter(|job| {
            query
                .requester_program
                .as_ref()
                .is_none_or(|value| value == &job.requester_program)
                && query
                    .parent
                    .as_ref()
                    .is_none_or(|value| Some(value.as_str()) == job.parent_job_id.as_deref())
                && query
                    .state
                    .is_none_or(|value| store_job_state(value) == job.state)
        })
        .collect::<Vec<_>>();
    if let Some(after) = &query.after {
        let position = records
            .iter()
            .position(|job| job.id == after.as_str())
            .ok_or_else(|| ApiError::bad_request("invalid_cursor", "after job was not found"))?;
        records.drain(..=position);
    }
    let limit = usize::try_from(query.limit.unwrap_or(100))
        .unwrap_or(DEFAULT_PAGE)
        .min(MAX_PAGE);
    let has_more = records.len() > limit;
    records.truncate(limit);
    let next = has_more
        .then(|| records.last().map(|job| JobId::new(job.id.clone())))
        .flatten();
    Ok(Json(ListJobsResponseV1 {
        version: PROTOCOL_VERSION_V1,
        jobs: records.into_iter().map(job_summary).collect(),
        next,
    }))
}

async fn cancel_job(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<CancelJobResponseV1>, ApiError> {
    let requested_at = now();
    let (job, first_request) = {
        let mut store = state.store.lock().await;
        let before = store
            .get_job(&job_id)
            .map_err(ApiError::store)?
            .ok_or_else(|| ApiError::not_found("job", &job_id))?;
        if before.state.is_terminal() {
            (before, false)
        } else {
            let job = store
                .request_cancellation(&job_id, &requested_at)
                .map_err(ApiError::store)?;
            let first = before.cancellation_requested_at.is_none();
            if first {
                append_lifecycle_to_store(
                    &mut store,
                    &job_id,
                    job.current_attempt_id.as_deref(),
                    LifecycleEventKind::CancellationRequested,
                    None,
                    None,
                )?;
            }
            (job, first)
        }
    };
    if !job.state.is_terminal()
        && let Some(sender) = state.cancellations.lock().await.get(&job_id)
    {
        let _ = sender.send(true);
    }
    if first_request {
        state.notify(&job_id);
    }
    Ok(Json(CancelJobResponseV1 {
        version: PROTOCOL_VERSION_V1,
        job_id: JobId::new(job_id),
        state: core_job_state(job.state),
        cancellation_requested: !job.state.is_terminal(),
    }))
}

async fn get_logs(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    query: Result<Query<LogsQueryV1>, QueryRejection>,
) -> Result<Json<LogsResponseV1>, ApiError> {
    let Query(query) = query.map_err(ApiError::invalid_query)?;
    let limit = validated_limit(query.limit)?;
    let mut notifications = state.changes.subscribe();
    let mut records = load_logs(&state, &job_id, query.after, limit).await?;
    if records.is_empty() && query.follow && !job_terminal(&state, &job_id).await? {
        let _ = tokio::time::timeout(LOG_FOLLOW_WAIT, async {
            loop {
                match notifications.recv().await {
                    Ok(event_job) if event_job == job_id => break,
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
        .await;
        records = load_logs(&state, &job_id, query.after, limit).await?;
    }
    let next_sequence = records.last().map_or(query.after, |record| record.sequence);
    let records = records
        .into_iter()
        .map(log_to_core)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(LogsResponseV1 {
        version: PROTOCOL_VERSION_V1,
        job_id: JobId::new(job_id),
        records,
        next_sequence,
    }))
}

async fn register_schema(
    State(state): State<AppState>,
    payload: Result<Json<LogSchemaV1>, JsonRejection>,
) -> Result<Json<LogSchemaV1>, ApiError> {
    let Json(schema) = payload.map_err(ApiError::invalid_json)?;
    validate_schema_registration(&schema)?;
    let record = state
        .store
        .lock()
        .await
        .put_log_schema(NewLogSchema {
            id: schema.id.to_string(),
            name: schema.name,
            version: schema.schema_version,
            media_type: schema.media_type,
            producer: schema.producer,
            producer_version: schema.producer_version,
            schema_bytes: schema.schema.get().as_bytes().to_vec(),
            created_at: now(),
        })
        .map_err(ApiError::store)?
        .into_inner();
    Ok(Json(schema_to_core(record)?))
}

async fn get_schema(
    State(state): State<AppState>,
    AxumPath(schema_id): AxumPath<String>,
) -> Result<Json<LogSchemaV1>, ApiError> {
    let record = state
        .store
        .lock()
        .await
        .get_log_schema(&schema_id)
        .map_err(ApiError::store)?
        .ok_or_else(|| ApiError::not_found("schema", &schema_id))?;
    Ok(Json(schema_to_core(record)?))
}

async fn register_toolset(
    State(state): State<AppState>,
    payload: Result<Json<ToolsetRegistrationV1>, JsonRejection>,
) -> Result<Json<RegisteredToolsetV1>, ApiError> {
    let Json(registration) = payload.map_err(ApiError::invalid_json)?;
    registration.validate().map_err(ApiError::validation)?;
    if registration.definitions_schema_id.as_str() != TOOLSET_DEFINITIONS_ID {
        return Err(ApiError::unprocessable(
            "unsupported_definitions_schema",
            "v1 toolsets must use nucleus.toolset-definitions.v1",
        ));
    }
    for tool in &registration.definitions.tools {
        let schema: Value = serde_json::from_str(tool.input_schema.get()).map_err(|error| {
            ApiError::bad_request(
                "invalid_tool_schema",
                &format!("tool {} inputSchema is invalid: {error}", tool.name),
            )
        })?;
        if !schema.is_object() {
            return Err(ApiError::bad_request(
                "invalid_tool_schema",
                &format!("tool {} inputSchema must be an object", tool.name),
            ));
        }
    }
    let definitions_bytes =
        serde_json::to_vec(&registration.definitions).map_err(ApiError::encoding)?;
    {
        let mut store = state.store.lock().await;
        for tool in &registration.definitions.tools {
            match store
                .get_log_schema(tool.input_schema_id.as_str())
                .map_err(ApiError::store)?
            {
                Some(existing) if existing.schema_bytes != tool.input_schema.get().as_bytes() => {
                    return Err(ApiError::conflict(
                        "schema_conflict",
                        &format!(
                            "input schema {} is already registered differently",
                            tool.input_schema_id
                        ),
                    ));
                }
                Some(_) => {}
                None => {
                    store
                        .put_log_schema(NewLogSchema {
                            id: tool.input_schema_id.to_string(),
                            name: tool.input_schema_id.to_string(),
                            version: "requester-defined".to_owned(),
                            media_type: "application/schema+json".to_owned(),
                            producer: registration.toolset.provider.clone(),
                            producer_version: None,
                            schema_bytes: tool.input_schema.get().as_bytes().to_vec(),
                            created_at: now(),
                        })
                        .map_err(ApiError::store)?;
                }
            }
        }
        let record = store
            .register_toolset(NewToolset {
                provider: registration.toolset.provider,
                name: registration.toolset.name,
                version: registration.toolset.version,
                definitions_schema_id: registration.definitions_schema_id.to_string(),
                definitions_bytes,
                created_at: now(),
            })
            .map_err(ApiError::store)?
            .into_inner();
        Ok(Json(toolset_to_core(record)))
    }
}

async fn get_toolset(
    State(state): State<AppState>,
    AxumPath((provider, name, version)): AxumPath<(String, String, u32)>,
) -> Result<Json<RegisteredToolsetV1>, ApiError> {
    let record = state
        .store
        .lock()
        .await
        .get_toolset(&provider, &name, version)
        .map_err(ApiError::store)?
        .ok_or_else(|| ApiError::not_found("toolset", &format!("{provider}/{name}@{version}")))?;
    Ok(Json(toolset_to_core(record)))
}

async fn get_pending_tool_calls(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    query: Result<Query<ToolCallsQueryV1>, QueryRejection>,
) -> Result<Json<ToolCallsResponseV1>, ApiError> {
    let Query(query) = query.map_err(ApiError::invalid_query)?;
    if query.wait_seconds > 60 {
        return Err(ApiError::bad_request(
            "invalid_wait",
            "waitSeconds must be at most 60",
        ));
    }
    let mut notifications = state.mailbox_changes.subscribe();
    let mut calls = load_pending_calls(&state, &job_id, query.after).await?;
    if calls.is_empty() && query.wait_seconds > 0 && !job_terminal(&state, &job_id).await? {
        let _ = tokio::time::timeout(Duration::from_secs(u64::from(query.wait_seconds)), async {
            loop {
                match notifications.recv().await {
                    Ok(event_job) if event_job == job_id => break,
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
        .await;
        calls = load_pending_calls(&state, &job_id, query.after).await?;
    }
    let next_sequence = calls
        .last()
        .map_or(query.after, |call| call.request_sequence);
    let calls = calls
        .into_iter()
        .map(pending_call_to_core)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ToolCallsResponseV1 {
        version: PROTOCOL_VERSION_V1,
        job_id: JobId::new(job_id),
        calls,
        next_sequence,
    }))
}

#[allow(clippy::too_many_lines)]
async fn post_tool_result(
    State(state): State<AppState>,
    AxumPath((job_id, call_id)): AxumPath<(String, String)>,
    payload: Result<Json<ToolResultV1>, JsonRejection>,
) -> Result<Json<PendingToolCallV1>, ApiError> {
    let Json(result) = payload.map_err(ApiError::invalid_json)?;
    if result.version != PROTOCOL_VERSION_V1 || result.call_id.as_str() != call_id {
        return Err(ApiError::bad_request(
            "result_mismatch",
            "result version or callId does not match the route",
        ));
    }
    let (job, current) = {
        let store = state.store.lock().await;
        let job = store
            .get_job(&job_id)
            .map_err(ApiError::store)?
            .ok_or_else(|| ApiError::not_found("job", &job_id))?;
        let call = store
            .get_tool_call(&job_id, &call_id)
            .map_err(ApiError::store)?
            .ok_or_else(|| ApiError::not_found("tool call", &call_id))?;
        if store
            .get_log_schema(result.result_schema_id.as_str())
            .map_err(ApiError::store)?
            .is_none()
        {
            return Err(ApiError::not_found(
                "schema",
                result.result_schema_id.as_str(),
            ));
        }
        (job, call)
    };
    if job.requester_program != result.requester.program || job.requester_id != result.requester.id
    {
        return Err(ApiError::forbidden(
            "requester_mismatch",
            "tool results may only be posted by the job requester",
        ));
    }
    let (answered, was_created) = {
        let mut store = state.store.lock().await;
        let answered = store
            .answer_tool_call_with_log(
                &job_id,
                &call_id,
                NewToolResult {
                    schema_id: result.result_schema_id.to_string(),
                    result_bytes: result.result.get().as_bytes().to_vec(),
                    is_error: result.is_error,
                    answered_at: now(),
                },
                NewLogRecord {
                    job_id: job_id.clone(),
                    attempt_id: Some(current.attempt_id.clone()),
                    observed_at: now(),
                    emitted_at: None,
                    stream: log_stream_name(LogStream::Requester).to_owned(),
                    schema_id: result.result_schema_id.to_string(),
                    payload: result.result.get().as_bytes().to_vec(),
                },
            )
            .map_err(ApiError::store)?;
        let was_created = answered.was_created();
        let answered = answered.into_inner();
        if was_created {
            if let Some(attempt) = store
                .get_attempt(&answered.attempt_id)
                .map_err(ApiError::store)?
                && attempt.state == StoreAttemptState::WaitingOnRequester
            {
                store
                    .transition_attempt(
                        &answered.attempt_id,
                        StoreAttemptState::Running,
                        &now(),
                        None,
                    )
                    .map_err(ApiError::store)?;
            }
            append_lifecycle_to_store(
                &mut store,
                &job_id,
                Some(&answered.attempt_id),
                LifecycleEventKind::ToolCallAnswered,
                None,
                None,
            )?;
        }
        (answered, was_created)
    };
    if was_created {
        // The durable ToolCallAnswered event is committed before waking the
        // harness, so a fast completion cannot overtake its lifecycle record.
        let reply = state
            .tool_replies
            .lock()
            .await
            .remove(&(job_id.clone(), call_id.clone()));
        if let Some(reply) = reply {
            let _ = reply.send(ToolResult {
                success: !result.is_error,
                content: result.result.get().to_owned(),
            });
        } else {
            warn!(
                job_id,
                call_id, "accepted tool result without a live harness receiver"
            );
        }
    }
    state.notify(&job_id);
    Ok(Json(pending_call_to_core(answered)?))
}

#[allow(clippy::too_many_arguments)]
async fn run_job(
    state: AppState,
    request: JobRequestV1,
    attempt_id: String,
    inspection: HarnessInspection,
    protocol_schema_id: String,
    definitions: ToolsetDefinitionsV1,
    launch_environment: Option<BTreeMap<String, String>>,
    cancel_rx: watch::Receiver<bool>,
) {
    let job_id = request.id.to_string();
    let starting = now();
    if let Err(failure) = begin_attempt(&state, &job_id, &attempt_id, &starting).await {
        error!(job_id, attempt_id, error = %failure.message, "could not start attempt");
        finalize_internal_failure(&state, &job_id, &attempt_id, failure.message).await;
        return;
    }

    let tools = match dynamic_tools(&definitions) {
        Ok(tools) => tools,
        Err(error) => {
            finalize_internal_failure(&state, &job_id, &attempt_id, error.message).await;
            return;
        }
    };
    let spec = codex_spec(&request, tools, launch_environment);
    let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(256);
    let run = state.codex.run(&inspection, spec, events_tx, cancel_rx);
    tokio::pin!(run);
    let mut pending_tool_records = HashMap::new();
    let mut event_failure: Option<String> = None;
    let run_result = loop {
        tokio::select! {
            result = &mut run => break result,
            event = events_rx.recv() => {
                let Some(event) = event else {
                    // The producer drops its final sender immediately before the run
                    // future resolves. If channel closure wins this select race, poll
                    // the run to its actual result instead of reporting a false
                    // consumer-disconnected protocol failure.
                    break (&mut run).await;
                };
                if event_failure.is_some() {
                    continue;
                }
                if let Err(error) = handle_codex_event(
                    &state,
                    &job_id,
                    &attempt_id,
                    &protocol_schema_id,
                    &definitions,
                    event,
                    &mut pending_tool_records,
                ).await {
                    event_failure = Some(error.message);
                    if let Some(sender) = state.cancellations.lock().await.get(&job_id) {
                        let _ = sender.send(true);
                    }
                }
            }
        }
    };
    while let Some(event) = events_rx.recv().await {
        if event_failure.is_none()
            && let Err(error) = handle_codex_event(
                &state,
                &job_id,
                &attempt_id,
                &protocol_schema_id,
                &definitions,
                event,
                &mut pending_tool_records,
            )
            .await
        {
            event_failure = Some(error.message);
        }
    }
    if event_failure.is_none() && !pending_tool_records.is_empty() {
        event_failure = Some(
            "a raw Codex tool request was not projected into the durable requester mailbox"
                .to_owned(),
        );
    }

    let terminal = if let Some(message) = event_failure {
        TerminalOutcome::failed(
            StoreAttemptState::Failed,
            AttemptTerminalReason::ProtocolError,
            LifecycleEventKind::AttemptFailed,
            message,
        )
    } else {
        terminal_outcome(run_result)
    };
    if let Err(error) = finish_attempt(&state, &job_id, &attempt_id, &terminal).await {
        error!(job_id, attempt_id, error = %error.message, "could not persist terminal attempt state");
    }
    state.cancellations.lock().await.remove(&job_id);
    state
        .tool_replies
        .lock()
        .await
        .retain(|(reply_job, _), _| reply_job != &job_id);
    state.notify(&job_id);
    state.notify_mailbox(&job_id);
}

async fn begin_attempt(
    state: &AppState,
    job_id: &str,
    attempt_id: &str,
    started_at: &str,
) -> Result<(), ApiError> {
    let mut store = state.store.lock().await;
    store
        .transition_attempt(attempt_id, StoreAttemptState::Starting, started_at, None)
        .map_err(ApiError::store)?;
    append_lifecycle_to_store(
        &mut store,
        job_id,
        Some(attempt_id),
        LifecycleEventKind::JobStarted,
        None,
        None,
    )?;
    store
        .transition_attempt(attempt_id, StoreAttemptState::Running, &now(), None)
        .map_err(ApiError::store)?;
    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn handle_codex_event(
    state: &AppState,
    job_id: &str,
    attempt_id: &str,
    protocol_schema_id: &str,
    definitions: &ToolsetDefinitionsV1,
    event: CodexEvent,
    pending_tool_records: &mut HashMap<String, NewLogRecord>,
) -> Result<(), ApiError> {
    match event {
        CodexEvent::Protocol { direction, bytes } => {
            let stream = match direction {
                ProtocolDirection::ToHarness => LogStream::HarnessInput,
                ProtocolDirection::FromHarness => LogStream::HarnessOutput,
            };
            let payload = trim_jsonl(bytes);
            let decoded = serde_json::from_slice::<Value>(&payload);
            let (schema_id, payload, decode_failed) = match &decoded {
                Ok(_) => (protocol_schema_id.to_owned(), payload, false),
                Err(_) => (BYTES_ID.to_owned(), byte_envelope(&payload), true),
            };
            let raw_record = NewLogRecord {
                job_id: job_id.to_owned(),
                attempt_id: Some(attempt_id.to_owned()),
                observed_at: now(),
                emitted_at: None,
                stream: log_stream_name(stream).to_owned(),
                schema_id,
                payload,
            };
            if direction == ProtocolDirection::FromHarness
                && let Ok(message) = decoded
                && message.get("method").and_then(Value::as_str) == Some("item/tool/call")
                && let Some(call_id) = message.pointer("/params/callId").and_then(Value::as_str)
            {
                if pending_tool_records
                    .insert(call_id.to_owned(), raw_record)
                    .is_some()
                {
                    return Err(ApiError::conflict(
                        "tool_call_conflict",
                        "Codex reused a tool call ID before mailbox projection",
                    ));
                }
                return Ok(());
            }
            {
                let mut store = state.store.lock().await;
                store.append_log(raw_record).map_err(ApiError::store)?;
                if decode_failed {
                    append_lifecycle_to_store(
                        &mut store,
                        job_id,
                        Some(attempt_id),
                        LifecycleEventKind::RecordDecodeFailed,
                        Some(
                            "Codex emitted a non-JSON protocol line; exact bytes are base64 encoded",
                        ),
                        None,
                    )?;
                }
            }
            state.notify(job_id);
        }
        CodexEvent::Stderr(bytes) => {
            state
                .store
                .lock()
                .await
                .append_log(NewLogRecord {
                    job_id: job_id.to_owned(),
                    attempt_id: Some(attempt_id.to_owned()),
                    observed_at: now(),
                    emitted_at: None,
                    stream: log_stream_name(LogStream::HarnessStderr).to_owned(),
                    schema_id: BYTES_ID.to_owned(),
                    payload: byte_envelope(&bytes),
                })
                .map_err(ApiError::store)?;
            state.notify(job_id);
        }
        CodexEvent::ToolCall(call) => {
            let definition = definitions
                .tools
                .iter()
                .find(|definition| definition.name == call.name)
                .ok_or_else(|| {
                    ApiError::internal(
                        "undeclared_tool",
                        "Codex requested a tool missing from the admitted toolset",
                    )
                })?;
            let raw_record = pending_tool_records.remove(&call.call_id).ok_or_else(|| {
                ApiError::internal(
                    "missing_tool_record",
                    "tool call arrived without its raw harness-output record",
                )
            })?;
            let key = (job_id.to_owned(), call.call_id.clone());
            state
                .tool_replies
                .lock()
                .await
                .insert(key.clone(), call.reply);
            let created = {
                let mut store = state.store.lock().await;
                let created = store.record_pending_tool_call(
                    NewPendingToolCall {
                        id: call.call_id.clone(),
                        job_id: job_id.to_owned(),
                        attempt_id: attempt_id.to_owned(),
                        tool_name: call.name,
                        arguments_schema_id: definition.input_schema_id.to_string(),
                        arguments_bytes: serde_json::to_vec(&call.arguments)
                            .map_err(ApiError::encoding)?,
                        created_at: now(),
                    },
                    raw_record,
                );
                let created = match created {
                    Ok(value) => value,
                    Err(error) => {
                        state.tool_replies.lock().await.remove(&key);
                        return Err(ApiError::store(error));
                    }
                };
                store
                    .transition_attempt(
                        attempt_id,
                        StoreAttemptState::WaitingOnRequester,
                        &now(),
                        None,
                    )
                    .map_err(ApiError::store)?;
                append_lifecycle_to_store(
                    &mut store,
                    job_id,
                    Some(attempt_id),
                    LifecycleEventKind::WaitingOnRequester,
                    Some(&format!("waiting for tool call {}", call.call_id)),
                    None,
                )?;
                append_lifecycle_to_store(
                    &mut store,
                    job_id,
                    Some(attempt_id),
                    LifecycleEventKind::ToolCallPending,
                    Some(&format!("{}:{}", definition.name, call.call_id)),
                    None,
                )?;
                created
            };
            if !created.was_created() {
                state.tool_replies.lock().await.remove(&key);
                return Err(ApiError::conflict(
                    "tool_call_conflict",
                    "Codex reused a tool call ID",
                ));
            }
            state.notify(job_id);
            state.notify_mailbox(job_id);
        }
    }
    Ok(())
}

struct TerminalOutcome {
    attempt_state: StoreAttemptState,
    reason: AttemptTerminalReason,
    attempt_event: LifecycleEventKind,
    job_event: LifecycleEventKind,
    message: String,
    details: Option<Box<RawValue>>,
}

impl TerminalOutcome {
    fn failed(
        attempt_state: StoreAttemptState,
        reason: AttemptTerminalReason,
        attempt_event: LifecycleEventKind,
        message: String,
    ) -> Self {
        let job_event = match attempt_state {
            StoreAttemptState::Cancelled => LifecycleEventKind::JobCancelled,
            _ => LifecycleEventKind::JobFailed,
        };
        Self {
            attempt_state,
            reason,
            attempt_event,
            job_event,
            message,
            details: None,
        }
    }
}

fn terminal_outcome(result: Result<nucleus_codex::CodexOutcome, CodexError>) -> TerminalOutcome {
    match result {
        Ok(outcome) => TerminalOutcome {
            attempt_state: StoreAttemptState::Completed,
            reason: AttemptTerminalReason::Completed,
            attempt_event: LifecycleEventKind::AttemptCompleted,
            job_event: LifecycleEventKind::JobCompleted,
            message: "Codex turn completed".to_owned(),
            details: to_raw_value(&json!({
                "threadId": outcome.thread_id,
                "turnId": outcome.turn_id,
                "finalMessage": outcome.final_message
            }))
            .ok(),
        },
        Err(CodexError::Cancelled) => TerminalOutcome::failed(
            StoreAttemptState::Cancelled,
            AttemptTerminalReason::Cancelled,
            LifecycleEventKind::AttemptCancelled,
            "job cancellation reached the Codex process".to_owned(),
        ),
        Err(CodexError::TimedOut) => TerminalOutcome::failed(
            StoreAttemptState::TimedOut,
            AttemptTerminalReason::TimedOut,
            LifecycleEventKind::AttemptTimedOut,
            "job exceeded its wall-clock timeout".to_owned(),
        ),
        Err(error @ (CodexError::Protocol(_) | CodexError::EventConsumerDisconnected)) => {
            TerminalOutcome::failed(
                StoreAttemptState::Failed,
                AttemptTerminalReason::ProtocolError,
                LifecycleEventKind::AttemptFailed,
                error.to_string(),
            )
        }
        Err(error) => TerminalOutcome::failed(
            StoreAttemptState::Failed,
            AttemptTerminalReason::HarnessFailure,
            LifecycleEventKind::AttemptFailed,
            error.to_string(),
        ),
    }
}

async fn finish_attempt(
    state: &AppState,
    job_id: &str,
    attempt_id: &str,
    outcome: &TerminalOutcome,
) -> Result<(), ApiError> {
    let finished_at = now();
    let reason = terminal_reason_name(outcome.reason);
    let mut store = state.store.lock().await;
    store
        .transition_attempt_with_message(
            attempt_id,
            outcome.attempt_state,
            &finished_at,
            Some(reason),
            Some(&outcome.message),
        )
        .map_err(ApiError::store)?;
    if outcome.attempt_state == StoreAttemptState::Completed {
        append_lifecycle_to_store(
            &mut store,
            job_id,
            Some(attempt_id),
            LifecycleEventKind::TurnCompleted,
            Some(&outcome.message),
            outcome.details.clone(),
        )?;
    }
    append_lifecycle_to_store(
        &mut store,
        job_id,
        Some(attempt_id),
        outcome.attempt_event,
        Some(&outcome.message),
        None,
    )?;
    append_lifecycle_to_store(
        &mut store,
        job_id,
        Some(attempt_id),
        outcome.job_event,
        Some(&outcome.message),
        None,
    )?;
    Ok(())
}

async fn finalize_internal_failure(
    state: &AppState,
    job_id: &str,
    attempt_id: &str,
    message: String,
) {
    let terminal = TerminalOutcome::failed(
        StoreAttemptState::Failed,
        AttemptTerminalReason::ProtocolError,
        LifecycleEventKind::AttemptFailed,
        message,
    );
    let _ = finish_attempt(state, job_id, attempt_id, &terminal).await;
    state.cancellations.lock().await.remove(job_id);
    state.notify(job_id);
    state.notify_mailbox(job_id);
}

async fn load_toolset_definitions(
    state: &AppState,
    request: &JobRequestV1,
) -> Result<ToolsetDefinitionsV1, ApiError> {
    let Some(reference) = &request.invocation.toolset else {
        return Ok(ToolsetDefinitionsV1 {
            version: PROTOCOL_VERSION_V1,
            tools: Vec::new(),
        });
    };
    if reference.provider != request.requester.program {
        return Err(ApiError::forbidden(
            "toolset_provider_mismatch",
            "a job may only use a toolset owned by its requester program",
        ));
    }
    let record = state
        .store
        .lock()
        .await
        .get_toolset(&reference.provider, &reference.name, reference.version)
        .map_err(ApiError::store)?
        .ok_or_else(|| {
            ApiError::not_found(
                "toolset",
                &format!(
                    "{}/{}@{}",
                    reference.provider, reference.name, reference.version
                ),
            )
        })?;
    serde_json::from_slice(&record.definitions_bytes).map_err(|error| {
        ApiError::internal(
            "stored_toolset_invalid",
            &format!("stored toolset could not be decoded: {error}"),
        )
    })
}

async fn exact_existing_job(
    state: &AppState,
    request: &JobRequestV1,
) -> Result<Option<JobRecord>, ApiError> {
    let existing = state
        .store
        .lock()
        .await
        .get_job(request.id.as_str())
        .map_err(ApiError::store)?;
    let Some(existing) = existing else {
        return Ok(None);
    };
    let requested = serde_json::to_vec(request).map_err(ApiError::encoding)?;
    if existing.request_bytes != requested {
        return Err(ApiError::conflict(
            "job_conflict",
            "the job ID is already bound to a different request",
        ));
    }
    Ok(Some(existing))
}

enum LaunchEnvironmentResolution {
    Environment(Option<BTreeMap<String, String>>),
    Existing(Box<JobRecord>),
}

async fn resolve_launch_environment(
    state: &AppState,
    request: &JobRequestV1,
) -> Result<LaunchEnvironmentResolution, ApiError> {
    match load_launch_environment(state, request).await {
        Ok(environment) => Ok(LaunchEnvironmentResolution::Environment(environment)),
        Err(error) if error.status == StatusCode::NOT_FOUND => {
            // Another identical submission may have admitted the job and
            // consumed its launch context after our initial idempotency check.
            // Recheck the durable job before surfacing a stale context miss.
            if let Some(existing) = exact_existing_job(state, request).await? {
                Ok(LaunchEnvironmentResolution::Existing(Box::new(existing)))
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

async fn load_launch_environment(
    state: &AppState,
    request: &JobRequestV1,
) -> Result<Option<BTreeMap<String, String>>, ApiError> {
    let Some(reference) = &request.invocation.launch_context else {
        return Ok(None);
    };
    if !request.invocation.builtin_tools.local_execution {
        return Err(ApiError::unprocessable(
            "launch_context_without_local_execution",
            "a launch context is only valid when local execution is enabled",
        ));
    }
    let mut contexts = state.launch_contexts.lock().await;
    let now = tokio::time::Instant::now();
    contexts.retain(|_, context| context.expires > now);
    let context = contexts
        .get_mut(reference.as_str())
        .ok_or_else(|| ApiError::not_found("launch context", reference.as_str()))?;
    if context.requester != request.requester {
        return Err(ApiError::forbidden(
            "launch_context_requester_mismatch",
            "the launch context belongs to a different requester",
        ));
    }
    match context.bound_job_id.as_deref() {
        None => context.bound_job_id = Some(request.id.to_string()),
        Some(job_id) if job_id == request.id.as_str() => {}
        Some(_) => {
            return Err(ApiError::conflict(
                "launch_context_in_use",
                "the launch context is already bound to a different job",
            ));
        }
    }
    Ok(Some(context.environment.clone()))
}

fn dynamic_tools(definitions: &ToolsetDefinitionsV1) -> Result<Vec<DynamicTool>, ApiError> {
    definitions
        .tools
        .iter()
        .map(|definition| {
            let input_schema =
                serde_json::from_str(definition.input_schema.get()).map_err(|error| {
                    ApiError::bad_request(
                        "invalid_tool_schema",
                        &format!("tool {} has invalid input schema: {error}", definition.name),
                    )
                })?;
            Ok(DynamicTool {
                name: definition.name.clone(),
                description: definition.description.clone(),
                input_schema,
            })
        })
        .collect()
}

fn codex_spec(
    request: &JobRequestV1,
    tools: Vec<DynamicTool>,
    launch_environment: Option<BTreeMap<String, String>>,
) -> CodexRunSpec {
    CodexRunSpec {
        instructions: request.instructions.clone(),
        developer_instructions: request.developer_instructions.clone(),
        prompt: request.prompt.clone(),
        model: request.invocation.model.to_string(),
        reasoning_effort: request
            .invocation
            .reasoning_effort
            .map(reasoning_effort_name)
            .map(str::to_owned),
        working_directory: request.invocation.cwd.as_path().to_path_buf(),
        workspace_access: request.invocation.workspace_access,
        builtin_tools: request.invocation.builtin_tools,
        timeout: Duration::from_secs(request.invocation.timeout_seconds.get()),
        tools,
        launch_environment,
    }
}

async fn register_codex_schema(
    state: &AppState,
    inspection: &HarnessInspection,
    generated: GeneratedSchema,
) -> Result<String, ApiError> {
    let id = format!(
        "codex.app-server.protocol.{}",
        sanitize_identifier(&inspection.version)
    );
    state
        .store
        .lock()
        .await
        .put_log_schema(NewLogSchema {
            id: id.clone(),
            name: "Codex app-server protocol".to_owned(),
            version: inspection.version.clone(),
            media_type: generated.media_type.to_owned(),
            producer: "codex".to_owned(),
            producer_version: Some(inspection.version.clone()),
            schema_bytes: generated.bytes,
            created_at: now(),
        })
        .map_err(ApiError::store)?;
    Ok(id)
}

async fn accepted_response(
    state: &AppState,
    job: JobRecord,
    status: StatusCode,
) -> Result<(StatusCode, Json<JobAcceptedV1>), ApiError> {
    let attempt = if let Some(attempt_id) = &job.current_attempt_id {
        state
            .store
            .lock()
            .await
            .get_attempt(attempt_id)
            .map_err(ApiError::store)?
            .map(|attempt| attempt_to_core(attempt, None))
    } else {
        None
    };
    let cursor = last_log_sequence(state, &job.id).await?;
    Ok((
        status,
        Json(JobAcceptedV1 {
            version: PROTOCOL_VERSION_V1,
            job_id: JobId::new(job.id),
            state: core_job_state(job.state),
            request_digest: sha256_digest(&job.request_bytes),
            attempt,
            log_cursor: cursor,
        }),
    ))
}

async fn last_log_sequence(state: &AppState, job_id: &str) -> Result<u64, ApiError> {
    let store = state.store.lock().await;
    let mut cursor = 0;
    loop {
        let page = store
            .list_logs(job_id, cursor, MAX_PAGE)
            .map_err(ApiError::store)?;
        let Some(last) = page.last() else {
            return Ok(cursor);
        };
        cursor = last.sequence;
        if page.len() < MAX_PAGE {
            return Ok(cursor);
        }
    }
}

async fn load_logs(
    state: &AppState,
    job_id: &str,
    after: u64,
    limit: usize,
) -> Result<Vec<LogRecord>, ApiError> {
    let store = state.store.lock().await;
    if store.get_job(job_id).map_err(ApiError::store)?.is_none() {
        return Err(ApiError::not_found("job", job_id));
    }
    store
        .list_logs(job_id, after, limit)
        .map_err(ApiError::store)
}

async fn load_pending_calls(
    state: &AppState,
    job_id: &str,
    after: u64,
) -> Result<Vec<PendingToolCallRecord>, ApiError> {
    let store = state.store.lock().await;
    if store.get_job(job_id).map_err(ApiError::store)?.is_none() {
        return Err(ApiError::not_found("job", job_id));
    }
    store
        .list_pending_tool_calls(job_id, after, MAX_PAGE)
        .map_err(ApiError::store)
}

async fn job_terminal(state: &AppState, job_id: &str) -> Result<bool, ApiError> {
    state
        .store
        .lock()
        .await
        .get_job(job_id)
        .map_err(ApiError::store)?
        .map(|job| job.state.is_terminal())
        .ok_or_else(|| ApiError::not_found("job", job_id))
}

fn job_to_core(
    job: JobRecord,
    attempts: Vec<AttemptRecord>,
    outputs: &HashMap<String, AttemptOutputV1>,
) -> Result<JobV1, ApiError> {
    let request = serde_json::from_slice(&job.request_bytes).map_err(|error| {
        ApiError::internal(
            "stored_request_invalid",
            &format!("stored job request could not be decoded: {error}"),
        )
    })?;
    Ok(JobV1 {
        version: PROTOCOL_VERSION_V1,
        summary: job_summary(job),
        request,
        attempts: attempts
            .into_iter()
            .map(|attempt| {
                let output = outputs.get(&attempt.id).cloned();
                attempt_to_core(attempt, output)
            })
            .collect(),
    })
}

fn job_summary(job: JobRecord) -> JobSummaryV1 {
    JobSummaryV1 {
        version: PROTOCOL_VERSION_V1,
        id: JobId::new(job.id),
        label: job.label,
        requester: Requester {
            program: job.requester_program,
            id: job.requester_id,
        },
        parent: job.parent_job_id.map(JobId::new),
        state: core_job_state(job.state),
        request_digest: sha256_digest(&job.request_bytes),
        created_at: job.created_at,
        updated_at: job.updated_at,
        completed_at: job.completed_at,
        current_attempt_id: job.current_attempt_id.map(AttemptId::new),
    }
}

fn attempt_to_core(attempt: AttemptRecord, output: Option<AttemptOutputV1>) -> AttemptV1 {
    AttemptV1 {
        version: PROTOCOL_VERSION_V1,
        id: AttemptId::new(attempt.id),
        job_id: JobId::new(attempt.job_id),
        ordinal: attempt.ordinal,
        harness: nucleus_core::HarnessIdentity {
            harness: attempt.harness.into(),
            harness_version: attempt.harness_version,
            adapter_version: attempt.adapter_version,
        },
        state: core_attempt_state(attempt.state),
        created_at: attempt.created_at,
        started_at: attempt.started_at,
        completed_at: attempt.completed_at,
        terminal_reason: attempt
            .terminal_reason
            .as_deref()
            .and_then(parse_terminal_reason),
        terminal_message: attempt.terminal_message,
        output,
    }
}

fn attempt_outputs(
    store: &Store,
    job_id: &str,
) -> Result<HashMap<String, AttemptOutputV1>, ApiError> {
    let mut outputs = HashMap::new();
    let mut cursor = 0_u64;
    loop {
        let records = store
            .list_logs(job_id, cursor, MAX_PAGE)
            .map_err(ApiError::store)?;
        let count = records.len();
        for record in records {
            cursor = record.sequence;
            if record.schema_id != LIFECYCLE_ID {
                continue;
            }
            let Ok(event) = serde_json::from_slice::<LifecycleEventV1>(&record.payload) else {
                continue;
            };
            if event.event != LifecycleEventKind::TurnCompleted {
                continue;
            }
            let (Some(attempt), Some(details)) = (event.attempt_id, event.details) else {
                continue;
            };
            if let Ok(output) = serde_json::from_str::<AttemptOutputV1>(details.get()) {
                outputs.insert(attempt.to_string(), output);
            }
        }
        if count < MAX_PAGE {
            break;
        }
    }
    Ok(outputs)
}

fn log_to_core(record: LogRecord) -> Result<LogRecordV1, ApiError> {
    let payload = raw_from_bytes(record.payload, "stored log payload")?;
    Ok(LogRecordV1 {
        version: PROTOCOL_VERSION_V1,
        job_id: JobId::new(record.job_id),
        attempt_id: record.attempt_id.map(AttemptId::new),
        sequence: record.sequence,
        observed_at: record.observed_at,
        stream: parse_log_stream(&record.stream)?,
        schema_id: SchemaId::new(record.schema_id),
        payload,
        payload_digest: digest_text(record.payload_digest),
    })
}

fn schema_to_core(record: LogSchemaRecord) -> Result<LogSchemaV1, ApiError> {
    let schema = raw_from_bytes(record.schema_bytes, "stored schema")?;
    Ok(LogSchemaV1 {
        version: PROTOCOL_VERSION_V1,
        id: SchemaId::new(record.id),
        name: record.name,
        schema_version: record.version,
        media_type: record.media_type,
        producer: record.producer,
        producer_version: record.producer_version,
        schema,
        digest: digest_text(record.schema_digest),
    })
}

fn toolset_to_core(record: ToolsetRecord) -> RegisteredToolsetV1 {
    RegisteredToolsetV1 {
        version: PROTOCOL_VERSION_V1,
        toolset: nucleus_core::ToolsetRef {
            provider: record.provider,
            name: record.name,
            version: record.version,
        },
        definitions_schema_id: SchemaId::new(record.definitions_schema_id),
        digest: digest_text(record.definitions_digest),
        registered_at: record.created_at,
    }
}

fn pending_call_to_core(record: PendingToolCallRecord) -> Result<PendingToolCallV1, ApiError> {
    let arguments = raw_from_bytes(record.arguments_bytes, "stored tool arguments")?;
    Ok(PendingToolCallV1 {
        version: PROTOCOL_VERSION_V1,
        call: ToolCallV1 {
            version: PROTOCOL_VERSION_V1,
            id: ToolCallId::new(record.id),
            job_id: JobId::new(record.job_id),
            attempt_id: AttemptId::new(record.attempt_id),
            request_sequence: record.request_sequence,
            tool_name: record.tool_name,
            arguments_schema_id: SchemaId::new(record.arguments_schema_id),
            arguments,
        },
        state: match record.state {
            StoreToolCallState::Pending => ToolCallState::Pending,
            StoreToolCallState::Answered => ToolCallState::Answered,
        },
        created_at: record.created_at,
        answered_at: record.answered_at,
        result_sequence: record.result_sequence,
    })
}

fn validate_schema_registration(schema: &LogSchemaV1) -> Result<(), ApiError> {
    if schema.version != PROTOCOL_VERSION_V1 {
        return Err(ApiError::bad_request(
            "unsupported_version",
            "only schema registration version 1 is supported",
        ));
    }
    if schema.id.as_str().is_empty()
        || !valid_identifier(schema.id.as_str())
        || schema.name.trim().is_empty()
        || schema.schema_version.trim().is_empty()
        || schema.media_type.trim().is_empty()
        || schema.producer.trim().is_empty()
    {
        return Err(ApiError::bad_request(
            "invalid_schema",
            "schema identity and producer fields must not be empty",
        ));
    }
    let actual = sha256_digest(schema.schema.get().as_bytes());
    if actual != schema.digest {
        return Err(ApiError::bad_request(
            "digest_mismatch",
            &format!("schema digest must be {actual}"),
        ));
    }
    Ok(())
}

fn append_lifecycle_to_store(
    store: &mut Store,
    job_id: &str,
    attempt_id: Option<&str>,
    event: LifecycleEventKind,
    message: Option<&str>,
    details: Option<Box<RawValue>>,
) -> Result<LogRecord, StoreError> {
    let payload = serde_json::to_vec(&LifecycleEventV1 {
        version: PROTOCOL_VERSION_V1,
        event,
        job_id: JobId::new(job_id),
        attempt_id: attempt_id.map(AttemptId::new),
        message: message.map(str::to_owned),
        details,
    })
    .unwrap_or_else(|error| panic!("Nucleus lifecycle events must be serializable: {error}"));
    store.append_log(NewLogRecord {
        job_id: job_id.to_owned(),
        attempt_id: attempt_id.map(str::to_owned),
        observed_at: now(),
        emitted_at: None,
        stream: log_stream_name(LogStream::NucleusLifecycle).to_owned(),
        schema_id: LIFECYCLE_ID.to_owned(),
        payload,
    })
}

fn raw_from_bytes(bytes: Vec<u8>, context: &str) -> Result<Box<RawValue>, ApiError> {
    let value = String::from_utf8(bytes).map_err(|error| {
        ApiError::internal(
            "stored_record_invalid",
            &format!("{context} is not UTF-8 JSON: {error}"),
        )
    })?;
    RawValue::from_string(value).map_err(|error| {
        ApiError::internal(
            "stored_record_invalid",
            &format!("{context} is not one JSON value: {error}"),
        )
    })
}

fn trim_jsonl(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    bytes
}

fn byte_envelope(bytes: &[u8]) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "encoding": "base64",
        "data": base64::engine::general_purpose::STANDARD.encode(bytes)
    }))
    .unwrap_or_else(|error| panic!("byte envelope must be serializable: {error}"))
}

fn validated_limit(limit: Option<u32>) -> Result<usize, ApiError> {
    let value = limit.unwrap_or(100);
    if value == 0 || value > 1_000 {
        return Err(ApiError::bad_request(
            "invalid_limit",
            "limit must be between 1 and 1000",
        ));
    }
    Ok(value as usize)
}

fn reasoning_effort_name(value: ReasoningEffort) -> &'static str {
    match value {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Max => "max",
    }
}

fn terminal_reason_name(value: AttemptTerminalReason) -> &'static str {
    match value {
        AttemptTerminalReason::Completed => "completed",
        AttemptTerminalReason::HarnessFailure => "harness_failure",
        AttemptTerminalReason::ProtocolError => "protocol_error",
        AttemptTerminalReason::TimedOut => "timed_out",
        AttemptTerminalReason::Cancelled => "cancelled",
        AttemptTerminalReason::Lost => "lost",
        AttemptTerminalReason::RequesterUnavailable => "requester_unavailable",
    }
}

fn parse_terminal_reason(value: &str) -> Option<AttemptTerminalReason> {
    match value {
        "completed" => Some(AttemptTerminalReason::Completed),
        "harness_failure" => Some(AttemptTerminalReason::HarnessFailure),
        "protocol_error" => Some(AttemptTerminalReason::ProtocolError),
        "timed_out" => Some(AttemptTerminalReason::TimedOut),
        "cancelled" => Some(AttemptTerminalReason::Cancelled),
        "lost" => Some(AttemptTerminalReason::Lost),
        "requester_unavailable" => Some(AttemptTerminalReason::RequesterUnavailable),
        _ => None,
    }
}

fn core_job_state(value: StoreJobState) -> JobState {
    match value {
        StoreJobState::Accepted => JobState::Accepted,
        StoreJobState::Running => JobState::Running,
        StoreJobState::WaitingOnRequester => JobState::WaitingOnRequester,
        StoreJobState::Completed => JobState::Completed,
        StoreJobState::Failed => JobState::Failed,
        StoreJobState::Cancelled => JobState::Cancelled,
    }
}

fn store_job_state(value: JobState) -> StoreJobState {
    match value {
        JobState::Accepted => StoreJobState::Accepted,
        JobState::Running => StoreJobState::Running,
        JobState::WaitingOnRequester => StoreJobState::WaitingOnRequester,
        JobState::Completed => StoreJobState::Completed,
        JobState::Failed => StoreJobState::Failed,
        JobState::Cancelled => StoreJobState::Cancelled,
    }
}

fn core_attempt_state(value: StoreAttemptState) -> AttemptState {
    match value {
        StoreAttemptState::Pending => AttemptState::Pending,
        StoreAttemptState::Starting => AttemptState::Starting,
        StoreAttemptState::Running => AttemptState::Running,
        StoreAttemptState::WaitingOnRequester => AttemptState::WaitingOnRequester,
        StoreAttemptState::Completed => AttemptState::Completed,
        StoreAttemptState::Failed => AttemptState::Failed,
        StoreAttemptState::Cancelled => AttemptState::Cancelled,
        StoreAttemptState::TimedOut => AttemptState::TimedOut,
        StoreAttemptState::Lost => AttemptState::Lost,
    }
}

fn log_stream_name(value: LogStream) -> &'static str {
    match value {
        LogStream::NucleusLifecycle => "nucleus.lifecycle",
        LogStream::NucleusControl => "nucleus.control",
        LogStream::HarnessInput => "harness.input",
        LogStream::HarnessOutput => "harness.output",
        LogStream::HarnessStderr => "harness.stderr",
        LogStream::Requester => "requester",
    }
}

fn parse_log_stream(value: &str) -> Result<LogStream, ApiError> {
    match value {
        "nucleus.lifecycle" => Ok(LogStream::NucleusLifecycle),
        "nucleus.control" => Ok(LogStream::NucleusControl),
        "harness.input" => Ok(LogStream::HarnessInput),
        "harness.output" => Ok(LogStream::HarnessOutput),
        "harness.stderr" => Ok(LogStream::HarnessStderr),
        "requester" => Ok(LogStream::Requester),
        _ => Err(ApiError::internal(
            "stored_stream_invalid",
            &format!("unknown stored log stream {value:?}"),
        )),
    }
}

fn digest_text(digest: [u8; 32]) -> String {
    let mut text = String::with_capacity(71);
    text.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn sanitize_identifier(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | ':') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|error| panic!("UTC timestamps must format as RFC 3339: {error}"))
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
    issues: Vec<nucleus_core::ValidationIssue>,
    details: Option<Value>,
}

#[allow(clippy::needless_pass_by_value)]
impl ApiError {
    fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.to_owned(),
            message: message.into(),
            issues: Vec::new(),
            details: None,
        }
    }

    fn bad_request(code: &str, message: &str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    fn conflict(code: &str, message: &str) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    fn forbidden(code: &str, message: &str) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }

    fn internal(code: &str, message: &str) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, code, message)
    }

    fn unprocessable(code: &str, message: &str) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
    }

    fn not_found(entity: &str, id: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("{entity} {id:?} was not found"),
        )
    }

    fn validation(error: nucleus_core::ValidationError) -> Self {
        let mut response = Self::bad_request("validation_failed", &error.to_string());
        response.issues = error.issues;
        response
    }

    fn invalid_json(error: JsonRejection) -> Self {
        Self::bad_request("invalid_json", &error.body_text())
    }

    fn invalid_query(error: QueryRejection) -> Self {
        Self::bad_request("invalid_query", &error.body_text())
    }

    fn encoding(error: serde_json::Error) -> Self {
        Self::internal("encoding_failed", &error.to_string())
    }

    fn harness_unavailable(error: CodexError) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "harness_unavailable",
            error.to_string(),
        )
    }

    fn harness_compatibility(error: CodexError) -> Self {
        match error {
            CodexError::UnsupportedSetting { setting, reason } => {
                let message = format!("unsupported invocation setting {setting}: {reason}");
                let mut response = Self::unprocessable("unsupported_setting", &message);
                response.details = Some(json!({ "field": setting, "reason": reason }));
                response
            }
            error => Self::unprocessable("unsupported_setting", &error.to_string()),
        }
    }

    fn store(error: StoreError) -> Self {
        match error {
            StoreError::ToolCallOwnerTerminal { .. } => Self::conflict(
                "job_terminal",
                "the job ended before this tool result was posted",
            ),
            error @ (StoreError::JobConflict(_)
            | StoreError::LogSchemaConflict(_)
            | StoreError::ToolsetConflict { .. }
            | StoreError::ToolCallConflict { .. }
            | StoreError::ToolResultConflict { .. }
            | StoreError::InvalidStateTransition { .. }) => {
                Self::conflict("conflict", &error.to_string())
            }
            StoreError::NotFound { entity, id } => Self::not_found(entity, &id),
            error => {
                error!(error = %error, "Nucleus store operation failed");
                Self::internal("store_failed", "durable store operation failed")
            }
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        Self::store(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let response = ErrorResponseV1 {
            version: PROTOCOL_VERSION_V1,
            code: self.code,
            message: self.message,
            issues: self.issues,
            details: self.details,
        };
        (self.status, Json(response)).into_response()
    }
}

fn prepare_parent(path: &Path) -> Result<(), DaemonError> {
    let parent = path.parent().ok_or_else(|| DaemonError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
    })?;
    if !parent.exists() {
        fs::create_dir_all(parent).map_err(|source| DaemonError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|source| {
            DaemonError::Io {
                path: parent.to_path_buf(),
                source,
            }
        })?;
    }
    Ok(())
}

async fn bind_socket(path: &Path) -> Result<UnixListener, DaemonError> {
    if path.exists() {
        let metadata = fs::symlink_metadata(path).map_err(|source| DaemonError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if !metadata.file_type().is_socket() {
            return Err(DaemonError::SocketPathOccupied(path.to_path_buf()));
        }
        if tokio::net::UnixStream::connect(path).await.is_ok() {
            return Err(DaemonError::AlreadyRunning(path.to_path_buf()));
        }
        fs::remove_file(path).map_err(|source| DaemonError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    }
    let listener = UnixListener::bind(path).map_err(|source| DaemonError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
        DaemonError::Io {
            path: path.to_path_buf(),
            source,
        }
    })?;
    Ok(listener)
}

fn secure_store_files(database: &Path) -> Result<(), DaemonError> {
    let mut paths = vec![database.to_path_buf()];
    let database_text = database.as_os_str().to_string_lossy();
    paths.push(PathBuf::from(format!("{database_text}-wal")));
    paths.push(PathBuf::from(format!("{database_text}-shm")));
    for path in paths.into_iter().filter(|path| path.exists()) {
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            DaemonError::Io {
                path: path.clone(),
                source,
            }
        })?;
    }
    Ok(())
}

fn executable_on_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|value| {
        env::split_paths(&value)
            .map(|directory| directory.join(name))
            .find(|candidate| is_executable(candidate))
    })
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

async fn shutdown_signal() {
    let interrupt = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut terminate =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(terminate) => terminate,
                Err(error) => {
                    warn!(%error, "unable to install SIGTERM handler");
                    let _ = interrupt.await;
                    return;
                }
            };
        tokio::select! {
            _ = interrupt => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = interrupt.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch_context_request(id: &str, context: &LaunchContextId) -> JobRequestV1 {
        let mut invocation = nucleus_core::AgentInvocationV1::new(
            "codex",
            "test-model",
            AbsolutePath::new("/tmp"),
            nucleus_core::WorkspaceAccess::ReadOnly,
            nucleus_core::BuiltinToolsV1 {
                local_execution: true,
                web_search: false,
            },
            nucleus_core::TimeoutSeconds::new(30),
        );
        invocation.launch_context = Some(context.clone());
        JobRequestV1::new(
            id,
            "launch-context reservation test",
            Requester {
                program: "todo".to_owned(),
                id: "todo-request-1".to_owned(),
            },
            "base instructions",
            "prompt",
            invocation,
        )
    }

    #[tokio::test]
    async fn launch_context_atomically_binds_to_one_job_and_allows_its_retries()
    -> Result<(), Box<dyn std::error::Error>> {
        let state =
            AppState::new(Store::open_in_memory()?, CodexHarness::new("unused-codex")).await?;
        let context_id = LaunchContextId::new("launch_concurrency_test");
        state.launch_contexts.lock().await.insert(
            context_id.to_string(),
            EphemeralLaunchContext {
                requester: Requester {
                    program: "todo".to_owned(),
                    id: "todo-request-1".to_owned(),
                },
                environment: BTreeMap::from([("SECRET".to_owned(), "value".to_owned())]),
                expires: tokio::time::Instant::now() + LAUNCH_CONTEXT_TTL,
                bound_job_id: None,
            },
        );
        let first_request = launch_context_request("job-first", &context_id);
        let second_request = launch_context_request("job-second", &context_id);

        let (first, second) = tokio::join!(
            load_launch_environment(&state, &first_request),
            load_launch_environment(&state, &second_request)
        );
        let (winner, loser_error) = match (first, second) {
            (Ok(Some(_)), Err(error)) => (&first_request, error),
            (Err(error), Ok(Some(_))) => (&second_request, error),
            results => panic!("exactly one distinct job must reserve the context: {results:?}"),
        };
        assert_eq!(loser_error.status, StatusCode::CONFLICT);
        assert_eq!(loser_error.code, "launch_context_in_use");

        let Ok(Some(retry_environment)) = load_launch_environment(&state, winner).await else {
            panic!("the bound job's identical retry must retain access");
        };
        assert_eq!(
            retry_environment.get("SECRET").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            state
                .launch_contexts
                .lock()
                .await
                .get(context_id.as_str())
                .and_then(|context| context.bound_job_id.as_deref()),
            Some(winner.id.as_str())
        );
        Ok(())
    }

    #[tokio::test]
    async fn identical_submission_rechecks_job_after_context_was_consumed()
    -> Result<(), Box<dyn std::error::Error>> {
        let state =
            AppState::new(Store::open_in_memory()?, CodexHarness::new("unused-codex")).await?;
        let context_id = LaunchContextId::new("launch_staged_retry_test");
        state.launch_contexts.lock().await.insert(
            context_id.to_string(),
            EphemeralLaunchContext {
                requester: Requester {
                    program: "todo".to_owned(),
                    id: "todo-request-1".to_owned(),
                },
                environment: BTreeMap::from([("SECRET".to_owned(), "value".to_owned())]),
                expires: tokio::time::Instant::now() + LAUNCH_CONTEXT_TTL,
                bound_job_id: None,
            },
        );
        let first = launch_context_request("job-identical", &context_id);
        let second = first.clone();
        assert!(matches!(exact_existing_job(&state, &first).await, Ok(None)));
        assert!(matches!(
            exact_existing_job(&state, &second).await,
            Ok(None)
        ));

        let Ok(Some(_)) = load_launch_environment(&state, &first).await else {
            panic!("first submission must reserve the context");
        };
        state.store.lock().await.admit_job(NewJob {
            id: first.id.to_string(),
            label: first.label.clone(),
            requester_program: first.requester.program.clone(),
            requester_id: first.requester.id.clone(),
            parent_job_id: None,
            request_schema_id: JOB_REQUEST_ID.to_owned(),
            request_bytes: serde_json::to_vec(&first)?,
            created_at: "2026-08-27T00:00:00Z".to_owned(),
        })?;
        state
            .launch_contexts
            .lock()
            .await
            .remove(context_id.as_str());

        let resolution = resolve_launch_environment(&state, &second)
            .await
            .map_err(|error| format!("identical retry was rejected: {}", error.message))?;
        let LaunchEnvironmentResolution::Existing(existing) = resolution else {
            panic!("the post-consumption retry must resolve to the admitted job");
        };
        assert_eq!(existing.id, first.id.as_str());
        Ok(())
    }

    #[tokio::test]
    async fn launch_context_is_physically_erased_at_expiry()
    -> Result<(), Box<dyn std::error::Error>> {
        let state =
            AppState::new(Store::open_in_memory()?, CodexHarness::new("unused-codex")).await?;
        let context_id = "launch_expiry_test".to_owned();
        let expires = tokio::time::Instant::now() + Duration::from_millis(10);
        state.launch_contexts.lock().await.insert(
            context_id.clone(),
            EphemeralLaunchContext {
                requester: Requester {
                    program: "todo".to_owned(),
                    id: "todo-request-1".to_owned(),
                },
                environment: BTreeMap::from([("SECRET".to_owned(), "value".to_owned())]),
                expires,
                bound_job_id: None,
            },
        );
        schedule_launch_context_expiry(
            Arc::clone(&state.launch_contexts),
            context_id.clone(),
            expires,
        );

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if !state.launch_contexts.lock().await.contains_key(&context_id) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await?;
        assert!(!state.launch_contexts.lock().await.contains_key(&context_id));
        Ok(())
    }

    #[tokio::test]
    async fn cancellation_persisted_before_watch_registration_seeds_receiver()
    -> Result<(), Box<dyn std::error::Error>> {
        let state =
            AppState::new(Store::open_in_memory()?, CodexHarness::new("unused-codex")).await?;
        {
            let mut store = state.store.lock().await;
            store.admit_job(NewJob {
                id: "cancel-overlap-job".to_owned(),
                label: "cancellation overlap test".to_owned(),
                requester_program: "test".to_owned(),
                requester_id: "request-1".to_owned(),
                parent_job_id: None,
                request_schema_id: JOB_REQUEST_ID.to_owned(),
                request_bytes: b"{}".to_vec(),
                created_at: "2026-08-27T00:00:00Z".to_owned(),
            })?;
            store.create_attempt(NewAttempt {
                id: "cancel-overlap-attempt".to_owned(),
                job_id: "cancel-overlap-job".to_owned(),
                ordinal: 1,
                harness: "codex".to_owned(),
                harness_version: "test".to_owned(),
                adapter_version: "test".to_owned(),
                created_at: "2026-08-27T00:00:01Z".to_owned(),
            })?;
            store.request_cancellation("cancel-overlap-job", "2026-08-27T00:00:02Z")?;
        }
        assert!(
            !state
                .cancellations
                .lock()
                .await
                .contains_key("cancel-overlap-job")
        );

        let receiver = register_cancellation_watch(&state, "cancel-overlap-job")
            .await
            .map_err(|error| error.message)?;

        assert!(*receiver.borrow());
        assert!(
            state
                .cancellations
                .lock()
                .await
                .contains_key("cancel-overlap-job")
        );
        Ok(())
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn tool_call_answer_lifecycle_precedes_completion_awakened_by_reply()
    -> Result<(), Box<dyn std::error::Error>> {
        let state =
            AppState::new(Store::open_in_memory()?, CodexHarness::new("unused-codex")).await?;
        let job_id = "tool-order-job";
        let attempt_id = "tool-order-attempt";
        let call_id = "tool-order-call";
        let requester = Requester {
            program: "todo".to_owned(),
            id: "request-1".to_owned(),
        };
        {
            let mut store = state.store.lock().await;
            store.admit_job(NewJob {
                id: job_id.to_owned(),
                label: "tool answer ordering test".to_owned(),
                requester_program: requester.program.clone(),
                requester_id: requester.id.clone(),
                parent_job_id: None,
                request_schema_id: JOB_REQUEST_ID.to_owned(),
                request_bytes: b"{}".to_vec(),
                created_at: "2026-08-27T00:00:00Z".to_owned(),
            })?;
            store.create_attempt(NewAttempt {
                id: attempt_id.to_owned(),
                job_id: job_id.to_owned(),
                ordinal: 1,
                harness: "codex".to_owned(),
                harness_version: "test".to_owned(),
                adapter_version: "test".to_owned(),
                created_at: "2026-08-27T00:00:01Z".to_owned(),
            })?;
            store.transition_attempt(
                attempt_id,
                StoreAttemptState::Running,
                "2026-08-27T00:00:02Z",
                None,
            )?;
            store.record_pending_tool_call(
                NewPendingToolCall {
                    id: call_id.to_owned(),
                    job_id: job_id.to_owned(),
                    attempt_id: attempt_id.to_owned(),
                    tool_name: "read_todo".to_owned(),
                    arguments_schema_id: BYTES_ID.to_owned(),
                    arguments_bytes: br#"{"todoId":"todo-1"}"#.to_vec(),
                    created_at: "2026-08-27T00:00:03Z".to_owned(),
                },
                NewLogRecord {
                    job_id: job_id.to_owned(),
                    attempt_id: Some(attempt_id.to_owned()),
                    observed_at: "2026-08-27T00:00:03Z".to_owned(),
                    emitted_at: None,
                    stream: log_stream_name(LogStream::HarnessOutput).to_owned(),
                    schema_id: BYTES_ID.to_owned(),
                    payload: b"tool call".to_vec(),
                },
            )?;
            store.transition_attempt(
                attempt_id,
                StoreAttemptState::WaitingOnRequester,
                "2026-08-27T00:00:04Z",
                None,
            )?;
        }

        let (reply_sender, reply_receiver) = oneshot::channel();
        state
            .tool_replies
            .lock()
            .await
            .insert((job_id.to_owned(), call_id.to_owned()), reply_sender);
        let completion_state = state.clone();
        let completion = tokio::spawn(async move {
            reply_receiver.await.map_err(|_| "tool reply was dropped")?;
            let mut store = completion_state.store.lock().await;
            store.transition_attempt(
                attempt_id,
                StoreAttemptState::Completed,
                "2026-08-27T00:00:05Z",
                Some("completed"),
            )?;
            append_lifecycle_to_store(
                &mut store,
                job_id,
                Some(attempt_id),
                LifecycleEventKind::AttemptCompleted,
                None,
                None,
            )?;
            append_lifecycle_to_store(
                &mut store,
                job_id,
                Some(attempt_id),
                LifecycleEventKind::JobCompleted,
                None,
                None,
            )?;
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });
        let result = ToolResultV1 {
            version: PROTOCOL_VERSION_V1,
            call_id: ToolCallId::new(call_id),
            requester,
            result_schema_id: SchemaId::new(BYTES_ID),
            result: to_raw_value(&json!({"answer": "done"}))?,
            is_error: false,
        };

        let _response = post_tool_result(
            State(state.clone()),
            AxumPath((job_id.to_owned(), call_id.to_owned())),
            Ok(Json(result)),
        )
        .await
        .map_err(|error| error.message)?;
        if let Err(error) = completion.await? {
            return Err(format!("completion task failed: {error}").into());
        }

        let events = state
            .store
            .lock()
            .await
            .list_logs(job_id, 0, 100)?
            .into_iter()
            .filter(|record| record.schema_id == LIFECYCLE_ID)
            .map(|record| serde_json::from_slice::<LifecycleEventV1>(&record.payload))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            events.iter().map(|event| event.event).collect::<Vec<_>>(),
            vec![
                LifecycleEventKind::ToolCallAnswered,
                LifecycleEventKind::AttemptCompleted,
                LifecycleEventKind::JobCompleted,
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn recovery_records_attempt_and_job_terminal_lifecycle_events()
    -> Result<(), Box<dyn std::error::Error>> {
        let state =
            AppState::new(Store::open_in_memory()?, CodexHarness::new("unused-codex")).await?;
        {
            let mut store = state.store.lock().await;
            store.admit_job(NewJob {
                id: "recovery-job".to_owned(),
                label: "recovery test".to_owned(),
                requester_program: "test".to_owned(),
                requester_id: "request-1".to_owned(),
                parent_job_id: None,
                request_schema_id: JOB_REQUEST_ID.to_owned(),
                request_bytes: b"{}".to_vec(),
                created_at: "2026-08-27T00:00:00Z".to_owned(),
            })?;
            store.create_attempt(NewAttempt {
                id: "recovery-attempt".to_owned(),
                job_id: "recovery-job".to_owned(),
                ordinal: 1,
                harness: "codex".to_owned(),
                harness_version: "test".to_owned(),
                adapter_version: "test".to_owned(),
                created_at: "2026-08-27T00:00:01Z".to_owned(),
            })?;
            store.transition_attempt(
                "recovery-attempt",
                StoreAttemptState::Running,
                "2026-08-27T00:00:02Z",
                None,
            )?;
        }

        state.recover_interrupted_work().await?;

        let store = state.store.lock().await;
        let job = store
            .get_job("recovery-job")?
            .ok_or("recovered job disappeared")?;
        let attempt = store
            .get_attempt("recovery-attempt")?
            .ok_or("recovered attempt disappeared")?;
        assert_eq!(job.state, StoreJobState::Failed);
        assert_eq!(attempt.state, StoreAttemptState::Lost);
        let events = store
            .list_logs("recovery-job", 0, 100)?
            .into_iter()
            .filter(|record| record.schema_id == LIFECYCLE_ID)
            .map(|record| serde_json::from_slice::<LifecycleEventV1>(&record.payload))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            events.iter().map(|event| event.event).collect::<Vec<_>>(),
            vec![
                LifecycleEventKind::AttemptLost,
                LifecycleEventKind::JobFailed
            ]
        );
        Ok(())
    }
}
