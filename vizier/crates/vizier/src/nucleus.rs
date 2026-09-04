use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{
    AbsolutePath, AgentInvocationV1, AttemptState as NucleusAttemptState, BuiltinToolsV1,
    HarnessCapability, JobId, JobRequestV1, JobState, ModelId, PROTOCOL_VERSION_V1,
    ReasoningEffort, Requester, TimeoutSeconds, ToolCallState, ToolCallsQueryV1, ToolResultV1,
    ToolsetRef, WorkspaceAccess,
};
use serde::Serialize;
use serde_json::value::RawValue;
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::contracts::{
    DEVELOPER_INSTRUCTIONS, RESULT_SCHEMA_ID, ToolsetKind, decode_submission, schema_registrations,
    toolset_registration,
};
use crate::error::{AppError, AppResult};
use crate::model::{AttemptState, AttemptView, PathScope, Role, RunView};
use crate::store::{NewAttempt, Receipt, Store};

pub const MODEL: &str = "gpt-5.6-sol";
pub const TIMEOUT_SECONDS: u64 = 20 * 60;
const TRANSPORT_RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Clone, Debug)]
pub struct AgentRunner {
    socket: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealthSummary {
    pub daemon_version: String,
    pub harness_version: String,
    pub adapter_version: String,
    pub max_active_jobs: u32,
    pub available_slots: u32,
}

#[derive(Clone, Debug)]
pub struct AttemptSpec<'a> {
    pub run: &'a RunView,
    pub role: Role,
    pub subject_id: &'a str,
    pub round: u32,
    pub targeted: bool,
    pub prompt: &'a str,
    pub workspace: &'a Path,
    pub base_commit: Option<&'a str>,
    pub allowed_scopes: &'a [PathScope],
    pub predecessor_attempt_id: Option<&'a str>,
}

impl AgentRunner {
    #[must_use]
    pub const fn for_current_user() -> Self {
        Self { socket: None }
    }

    #[cfg(test)]
    #[must_use]
    pub fn with_socket(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: Some(socket.into()),
        }
    }

    pub async fn doctor(&self) -> AppResult<HealthSummary> {
        let client = self.client()?;
        let summary = require_health(&client).await?;
        for kind in [
            ToolsetKind::UnitPlan,
            ToolsetKind::DelegationPlan,
            ToolsetKind::CandidateHandoff,
            ToolsetKind::CandidateReview,
        ] {
            register_contract(&client, kind).await?;
        }
        Ok(summary)
    }

    pub fn prepare_attempt(&self, store: &Store, spec: &AttemptSpec<'_>) -> AppResult<AttemptView> {
        if !spec.workspace.is_absolute() {
            return Err(AppError::new(
                "attempt_workspace_invalid",
                "attempt workspace must be absolute",
            ));
        }
        let suffix = Uuid::now_v7();
        let job_id = format!("vizier-{}-{suffix}", spec.role.as_str().replace('_', "-"));
        let kind = ToolsetKind::for_role(spec.role);
        let registration = toolset_registration(kind)?;
        let (workspace_access, local_execution) = role_permissions(spec.role);
        let mut invocation = AgentInvocationV1::new(
            "codex",
            ModelId::new(MODEL),
            AbsolutePath::new(spec.workspace),
            workspace_access,
            BuiltinToolsV1 {
                local_execution,
                web_search: false,
            },
            TimeoutSeconds::new(TIMEOUT_SECONDS),
        );
        invocation.reasoning_effort = Some(ReasoningEffort::Max);
        invocation.toolset = Some(registration.toolset);
        let mut request = JobRequestV1::new(
            JobId::new(&job_id),
            format!("Vizier {} for {}", spec.role.as_str(), spec.subject_id),
            Requester {
                program: "vizier".to_owned(),
                id: spec.run.id.clone(),
            },
            kind.instructions(),
            spec.prompt,
            invocation,
        );
        request.developer_instructions = Some(DEVELOPER_INSTRUCTIONS.to_owned());
        request.validate().map_err(|error| {
            AppError::new(
                "nucleus_request_invalid",
                format!("unable to construct constrained Nucleus request: {error}"),
            )
        })?;
        let request_bytes = serde_json::to_vec(&request)?;
        let request_sha256 = sha256(&request_bytes);
        store.create_attempt(&NewAttempt {
            run_id: &spec.run.id,
            role: spec.role,
            subject_id: spec.subject_id,
            round: spec.round,
            targeted: spec.targeted,
            nucleus_job_id: &job_id,
            request_bytes: &request_bytes,
            request_sha256: &request_sha256,
            toolset_name: kind.name(),
            workspace_path: &spec.workspace.to_string_lossy(),
            base_commit: spec.base_commit,
            allowed_scopes: spec.allowed_scopes,
            predecessor_attempt_id: spec.predecessor_attempt_id,
        })
    }

    pub async fn run_attempt(&self, store: &Store, attempt_id: &str) -> AppResult<AttemptView> {
        let attempt = store.attempt(attempt_id)?;
        if attempt.state.is_terminal() {
            if attempt.domain_document_id.is_some() {
                return Ok(attempt);
            }
            return Err(AppError::new(
                "attempt_terminal_without_result",
                "attempt is terminal without a committed Vizier result; use attempt retry",
            ));
        }
        let request = persisted_request(&attempt)?;
        let client = self.client()?;
        require_health(&client).await?;
        register_contract(&client, ToolsetKind::for_role(attempt.role)).await?;
        if attempt.admitted {
            let job = client
                .get_job(&JobId::new(&attempt.nucleus_job_id))
                .await
                .map_err(client_error)?;
            verify_job(&attempt, &request, &job)?;
        } else {
            let accepted = submit_stably(&client, &request).await?;
            if accepted.version != PROTOCOL_VERSION_V1
                || accepted.job_id.as_str() != attempt.nucleus_job_id
            {
                return Err(AppError::new(
                    "nucleus_job_mismatch",
                    "Nucleus admitted a different job identity",
                ));
            }
            verify_digest(&attempt, &accepted.request_digest)?;
            store.mark_attempt_admitted(&attempt.id, &attempt.request_sha256)?;
        }
        self.serve_mailbox(store, &attempt, &request, &client).await
    }

    pub async fn cancel_run(&self, store: &Store, run_id: &str) -> AppResult<()> {
        let client = self.client()?;
        for attempt in store.active_attempts(run_id)? {
            if attempt.admitted {
                match client
                    .cancel_job(&JobId::new(&attempt.nucleus_job_id))
                    .await
                {
                    Ok(_) | Err(ClientError::Api { status: 404, .. }) => {}
                    Err(error) => return Err(client_error(error)),
                }
            }
            store.set_attempt_runtime(
                &attempt.id,
                AttemptState::Cancelled,
                Some("Vizier run cancellation requested"),
            )?;
        }
        Ok(())
    }

    async fn serve_mailbox(
        &self,
        store: &Store,
        initial: &AttemptView,
        request: &JobRequestV1,
        client: &NucleusClient,
    ) -> AppResult<AttemptView> {
        let job_id = JobId::new(&initial.nucleus_job_id);
        let mut tool_after = initial.tool_after;
        loop {
            let run = store.run(&initial.run_id)?;
            if run.cancel_requested {
                let _ = client.cancel_job(&job_id).await;
            }
            let calls = client
                .pending_tool_calls(
                    &job_id,
                    &ToolCallsQueryV1 {
                        after: tool_after,
                        wait_seconds: 1,
                    },
                )
                .await
                .map_err(client_error)?;
            if calls.version != PROTOCOL_VERSION_V1
                || calls.job_id != job_id
                || calls.next_sequence < tool_after
            {
                return Err(AppError::new(
                    "nucleus_tool_contract_mismatch",
                    "Nucleus returned a mailbox page outside the admitted Vizier job",
                ));
            }
            for pending in calls.calls {
                let call = pending.call;
                let kind = ToolsetKind::for_role(initial.role);
                if pending.version != PROTOCOL_VERSION_V1
                    || pending.state != ToolCallState::Pending
                    || pending.answered_at.is_some()
                    || call.version != PROTOCOL_VERSION_V1
                    || call.job_id != job_id
                    || call.request_sequence <= tool_after
                    || call.tool_name != kind.tool_name()
                    || call.arguments_schema_id.as_str() != kind.input_schema_id()
                {
                    return Err(AppError::new(
                        "nucleus_tool_contract_mismatch",
                        "Nucleus returned a call outside the admitted Vizier tool contract",
                    ));
                }
                let arguments_sha256 = sha256(call.arguments.get().as_bytes());
                let receipt = match store.receipt(job_id.as_str(), call.id.as_str())? {
                    Some(receipt) => {
                        if receipt.arguments_sha256 != arguments_sha256 {
                            return Err(AppError::new(
                                "tool_replay_conflict",
                                "replayed tool call changed its exact arguments",
                            ));
                        }
                        receipt
                    }
                    None => {
                        match decode_submission(initial.role, &call.tool_name, call.arguments.get())
                        {
                            Ok(submission) => match store.commit_managed_submission(
                                &initial.id,
                                job_id.as_str(),
                                call.id.as_str(),
                                &arguments_sha256,
                                RESULT_SCHEMA_ID,
                                &submission,
                            ) {
                                Ok(receipt) => receipt,
                                Err(error) if is_managed_rejection(&error) => store
                                    .record_managed_error(
                                        job_id.as_str(),
                                        call.id.as_str(),
                                        &arguments_sha256,
                                        RESULT_SCHEMA_ID,
                                        error.code(),
                                        error.message(),
                                    )?,
                                Err(error) => return Err(error),
                            },
                            Err(error) => store.record_managed_error(
                                job_id.as_str(),
                                call.id.as_str(),
                                &arguments_sha256,
                                RESULT_SCHEMA_ID,
                                error.code(),
                                error.message(),
                            )?,
                        }
                    }
                };
                let result = receipt_result(initial, &call.id, &receipt)?;
                let sequence = post_result_stably(client, &job_id, &call.id, &result).await?;
                if sequence != call.request_sequence {
                    return Err(AppError::new(
                        "nucleus_tool_contract_mismatch",
                        "Nucleus acknowledged a different mailbox sequence",
                    ));
                }
                tool_after = tool_after.max(sequence);
                store.advance_tool_after(&initial.id, tool_after)?;
            }
            let refreshed = store.attempt(&initial.id)?;
            let job = client.get_job(&job_id).await.map_err(client_error)?;
            verify_job(&refreshed, request, &job)?;
            if job.summary.state.is_terminal() {
                let detail = job
                    .attempts
                    .last()
                    .and_then(|attempt| attempt.terminal_message.as_deref());
                let state = state_for_job(&job);
                store.set_attempt_runtime(&refreshed.id, state, detail)?;
                let terminal = store.attempt(&refreshed.id)?;
                if terminal.domain_document_id.is_some() {
                    return Ok(terminal);
                }
                return Err(AppError::new(
                    "nucleus_job_terminal_without_result",
                    detail.unwrap_or("Nucleus job ended without an accepted Vizier submission"),
                ));
            }
            store.set_attempt_runtime(&refreshed.id, AttemptState::Running, None)?;
        }
    }

    fn client(&self) -> AppResult<NucleusClient> {
        match &self.socket {
            Some(socket) => NucleusClient::new(socket).map_err(client_error),
            None => NucleusClient::for_current_user().map_err(client_error),
        }
    }
}

pub fn neutral_workspace(state_root: &Path, job_hint: &str) -> AppResult<PathBuf> {
    let path = state_root.join("neutral").join(job_hint);
    fs::create_dir_all(&path)?;
    #[cfg(unix)]
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    Ok(path)
}

fn role_permissions(role: Role) -> (WorkspaceAccess, bool) {
    match role {
        Role::Assembler => (WorkspaceAccess::None, false),
        Role::Planner | Role::PlanReviewer | Role::PacketReviewer | Role::IntegratedReviewer => {
            (WorkspaceAccess::ReadOnly, true)
        }
        Role::Implementor | Role::Integrator => (WorkspaceAccess::ReadWrite, true),
    }
}

async fn require_health(client: &NucleusClient) -> AppResult<HealthSummary> {
    let health = client.health().await.map_err(client_error)?;
    let required = [
        HarnessCapability::ExactModel,
        HarnessCapability::ReasoningEffort,
        HarnessCapability::WorkspaceNone,
        HarnessCapability::WorkspaceReadOnly,
        HarnessCapability::WorkspaceReadWrite,
        HarnessCapability::BuiltinLocalExecution,
        HarnessCapability::BuiltinWebSearch,
        HarnessCapability::DynamicClientTools,
        HarnessCapability::DeveloperInstructions,
        HarnessCapability::PersistentFileAuthentication,
    ];
    let missing = required
        .iter()
        .filter(|capability| !health.capabilities.contains(capability))
        .map(|capability| format!("{capability:?}"))
        .collect::<Vec<_>>();
    let harness = health
        .harness
        .as_ref()
        .filter(|value| value.harness.as_str() == "codex");
    let execution = health.execution.as_ref();
    if health.version != PROTOCOL_VERSION_V1
        || health.status != "ok"
        || !health.accepting_jobs
        || !health
            .supported_protocol_versions
            .contains(&PROTOCOL_VERSION_V1)
        || !health.authentication.configured
        || !health.authentication.authenticated
        || harness.is_none()
        || health.harness_executable.is_none()
        || execution.is_none()
        || !missing.is_empty()
    {
        return Err(AppError::new(
            "nucleus_not_ready",
            format!(
                "status={}, accepting_jobs={}, configured={}, authenticated={}, codex_harness={}, execution={}, missing_capabilities={}",
                health.status,
                health.accepting_jobs,
                health.authentication.configured,
                health.authentication.authenticated,
                harness.is_some(),
                execution.is_some(),
                missing.join(",")
            ),
        ));
    }
    let harness = harness.ok_or_else(|| {
        AppError::new(
            "nucleus_not_ready",
            "Nucleus health omitted the required Codex harness identity",
        )
    })?;
    let execution = execution.ok_or_else(|| {
        AppError::new(
            "nucleus_not_ready",
            "Nucleus health omitted the required execution capacity",
        )
    })?;
    Ok(HealthSummary {
        daemon_version: health.daemon_version,
        harness_version: harness.harness_version.clone(),
        adapter_version: harness.adapter_version.clone(),
        max_active_jobs: execution.max_active_jobs,
        available_slots: execution.available_slots,
    })
}

async fn register_contract(client: &NucleusClient, kind: ToolsetKind) -> AppResult<()> {
    for schema in schema_registrations(kind)? {
        let registered = client
            .register_schema(&schema)
            .await
            .map_err(client_error)?;
        if registered.id != schema.id || registered.digest != schema.digest {
            return Err(AppError::new(
                "nucleus_schema_conflict",
                format!("Nucleus registered different bytes for {}", schema.id),
            ));
        }
    }
    let registration = toolset_registration(kind)?;
    let registered = client
        .register_toolset(&registration)
        .await
        .map_err(client_error)?;
    if registered.toolset != registration.toolset
        || registered.definitions_schema_id != registration.definitions_schema_id
        || registered.digest != registration.digest
    {
        return Err(AppError::new(
            "nucleus_toolset_conflict",
            format!("Nucleus registered different bytes for {}", kind.name()),
        ));
    }
    Ok(())
}

async fn submit_stably(
    client: &NucleusClient,
    request: &JobRequestV1,
) -> AppResult<nucleus_core::JobAcceptedV1> {
    let mut retries = 0_u8;
    loop {
        match client.submit_job(request).await {
            Ok(accepted) => return Ok(accepted),
            Err(ClientError::Transport { .. }) if retries < 2 => {
                retries += 1;
                tokio::time::sleep(TRANSPORT_RETRY_DELAY).await;
            }
            Err(error) => return Err(client_error(error)),
        }
    }
}

async fn post_result_stably(
    client: &NucleusClient,
    job_id: &JobId,
    call_id: &nucleus_core::ToolCallId,
    result: &ToolResultV1,
) -> AppResult<u64> {
    let mut retries = 0_u8;
    loop {
        match client.post_tool_result(job_id, call_id, result).await {
            Ok(answered)
                if answered.version == PROTOCOL_VERSION_V1
                    && answered.state == ToolCallState::Answered
                    && answered.answered_at.is_some()
                    && answered.call.job_id == *job_id
                    && answered.call.id == *call_id =>
            {
                return Ok(answered.call.request_sequence);
            }
            Ok(_) => {
                return Err(AppError::new(
                    "nucleus_tool_contract_mismatch",
                    "Nucleus returned an invalid tool-result acknowledgement",
                ));
            }
            Err(ClientError::Transport { .. }) if retries < 2 => {
                retries += 1;
                tokio::time::sleep(TRANSPORT_RETRY_DELAY).await;
            }
            Err(error) => return Err(client_error(error)),
        }
    }
}

fn receipt_result(
    attempt: &AttemptView,
    call_id: &nucleus_core::ToolCallId,
    receipt: &Receipt,
) -> AppResult<ToolResultV1> {
    let result = RawValue::from_string(receipt.result_json.clone())
        .map_err(|error| AppError::new("tool_receipt_invalid", error.to_string()))?;
    Ok(ToolResultV1 {
        version: PROTOCOL_VERSION_V1,
        call_id: call_id.clone(),
        requester: Requester {
            program: "vizier".to_owned(),
            id: attempt.run_id.clone(),
        },
        result_schema_id: nucleus_core::SchemaId::new(&receipt.result_schema_id),
        result,
        is_error: receipt.is_error,
    })
}

fn persisted_request(attempt: &AttemptView) -> AppResult<JobRequestV1> {
    if sha256(&attempt.request_bytes) != attempt.request_sha256 {
        return Err(AppError::new(
            "attempt_request_digest_mismatch",
            "persisted Nucleus request bytes do not match their digest",
        ));
    }
    let request: JobRequestV1 = serde_json::from_slice(&attempt.request_bytes)?;
    let kind = ToolsetKind::for_role(attempt.role);
    let (workspace_access, local_execution) = role_permissions(attempt.role);
    if request.id.as_str() != attempt.nucleus_job_id
        || request.requester.program != "vizier"
        || request.requester.id != attempt.run_id
        || request.parent.is_some()
        || request.instructions != kind.instructions()
        || request.developer_instructions.as_deref() != Some(DEVELOPER_INSTRUCTIONS)
        || request.invocation.harness.as_str() != "codex"
        || request.invocation.model.as_str() != MODEL
        || request.invocation.reasoning_effort != Some(ReasoningEffort::Max)
        || request.invocation.cwd.as_path() != Path::new(&attempt.workspace_path)
        || request.invocation.workspace_access != workspace_access
        || request.invocation.builtin_tools.local_execution != local_execution
        || request.invocation.builtin_tools.web_search
        || request.invocation.timeout_seconds != TimeoutSeconds::new(TIMEOUT_SECONDS)
        || request.invocation.launch_context.is_some()
        || request.invocation.toolset.as_ref()
            != Some(&ToolsetRef {
                provider: "vizier".to_owned(),
                name: kind.name().to_owned(),
                version: 1,
            })
        || attempt.toolset_name != kind.name()
    {
        return Err(AppError::new(
            "attempt_request_contract_mismatch",
            "persisted request does not match its constrained Vizier role",
        ));
    }
    request
        .validate()
        .map_err(|error| AppError::new("attempt_request_invalid", error.to_string()))?;
    Ok(request)
}

fn verify_job(
    attempt: &AttemptView,
    request: &JobRequestV1,
    job: &nucleus_core::JobV1,
) -> AppResult<()> {
    verify_digest(attempt, &job.summary.request_digest)?;
    if job.version != PROTOCOL_VERSION_V1
        || job.summary.version != PROTOCOL_VERSION_V1
        || job.summary.id.as_str() != attempt.nucleus_job_id
        || job.summary.requester.program != "vizier"
        || job.summary.requester.id != attempt.run_id
        || job.request != *request
    {
        return Err(AppError::new(
            "nucleus_job_identity_mismatch",
            "Nucleus job does not match the persisted Vizier request",
        ));
    }
    Ok(())
}

fn verify_digest(attempt: &AttemptView, digest: &str) -> AppResult<()> {
    if digest != format!("sha256:{}", attempt.request_sha256) {
        return Err(AppError::new(
            "nucleus_request_digest_mismatch",
            "Nucleus admitted different immutable request bytes",
        ));
    }
    Ok(())
}

fn state_for_job(job: &nucleus_core::JobV1) -> AttemptState {
    match job.attempts.last().map(|attempt| attempt.state) {
        Some(NucleusAttemptState::TimedOut) => AttemptState::TimedOut,
        Some(NucleusAttemptState::Lost) => AttemptState::Lost,
        Some(NucleusAttemptState::Cancelled) => AttemptState::Cancelled,
        _ => match job.summary.state {
            JobState::Completed => AttemptState::Completed,
            JobState::Cancelled => AttemptState::Cancelled,
            JobState::Failed => AttemptState::Failed,
            JobState::Accepted | JobState::Running | JobState::WaitingOnRequester => {
                AttemptState::Failed
            }
        },
    }
}

fn is_managed_rejection(error: &AppError) -> bool {
    matches!(
        error.code(),
        "delegation_invalid"
            | "delegation_incomplete"
            | "delegation_cycle"
            | "delegation_overlap"
            | "packet_scope_empty"
            | "packet_scope_invalid"
            | "review_scope_invalid"
            | "markdown_empty"
            | "markdown_too_large"
            | "markdown_invalid_utf8"
    )
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn client_error(error: ClientError) -> AppError {
    let message = error.to_string();
    drop(error);
    AppError::new("nucleus_error", message)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use nucleus_core::{JobRequestV1, ReasoningEffort};

    use super::{AgentRunner, AttemptSpec, persisted_request};
    use crate::model::{NewRun, OpaqueMarkdown, Role};
    use crate::store::Store;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn every_role_persists_sol_with_max_reasoning() -> TestResult {
        let directory = tempfile::tempdir()?;
        let store = Store::new(directory.path().join("vizier.db"));
        store.initialize()?;
        let run = store.create_run(&NewRun {
            id: "run-requester-policy".to_owned(),
            request_key: None,
            repository: directory.path().display().to_string(),
            source_commit: "source".to_owned(),
            brief: OpaqueMarkdown::from_text("# Brief\n")?,
            terminology: OpaqueMarkdown::from_text("# Terminology\n")?,
            contracts: vec![(
                "policy".to_owned(),
                OpaqueMarkdown::from_text("# Policy\n")?,
            )],
            gates: Vec::new(),
            remediation_limit: 1,
        })?;
        let runner = AgentRunner::for_current_user();
        let workspace = directory.path();

        for role in [
            Role::Planner,
            Role::Assembler,
            Role::PlanReviewer,
            Role::Implementor,
            Role::PacketReviewer,
            Role::Integrator,
            Role::IntegratedReviewer,
        ] {
            let attempt = runner.prepare_attempt(
                &store,
                &AttemptSpec {
                    run: &run,
                    role,
                    subject_id: role.as_str(),
                    round: 0,
                    targeted: false,
                    prompt: "# Exact prompt\n",
                    workspace: Path::new(workspace),
                    base_commit: None,
                    allowed_scopes: &[],
                    predecessor_attempt_id: None,
                },
            )?;
            let request: JobRequestV1 = serde_json::from_slice(&attempt.request_bytes)?;
            assert_eq!(
                request.invocation.model.as_str(),
                "gpt-5.6-sol",
                "{}",
                role.as_str()
            );
            assert_eq!(
                request.invocation.reasoning_effort,
                Some(ReasoningEffort::Max),
                "{}",
                role.as_str()
            );
            persisted_request(&attempt)?;
        }
        Ok(())
    }
}
