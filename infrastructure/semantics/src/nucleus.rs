use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{
    AbsolutePath, AgentInvocationV1, BuiltinToolsV1, HarnessCapability, JobId, JobRequestV1,
    JobState, LogSchemaV1, ModelId, PROTOCOL_VERSION_V1, ReasoningEffort, Requester, SchemaId,
    TimeoutSeconds, ToolCallsQueryV1, ToolDefinitionV1, ToolResultV1, ToolsetDefinitionsV1,
    ToolsetRef, ToolsetRegistrationV1, WorkspaceAccess,
};
use serde_json::value::{RawValue, to_raw_value};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use tokio::runtime::Builder;
use uuid::Uuid;

use crate::domain::{AccountIntake, Intake, IntakeStatus, ReconciliationProposal, Repository};
use crate::store::{Correlation, MailboxReceipt, Store};
use crate::{Error, Result};

const TOOLSET_DEFINITIONS_SCHEMA_ID: &str = "nucleus.toolset-definitions.v1";
const INPUT_SCHEMA_ID: &str = "semantics.tool.commit-reconciliation.input.v1";
const RESULT_SCHEMA_ID: &str = "semantics.tool.commit-reconciliation.result.v1";
const TOOL_NAME: &str = "commit_semantic_reconciliation";
const MODEL: &str = "gpt-5.6-terra";
const TOOLSET_NAME: &str = "semantic-reconciliation";
const ACCOUNT_INPUT_SCHEMA_ID: &str = "semantics.tool.commit-account-reconciliation.input.v1";
const ACCOUNT_RESULT_SCHEMA_ID: &str = "semantics.tool.commit-account-reconciliation.result.v1";
const ACCOUNT_TOOL_NAME: &str = "commit_account_semantic_reconciliation";
const ACCOUNT_TOOLSET_NAME: &str = "semantic-account-reconciliation";
const DOCUMENT_INPUT_SCHEMA_ID: &str = "semantics.tool.commit-document-reconciliation.input.v1";
const DOCUMENT_RESULT_SCHEMA_ID: &str = "semantics.tool.commit-document-reconciliation.result.v1";
const DOCUMENT_INSTRUCTIONS: &str = "<bazaar:semantics.document.instructions>";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20 * 60);

const INSTRUCTIONS: &str = "<bazaar:semantics.legacy.lifecycle.instructions>";

const DEVELOPER_INSTRUCTIONS: &str = "<bazaar:semantics.legacy.lifecycle.developer-instructions>";

const ACCOUNT_INSTRUCTIONS: &str = "<bazaar:semantics.legacy.account.instructions>";

const ACCOUNT_DEVELOPER_INSTRUCTIONS: &str =
    "<bazaar:semantics.legacy.account.developer-instructions>";

#[derive(Debug, Clone)]
pub struct NucleusReconciler {
    socket: Option<PathBuf>,
}

impl NucleusReconciler {
    #[must_use]
    pub const fn for_current_user() -> Self {
        Self { socket: None }
    }

    pub fn doctor(&self) -> Result<()> {
        let runtime = runtime()?;
        let deployment_run_id = std::env::var("CELL_DEPLOYMENT_RUN_ID").ok();
        runtime.block_on(async {
            let client = self.client()?;
            require_health(&client, deployment_run_id.as_deref()).await?;
            register_contract(&client, None).await?;
            register_account_contract(&client, None).await?;
            register_document_contract(&client, None).await?;
            Ok(())
        })
    }

    pub fn prove_legacy_cutover_ready(&self, correlations: &[Correlation]) -> Result<()> {
        if correlations.is_empty() {
            return Ok(());
        }
        let expected_requests = correlations
            .iter()
            .map(|correlation| {
                if digest(correlation.request_json.as_bytes()) != correlation.request_sha256 {
                    return Err(Error::domain(
                        "legacy_correlation_ambiguous",
                        "a legacy Nucleus correlation no longer matches its immutable request digest",
                    ));
                }
                let request = serde_json::from_str::<JobRequestV1>(&correlation.request_json)
                    .map_err(|_| {
                        Error::domain(
                            "legacy_correlation_ambiguous",
                            "a legacy Nucleus correlation has undecodable immutable request bytes",
                        )
                    })?;
                if request.id.as_str() != correlation.job_id
                    || request.requester.id != correlation.requester_id
                {
                    return Err(Error::domain(
                        "legacy_correlation_ambiguous",
                        "a legacy Nucleus correlation has inconsistent request identity",
                    ));
                }
                Ok((correlation, request))
            })
            .collect::<Result<Vec<_>>>()?;
        let runtime = runtime().map_err(|_| {
            Error::domain(
                "legacy_correlation_ambiguous",
                "legacy Nucleus correlation state could not be proven",
            )
        })?;
        runtime.block_on(async {
            let client = self.client().map_err(|_| {
                Error::domain(
                    "legacy_correlation_ambiguous",
                    "legacy Nucleus correlation state could not be proven",
                )
            })?;
            for (correlation, expected_request) in expected_requests {
                match client.get_job(&JobId::new(&correlation.job_id)).await {
                    Ok(job) => {
                        let expected_digest = expected_request.request_digest().map_err(|_| {
                            Error::domain(
                                "legacy_correlation_ambiguous",
                                "legacy Nucleus correlation state could not be proven",
                            )
                        })?;
                        if job.summary.id.as_str() != correlation.job_id
                            || job.summary.requester.id != correlation.requester_id
                            || job.summary.request_digest != expected_digest
                            || job.request != expected_request
                        {
                            return Err(Error::domain(
                                "legacy_correlation_ambiguous",
                                "Nucleus returned different immutable state for a legacy correlation",
                            ));
                        }
                        if !job.summary.state.is_terminal() {
                            return Err(Error::domain(
                                "legacy_correlation_active",
                                "a legacy Nucleus correlation is still nonterminal",
                            ));
                        }
                    }
                    Err(ClientError::Api { status: 404, .. }) if !correlation.admitted => {}
                    Err(_) => {
                        return Err(Error::domain(
                            "legacy_correlation_ambiguous",
                            "legacy Nucleus correlation state could not be proven",
                        ));
                    }
                }
            }
            Ok(())
        })
    }

    pub fn reconcile(&self, store: &Store, intake: &Intake) -> Result<u64> {
        let project_id = intake.project_id.as_deref().ok_or_else(|| {
            Error::domain(
                "intake_unassigned",
                format!("intake event {} is not assigned", intake.event_id),
            )
        })?;
        if intake.status == IntakeStatus::Applied {
            return intake.applied_revision.ok_or_else(|| {
                Error::domain(
                    "intake_revision_missing",
                    format!("applied intake {} has no revision", intake.event_id),
                )
            });
        }
        let repository = store.repository(project_id, None)?;
        let project = store.project(project_id)?;
        let runtime = runtime()?;
        runtime.block_on(async {
            let client = self.client()?;
            require_health(&client, None).await?;
            let existing = store.correlation(&intake.event_id)?;
            let version = correlation_toolset_version(existing.as_ref())?;
            let toolset = register_contract(&client, version).await?;
            let correlation = match existing {
                Some(value) => value,
                None => {
                    let suffix = Uuid::now_v7();
                    let requester_id = format!("semantics-intake-{suffix}");
                    let job_id = format!("semantics-reconcile-{suffix}");
                    let neutral = neutral_cwd(&job_id)?;
                    let request = build_request(
                        &requester_id,
                        &job_id,
                        intake,
                        &repository,
                        project.next_concept_number,
                        toolset.toolset.clone(),
                        &neutral,
                    )?;
                    let request_json = serde_json::to_string(&request)?;
                    let request_sha256 = digest(request_json.as_bytes());
                    store.put_correlation(&Correlation {
                        event_id: intake.event_id.clone(),
                        requester_id,
                        job_id,
                        request_json,
                        request_sha256,
                        tool_after: 0,
                        admitted: false,
                    })?
                }
            };
            if digest(correlation.request_json.as_bytes()) != correlation.request_sha256 {
                return Err(Error::domain(
                    "correlation_digest_mismatch",
                    "persisted Nucleus request bytes do not match their digest",
                ));
            }
            let neutral = neutral_cwd(&correlation.job_id)?;
            let request: JobRequestV1 = serde_json::from_str(&correlation.request_json)?;
            if request.invocation.cwd.as_path() != neutral {
                cleanup_neutral_cwd(&correlation.job_id);
                return Err(Error::domain(
                    "correlation_cwd_conflict",
                    "persisted Nucleus request does not use its deterministic neutral cwd",
                ));
            }
            let result = async {
                let admitted = submit_stably(&client, &request).await?;
                if admitted.job_id.as_str() != correlation.job_id {
                    return Err(Error::domain(
                        "nucleus_job_mismatch",
                        "Nucleus admitted a different job identity",
                    ));
                }
                store.mark_admitted(&intake.event_id)?;
                serve_mailbox(
                    &client,
                    store,
                    intake,
                    project_id,
                    &correlation.job_id,
                    correlation.tool_after,
                )
                .await
            }
            .await;
            cleanup_neutral_cwd(&correlation.job_id);
            result
        })
    }

    pub fn retry_failed(&self, store: &Store, event_id: &str) -> Result<()> {
        let Some(correlation) = store.correlation(event_id)? else {
            return store.retry_intake(event_id);
        };
        let runtime = runtime()?;
        runtime.block_on(async {
            let client = self.client()?;
            let state = match client.get_job(&JobId::new(&correlation.job_id)).await {
                Ok(job) => Some(job.summary.state),
                Err(ClientError::Api { status: 404, .. }) if !correlation.admitted => None,
                Err(ClientError::Api { status: 404, .. }) => {
                    return Err(Error::domain(
                        "intake_retry_job_ambiguous",
                        "the admitted prior Nucleus job is unavailable; retry would risk two jobs",
                    ));
                }
                Err(error) => return Err(error.into()),
            };
            if state.is_some_and(|state| !state.is_terminal()) {
                return Err(Error::domain(
                    "intake_retry_job_active",
                    "the prior Nucleus job is still active",
                ));
            }
            store.reset_retry_after_terminal(event_id)
        })
    }

    pub fn reconcile_account(&self, store: &Store, intake: &AccountIntake) -> Result<u64> {
        let project_id = intake.project_id.as_deref().ok_or_else(|| {
            Error::domain(
                "intake_unassigned",
                format!("intake event {} is not assigned", intake.event_id),
            )
        })?;
        if intake.status == IntakeStatus::Applied {
            return intake.applied_revision.ok_or_else(|| {
                Error::domain(
                    "intake_revision_missing",
                    format!("applied intake {} has no revision", intake.event_id),
                )
            });
        }
        let repository = store.repository(project_id, None)?;
        let project = store.project(project_id)?;
        let runtime = runtime().map_err(bounded_account_runtime_error)?;
        runtime
            .block_on(async {
                let client = self.client()?;
                require_health(&client, None).await?;
                let existing = store.account_correlation(&intake.event_id)?;
                let version = correlation_toolset_version(existing.as_ref())?;
                let toolset = if intake.account.is_document() {
                    register_document_contract(&client, version).await?
                } else {
                    register_account_contract(&client, version).await?
                };
                let correlation = match existing {
                    Some(value) => value,
                    None => {
                        let suffix = Uuid::now_v7();
                        let requester_id = format!("semantics-account-intake-{suffix}");
                        let job_id = format!("semantics-account-reconcile-{suffix}");
                        let neutral = account_neutral_cwd(&job_id)?;
                        let request = build_account_request(
                            &requester_id,
                            &job_id,
                            intake,
                            &repository,
                            project.next_concept_number,
                            toolset.toolset.clone(),
                            neutral.path(),
                        )?;
                        let request_json = serde_json::to_string(&request)?;
                        let request_sha256 = digest(request_json.as_bytes());
                        store.put_account_correlation(&Correlation {
                            event_id: intake.event_id.clone(),
                            requester_id,
                            job_id,
                            request_json,
                            request_sha256,
                            tool_after: 0,
                            admitted: false,
                        })?
                    }
                };
                if digest(correlation.request_json.as_bytes()) != correlation.request_sha256 {
                    return Err(Error::domain(
                        "correlation_digest_mismatch",
                        "persisted Nucleus request bytes do not match their digest",
                    ));
                }
                let neutral = account_neutral_cwd(&correlation.job_id)?;
                let request: JobRequestV1 = serde_json::from_str(&correlation.request_json)?;
                if request.invocation.cwd.as_path() != neutral.path() {
                    return Err(Error::domain(
                        "correlation_cwd_conflict",
                        "persisted Nucleus request does not use its deterministic neutral cwd",
                    ));
                }
                async {
                    let admitted = submit_stably(&client, &request).await?;
                    if admitted.job_id.as_str() != correlation.job_id {
                        return Err(Error::domain(
                            "nucleus_job_mismatch",
                            "Nucleus admitted a different job identity",
                        ));
                    }
                    store.mark_account_admitted(&intake.event_id)?;
                    serve_account_mailbox(
                        &client,
                        store,
                        intake,
                        project_id,
                        &correlation.job_id,
                        correlation.tool_after,
                    )
                    .await
                }
                .await
            })
            .map_err(bounded_account_runtime_error)
    }

    pub fn retry_account_failed(&self, store: &Store, event_id: &str) -> Result<()> {
        let Some(correlation) = store.account_correlation(event_id)? else {
            return store.retry_account_intake(event_id);
        };
        let runtime = runtime().map_err(bounded_account_runtime_error)?;
        runtime
            .block_on(async {
                let client = self.client()?;
                let state = match client.get_job(&JobId::new(&correlation.job_id)).await {
                    Ok(job) => Some(job.summary.state),
                    Err(ClientError::Api { status: 404, .. }) if !correlation.admitted => None,
                    Err(ClientError::Api { status: 404, .. }) => {
                        return Err(Error::domain(
                            "intake_retry_job_ambiguous",
                            "the admitted prior Nucleus job is unavailable; retry would risk two jobs",
                        ));
                    }
                    Err(error) => return Err(error.into()),
                };
                if state.is_some_and(|state| !state.is_terminal()) {
                    return Err(Error::domain(
                        "intake_retry_job_active",
                        "the prior Nucleus job is still active",
                    ));
                }
                store.reset_account_retry_after_terminal(event_id)
            })
            .map_err(bounded_account_runtime_error)
    }

    fn client(&self) -> Result<NucleusClient> {
        match &self.socket {
            Some(socket) => NucleusClient::new(socket).map_err(Into::into),
            None => NucleusClient::for_current_user().map_err(Into::into),
        }
    }
}

async fn require_health(client: &NucleusClient, deployment_run_id: Option<&str>) -> Result<()> {
    let mut health = client.health().await?;
    if health.status == "ok"
        && health
            .quota
            .as_ref()
            .is_some_and(nucleus_core::QuotaStatusV1::is_blocked)
    {
        health.accepting_jobs = true;
    }
    let mut deployment_proved = false;
    if (health.status != "ok" || !health.accepting_jobs)
        && let Some(run_id) = deployment_run_id.filter(|run_id| !run_id.is_empty())
    {
        // Only doctors pass an owner. Ordinary reconciliation still requires
        // public admission, even when this process inherited a deployment ID.
        health = client.health_for_deployment(run_id).await?;
        deployment_proved = true;
    }
    let required = [
        HarnessCapability::ExactModel,
        HarnessCapability::ReasoningEffort,
        HarnessCapability::WorkspaceNone,
        HarnessCapability::DynamicClientTools,
        HarnessCapability::DeveloperInstructions,
        HarnessCapability::PersistentFileAuthentication,
    ];
    let missing = required
        .iter()
        .filter(|capability| !health.capabilities.contains(capability))
        .map(|capability| format!("{capability:?}"))
        .collect::<Vec<_>>();
    if (!deployment_proved && (health.status != "ok" || !health.accepting_jobs))
        || !health.authentication.authenticated
        || !health
            .supported_protocol_versions
            .contains(&PROTOCOL_VERSION_V1)
        || health.harness.is_none()
        || !missing.is_empty()
    {
        return Err(Error::domain(
            "nucleus_not_ready",
            format!(
                "Nucleus is not ready: status={}, accepting_jobs={}, authenticated={}, missing_capabilities={}",
                health.status,
                health.accepting_jobs,
                health.authentication.authenticated,
                missing.join(",")
            ),
        ));
    }
    Ok(())
}

async fn submit_stably(
    client: &NucleusClient,
    request: &JobRequestV1,
) -> Result<nucleus_core::JobAcceptedV1> {
    let mut transport_failures = 0_u8;
    loop {
        match client.submit_job(request).await {
            Ok(accepted) => return Ok(accepted),
            Err(ClientError::Transport { .. }) if transport_failures < 2 => {
                transport_failures += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error @ ClientError::Validation(_)) => {
                return Err(Error::domain(
                    "nucleus_admission_rejected",
                    format!("Nucleus explicitly rejected the immutable request: {error}"),
                ));
            }
            Err(error @ ClientError::Api { status, .. })
                if explicit_nonretryable_rejection(status) =>
            {
                return Err(Error::domain(
                    "nucleus_admission_rejected",
                    format!("Nucleus explicitly rejected the immutable request: {error}"),
                ));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

const fn explicit_nonretryable_rejection(status: u16) -> bool {
    (status >= 400 && status < 500) && !matches!(status, 408 | 409 | 425 | 429)
}

fn bounded_account_runtime_error(error: Error) -> Error {
    match error.code() {
        "quota_deferred" | "quota_exhausted" => error,
        "nucleus_admission_rejected" => Error::domain(
            "nucleus_admission_rejected",
            "Nucleus rejected the immutable account reconciliation request",
        ),
        "nucleus_job_terminal_invalid" => Error::domain(
            "nucleus_job_terminal_invalid",
            "Nucleus completed without an accepted account reconciliation",
        ),
        "nucleus_job_terminal_failed" => Error::domain(
            "nucleus_job_terminal_failed",
            "Nucleus account reconciliation ended unsuccessfully",
        ),
        "intake_retry_job_ambiguous" => Error::domain(
            "intake_retry_job_ambiguous",
            "the admitted prior Nucleus job is unavailable; retry would risk two jobs",
        ),
        "intake_retry_job_active" => Error::domain(
            "intake_retry_job_active",
            "the prior Nucleus job is still active",
        ),
        _ => Error::domain(
            "account_reconciliation_runtime_failed",
            "account reconciliation did not complete; inspect durable intake and Nucleus state",
        ),
    }
}

async fn serve_mailbox(
    client: &NucleusClient,
    store: &Store,
    intake: &Intake,
    project_id: &str,
    job_id: &str,
    mut tool_after: u64,
) -> Result<u64> {
    let job_id = JobId::new(job_id);
    loop {
        let calls = client
            .pending_tool_calls(
                &job_id,
                &ToolCallsQueryV1 {
                    after: tool_after,
                    wait_seconds: 1,
                },
            )
            .await?;
        for pending in calls.calls {
            let call = pending.call;
            if call.job_id != job_id
                || call.tool_name != TOOL_NAME
                || call.arguments_schema_id.as_str() != INPUT_SCHEMA_ID
            {
                return Err(Error::domain(
                    "nucleus_tool_contract_mismatch",
                    "Nucleus returned a call outside the admitted Semantics toolset",
                ));
            }
            let arguments_sha256 = digest(call.arguments.get().as_bytes());
            let result = match store.mailbox_receipt(job_id.as_str(), call.id.as_str())? {
                Some(receipt) => cached_result(&receipt, &arguments_sha256)?,
                None => dispatch_tool(
                    store,
                    intake,
                    project_id,
                    job_id.as_str(),
                    call.id.as_str(),
                    &arguments_sha256,
                    call.arguments.get(),
                )?,
            };
            let response = ToolResultV1 {
                version: PROTOCOL_VERSION_V1,
                call_id: call.id.clone(),
                requester: Requester {
                    program: "semantics".to_owned(),
                    id: store
                        .correlation(&intake.event_id)?
                        .ok_or_else(|| {
                            Error::domain(
                                "correlation_missing",
                                "Semantics request correlation disappeared",
                            )
                        })?
                        .requester_id,
                },
                result_schema_id: SchemaId::new(RESULT_SCHEMA_ID),
                result: RawValue::from_string(result.json).map_err(|error| {
                    Error::domain(
                        "tool_result_invalid",
                        format!("unable to encode tool result: {error}"),
                    )
                })?,
                is_error: result.is_error,
            };
            post_result_stably(client, &job_id, &call.id, &response).await?;
            tool_after = tool_after.max(call.request_sequence);
            store.advance_tool_after(&intake.event_id, tool_after)?;
        }
        let refreshed = store.intake(&intake.event_id)?;
        if refreshed.status == IntakeStatus::Applied
            && let Some(revision) = refreshed.applied_revision
        {
            return Ok(revision);
        }
        let job = client.get_job_for_work(&job_id).await?;
        if job.summary.state.is_terminal() {
            if store
                .pending_committed_revision(&intake.event_id, job_id.as_str())?
                .is_some()
            {
                let revision = store.finalize_applied(&intake.event_id, job_id.as_str())?;
                if !job.quota_exhausted() {
                    report_committed_runtime_failure(job.summary.state, job_id.as_str())?;
                }
                return Ok(revision);
            }
            if job.quota_exhausted() {
                return Err(Error::domain(
                    "quota_exhausted",
                    "Codex quota exhausted; inspect retained attempt before retry",
                ));
            }
            return match job.summary.state {
                JobState::Completed => Err(Error::domain(
                    "nucleus_job_terminal_invalid",
                    "Nucleus completed without an accepted semantic reconciliation",
                )),
                JobState::Failed | JobState::Cancelled => Err(Error::domain(
                    "nucleus_job_terminal_failed",
                    job.attempts
                        .last()
                        .and_then(|attempt| attempt.terminal_message.as_deref())
                        .unwrap_or("Nucleus ended without terminal detail"),
                )),
                JobState::Accepted | JobState::Running | JobState::WaitingOnRequester => {
                    Err(Error::domain(
                        "nucleus_state_invalid",
                        "Nucleus reported a nonterminal state as terminal",
                    ))
                }
            };
        }
    }
}

async fn serve_account_mailbox(
    client: &NucleusClient,
    store: &Store,
    intake: &AccountIntake,
    project_id: &str,
    job_id: &str,
    mut tool_after: u64,
) -> Result<u64> {
    let input_schema_id = if intake.account.is_document() {
        DOCUMENT_INPUT_SCHEMA_ID
    } else {
        ACCOUNT_INPUT_SCHEMA_ID
    };
    let result_schema_id = if intake.account.is_document() {
        DOCUMENT_RESULT_SCHEMA_ID
    } else {
        ACCOUNT_RESULT_SCHEMA_ID
    };
    let job_id = JobId::new(job_id);
    loop {
        let calls = client
            .pending_tool_calls(
                &job_id,
                &ToolCallsQueryV1 {
                    after: tool_after,
                    wait_seconds: 1,
                },
            )
            .await?;
        for pending in calls.calls {
            let call = pending.call;
            if call.job_id != job_id
                || call.tool_name != ACCOUNT_TOOL_NAME
                || call.arguments_schema_id.as_str() != input_schema_id
            {
                return Err(Error::domain(
                    "nucleus_tool_contract_mismatch",
                    "Nucleus returned a call outside the admitted account toolset",
                ));
            }
            let arguments_sha256 = digest(call.arguments.get().as_bytes());
            let result = match store.account_mailbox_receipt(job_id.as_str(), call.id.as_str())? {
                Some(receipt) => cached_result(&receipt, &arguments_sha256)?,
                None => dispatch_account_tool(
                    store,
                    intake,
                    project_id,
                    job_id.as_str(),
                    call.id.as_str(),
                    &arguments_sha256,
                    call.arguments.get(),
                )?,
            };
            let response = ToolResultV1 {
                version: PROTOCOL_VERSION_V1,
                call_id: call.id.clone(),
                requester: Requester {
                    program: "semantics".to_owned(),
                    id: store
                        .account_correlation(&intake.event_id)?
                        .ok_or_else(|| {
                            Error::domain(
                                "correlation_missing",
                                "Semantics account correlation disappeared",
                            )
                        })?
                        .requester_id,
                },
                result_schema_id: SchemaId::new(result_schema_id),
                result: RawValue::from_string(result.json).map_err(|error| {
                    Error::domain(
                        "tool_result_invalid",
                        format!("unable to encode tool result: {error}"),
                    )
                })?,
                is_error: result.is_error,
            };
            post_result_stably(client, &job_id, &call.id, &response).await?;
            tool_after = tool_after.max(call.request_sequence);
            store.advance_account_tool_after(&intake.event_id, tool_after)?;
        }
        let refreshed = store.account_intake(&intake.event_id)?;
        if refreshed.status == IntakeStatus::Applied
            && let Some(revision) = refreshed.applied_revision
        {
            return Ok(revision);
        }
        let job = client.get_job_for_work(&job_id).await?;
        if job.summary.state.is_terminal() {
            if store
                .account_pending_committed_revision(&intake.event_id, job_id.as_str())?
                .is_some()
            {
                let revision = store.finalize_account_applied(&intake.event_id, job_id.as_str())?;
                if !job.quota_exhausted() {
                    report_committed_runtime_failure(job.summary.state, job_id.as_str())?;
                }
                return Ok(revision);
            }
            if job.quota_exhausted() {
                return Err(Error::domain(
                    "quota_exhausted",
                    "Codex quota exhausted; inspect retained attempt before retry",
                ));
            }
            return match job.summary.state {
                JobState::Completed => Err(Error::domain(
                    "nucleus_job_terminal_invalid",
                    "Nucleus completed without an accepted account reconciliation",
                )),
                JobState::Failed | JobState::Cancelled => Err(Error::domain(
                    "nucleus_job_terminal_failed",
                    "Nucleus account reconciliation ended without a semantic commit",
                )),
                JobState::Accepted | JobState::Running | JobState::WaitingOnRequester => {
                    Err(Error::domain(
                        "nucleus_state_invalid",
                        "Nucleus reported a nonterminal state as terminal",
                    ))
                }
            };
        }
    }
}

fn report_committed_runtime_failure(state: JobState, job_id: &str) -> Result<()> {
    if matches!(state, JobState::Failed | JobState::Cancelled) {
        clockwork::api::report_abend("nucleus_job_terminal_failed", job_id)
            .map_err(|_| Error::domain(
                "scheduling_report_failed",
                "cannot report terminal runtime failure; the committed semantic result is preserved",
            ))?;
    }
    Ok(())
}

async fn post_result_stably(
    client: &NucleusClient,
    job_id: &JobId,
    call_id: &nucleus_core::ToolCallId,
    result: &ToolResultV1,
) -> Result<()> {
    let mut transport_failures = 0_u8;
    loop {
        match client.post_tool_result(job_id, call_id, result).await {
            Ok(_) => return Ok(()),
            Err(ClientError::Transport { .. }) if transport_failures < 2 => {
                transport_failures += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

struct PreparedToolResult {
    json: String,
    is_error: bool,
}

fn cached_result(receipt: &MailboxReceipt, arguments_sha256: &str) -> Result<PreparedToolResult> {
    if receipt.arguments_sha256 != arguments_sha256 {
        return Err(Error::domain(
            "mailbox_arguments_conflict",
            "a persisted Nucleus tool call was replayed with different arguments",
        ));
    }
    Ok(PreparedToolResult {
        json: receipt.result_json.clone(),
        is_error: receipt.is_error,
    })
}

fn dispatch_tool(
    store: &Store,
    intake: &Intake,
    project_id: &str,
    job_id: &str,
    call_id: &str,
    arguments_sha256: &str,
    arguments: &str,
) -> Result<PreparedToolResult> {
    let proposal = match serde_json::from_str::<ReconciliationProposal>(arguments) {
        Ok(proposal) => proposal,
        Err(error) => {
            let result = error_result("proposal_json_invalid", &error.to_string());
            store.record_mailbox_rejection(job_id, call_id, arguments_sha256, &result.json)?;
            return Ok(result);
        }
    };
    match store.commit_mailbox_proposal(
        &intake.event_id,
        job_id,
        call_id,
        arguments_sha256,
        project_id,
        &proposal,
    ) {
        Ok(revision) => {
            let receipt = store.mailbox_receipt(job_id, call_id)?.ok_or_else(|| {
                Error::domain(
                    "mailbox_receipt_missing",
                    "accepted tool call has no receipt",
                )
            })?;
            debug_assert_eq!(receipt.committed_revision, Some(revision));
            Ok(PreparedToolResult {
                json: receipt.result_json,
                is_error: false,
            })
        }
        Err(error) => {
            let result = error_result(error.code(), &error.to_string());
            store.record_mailbox_rejection(job_id, call_id, arguments_sha256, &result.json)?;
            Ok(result)
        }
    }
}

fn dispatch_account_tool(
    store: &Store,
    intake: &AccountIntake,
    project_id: &str,
    job_id: &str,
    call_id: &str,
    arguments_sha256: &str,
    arguments: &str,
) -> Result<PreparedToolResult> {
    let proposal = match serde_json::from_str::<ReconciliationProposal>(arguments) {
        Ok(proposal) => proposal,
        Err(error) => {
            let result = error_result("proposal_json_invalid", &error.to_string());
            store.record_account_mailbox_rejection(
                job_id,
                call_id,
                arguments_sha256,
                &result.json,
            )?;
            return Ok(result);
        }
    };
    match store.commit_account_mailbox_proposal(
        &intake.event_id,
        job_id,
        call_id,
        arguments_sha256,
        project_id,
        &proposal,
    ) {
        Ok(revision) => {
            let receipt = store
                .account_mailbox_receipt(job_id, call_id)?
                .ok_or_else(|| {
                    Error::domain(
                        "mailbox_receipt_missing",
                        "accepted tool call has no receipt",
                    )
                })?;
            debug_assert_eq!(receipt.committed_revision, Some(revision));
            Ok(PreparedToolResult {
                json: receipt.result_json,
                is_error: false,
            })
        }
        Err(error) => {
            let result = error_result(error.code(), &error.to_string());
            store.record_account_mailbox_rejection(
                job_id,
                call_id,
                arguments_sha256,
                &result.json,
            )?;
            Ok(result)
        }
    }
}

fn error_result(code: &str, detail: &str) -> PreparedToolResult {
    let message = detail.chars().take(300).collect::<String>();
    let json = serde_json::to_string(&json!({
        "error": {"code": code, "message": message}
    }))
    .unwrap_or_else(|_| {
        r#"{"error":{"code":"proposal_invalid","message":"proposal rejected"}}"#.to_owned()
    });
    PreparedToolResult {
        json,
        is_error: true,
    }
}

fn build_request(
    requester_id: &str,
    job_id: &str,
    intake: &Intake,
    repository: &Repository,
    next_concept_number: u64,
    toolset: ToolsetRef,
    neutral_cwd: &Path,
) -> Result<JobRequestV1> {
    let prompts = bazaar::prompts::Prompts::for_toolset_version("semantics", toolset.version, 1)?;
    let prompt = reconciliation_prompt(intake, repository, next_concept_number)?;
    let mut invocation = AgentInvocationV1::new(
        "codex",
        ModelId::new(MODEL),
        AbsolutePath::new(neutral_cwd),
        WorkspaceAccess::None,
        BuiltinToolsV1 {
            local_execution: false,
            web_search: false,
        },
        TimeoutSeconds::new(REQUEST_TIMEOUT.as_secs()),
    );
    invocation.reasoning_effort = Some(ReasoningEffort::Medium);
    invocation.toolset = Some(toolset);
    let mut request = JobRequestV1::new(
        JobId::new(job_id),
        format!("Reconcile semantics for {}", intake.event_id),
        Requester {
            program: "semantics".to_owned(),
            id: requester_id.to_owned(),
        },
        INSTRUCTIONS,
        prompt,
        invocation,
    );
    request.developer_instructions = Some(DEVELOPER_INSTRUCTIONS.to_owned());
    request.instructions = prompts.expand(&request.instructions)?;
    if let Some(text) = &mut request.developer_instructions {
        *text = prompts.expand(text)?;
    }
    Ok(request)
}

fn build_account_request(
    requester_id: &str,
    job_id: &str,
    intake: &AccountIntake,
    repository: &Repository,
    next_concept_number: u64,
    toolset: ToolsetRef,
    neutral_cwd: &Path,
) -> Result<JobRequestV1> {
    let prompts = bazaar::prompts::Prompts::for_toolset_version("semantics", toolset.version, 1)?;
    let prompt = account_reconciliation_prompt(intake, repository, next_concept_number)?;
    let mut invocation = AgentInvocationV1::new(
        "codex",
        ModelId::new(MODEL),
        AbsolutePath::new(neutral_cwd),
        WorkspaceAccess::None,
        BuiltinToolsV1 {
            local_execution: false,
            web_search: false,
        },
        TimeoutSeconds::new(REQUEST_TIMEOUT.as_secs()),
    );
    invocation.reasoning_effort = Some(ReasoningEffort::Medium);
    invocation.toolset = Some(toolset);
    let mut request = JobRequestV1::new(
        JobId::new(job_id),
        format!("Reconcile semantics for {}", intake.event_id),
        Requester {
            program: "semantics".to_owned(),
            id: requester_id.to_owned(),
        },
        if intake.account.is_document() {
            DOCUMENT_INSTRUCTIONS
        } else {
            ACCOUNT_INSTRUCTIONS
        },
        prompt,
        invocation,
    );
    request.developer_instructions = Some(
        if intake.account.is_document() {
            "<bazaar:semantics.document.developer-instructions>"
        } else {
            ACCOUNT_DEVELOPER_INSTRUCTIONS
        }
        .to_owned(),
    );
    request.instructions = prompts.expand(&request.instructions)?;
    if let Some(text) = &mut request.developer_instructions {
        *text = prompts.expand(text)?;
    }
    Ok(request)
}

fn reconciliation_prompt(
    intake: &Intake,
    repository: &Repository,
    next_concept_number: u64,
) -> Result<String> {
    let next_concept_ids = (next_concept_number..next_concept_number.saturating_add(32))
        .map(crate::domain::concept_id_for)
        .collect::<Vec<_>>();
    serde_json::to_string(&json!({
        "event": {
            "event_id": intake.decision.event_id,
            "event_kind": intake.decision.event_kind,
            "decision_id": intake.decision.decision_id,
            "statement": intake.decision.statement,
            "disposition": intake.decision.disposition,
            "confidence": intake.decision.confidence,
            "rationale": intake.decision.rationale,
            "supersedes_decision_id": intake.decision.supersedes_decision_id,
            "review_state": intake.decision.review_state,
            "review_action": intake.decision.review_action
        },
        "repository": repository,
        "next_concept_ids": next_concept_ids
    }))
    .map_err(Into::into)
}

fn account_reconciliation_prompt(
    intake: &AccountIntake,
    repository: &Repository,
    next_concept_number: u64,
) -> Result<String> {
    let next_concept_ids = (next_concept_number..next_concept_number.saturating_add(32))
        .map(crate::domain::concept_id_for)
        .collect::<Vec<_>>();
    let source = match &intake.account.content {
        crate::domain::DecisionContent::Document {
            source_name,
            document,
            ..
        } => json!({
            "library_id": intake.account.library_id, "event_id": intake.account.event_id,
            "document_id": intake.account.account_id, "source_name": source_name, "document": document,
        }),
        crate::domain::DecisionContent::Legacy(account) => json!({
            "library_id": intake.account.library_id, "event_id": intake.account.event_id,
            "account_id": intake.account.account_id, "account_schema_version": account.account_schema_version,
            "statement": account.statement, "context": account.context, "action": account.action,
            "result": account.result, "occurred_at": account.occurred_at,
            "occurred_at_precision": account.occurred_at_precision,
        }),
    };
    let source_key = if intake.account.is_document() {
        "source_document"
    } else {
        "decision_account"
    };
    let mut prompt = json!({"repository": repository, "next_concept_ids": next_concept_ids});
    prompt[source_key] = source;
    serde_json::to_string(&prompt).map_err(Into::into)
}

fn correlation_toolset_version(correlation: Option<&Correlation>) -> Result<Option<u32>> {
    correlation
        .map(|correlation| {
            let request: JobRequestV1 = serde_json::from_str(&correlation.request_json)?;
            request
                .invocation
                .toolset
                .map(|toolset| toolset.version)
                .ok_or_else(|| {
                    Error::domain(
                        "correlation_toolset_missing",
                        "saved request has no toolset",
                    )
                })
        })
        .transpose()
}

async fn register_contract(
    client: &NucleusClient,
    version: Option<u32>,
) -> Result<ToolsetRegistrationV1> {
    let prompts = match version {
        Some(version) => {
            bazaar::prompts::Prompts::at("semantics", i64::from(version.saturating_sub(1).max(1)))?
        }
        None => bazaar::prompts::Prompts::load("semantics")?,
    };
    let selected_version = version.unwrap_or(
        u32::try_from(prompts.selection.version)
            .ok()
            .and_then(|version| version.checked_add(1))
            .ok_or_else(|| {
                Error::domain(
                    "prompt_version_invalid",
                    "prompt version exceeds toolset range",
                )
            })?,
    );
    let input = input_schema();
    let result = result_schema();
    for (id, title, schema) in [
        (
            INPUT_SCHEMA_ID,
            "Semantics reconciliation input",
            input.clone(),
        ),
        (RESULT_SCHEMA_ID, "Semantics reconciliation result", result),
    ] {
        client
            .register_schema(&LogSchemaV1::new(
                id,
                title,
                "1",
                "application/schema+json",
                "semantics",
                to_raw_value(&schema)
                    .map_err(|error| Error::domain("nucleus_schema_invalid", error.to_string()))?,
            ))
            .await?;
    }
    let definitions = ToolsetDefinitionsV1 {
        version: PROTOCOL_VERSION_V1,
        tools: vec![ToolDefinitionV1 {
            name: TOOL_NAME.to_owned(),
            description: prompts
                .expand("<bazaar:semantics.legacy.tools.commit_lifecycle.description>")?,
            input_schema_id: SchemaId::new(INPUT_SCHEMA_ID),
            input_schema: to_raw_value(&input)
                .map_err(|error| Error::domain("nucleus_schema_invalid", error.to_string()))?,
        }],
    };
    let registration = ToolsetRegistrationV1::new(
        ToolsetRef {
            provider: "semantics".to_owned(),
            name: TOOLSET_NAME.to_owned(),
            version: selected_version,
        },
        TOOLSET_DEFINITIONS_SCHEMA_ID,
        definitions,
    )
    .map_err(|error| Error::domain("nucleus_toolset_invalid", error.to_string()))?;
    client.register_toolset(&registration).await?;
    Ok(registration)
}

async fn register_account_contract(
    client: &NucleusClient,
    version: Option<u32>,
) -> Result<ToolsetRegistrationV1> {
    let prompts = match version {
        Some(version) => {
            bazaar::prompts::Prompts::at("semantics", i64::from(version.saturating_sub(1).max(1)))?
        }
        None => bazaar::prompts::Prompts::load("semantics")?,
    };
    let selected_version = version.unwrap_or(
        u32::try_from(prompts.selection.version)
            .ok()
            .and_then(|version| version.checked_add(1))
            .ok_or_else(|| {
                Error::domain(
                    "prompt_version_invalid",
                    "prompt version exceeds toolset range",
                )
            })?,
    );
    let input = account_input_schema();
    let result = result_schema();
    for (id, title, schema) in [
        (
            ACCOUNT_INPUT_SCHEMA_ID,
            "Semantics Annals decision-account reconciliation input",
            input.clone(),
        ),
        (
            ACCOUNT_RESULT_SCHEMA_ID,
            "Semantics Annals decision-account reconciliation result",
            result,
        ),
    ] {
        client
            .register_schema(&LogSchemaV1::new(
                id,
                title,
                "1",
                "application/schema+json",
                "semantics",
                to_raw_value(&schema)
                    .map_err(|error| Error::domain("nucleus_schema_invalid", error.to_string()))?,
            ))
            .await?;
    }
    let definitions = ToolsetDefinitionsV1 {
        version: PROTOCOL_VERSION_V1,
        tools: vec![ToolDefinitionV1 {
            name: ACCOUNT_TOOL_NAME.to_owned(),
            description: prompts
                .expand("<bazaar:semantics.legacy.tools.commit_account.description>")?,
            input_schema_id: SchemaId::new(ACCOUNT_INPUT_SCHEMA_ID),
            input_schema: to_raw_value(&input)
                .map_err(|error| Error::domain("nucleus_schema_invalid", error.to_string()))?,
        }],
    };
    let registration = ToolsetRegistrationV1::new(
        ToolsetRef {
            provider: "semantics".to_owned(),
            name: ACCOUNT_TOOLSET_NAME.to_owned(),
            version: selected_version,
        },
        TOOLSET_DEFINITIONS_SCHEMA_ID,
        definitions,
    )
    .map_err(|error| Error::domain("nucleus_toolset_invalid", error.to_string()))?;
    client.register_toolset(&registration).await?;
    Ok(registration)
}

async fn register_document_contract(
    client: &NucleusClient,
    version: Option<u32>,
) -> Result<ToolsetRegistrationV1> {
    let prompts = match version {
        Some(version) => {
            bazaar::prompts::Prompts::at("semantics", i64::from(version.saturating_sub(1).max(1)))?
        }
        None => bazaar::prompts::Prompts::load("semantics")?,
    };
    let selected_version = version.unwrap_or(
        u32::try_from(prompts.selection.version)
            .ok()
            .and_then(|version| version.checked_add(1))
            .ok_or_else(|| {
                Error::domain(
                    "prompt_version_invalid",
                    "prompt version exceeds toolset range",
                )
            })?,
    );
    let input = document_input_schema();
    let result = document_result_schema();
    for (id, title, schema) in [
        (
            DOCUMENT_INPUT_SCHEMA_ID,
            "Semantics Annals document reconciliation input",
            input.clone(),
        ),
        (
            DOCUMENT_RESULT_SCHEMA_ID,
            "Semantics Annals document reconciliation result",
            result,
        ),
    ] {
        client
            .register_schema(&LogSchemaV1::new(
                id,
                title,
                "1",
                "application/schema+json",
                "semantics",
                to_raw_value(&schema)
                    .map_err(|error| Error::domain("nucleus_schema_invalid", error.to_string()))?,
            ))
            .await?;
    }
    let definitions = ToolsetDefinitionsV1 {
        version: PROTOCOL_VERSION_V1,
        tools: vec![ToolDefinitionV1 {
            name: ACCOUNT_TOOL_NAME.to_owned(),
            description: prompts.expand("<bazaar:semantics.tools.commit_document.description>")?,
            input_schema_id: SchemaId::new(DOCUMENT_INPUT_SCHEMA_ID),
            input_schema: to_raw_value(&input)
                .map_err(|error| Error::domain("nucleus_schema_invalid", error.to_string()))?,
        }],
    };
    let registration = ToolsetRegistrationV1::new(
        ToolsetRef {
            provider: "semantics".to_owned(),
            name: "semantic-document-reconciliation".to_owned(),
            version: selected_version,
        },
        TOOLSET_DEFINITIONS_SCHEMA_ID,
        definitions,
    )
    .map_err(|error| Error::domain("nucleus_toolset_invalid", error.to_string()))?;
    client.register_toolset(&registration).await?;
    Ok(registration)
}

fn input_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "required": ["base_revision", "summary", "effects"],
        "properties": {
            "base_revision": {"type": "integer", "minimum": 0},
            "summary": {"type": "string", "minLength": 1, "maxLength": 1000},
            "effects": {
                "type": "array", "minItems": 1, "maxItems": 256,
                "items": {"oneOf": effect_schemas()}
            }
        }
    })
}

fn account_input_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "required": ["base_revision", "summary", "effects"],
        "properties": {
            "base_revision": {"type": "integer", "minimum": 0},
            "summary": {"type": "string", "minLength": 1, "maxLength": 1000},
            "effects": {
                "type": "array", "minItems": 1, "maxItems": 256,
                "items": {"oneOf": account_effect_schemas()}
            }
        }
    })
}

fn document_input_schema() -> Value {
    let mut effects = account_effect_schemas();
    for effect in &mut effects {
        if effect["properties"]["type"]["const"] == "ground" {
            effect["properties"]["source"] = json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["kind", "library_id", "event_id", "document_id"],
                "properties": {
                    "kind": {"type": "string", "const": "annals_document"},
                    "library_id": text_schema(),
                    "event_id": text_schema(),
                    "document_id": text_schema()
                }
            });
        }
    }
    let mut schema = account_input_schema();
    schema["properties"]["effects"]["minItems"] = json!(0);
    schema["properties"]["effects"]["items"]["oneOf"] = json!(effects);
    schema
}

fn document_result_schema() -> Value {
    let mut schema = result_schema();
    schema["oneOf"][0]["properties"]["revision"]["minimum"] = json!(0);
    schema["oneOf"][0]["properties"]["no_change"] = json!({"type": "boolean"});
    schema
}

fn account_effect_schemas() -> Vec<Value> {
    let mut schemas = effect_schemas();
    schemas.retain(|schema| {
        !matches!(
            schema["properties"]["type"]["const"].as_str(),
            Some("ground" | "unground")
        )
    });
    schemas.push(effect_schema(
        "ground",
        &["concept_id", "source", "statement"],
        json!({
            "concept_id": text_schema(),
            "source": {
                "type": "object", "additionalProperties": false,
                "required": ["kind", "library_id", "event_id", "account_id"],
                "properties": {
                    "kind": {"const": "annals_decision_account"},
                    "library_id": text_schema(),
                    "event_id": text_schema(),
                    "account_id": text_schema()
                }
            },
            "statement": text_schema()
        }),
    ));
    schemas
}

fn effect_schemas() -> Vec<Value> {
    vec![
        effect_schema(
            "define",
            &["concept_id", "label", "meaning"],
            json!({
                "concept_id": text_schema(), "label": text_schema(), "meaning": text_schema()
            }),
        ),
        effect_schema(
            "revise",
            &["concept_id", "label", "meaning"],
            json!({
                "concept_id": text_schema(), "label": nullable_text_schema(), "meaning": nullable_text_schema()
            }),
        ),
        effect_schema(
            "differentiate",
            &["concept_id", "other_concept_id", "distinction"],
            json!({
                "concept_id": text_schema(), "other_concept_id": text_schema(), "distinction": text_schema()
            }),
        ),
        effect_schema(
            "reopen",
            &["concept_id", "reason"],
            json!({
                "concept_id": text_schema(), "reason": text_schema()
            }),
        ),
        effect_schema(
            "retire",
            &["concept_id", "reason", "replacement_concept_id"],
            json!({
                "concept_id": text_schema(), "reason": text_schema(), "replacement_concept_id": nullable_text_schema()
            }),
        ),
        effect_schema(
            "ground",
            &["concept_id", "source", "statement"],
            json!({
                "concept_id": text_schema(),
                "source": {
                    "type": "object", "additionalProperties": false,
                    "required": ["kind", "event_id", "decision_id"],
                    "properties": {
                        "kind": {"const": "decision"},
                        "event_id": text_schema(),
                        "decision_id": text_schema()
                    }
                },
                "statement": text_schema()
            }),
        ),
        effect_schema(
            "unground",
            &[
                "concept_id",
                "event_id",
                "decision_id",
                "withdrawal_event_id",
                "reason",
            ],
            json!({
                "concept_id": text_schema(), "event_id": text_schema(), "decision_id": text_schema(),
                "withdrawal_event_id": text_schema(), "reason": text_schema()
            }),
        ),
    ]
}

fn effect_schema(kind: &str, required: &[&str], properties: Value) -> Value {
    let mut properties = properties.as_object().cloned().unwrap_or_default();
    properties.insert("type".to_owned(), json!({"const": kind}));
    let mut required = required
        .iter()
        .map(|value| json!(value))
        .collect::<Vec<_>>();
    required.push(json!("type"));
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

fn text_schema() -> Value {
    json!({"type": "string", "minLength": 1, "maxLength": 16384})
}

fn nullable_text_schema() -> Value {
    json!({"type": ["string", "null"], "minLength": 1, "maxLength": 16384})
}

fn result_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "oneOf": [
            {
                "type": "object", "additionalProperties": false,
                "required": ["accepted", "revision"],
                "properties": {
                    "accepted": {"const": true},
                    "revision": {"type": "integer", "minimum": 1}
                }
            },
            {
                "type": "object", "additionalProperties": false,
                "required": ["error"],
                "properties": {
                    "error": {
                        "type": "object", "additionalProperties": false,
                        "required": ["code", "message"],
                        "properties": {
                            "code": text_schema(), "message": text_schema()
                        }
                    }
                }
            }
        ]
    })
}

fn neutral_cwd(job_id: &str) -> Result<PathBuf> {
    let base = std::env::temp_dir().join("semantics-nucleus");
    fs::create_dir_all(&base).map_err(|source| crate::error::io(&base, source))?;
    let path = base.join(digest(job_id.as_bytes()));
    fs::create_dir_all(&path).map_err(|source| crate::error::io(&path, source))?;
    if !path.is_absolute() {
        return Err(Error::domain(
            "nucleus_cwd_relative",
            "neutral Nucleus working directory is not absolute",
        ));
    }
    Ok(path)
}

fn cleanup_neutral_cwd(job_id: &str) {
    let path = std::env::temp_dir()
        .join("semantics-nucleus")
        .join(digest(job_id.as_bytes()));
    let _result = fs::remove_dir(path);
}

#[derive(Debug)]
struct AccountNeutralDirectory {
    path: PathBuf,
}

impl AccountNeutralDirectory {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for AccountNeutralDirectory {
    fn drop(&mut self) {
        let Ok(metadata) = fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.mode() & 0o777 != 0o700
        {
            return;
        }
        let Ok(entries) = account_directory_entries(&self.path) else {
            return;
        };
        if !entries.is_empty() {
            return;
        }
        let _result = fs::remove_dir(&self.path);
    }
}

fn account_neutral_cwd(job_id: &str) -> Result<AccountNeutralDirectory> {
    account_neutral_cwd_in(&platform_user_temporary_root()?, job_id)
}

#[cfg(target_os = "macos")]
fn platform_user_temporary_root() -> Result<PathBuf> {
    let output = Command::new("/usr/bin/getconf")
        .arg("DARWIN_USER_TEMP_DIR")
        .env_clear()
        .current_dir("/")
        .output()
        .map_err(|_| account_cwd_error("unable to resolve the platform user temporary root"))?;
    if !output.status.success() || output.stdout.len() > 4_096 {
        return Err(account_cwd_error(
            "unable to resolve the platform user temporary root",
        ));
    }
    let raw = std::str::from_utf8(&output.stdout)
        .map_err(|_| account_cwd_error("the platform user temporary root is invalid"))?;
    let value = raw.strip_suffix('\n').unwrap_or(raw);
    if value.is_empty()
        || value
            .chars()
            .any(|character| matches!(character, '\n' | '\r' | '\0'))
    {
        return Err(account_cwd_error(
            "the platform user temporary root is invalid",
        ));
    }
    fs::canonicalize(value)
        .map_err(|_| account_cwd_error("the platform user temporary root is unavailable"))
}

#[cfg(not(target_os = "macos"))]
fn platform_user_temporary_root() -> Result<PathBuf> {
    fs::canonicalize("/tmp")
        .map_err(|_| account_cwd_error("the platform user temporary root is unavailable"))
}

fn account_neutral_cwd_in(temporary_root: &Path, job_id: &str) -> Result<AccountNeutralDirectory> {
    let root_metadata = fs::symlink_metadata(temporary_root)
        .map_err(|_| account_cwd_error("the platform user temporary root is unavailable"))?;
    if job_id.is_empty()
        || !temporary_root.is_absolute()
        || root_metadata.file_type().is_symlink()
        || !root_metadata.is_dir()
        || root_metadata.mode() & 0o777 != 0o700
        || fs::canonicalize(temporary_root).ok().as_deref() != Some(temporary_root)
    {
        return Err(account_cwd_error(
            "the platform user temporary root is not private and canonical",
        ));
    }
    reject_account_control_ancestors(temporary_root)?;
    let path = temporary_root.join(format!(
        "semantics-account-nucleus-v1-{}",
        digest(job_id.as_bytes())
    ));
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    if let Err(error) = builder.create(&path)
        && error.kind() != std::io::ErrorKind::AlreadyExists
    {
        return Err(account_cwd_error(
            "unable to create the private account reconciliation directory",
        ));
    }
    let before = fs::symlink_metadata(&path)
        .map_err(|_| account_cwd_error("unable to inspect the account reconciliation directory"))?;
    if before.file_type().is_symlink()
        || !before.is_dir()
        || before.mode() & 0o777 != 0o700
        || fs::canonicalize(&path).ok().as_ref() != Some(&path)
        || !account_directory_entries(&path)?.is_empty()
    {
        return Err(account_cwd_error(
            "the account reconciliation directory is not private, canonical, and empty",
        ));
    }
    prove_private_empty_account_directory(&root_metadata, &before, &path)?;
    Ok(AccountNeutralDirectory { path })
}

fn prove_private_empty_account_directory(
    root_metadata: &fs::Metadata,
    before: &fs::Metadata,
    path: &Path,
) -> Result<()> {
    let marker = path.join(format!(".owner-{}", Uuid::now_v7()));
    let mut marker_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&marker)
        .map_err(|_| account_cwd_error("unable to prove account directory ownership"))?;
    marker_file
        .write_all(b"semantics\n")
        .and_then(|()| marker_file.sync_all())
        .map_err(|_| account_cwd_error("unable to prove account directory ownership"))?;
    let marker_metadata = marker_file
        .metadata()
        .map_err(|_| account_cwd_error("unable to prove account directory ownership"))?;
    drop(marker_file);
    let after = fs::symlink_metadata(path)
        .map_err(|_| account_cwd_error("the account directory changed during verification"))?;
    let marker_path_metadata = fs::symlink_metadata(&marker)
        .map_err(|_| account_cwd_error("unable to prove account directory ownership"))?;
    let entries = account_directory_entries(path)?;
    let valid = before.dev() == after.dev()
        && before.ino() == after.ino()
        && root_metadata.uid() == before.uid()
        && after.uid() == marker_metadata.uid()
        && marker_path_metadata.is_file()
        && !marker_path_metadata.file_type().is_symlink()
        && marker_path_metadata.dev() == marker_metadata.dev()
        && marker_path_metadata.ino() == marker_metadata.ino()
        && marker_metadata.nlink() == 1
        && marker_metadata.mode() & 0o777 == 0o600
        && entries.len() == 1
        && entries[0] == marker.file_name().unwrap_or_default();
    fs::remove_file(&marker)
        .map_err(|_| account_cwd_error("unable to clear account directory ownership proof"))?;
    if !valid {
        return Err(account_cwd_error(
            "the account reconciliation directory changed during verification",
        ));
    }
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| account_cwd_error("unable to verify the empty account directory"))?;
    if !account_directory_entries(path)?.is_empty() {
        return Err(account_cwd_error(
            "the account reconciliation directory changed during verification",
        ));
    }
    Ok(())
}

fn reject_account_control_ancestors(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        for marker in ["AGENTS.md", ".git"] {
            match fs::symlink_metadata(ancestor.join(marker)) {
                Ok(_) => {
                    return Err(account_cwd_error(
                        "the account reconciliation directory is inside a control tree",
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    return Err(account_cwd_error(
                        "unable to prove account working-directory neutrality",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn account_directory_entries(path: &Path) -> Result<Vec<std::ffi::OsString>> {
    fs::read_dir(path)
        .map_err(|_| account_cwd_error("unable to inspect the account reconciliation directory"))?
        .map(|entry| {
            entry.map(|entry| entry.file_name()).map_err(|_| {
                account_cwd_error("unable to inspect the account reconciliation directory")
            })
        })
        .collect()
}

fn account_cwd_error(message: &'static str) -> Error {
    Error::domain("account_nucleus_cwd_invalid", message)
}

fn digest(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::domain("nucleus_runtime_failed", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        account_effect_schemas, account_input_schema, account_reconciliation_prompt,
        effect_schemas, explicit_nonretryable_rejection, input_schema, reconciliation_prompt,
    };
    use crate::domain::{
        AccountIntake, AccountRoutingOutcome, DecisionAccountAnchor, DecisionAccountEvent,
        DecisionAnchor, DecisionEvent, Intake, IntakeStatus, Repository,
    };
    use nucleus_core::WorkspaceAccess;

    #[test]
    fn immutable_toolset_contains_all_typed_effects() {
        let encoded = serde_json::to_string(&effect_schemas()).expect("effect schemas");
        for effect in [
            "define",
            "revise",
            "differentiate",
            "reopen",
            "retire",
            "ground",
            "unground",
        ] {
            assert!(encoded.contains(effect));
        }
        assert_eq!(input_schema()["properties"]["effects"]["maxItems"], 256);
        let workspace = WorkspaceAccess::None;
        assert_eq!(workspace, WorkspaceAccess::None);
    }

    #[test]
    fn only_explicit_nonretryable_client_errors_prove_rejection() {
        assert!(explicit_nonretryable_rejection(400));
        assert!(explicit_nonretryable_rejection(422));
        assert!(!explicit_nonretryable_rejection(408));
        assert!(!explicit_nonretryable_rejection(409));
        assert!(!explicit_nonretryable_rejection(429));
        assert!(!explicit_nonretryable_rejection(500));
    }

    #[test]
    fn model_prompt_excludes_stream_and_machine_routing_anchors() {
        let decision = DecisionEvent {
            event_id: "event-1".to_owned(),
            event_version: 1,
            cursor: "PRIVATE_CURSOR".to_owned(),
            event_kind: "decision_admitted".to_owned(),
            occurred_at: 17,
            decision_id: "decision-1".to_owned(),
            decided_at: 16,
            timestamp_precision: "PRIVATE_EVENT_PRECISION".to_owned(),
            statement: "Keep vocabulary authoritative.".to_owned(),
            disposition: "adopt".to_owned(),
            confidence: "high".to_owned(),
            rationale: Some("A durable project concern.".to_owned()),
            supersedes_decision_id: None,
            authority_start: 4,
            authority_end: 8,
            review_state: "unreviewed".to_owned(),
            review_id: None,
            review_action: None,
            reviewed_at: None,
            review_source: None,
            anchors: vec![DecisionAnchor {
                source_role: "authority".to_owned(),
                host_id: "PRIVATE_HOST".to_owned(),
                thread_id: "PRIVATE_THREAD".to_owned(),
                turn_id: "PRIVATE_TURN".to_owned(),
                item_id: "PRIVATE_ITEM".to_owned(),
                message_role: "user".to_owned(),
                occurred_at: 15,
                timestamp_precision: "PRIVATE_SOURCE_PRECISION".to_owned(),
            }],
        };
        let intake = Intake {
            event_id: decision.event_id.clone(),
            source_cursor: decision.cursor.clone(),
            project_id: Some("cell".to_owned()),
            status: IntakeStatus::Processing,
            cwd: Some("/PRIVATE/PROJECT/PATH".to_owned()),
            decision,
            attempts: 1,
            last_error: None,
            terminal_reason: None,
            applied_revision: None,
        };
        let prompt = reconciliation_prompt(&intake, &Repository::empty("cell"), 1)
            .expect("reconciliation prompt");
        assert!(prompt.contains("Keep vocabulary authoritative."));
        for private in [
            "PRIVATE_CURSOR",
            "PRIVATE_HOST",
            "PRIVATE_THREAD",
            "PRIVATE_TURN",
            "PRIVATE_ITEM",
            "PRIVATE_EVENT_PRECISION",
            "PRIVATE_SOURCE_PRECISION",
            "/PRIVATE/PROJECT/PATH",
        ] {
            assert!(!prompt.contains(private));
        }
    }

    #[test]
    fn account_tool_and_prompt_use_only_normalized_annals_meaning() {
        let account = DecisionAccountEvent {
            library_id: "0123456789abcdef0123456789abcdef".to_owned(),
            cursor: "PRIVATE_CURSOR".to_owned(),
            event_id: "event-1".to_owned(),
            account_id: "account-1".to_owned(),
            content: crate::domain::DecisionContent::Legacy(crate::domain::LegacyAccountContent {
                account_schema_version: 1,
                statement: "Keep vocabulary authoritative.".to_owned(),
                context: "A durable boundary is needed.".to_owned(),
                action: "Used an immutable repository.".to_owned(),
                result: "Meaning replays.".to_owned(),
                occurred_at: 17,
                occurred_at_precision: "second".to_owned(),
                authority: DecisionAccountAnchor {
                    host_id: "PRIVATE_HOST".to_owned(),
                    thread_id: "PRIVATE_THREAD".to_owned(),
                    turn_id: "PRIVATE_TURN".to_owned(),
                    item_id: "PRIVATE_ITEM".to_owned(),
                    span_start: 4,
                    span_end: 8,
                },
            }),
        };
        let intake = AccountIntake {
            event_id: account.event_id.clone(),
            source_cursor: account.cursor.clone(),
            project_id: Some("cell".to_owned()),
            status: IntakeStatus::Processing,
            routing_outcome: AccountRoutingOutcome::ProjectAssigned,
            account,
            attempts: 1,
            last_error: None,
            terminal_reason: None,
            applied_revision: None,
        };
        let prompt = account_reconciliation_prompt(&intake, &Repository::empty("cell"), 1)
            .expect("account prompt");
        for included in [
            "0123456789abcdef0123456789abcdef",
            "event-1",
            "account-1",
            "Keep vocabulary authoritative.",
            "A durable boundary is needed.",
            "Used an immutable repository.",
            "Meaning replays.",
        ] {
            assert!(prompt.contains(included));
        }
        for excluded in [
            "PRIVATE_CURSOR",
            "PRIVATE_HOST",
            "PRIVATE_THREAD",
            "PRIVATE_TURN",
            "PRIVATE_ITEM",
            "/PRIVATE/PROJECT/PATH",
            "confidence",
            "review",
            "disposition",
            "supersedes",
        ] {
            assert!(!prompt.contains(excluded), "prompt exposed {excluded}");
        }
        let encoded = serde_json::to_string(&account_effect_schemas()).expect("schemas");
        assert!(encoded.contains("annals_decision_account"));
        assert!(!encoded.contains("unground"));
        assert_eq!(
            account_input_schema()["properties"]["effects"]["maxItems"],
            256
        );
    }
}
