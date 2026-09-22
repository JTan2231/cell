//! Scripted responses at Annals' Nucleus client boundary. This fixture runs no
//! daemon, database, Codex executable, authentication session, or background job.
//! Annals still dispatches the managed tool and commits its own domain result.

use std::collections::BTreeMap;
use std::fs;
use std::future::IntoFuture as _;
use std::io;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use axum::extract::{Path as RoutePath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use nucleus_core::{
    AccountSnapshotV1, AttemptId, AttemptOutputV1, AttemptState, AttemptTerminalReason, AttemptV1,
    CancelJobResponseV1, ErrorResponseV1, HarnessIdentity, JobAcceptedV1, JobRequestV1, JobState,
    JobSummaryV1, JobV1, LogSchemaV1, PendingToolCallV1, RegisteredToolsetV1, SchemaId, ToolCallId,
    ToolCallState, ToolCallV1, ToolCallsQueryV1, ToolCallsResponseV1, ToolResultV1,
    ToolsetRegistrationV1,
};
use serde_json::{json, value::to_raw_value};
use tokio::sync::{Mutex, oneshot};

const TIMESTAMP: &str = "2026-09-21T00:00:00Z";
type Shared = Arc<Mutex<Script>>;
type ApiResult<T> = Result<Json<T>, FixtureError>;

pub struct FakeNucleus {
    enabled: Arc<AtomicBool>,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<io::Result<()>>>,
}

impl FakeNucleus {
    pub fn start(socket: &Path, counter: &Path, controls: &Path) -> io::Result<Self> {
        let listener = UnixListener::bind(socket)?;
        listener.set_nonblocking(true)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let enabled = Arc::new(AtomicBool::new(true));
        let script = Arc::new(Mutex::new(Script {
            enabled: enabled.clone(),
            counter: counter.to_path_buf(),
            controls: controls.to_path_buf(),
            input_schemas: BTreeMap::new(),
            jobs: BTreeMap::new(),
        }));
        let router = Router::new()
            .route("/v1/account", get(account))
            .route("/v1/schemas", post(register_schema))
            .route("/v1/toolsets", post(register_toolset))
            .route("/v1/jobs", post(submit_job))
            .route("/v1/jobs/{job}", get(job))
            .route("/v1/jobs/{job}/tool-calls", get(tool_calls))
            .route("/v1/jobs/{job}/tool-calls/{call}/result", post(tool_result))
            .route("/v1/jobs/{job}/cancel", post(cancel))
            .with_state(script);
        let (shutdown, stopped) = oneshot::channel();
        let thread = thread::spawn(move || {
            runtime.block_on(async move {
                let listener = tokio::net::UnixListener::from_std(listener)?;
                // Dropping the server and runtime also closes idle client connections.
                tokio::select! {
                    result = axum::serve(listener, router).into_future() => result,
                    _ = stopped => Ok(()),
                }
            })
        });
        Ok(Self {
            enabled,
            shutdown: Some(shutdown),
            thread: Some(thread),
        })
    }

    pub fn disable_execution(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }
}

impl Drop for FakeNucleus {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            if !thread::panicking() {
                assert!(
                    matches!(result, Ok(Ok(()))),
                    "Nucleus fixture failed: {result:?}"
                );
            }
        }
    }
}

struct Script {
    enabled: Arc<AtomicBool>,
    counter: PathBuf,
    controls: PathBuf,
    input_schemas: BTreeMap<u32, SchemaId>,
    jobs: BTreeMap<String, ScriptedJob>,
}

struct ScriptedJob {
    request: JobRequestV1,
    digest: String,
    call: PendingToolCallV1,
    state: JobState,
    before_submit: Option<Barrier>,
    after_submit: Option<Barrier>,
    fail_first: bool,
    fail_after_submit: bool,
}

impl ScriptedJob {
    fn new(
        request: JobRequestV1,
        schema: SchemaId,
        number: u64,
        controls: &Path,
    ) -> Result<Self, FixtureError> {
        let call = PendingToolCallV1 {
            version: 1,
            call: ToolCallV1 {
                version: 1,
                id: ToolCallId::new("call"),
                job_id: request.id.clone(),
                attempt_id: AttemptId::new(format!("attempt-{}", request.id)),
                request_sequence: 1,
                tool_name: "submit_reconciliation".into(),
                arguments_schema_id: schema,
                arguments: to_raw_value(&json!({
                    "summary": format!("Integrate inbox source {number}"),
                    "operations": [{
                        "action": "create_concept",
                        "ref": "inbox_item",
                        "label": format!("Inbox concept {number}"),
                        "parents": [],
                        "evidence": [{"quote": "Shared inbox claim."}]
                    }]
                }))?,
            },
            state: ToolCallState::Pending,
            created_at: TIMESTAMP.into(),
            answered_at: None,
        };
        let mut job = Self {
            digest: request.digest()?,
            request,
            call,
            state: JobState::Running,
            before_submit: None,
            after_submit: None,
            fail_first: false,
            fail_after_submit: false,
        };
        if number == 1 {
            job.before_submit = Barrier::read(controls, "block")?;
            job.after_submit = Barrier::read(controls, "after-submit")?;
            job.fail_first = controls.join("fail-first").exists();
            job.fail_after_submit = controls.join("fail-after-submit").exists();
        }
        job.advance()?;
        Ok(job)
    }

    fn advance(&mut self) -> io::Result<()> {
        if self.state.is_terminal() {
            return Ok(());
        }
        if self.call.state == ToolCallState::Pending {
            if let Some(barrier) = &self.before_submit
                && barrier.waiting()?
            {
                return Ok(());
            }
            self.state = if self.fail_first {
                JobState::Failed
            } else {
                JobState::WaitingOnRequester
            };
        } else if self.fail_after_submit {
            self.state = JobState::Failed;
        } else if let Some(barrier) = &self.after_submit
            && barrier.waiting()?
        {
            self.state = JobState::Running;
        } else {
            self.state = JobState::Completed;
        }
        Ok(())
    }

    fn view(&self) -> JobV1 {
        let (state, reason) = match self.state {
            JobState::Accepted => (AttemptState::Pending, None),
            JobState::Running => (AttemptState::Running, None),
            JobState::WaitingOnRequester => (AttemptState::WaitingOnRequester, None),
            JobState::Completed => (
                AttemptState::Completed,
                Some(AttemptTerminalReason::Completed),
            ),
            JobState::Failed => (
                AttemptState::Failed,
                Some(AttemptTerminalReason::HarnessFailure),
            ),
            JobState::Cancelled => (
                AttemptState::Cancelled,
                Some(AttemptTerminalReason::Cancelled),
            ),
        };
        let completed_at: Option<String> = self.state.is_terminal().then(|| TIMESTAMP.into());
        JobV1 {
            version: 1,
            summary: JobSummaryV1 {
                version: 1,
                id: self.request.id.clone(),
                label: self.request.label.clone(),
                requester: self.request.requester.clone(),
                parent: self.request.parent.clone(),
                state: self.state,
                request_digest: self.digest.clone(),
                created_at: TIMESTAMP.into(),
                updated_at: TIMESTAMP.into(),
                completed_at: completed_at.clone(),
                current_attempt_id: Some(self.call.call.attempt_id.clone()),
            },
            request: self.request.clone(),
            attempts: vec![AttemptV1 {
                version: 1,
                id: self.call.call.attempt_id.clone(),
                job_id: self.request.id.clone(),
                ordinal: 1,
                harness: harness(),
                state,
                created_at: TIMESTAMP.into(),
                started_at: Some(TIMESTAMP.into()),
                completed_at,
                terminal_reason: reason,
                terminal_message: (self.state == JobState::Failed)
                    .then(|| "simulated model failure".into()),
                output: (self.state == JobState::Completed).then(|| AttemptOutputV1 {
                    thread_id: "thread".into(),
                    turn_id: "turn".into(),
                    final_message: "fake completed".into(),
                }),
            }],
            quota: None,
        }
    }
}

struct Barrier {
    ready: PathBuf,
    release: PathBuf,
}

impl Barrier {
    fn read(controls: &Path, name: &str) -> io::Result<Option<Self>> {
        let ready = controls.join(format!("{name}-ready"));
        if !ready.exists() {
            return Ok(None);
        }
        Ok(Some(Self {
            ready: fs::read_to_string(ready)?.into(),
            release: fs::read_to_string(controls.join(format!("{name}-release")))?.into(),
        }))
    }

    fn waiting(&self) -> io::Result<bool> {
        if !self.ready.exists() {
            fs::write(&self.ready, b"ready\n")?;
        }
        Ok(!self.release.exists())
    }
}

async fn account(State(script): State<Shared>) -> ApiResult<AccountSnapshotV1> {
    let script = script.lock().await;
    if !script.enabled.load(Ordering::SeqCst) || script.controls.join("auth-fail").exists() {
        return Err(FixtureError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "simulated authentication failure",
        ));
    }
    Ok(Json(AccountSnapshotV1 {
        version: 1,
        observed_at: TIMESTAMP.into(),
        harness: harness(),
        rate_limits: json!({}),
        quota: None,
        usage: None,
        usage_error: None,
    }))
}

fn register_schema(Json(schema): Json<LogSchemaV1>) -> std::future::Ready<Json<LogSchemaV1>> {
    std::future::ready(Json(schema))
}

async fn register_toolset(
    State(script): State<Shared>,
    Json(registration): Json<ToolsetRegistrationV1>,
) -> ApiResult<RegisteredToolsetV1> {
    let schema = registration
        .definitions
        .tools
        .iter()
        .find(|tool| tool.name == "submit_reconciliation")
        .ok_or_else(|| FixtureError::new(StatusCode::BAD_REQUEST, "missing reconciliation tool"))?
        .input_schema_id
        .clone();
    script
        .lock()
        .await
        .input_schemas
        .insert(registration.toolset.version, schema);
    Ok(Json(RegisteredToolsetV1 {
        version: 1,
        toolset: registration.toolset,
        definitions_schema_id: registration.definitions_schema_id,
        digest: registration.digest,
        registered_at: TIMESTAMP.into(),
    }))
}

async fn submit_job(
    State(script): State<Shared>,
    Json(request): Json<JobRequestV1>,
) -> ApiResult<JobAcceptedV1> {
    let mut script = script.lock().await;
    if let Some(previous) = script.jobs.get(request.id.as_str()) {
        if previous.digest != request.digest()? {
            return Err(FixtureError::new(
                StatusCode::CONFLICT,
                "job request changed",
            ));
        }
    } else {
        if !script.enabled.load(Ordering::SeqCst) {
            return Err(FixtureError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "execution disabled",
            ));
        }
        let schema = request
            .invocation
            .toolset
            .as_ref()
            .and_then(|toolset| script.input_schemas.get(&toolset.version))
            .cloned()
            .ok_or_else(|| FixtureError::new(StatusCode::BAD_REQUEST, "unregistered toolset"))?;
        let previous = match fs::read_to_string(&script.counter) {
            Ok(text) => text.trim().parse::<u64>().map_err(io::Error::other)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error.into()),
        };
        let number = previous + 1;
        fs::write(&script.counter, format!("{number}\n"))?;
        let job = ScriptedJob::new(request.clone(), schema, number, &script.controls)?;
        script.jobs.insert(request.id.to_string(), job);
    }
    let job = script
        .jobs
        .get(request.id.as_str())
        .ok_or_else(missing_job)?
        .view();
    Ok(Json(JobAcceptedV1 {
        version: 1,
        job_id: request.id,
        state: job.summary.state,
        request_digest: job.summary.request_digest,
        attempt: job.attempts.into_iter().next(),
        log_cursor: 0,
    }))
}

async fn job(State(script): State<Shared>, RoutePath(id): RoutePath<String>) -> ApiResult<JobV1> {
    let mut script = script.lock().await;
    let job = script.jobs.get_mut(&id).ok_or_else(missing_job)?;
    job.advance()?;
    Ok(Json(job.view()))
}

async fn tool_calls(
    State(script): State<Shared>,
    RoutePath(id): RoutePath<String>,
    Query(query): Query<ToolCallsQueryV1>,
) -> ApiResult<ToolCallsResponseV1> {
    let mut script = script.lock().await;
    let job = script.jobs.get_mut(&id).ok_or_else(missing_job)?;
    job.advance()?;
    let pending = job.state == JobState::WaitingOnRequester
        && job.call.state == ToolCallState::Pending
        && query.after < job.call.call.request_sequence;
    Ok(Json(ToolCallsResponseV1 {
        version: 1,
        job_id: job.request.id.clone(),
        calls: if pending {
            vec![job.call.clone()]
        } else {
            vec![]
        },
        next_sequence: if pending {
            job.call.call.request_sequence
        } else {
            query.after
        },
    }))
}

async fn tool_result(
    State(script): State<Shared>,
    RoutePath((id, call)): RoutePath<(String, String)>,
    Json(result): Json<ToolResultV1>,
) -> ApiResult<PendingToolCallV1> {
    let mut script = script.lock().await;
    let job = script.jobs.get_mut(&id).ok_or_else(missing_job)?;
    if call != job.call.call.id.as_str()
        || result.call_id != job.call.call.id
        || result.requester != job.request.requester
    {
        return Err(FixtureError::new(
            StatusCode::CONFLICT,
            "tool result identity differs",
        ));
    }
    job.call.state = ToolCallState::Answered;
    job.call.answered_at = Some(TIMESTAMP.into());
    job.advance()?;
    Ok(Json(job.call.clone()))
}

async fn cancel(
    State(script): State<Shared>,
    RoutePath(id): RoutePath<String>,
) -> ApiResult<CancelJobResponseV1> {
    let mut script = script.lock().await;
    let job = script.jobs.get_mut(&id).ok_or_else(missing_job)?;
    let cancellation_requested = !job.state.is_terminal();
    if cancellation_requested {
        job.state = JobState::Cancelled;
    }
    Ok(Json(CancelJobResponseV1 {
        version: 1,
        job_id: job.request.id.clone(),
        state: job.state,
        cancellation_requested,
    }))
}

fn harness() -> HarnessIdentity {
    HarnessIdentity {
        harness: "codex".into(),
        harness_version: "fixture".into(),
        adapter_version: "fixture".into(),
    }
}

fn missing_job() -> FixtureError {
    FixtureError::new(StatusCode::NOT_FOUND, "unknown fixture job")
}

struct FixtureError {
    status: StatusCode,
    message: String,
}

impl FixtureError {
    fn new(status: StatusCode, message: &str) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl From<io::Error> for FixtureError {
    fn from(error: io::Error) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
    }
}

impl From<serde_json::Error> for FixtureError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
    }
}

impl IntoResponse for FixtureError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponseV1 {
                version: 1,
                code: "fixture_error".into(),
                message: self.message,
                issues: vec![],
                details: None,
            }),
        )
            .into_response()
    }
}
