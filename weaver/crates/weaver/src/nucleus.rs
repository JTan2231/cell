use std::path::Path;
use std::time::Duration;

#[cfg(test)]
use std::path::PathBuf;

use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{
    AbsolutePath, AgentInvocationV1, AttemptState, AttemptTerminalReason, BuiltinToolsV1,
    CancelJobResponseV1, HarnessCapability, HealthResponseV1, JobId, JobRequestV1, JobState, JobV1,
    ModelId, PROTOCOL_VERSION_V1, ReasoningEffort, Requester, TimeoutSeconds, WorkspaceAccess,
};

use crate::error::{AppResult, WeaverError};
use crate::project::{Project, Stage};

const MODEL: &str = "gpt-5.6-sol";
const STAGE_TIMEOUT_SECONDS: u64 = 3_600;
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);
const BASE_INSTRUCTIONS: &str = "You are Weaver's unattended editorial Markdown worker. Follow the stage request exactly and return only the requested Markdown.";

const REQUIRED_CAPABILITIES: [HarnessCapability; 5] = [
    HarnessCapability::ExactModel,
    HarnessCapability::ReasoningEffort,
    HarnessCapability::WorkspaceReadOnly,
    HarnessCapability::BuiltinLocalExecution,
    HarnessCapability::BuiltinWebSearch,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StageOutput {
    pub(crate) job_id: JobId,
    pub(crate) final_message: String,
    pub(crate) cancellation_requested: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct NucleusRunner {
    client: NucleusClient,
    poll_interval: Duration,
    model: &'static str,
    stage_timeout_seconds: u64,
}

impl NucleusRunner {
    pub(crate) fn for_current_user() -> AppResult<Self> {
        let client = NucleusClient::for_current_user()
            .map_err(|error| client_error("cannot initialize the Nucleus client", &error))?;
        Ok(Self {
            client,
            poll_interval: DEFAULT_POLL_INTERVAL,
            model: MODEL,
            stage_timeout_seconds: STAGE_TIMEOUT_SECONDS,
        })
    }

    #[cfg(test)]
    fn for_socket(socket: impl Into<PathBuf>, poll_interval: Duration) -> AppResult<Self> {
        let client = NucleusClient::new(socket)
            .map_err(|error| client_error("cannot initialize the Nucleus client", &error))?;
        Ok(Self {
            client,
            poll_interval,
            model: MODEL,
            stage_timeout_seconds: STAGE_TIMEOUT_SECONDS,
        })
    }

    pub(crate) async fn ensure_ready(&self) -> AppResult<()> {
        let health = self
            .client
            .health()
            .await
            .map_err(|error| client_error("cannot read Nucleus health", &error))?;
        validate_health(&health)
    }

    pub(crate) fn stage_request(
        &self,
        project: &Project,
        workspace_root: &Path,
        run_id: &str,
        stage: Stage,
        prompt: &str,
        parent: Option<JobId>,
    ) -> AppResult<JobRequestV1> {
        let mut invocation = AgentInvocationV1::new(
            "codex",
            ModelId::new(self.model),
            AbsolutePath::new(workspace_root.to_path_buf()),
            WorkspaceAccess::ReadOnly,
            BuiltinToolsV1 {
                local_execution: false,
                web_search: false,
            },
            TimeoutSeconds::new(self.stage_timeout_seconds),
        );
        invocation.reasoning_effort = Some(ReasoningEffort::Max);
        invocation.toolset = None;
        invocation.launch_context = None;

        let mut request = JobRequestV1::new(
            stage_job_id(run_id, stage),
            format!(
                "Weaver {} stage {}/5: {}",
                project.slug, stage.ordinal, stage.name
            ),
            Requester {
                program: "weaver".to_owned(),
                id: run_id.to_owned(),
            },
            BASE_INSTRUCTIONS,
            prompt,
            invocation,
        );
        request.parent = parent;
        request.validate().map_err(|error| {
            WeaverError::runtime(format!("invalid Nucleus stage request: {error}"))
        })?;
        Ok(request)
    }

    /// Recover or submit one exact stage request and wait for its terminal result.
    ///
    /// The caller owns durable request persistence. A retry must pass the same
    /// request so Nucleus can use its job ID as the admission idempotency key.
    pub(crate) async fn run_stage(
        &self,
        request: &JobRequestV1,
        cancellation_requested: &dyn Fn() -> bool,
    ) -> AppResult<StageOutput> {
        request.validate().map_err(|error| {
            WeaverError::runtime(format!("invalid persisted Nucleus stage request: {error}"))
        })?;
        let expected_digest = request.request_digest().map_err(|error| {
            WeaverError::runtime(format!("cannot digest Nucleus job {}: {error}", request.id))
        })?;

        let mut initial = self.inspect_job(&request.id).await?;
        if let Some(job) = &initial {
            verify_recovered_job(job, request, &expected_digest)?;
        } else {
            if cancellation_requested() {
                return Err(WeaverError::runtime(format!(
                    "Nucleus job {} was cancelled before admission",
                    request.id
                )));
            }
            let accepted = self
                .client
                .submit_job(request)
                .await
                .map_err(|error| client_error("cannot submit the Nucleus stage job", &error))?;
            if accepted.job_id != request.id || accepted.request_digest != expected_digest {
                return Err(WeaverError::runtime(format!(
                    "Nucleus admitted job {} under unexpected identity or request content",
                    request.id
                )));
            }
        }

        let mut cancellation_sent = false;
        loop {
            let job = match initial.take() {
                Some(job) => job,
                None => self.inspect_job(&request.id).await?.ok_or_else(|| {
                    WeaverError::runtime(format!(
                        "Nucleus lost admitted job {} from its durable store",
                        request.id
                    ))
                })?,
            };
            verify_recovered_job(&job, request, &expected_digest)?;

            match job.summary.state {
                JobState::Completed => {
                    return completed_output(&job, cancellation_sent);
                }
                JobState::Failed | JobState::Cancelled => {
                    return Err(terminal_error(&job));
                }
                JobState::WaitingOnRequester => {
                    return Err(WeaverError::runtime(format!(
                        "Nucleus job {} is waiting on a requester tool despite Weaver submitting no toolset",
                        request.id
                    )));
                }
                JobState::Accepted | JobState::Running => {}
            }

            let requested = cancellation_requested();
            if requested && !cancellation_sent {
                self.cancel_job(&request.id).await?;
                cancellation_sent = true;
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    pub(crate) async fn inspect_job(&self, job_id: &JobId) -> AppResult<Option<JobV1>> {
        match self.client.get_job(job_id).await {
            Ok(job) => Ok(Some(job)),
            Err(ClientError::Api {
                status: 404, code, ..
            }) if code == "not_found" => Ok(None),
            Err(error) => Err(client_error("cannot inspect the Nucleus stage job", &error)),
        }
    }

    pub(crate) async fn cancel_job(&self, job_id: &JobId) -> AppResult<CancelJobResponseV1> {
        let response = self
            .client
            .cancel_job(job_id)
            .await
            .map_err(|error| client_error("cannot cancel the Nucleus stage job", &error))?;
        if response.job_id != *job_id {
            return Err(WeaverError::runtime(format!(
                "Nucleus returned cancellation state for {}, not {job_id}",
                response.job_id
            )));
        }
        Ok(response)
    }
}

pub(crate) fn stage_job_id(run_id: &str, stage: Stage) -> JobId {
    JobId::new(format!(
        "weaver-{run_id}-{:02}-{}",
        stage.ordinal, stage.name
    ))
}

fn validate_health(health: &HealthResponseV1) -> AppResult<()> {
    if health.version != PROTOCOL_VERSION_V1
        || !health
            .supported_protocol_versions
            .contains(&PROTOCOL_VERSION_V1)
    {
        return Err(WeaverError::runtime(format!(
            "Nucleus does not support invocation protocol {PROTOCOL_VERSION_V1}"
        )));
    }
    let Some(harness) = &health.harness else {
        return Err(WeaverError::retryable(
            "Nucleus has no ready harness for Weaver",
        ));
    };
    if harness.harness.as_str() != "codex" {
        return Err(WeaverError::runtime(format!(
            "Nucleus reported unsupported harness {}",
            harness.harness
        )));
    }
    for capability in REQUIRED_CAPABILITIES {
        if !health.capabilities.contains(&capability) {
            return Err(WeaverError::runtime(format!(
                "Nucleus lacks required Weaver capability {capability:?}"
            )));
        }
    }
    if health.status != "ok"
        || !health.accepting_jobs
        || !health.authentication.configured
        || !health.authentication.authenticated
    {
        let detail = health
            .detail
            .as_deref()
            .or(health.authentication.detail.as_deref())
            .unwrap_or("no readiness detail supplied");
        return Err(WeaverError::retryable(format!(
            "Nucleus is not ready for Weaver: status={}, accepting_jobs={}, configured={}, authenticated={}; {detail}",
            health.status,
            health.accepting_jobs,
            health.authentication.configured,
            health.authentication.authenticated
        )));
    }
    Ok(())
}

fn verify_recovered_job(
    job: &JobV1,
    request: &JobRequestV1,
    expected_digest: &str,
) -> AppResult<()> {
    if job.summary.id != request.id
        || job.summary.request_digest != expected_digest
        || job.request != *request
    {
        return Err(WeaverError::runtime(format!(
            "Nucleus job {} does not match Weaver's persisted stage request",
            request.id
        )));
    }
    Ok(())
}

fn completed_output(job: &JobV1, cancellation_requested: bool) -> AppResult<StageOutput> {
    let attempt_id = job.summary.current_attempt_id.as_ref().ok_or_else(|| {
        WeaverError::runtime(format!(
            "completed Nucleus job {} has no current attempt",
            job.summary.id
        ))
    })?;
    let attempt = job
        .attempts
        .iter()
        .find(|attempt| attempt.id == *attempt_id)
        .ok_or_else(|| {
            WeaverError::runtime(format!(
                "completed Nucleus job {} is missing attempt {attempt_id}",
                job.summary.id
            ))
        })?;
    if attempt.state != AttemptState::Completed
        || attempt.terminal_reason != Some(AttemptTerminalReason::Completed)
    {
        return Err(WeaverError::runtime(format!(
            "Nucleus job {} completed with inconsistent attempt state {:?} and reason {:?}",
            job.summary.id, attempt.state, attempt.terminal_reason
        )));
    }
    let output = attempt.output.as_ref().ok_or_else(|| {
        WeaverError::runtime(format!(
            "completed Nucleus job {} has no structured attempt output",
            job.summary.id
        ))
    })?;
    if output.thread_id.trim().is_empty()
        || output.turn_id.trim().is_empty()
        || output.final_message.trim().is_empty()
    {
        return Err(WeaverError::runtime(format!(
            "completed Nucleus job {} returned incomplete structured output",
            job.summary.id
        )));
    }
    Ok(StageOutput {
        job_id: job.summary.id.clone(),
        final_message: output.final_message.clone(),
        cancellation_requested,
    })
}

fn terminal_error(job: &JobV1) -> WeaverError {
    let attempt = job
        .summary
        .current_attempt_id
        .as_ref()
        .and_then(|id| job.attempts.iter().find(|attempt| attempt.id == *id));
    let state = attempt.map_or_else(
        || format!("job state {:?}", job.summary.state),
        |attempt| format!("attempt state {:?}", attempt.state),
    );
    let reason = attempt
        .and_then(|attempt| attempt.terminal_reason)
        .map_or_else(String::new, |reason| format!(", reason {reason:?}"));
    let message = attempt
        .and_then(|attempt| attempt.terminal_message.as_deref())
        .unwrap_or("no terminal detail supplied");
    WeaverError::runtime(format!(
        "Nucleus job {} ended with {state}{reason}: {message}",
        job.summary.id
    ))
}

fn client_error(context: &str, error: &ClientError) -> WeaverError {
    let message = format!("{context}: {error}");
    match error {
        ClientError::Transport { .. } => WeaverError::retryable(message),
        ClientError::Api { status, .. } if *status >= 500 => WeaverError::retryable(message),
        ClientError::MissingHome
        | ClientError::RelativeSocket(_)
        | ClientError::Validation(_)
        | ClientError::Build(_)
        | ClientError::Api { .. }
        | ClientError::UndecodableError { .. }
        | ClientError::Decode { .. } => WeaverError::runtime(message),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::thread;

    use nucleus_core::{
        AttemptId, AttemptOutputV1, AttemptV1, AuthenticationReadinessV1, HarnessIdentity,
        JobAcceptedV1, JobSummaryV1,
    };
    use serde::Serialize;
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    fn must<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("unexpected error: {error}"),
        }
    }

    fn project(root: &Path) -> Project {
        Project {
            repo_root: root.to_path_buf(),
            slug: "how-i-work".to_owned(),
            narrative_relative: "narratives/how-i-work".to_owned(),
            narrative_root: root.join("narratives/how-i-work"),
            basis_relative: "narratives/how-i-work/basis.md".to_owned(),
            brief_relative: "narratives/how-i-work/brief.md".to_owned(),
            prompt_root: root.join("workflow/narrative"),
        }
    }

    fn ready_health() -> HealthResponseV1 {
        HealthResponseV1 {
            version: PROTOCOL_VERSION_V1,
            status: "ok".to_owned(),
            daemon_version: "0.2.2".to_owned(),
            accepting_jobs: true,
            checked_at: "2026-08-28T00:00:00Z".to_owned(),
            supported_protocol_versions: vec![PROTOCOL_VERSION_V1],
            harness: Some(HarnessIdentity {
                harness: "codex".into(),
                harness_version: "0.146.0".to_owned(),
                adapter_version: "0.2.2".to_owned(),
            }),
            harness_executable: Some(AbsolutePath::new("/opt/codex")),
            capabilities: REQUIRED_CAPABILITIES.to_vec(),
            authentication: AuthenticationReadinessV1 {
                codex_home: AbsolutePath::new("/tmp/codex-home"),
                configured: true,
                authenticated: true,
                detail: None,
            },
            execution: None,
            detail: None,
        }
    }

    #[test]
    fn strict_health_requires_readiness_and_each_requested_semantic() {
        assert!(validate_health(&ready_health()).is_ok());

        let mut missing = ready_health();
        missing
            .capabilities
            .retain(|value| *value != HarnessCapability::BuiltinWebSearch);
        let error = validate_health(&missing)
            .err()
            .map(|error| error.to_string());
        assert!(error.is_some_and(|message| message.contains("BuiltinWebSearch")));

        let mut unauthenticated = ready_health();
        unauthenticated.authentication.authenticated = false;
        let error = validate_health(&unauthenticated);
        assert!(error.as_ref().is_err_and(WeaverError::is_retryable));
    }

    #[test]
    fn stage_request_is_stable_read_only_and_has_no_tools() {
        let temporary = must(TempDir::new());
        let workspace_root = temporary.path().join("private-weaver-workspace");
        let runner = must(NucleusRunner::for_socket(
            temporary.path().join("nucleus.sock"),
            Duration::ZERO,
        ));
        let request = must(runner.stage_request(
            &project(temporary.path()),
            &workspace_root,
            "0198-run",
            crate::project::STAGES[0],
            "stage prompt",
            None,
        ));
        assert_eq!(request.id.as_str(), "weaver-0198-run-01-stories");
        assert_eq!(request.requester.program, "weaver");
        assert_eq!(request.requester.id, "0198-run");
        assert_eq!(request.prompt, "stage prompt");
        assert_eq!(request.instructions, BASE_INSTRUCTIONS);
        assert_eq!(request.invocation.cwd.as_path(), workspace_root);
        assert_eq!(
            request.invocation.workspace_access,
            WorkspaceAccess::ReadOnly
        );
        assert!(!request.invocation.builtin_tools.local_execution);
        assert!(!request.invocation.builtin_tools.web_search);
        assert_eq!(
            request.invocation.reasoning_effort,
            Some(ReasoningEffort::Max)
        );
        assert!(request.invocation.toolset.is_none());
        assert!(request.invocation.launch_context.is_none());
    }

    #[test]
    fn completed_output_uses_the_current_attempt_not_vector_position() {
        let temporary = must(TempDir::new());
        let runner = must(NucleusRunner::for_socket(
            temporary.path().join("nucleus.sock"),
            Duration::ZERO,
        ));
        let request = must(runner.stage_request(
            &project(temporary.path()),
            temporary.path(),
            "run-1",
            crate::project::STAGES[0],
            "prompt",
            None,
        ));
        let mut job = terminal_job(&request, "generated markdown");
        let current = job.attempts[0].clone();
        let mut stale = current.clone();
        stale.id = AttemptId::new("attempt-stale");
        stale.output = None;
        job.attempts = vec![current, stale];
        let output = must(completed_output(&job, false));
        assert_eq!(output.final_message, "generated markdown");
        assert_eq!(output.job_id, request.id);
    }

    #[tokio::test]
    async fn recovers_missing_job_submits_and_reads_structured_output()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temporary = TempDir::new()?;
        let socket = temporary.path().join("nucleus.sock");
        let listener = UnixListener::bind(&socket)?;
        fs::create_dir_all(temporary.path().join("narratives/how-i-work"))?;
        let runner = NucleusRunner::for_socket(&socket, Duration::ZERO)?;
        let request = runner.stage_request(
            &project(temporary.path()),
            temporary.path(),
            "run-2",
            crate::project::STAGES[1],
            "prompt two",
            Some(JobId::new("weaver-run-2-01-stories")),
        )?;
        let expected = request.clone();
        let server = thread::spawn(move || serve_success(&listener, &expected));

        let output = runner.run_stage(&request, &|| false).await?;
        assert_eq!(output.final_message, "stage two output");
        assert_eq!(output.job_id, request.id);
        assert!(!output.cancellation_requested);

        match server.join() {
            Ok(result) => result?,
            Err(_) => return Err("fake Nucleus server panicked".into()),
        }
        Ok(())
    }

    #[tokio::test]
    async fn dropped_admission_response_recovers_the_exact_job_on_restart()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temporary = TempDir::new()?;
        let socket = temporary.path().join("nucleus.sock");
        let listener = UnixListener::bind(&socket)?;
        let runner = NucleusRunner::for_socket(&socket, Duration::ZERO)?;
        let request = runner.stage_request(
            &project(temporary.path()),
            temporary.path(),
            "run-dropped",
            crate::project::STAGES[0],
            "stable prompt",
            None,
        )?;
        let expected = request.clone();
        let first_server = thread::spawn(move || serve_dropped_admission(&listener, &expected));

        let Err(first_error) = runner.run_stage(&request, &|| false).await else {
            return Err("dropped admission response unexpectedly succeeded".into());
        };
        assert!(first_error.is_retryable());
        let submitted = match first_server.join() {
            Ok(result) => result?,
            Err(_) => return Err("first fake Nucleus server panicked".into()),
        };
        assert_eq!(submitted, request);

        fs::remove_file(&socket)?;
        let listener = UnixListener::bind(&socket)?;
        let expected = request.clone();
        let recovery_server = thread::spawn(move || {
            let (mut stream, _) = listener.accept()?;
            let (request_line, body) = read_request(&stream)?;
            assert!(body.is_empty());
            assert!(request_line.starts_with(&format!("GET /v1/jobs/{} ", expected.id)));
            write_json(
                &mut stream,
                "200 OK",
                &terminal_job(&expected, "recovered output"),
            )?;
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });

        let output = runner.run_stage(&request, &|| false).await?;
        assert_eq!(output.job_id, request.id);
        assert_eq!(output.final_message, "recovered output");
        match recovery_server.join() {
            Ok(result) => result?,
            Err(_) => return Err("recovery fake Nucleus server panicked".into()),
        }
        Ok(())
    }

    #[tokio::test]
    async fn cancellation_is_posted_once_while_terminal_state_is_polled()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temporary = TempDir::new()?;
        let socket = temporary.path().join("nucleus.sock");
        let listener = UnixListener::bind(&socket)?;
        let runner = NucleusRunner::for_socket(&socket, Duration::ZERO)?;
        let request = runner.stage_request(
            &project(temporary.path()),
            temporary.path(),
            "run-cancel",
            crate::project::STAGES[0],
            "cancel prompt",
            None,
        )?;
        let expected = request.clone();
        let server = thread::spawn(move || serve_cancellation(&listener, &expected));

        let Err(error) = runner.run_stage(&request, &|| true).await else {
            return Err("cancelled Nucleus job unexpectedly completed".into());
        };
        assert!(error.to_string().contains("Cancelled"));
        match server.join() {
            Ok(result) => result?,
            Err(_) => return Err("cancellation fake Nucleus server panicked".into()),
        }
        Ok(())
    }

    fn serve_dropped_admission(
        listener: &UnixListener,
        expected: &JobRequestV1,
    ) -> Result<JobRequestV1, Box<dyn std::error::Error + Send + Sync>> {
        let (mut stream, _) = listener.accept()?;
        let (request_line, _) = read_request(&stream)?;
        assert!(request_line.starts_with(&format!("GET /v1/jobs/{} ", expected.id)));
        write_json(
            &mut stream,
            "404 Not Found",
            &json!({
                "version": 1,
                "code": "not_found",
                "message": "job not found"
            }),
        )?;

        let (stream, _) = listener.accept()?;
        let (request_line, body) = read_request(&stream)?;
        assert!(request_line.starts_with("POST /v1/jobs "));
        let submitted = serde_json::from_slice(&body)?;
        assert_eq!(&submitted, expected);
        drop(stream);
        Ok(submitted)
    }

    fn serve_cancellation(
        listener: &UnixListener,
        expected: &JobRequestV1,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (mut stream, _) = listener.accept()?;
        let (request_line, _) = read_request(&stream)?;
        assert!(request_line.starts_with(&format!("GET /v1/jobs/{} ", expected.id)));
        write_json(&mut stream, "200 OK", &running_job(expected))?;

        let (mut stream, _) = listener.accept()?;
        let (request_line, body) = read_request(&stream)?;
        assert!(body.is_empty());
        assert!(request_line.starts_with(&format!("POST /v1/jobs/{}/cancel ", expected.id)));
        write_json(
            &mut stream,
            "200 OK",
            &CancelJobResponseV1 {
                version: PROTOCOL_VERSION_V1,
                job_id: expected.id.clone(),
                state: JobState::Running,
                cancellation_requested: true,
            },
        )?;

        let (mut stream, _) = listener.accept()?;
        let (request_line, _) = read_request(&stream)?;
        assert!(request_line.starts_with(&format!("GET /v1/jobs/{} ", expected.id)));
        write_json(&mut stream, "200 OK", &running_job(expected))?;

        let (mut stream, _) = listener.accept()?;
        let (request_line, _) = read_request(&stream)?;
        assert!(request_line.starts_with(&format!("GET /v1/jobs/{} ", expected.id)));
        write_json(&mut stream, "200 OK", &cancelled_job(expected))?;
        Ok(())
    }

    fn serve_success(
        listener: &UnixListener,
        expected: &JobRequestV1,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (mut stream, _) = listener.accept()?;
        let (request_line, _) = read_request(&stream)?;
        assert!(request_line.starts_with(&format!("GET /v1/jobs/{} ", expected.id)));
        write_json(
            &mut stream,
            "404 Not Found",
            &json!({
                "version": 1,
                "code": "not_found",
                "message": "job not found"
            }),
        )?;

        let (mut stream, _) = listener.accept()?;
        let (request_line, body) = read_request(&stream)?;
        assert!(request_line.starts_with("POST /v1/jobs "));
        let submitted: JobRequestV1 = serde_json::from_slice(&body)?;
        assert_eq!(&submitted, expected);
        let digest = expected.request_digest()?;
        write_json(
            &mut stream,
            "202 Accepted",
            &JobAcceptedV1 {
                version: PROTOCOL_VERSION_V1,
                job_id: expected.id.clone(),
                state: JobState::Accepted,
                request_digest: digest,
                attempt: None,
                log_cursor: 0,
            },
        )?;

        let (mut stream, _) = listener.accept()?;
        let (request_line, _) = read_request(&stream)?;
        assert!(request_line.starts_with(&format!("GET /v1/jobs/{} ", expected.id)));
        write_json(
            &mut stream,
            "200 OK",
            &terminal_job(expected, "stage two output"),
        )?;
        Ok(())
    }

    fn terminal_job(request: &JobRequestV1, final_message: &str) -> JobV1 {
        let attempt_id = AttemptId::new("attempt-1");
        JobV1 {
            version: PROTOCOL_VERSION_V1,
            summary: JobSummaryV1 {
                version: PROTOCOL_VERSION_V1,
                id: request.id.clone(),
                label: request.label.clone(),
                requester: request.requester.clone(),
                parent: request.parent.clone(),
                state: JobState::Completed,
                request_digest: must(request.request_digest()),
                created_at: "2026-08-28T00:00:00Z".to_owned(),
                updated_at: "2026-08-28T00:00:01Z".to_owned(),
                completed_at: Some("2026-08-28T00:00:01Z".to_owned()),
                current_attempt_id: Some(attempt_id.clone()),
            },
            request: request.clone(),
            attempts: vec![AttemptV1 {
                version: PROTOCOL_VERSION_V1,
                id: attempt_id,
                job_id: request.id.clone(),
                ordinal: 1,
                harness: HarnessIdentity {
                    harness: "codex".into(),
                    harness_version: "0.146.0".to_owned(),
                    adapter_version: "0.2.2".to_owned(),
                },
                state: AttemptState::Completed,
                created_at: "2026-08-28T00:00:00Z".to_owned(),
                started_at: Some("2026-08-28T00:00:00Z".to_owned()),
                completed_at: Some("2026-08-28T00:00:01Z".to_owned()),
                terminal_reason: Some(AttemptTerminalReason::Completed),
                terminal_message: Some("Codex turn completed".to_owned()),
                output: Some(AttemptOutputV1 {
                    thread_id: "thread-1".to_owned(),
                    turn_id: "turn-1".to_owned(),
                    final_message: final_message.to_owned(),
                }),
            }],
        }
    }

    fn running_job(request: &JobRequestV1) -> JobV1 {
        let mut job = terminal_job(request, "unused");
        job.summary.state = JobState::Running;
        job.summary.completed_at = None;
        let attempt = &mut job.attempts[0];
        attempt.state = AttemptState::Running;
        attempt.completed_at = None;
        attempt.terminal_reason = None;
        attempt.terminal_message = None;
        attempt.output = None;
        job
    }

    fn cancelled_job(request: &JobRequestV1) -> JobV1 {
        let mut job = running_job(request);
        job.summary.state = JobState::Cancelled;
        job.summary.completed_at = Some("2026-08-28T00:00:02Z".to_owned());
        let attempt = &mut job.attempts[0];
        attempt.state = AttemptState::Cancelled;
        attempt.completed_at = Some("2026-08-28T00:00:02Z".to_owned());
        attempt.terminal_reason = Some(AttemptTerminalReason::Cancelled);
        attempt.terminal_message = Some("job cancellation reached Codex".to_owned());
        job
    }

    fn read_request(stream: &UnixStream) -> std::io::Result<(String, Vec<u8>)> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut request_line = String::new();
        reader.read_line(&mut request_line)?;
        let mut content_length = 0_usize;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            if line == "\r\n" || line.is_empty() {
                break;
            }
            if let Some(value) = line
                .split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .map(|(_, value)| value.trim())
            {
                content_length = value
                    .parse()
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            }
        }
        let mut body = vec![0_u8; content_length];
        reader.read_exact(&mut body)?;
        Ok((request_line, body))
    }

    fn write_json(
        stream: &mut UnixStream,
        status: &str,
        value: &impl Serialize,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let body = serde_json::to_vec(value)?;
        write!(
            stream,
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        )?;
        stream.write_all(&body)?;
        stream.flush()?;
        Ok(())
    }
}
