use std::fs;
use std::path::{Path, PathBuf};
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

use crate::domain::{Intake, IntakeStatus, ReconciliationProposal, Repository};
use crate::store::{Correlation, MailboxReceipt, Store};
use crate::{Error, Result};

const TOOLSET_DEFINITIONS_SCHEMA_ID: &str = "nucleus.toolset-definitions.v1";
const INPUT_SCHEMA_ID: &str = "semantics.tool.commit-reconciliation.input.v1";
const RESULT_SCHEMA_ID: &str = "semantics.tool.commit-reconciliation.result.v1";
const TOOL_NAME: &str = "commit_semantic_reconciliation";
const MODEL: &str = "gpt-5.6-terra";
const TOOLSET_NAME: &str = "semantic-reconciliation";
const TOOLSET_VERSION: u32 = 1;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20 * 60);

const INSTRUCTIONS: &str = r"Maintain one project's authoritative semantic repository from one normalized Decisions lifecycle event. You have exactly one managed tool. For an admitted decision or an effective confirmation, submit a complete atomic reconciliation that includes at least one ground effect citing the exact supplied event_id and decision_id, even when the meaning is already represented. For a dismissal review, withdraw every active grounding whose decision_id is dismissed using unground; preserve history. Define only durable project terms, not incidental implementation nouns. Prefer revise, differentiate, retire, reopen, ground, or unground over duplicate definitions. Active canonical labels must remain unique. Use only the supplied decision and repository snapshot. Call commit_semantic_reconciliation; if it returns a validation error, correct the proposal and retry. Never finish without an accepted tool result.";

const DEVELOPER_INSTRUCTIONS: &str = r"Treat identifiers as opaque and copy them exactly. New concept IDs must use the supplied next_concept_ids in order. Do not invent source material, paths, conversation text, or implementation evidence. Do not use shell, web, local files, or any tool except commit_semantic_reconciliation. The repository snapshot is complete for the selected revision.";

#[derive(Debug, Clone)]
pub struct NucleusReconciler {
    socket: Option<PathBuf>,
}

impl NucleusReconciler {
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

    pub fn doctor(&self) -> Result<()> {
        let runtime = runtime()?;
        runtime.block_on(async {
            let client = self.client()?;
            require_health(&client).await?;
            register_contract(&client).await?;
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
            require_health(&client).await?;
            let toolset = register_contract(&client).await?;
            let correlation = match store.correlation(&intake.event_id)? {
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

    fn client(&self) -> Result<NucleusClient> {
        match &self.socket {
            Some(socket) => NucleusClient::new(socket).map_err(Into::into),
            None => NucleusClient::for_current_user().map_err(Into::into),
        }
    }
}

async fn require_health(client: &NucleusClient) -> Result<()> {
    let health = client.health().await?;
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
    if health.status != "ok"
        || !health.accepting_jobs
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
        let job = client.get_job(&job_id).await?;
        if job.summary.state.is_terminal() {
            if store
                .pending_committed_revision(&intake.event_id, job_id.as_str())?
                .is_some()
            {
                return store.finalize_applied(&intake.event_id, job_id.as_str());
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

async fn register_contract(client: &NucleusClient) -> Result<ToolsetRegistrationV1> {
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
            description:
                "Atomically append one validated semantic revision. Retry after a validation error."
                    .to_owned(),
            input_schema_id: SchemaId::new(INPUT_SCHEMA_ID),
            input_schema: to_raw_value(&input)
                .map_err(|error| Error::domain("nucleus_schema_invalid", error.to_string()))?,
        }],
    };
    let registration = ToolsetRegistrationV1::new(
        ToolsetRef {
            provider: "semantics".to_owned(),
            name: TOOLSET_NAME.to_owned(),
            version: TOOLSET_VERSION,
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
    use std::fs;
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use nucleus_core::{JobRequestV1, WorkspaceAccess};
    use serde_json::{Value, json};
    use tempfile::TempDir;

    use crate::domain::{DecisionAnchor, DecisionEvent, Intake, IntakeStatus, Repository};
    use crate::store::Store;

    use super::{
        INPUT_SCHEMA_ID, RESULT_SCHEMA_ID, TOOL_NAME, effect_schemas,
        explicit_nonretryable_rejection, input_schema, reconciliation_prompt,
    };

    type ServerError = Box<dyn std::error::Error + Send + Sync>;
    type ServerResult<T = ()> = std::result::Result<T, ServerError>;

    struct CapturedFlow {
        request_body: Vec<u8>,
        result_bodies: Vec<Vec<u8>>,
    }

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
    fn real_requester_commits_pending_call_and_finishes_completed_job() {
        let (temporary, store, intake) = requester_fixture();
        let socket = temporary.path().join("nucleus.sock");
        let listener = listener(&socket);
        let server = thread::spawn(move || serve_commit_flow(listener, "completed", 0));

        let revision = super::NucleusReconciler::with_socket(&socket)
            .reconcile(&store, &intake)
            .expect("real requester reconciliation");
        assert_eq!(revision, 1);
        assert_applied_authority(&store);

        let captured = join_server(server);
        let request: JobRequestV1 =
            serde_json::from_slice(&captured.request_body).expect("submitted request");
        assert_eq!(request.invocation.workspace_access, WorkspaceAccess::None);
        assert!(!request.invocation.builtin_tools.local_execution);
        assert!(!request.invocation.builtin_tools.web_search);
        assert!(request.developer_instructions.is_some());
        assert_ne!(
            request.invocation.cwd.as_path(),
            temporary.path().join("project")
        );
        assert_eq!(captured.result_bodies.len(), 1);
        let result: Value =
            serde_json::from_slice(&captured.result_bodies[0]).expect("tool result");
        assert_eq!(result["result"], json!({"accepted": true, "revision": 1}));
    }

    #[test]
    fn ambiguous_admission_and_result_redeliver_identical_bytes_after_restart() {
        let (temporary, store, intake) = requester_fixture();
        let socket = temporary.path().join("nucleus.sock");

        let first_listener = listener(&socket);
        let first_server = thread::spawn(move || serve_dropped_admission(first_listener));
        let first_error = super::NucleusReconciler::with_socket(&socket)
            .reconcile(&store, &intake)
            .expect_err("dropped admission must remain ambiguous");
        assert_eq!(first_error.code(), "nucleus_failed");
        let first_requests = join_server(first_server);
        assert_eq!(first_requests.len(), 3);
        assert!(first_requests.windows(2).all(|pair| pair[0] == pair[1]));
        let correlation = store
            .correlation("event-1")
            .expect("correlation read")
            .expect("durable correlation");
        assert!(!correlation.admitted);
        assert_eq!(first_requests[0], correlation.request_json.as_bytes());
        assert_eq!(
            store.repository("cell", None).expect("repository").revision,
            0
        );

        rebind(&socket);
        let second_listener = listener(&socket);
        let second_server = thread::spawn(move || serve_commit_flow(second_listener, "running", 3));
        let second_intake = store
            .intake("event-1")
            .expect("intake after admission loss");
        let second_error = super::NucleusReconciler::with_socket(&socket)
            .reconcile(&store, &second_intake)
            .expect_err("dropped result acknowledgements must remain ambiguous");
        assert_eq!(second_error.code(), "nucleus_failed");
        let second = join_server(second_server);
        assert_eq!(second.request_body, first_requests[0]);
        assert_eq!(second.result_bodies.len(), 3);
        assert!(
            second
                .result_bodies
                .windows(2)
                .all(|pair| pair[0] == pair[1])
        );
        let pending = store.intake("event-1").expect("pending committed intake");
        assert_eq!(pending.status, IntakeStatus::Processing);
        assert_eq!(pending.applied_revision, Some(1));
        assert_eq!(
            store.repository("cell", None).expect("repository").revision,
            1
        );

        rebind(&socket);
        let third_listener = listener(&socket);
        let third_server = thread::spawn(move || serve_commit_flow(third_listener, "completed", 0));
        let third_intake = store.intake("event-1").expect("restart intake");
        let revision = super::NucleusReconciler::with_socket(&socket)
            .reconcile(&store, &third_intake)
            .expect("cached result redelivery");
        assert_eq!(revision, 1);
        let third = join_server(third_server);
        assert_eq!(third.request_body, first_requests[0]);
        assert_eq!(third.result_bodies, vec![second.result_bodies[0].clone()]);
        assert_applied_authority(&store);
        assert_eq!(
            store.repository("cell", None).expect("repository").revision,
            1
        );
    }

    #[test]
    fn terminal_runtime_failure_after_commit_preserves_authoritative_revision() {
        let (temporary, store, intake) = requester_fixture();
        let socket = temporary.path().join("nucleus.sock");
        let listener = listener(&socket);
        let server = thread::spawn(move || serve_commit_flow(listener, "failed", 0));

        let revision = super::NucleusReconciler::with_socket(&socket)
            .reconcile(&store, &intake)
            .expect("committed revision survives terminal runtime failure");
        assert_eq!(revision, 1);
        assert_applied_authority(&store);
        let captured = join_server(server);
        assert_eq!(captured.result_bodies.len(), 1);
    }

    fn requester_fixture() -> (TempDir, Store, Intake) {
        let temporary = TempDir::new().expect("temporary directory");
        let root = temporary.path().join("project");
        fs::create_dir(&root).expect("project root");
        let store = Store::open(temporary.path().join("semantics.db")).expect("database");
        store
            .register_project("cell", &root, "cursor-0")
            .expect("project registration");
        let event = DecisionEvent {
            event_id: "event-1".to_owned(),
            event_version: 1,
            cursor: "cursor-1".to_owned(),
            event_kind: "decision_admitted".to_owned(),
            occurred_at: 1,
            decision_id: "decision-1".to_owned(),
            decided_at: 1,
            timestamp_precision: "second".to_owned(),
            statement: "Keep vocabulary authoritative.".to_owned(),
            disposition: "adopt".to_owned(),
            confidence: "high".to_owned(),
            rationale: Some("Preserve stable meaning.".to_owned()),
            supersedes_decision_id: None,
            authority_start: 0,
            authority_end: 10,
            review_state: "unreviewed".to_owned(),
            review_id: None,
            review_action: None,
            reviewed_at: None,
            review_source: None,
            anchors: vec![DecisionAnchor {
                source_role: "authority".to_owned(),
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
                turn_id: "turn".to_owned(),
                item_id: "item".to_owned(),
                message_role: "user".to_owned(),
                occurred_at: 1,
                timestamp_precision: "second".to_owned(),
            }],
        };
        store
            .insert_intake(&event, Some("cell"), Some(&root))
            .expect("intake");
        store.mark_processing("event-1").expect("processing");
        let intake = store.intake("event-1").expect("intake read");
        (temporary, store, intake)
    }

    fn assert_applied_authority(store: &Store) {
        let intake = store.intake("event-1").expect("applied intake");
        assert_eq!(intake.status, IntakeStatus::Applied);
        assert_eq!(intake.applied_revision, Some(1));
        let repository = store.repository("cell", None).expect("repository");
        assert_eq!(repository.revision, 1);
        let concept = repository.concepts.get("c000001").expect("concept");
        assert!(concept.active);
        assert!(concept.grounds.iter().any(|grounding| {
            grounding.active
                && matches!(
                    &grounding.source,
                    crate::domain::GroundingSource::Decision {
                        event_id,
                        decision_id
                    } if event_id == "event-1" && decision_id == "decision-1"
                )
        }));
    }

    fn listener(socket: &Path) -> UnixListener {
        let listener = UnixListener::bind(socket).expect("bind fake Nucleus socket");
        listener
            .set_nonblocking(true)
            .expect("nonblocking fake listener");
        listener
    }

    fn rebind(socket: &Path) {
        if socket.exists() {
            fs::remove_file(socket).expect("remove prior fake socket");
        }
    }

    fn join_server<T>(server: thread::JoinHandle<ServerResult<T>>) -> T {
        match server.join() {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => panic!("fake Nucleus server failed: {error}"),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    fn serve_dropped_admission(listener: UnixListener) -> ServerResult<Vec<Vec<u8>>> {
        serve_handshake(&listener)?;
        let mut requests = Vec::new();
        for _attempt in 0..3 {
            let (stream, request_line, body) = accept_request(&listener)?;
            assert!(request_line.starts_with("POST /v1/jobs "));
            serde_json::from_slice::<JobRequestV1>(&body)?;
            requests.push(body);
            drop(stream);
        }
        Ok(requests)
    }

    fn serve_commit_flow(
        listener: UnixListener,
        terminal_state: &str,
        dropped_result_responses: usize,
    ) -> ServerResult<CapturedFlow> {
        serve_handshake(&listener)?;
        let (mut stream, request_line, request_body) = accept_request(&listener)?;
        assert!(request_line.starts_with("POST /v1/jobs "));
        let request: JobRequestV1 = serde_json::from_slice(&request_body)?;
        assert_eq!(request.invocation.workspace_access, WorkspaceAccess::None);
        assert!(!request.invocation.builtin_tools.local_execution);
        assert!(!request.invocation.builtin_tools.web_search);
        assert!(request.invocation.toolset.is_some());
        assert!(request.invocation.cwd.as_path().is_dir());
        write_json(
            &mut stream,
            "202 Accepted",
            &json!({
                "version": 1,
                "jobId": request.id.as_str(),
                "state": "accepted",
                "requestDigest": "sha256:test",
                "logCursor": 0
            }),
        )?;

        let proposal = proposal();
        let (mut stream, request_line, body) = accept_request(&listener)?;
        assert!(body.is_empty());
        assert!(request_line.contains(&format!("/v1/jobs/{}/tool-calls?", request.id)));
        write_json(
            &mut stream,
            "200 OK",
            &pending_calls(request.id.as_str(), &proposal),
        )?;

        let mut result_bodies = Vec::new();
        let result_attempts = dropped_result_responses.max(1);
        for attempt in 0..result_attempts {
            let (mut stream, request_line, body) = accept_request(&listener)?;
            assert!(
                request_line.contains(&format!("/v1/jobs/{}/tool-calls/call-1/result", request.id))
            );
            let result: Value = serde_json::from_slice(&body)?;
            assert_eq!(result["callId"], "call-1");
            assert_eq!(result["requester"]["program"], "semantics");
            assert_eq!(result["resultSchemaId"], RESULT_SCHEMA_ID);
            assert_eq!(result["isError"], false);
            assert_eq!(result["result"], json!({"accepted": true, "revision": 1}));
            result_bodies.push(body);
            if attempt < dropped_result_responses {
                drop(stream);
            } else {
                write_json(
                    &mut stream,
                    "200 OK",
                    &answered_call(request.id.as_str(), &proposal),
                )?;
            }
        }
        if dropped_result_responses != 0 {
            return Ok(CapturedFlow {
                request_body,
                result_bodies,
            });
        }

        let (mut stream, request_line, body) = accept_request(&listener)?;
        assert!(body.is_empty());
        assert!(request_line.starts_with(&format!("GET /v1/jobs/{} ", request.id)));
        write_json(
            &mut stream,
            "200 OK",
            &terminal_job(&request, terminal_state),
        )?;
        Ok(CapturedFlow {
            request_body,
            result_bodies,
        })
    }

    fn serve_handshake(listener: &UnixListener) -> ServerResult {
        let (mut stream, request_line, body) = accept_request(listener)?;
        assert!(request_line.starts_with("GET /v1/health "));
        assert!(body.is_empty());
        write_json(&mut stream, "200 OK", &ready_health())?;

        for expected_id in [INPUT_SCHEMA_ID, RESULT_SCHEMA_ID] {
            let (mut stream, request_line, body) = accept_request(listener)?;
            assert!(request_line.starts_with("POST /v1/schemas "));
            let schema: Value = serde_json::from_slice(&body)?;
            assert_eq!(schema["id"], expected_id);
            assert_eq!(schema["producer"], "semantics");
            assert!(
                schema["digest"]
                    .as_str()
                    .is_some_and(|digest| !digest.is_empty())
            );
            write_json(&mut stream, "200 OK", &schema)?;
        }

        let (mut stream, request_line, body) = accept_request(listener)?;
        assert!(request_line.starts_with("POST /v1/toolsets "));
        let registration: Value = serde_json::from_slice(&body)?;
        assert_eq!(registration["toolset"]["provider"], "semantics");
        assert_eq!(registration["toolset"]["name"], "semantic-reconciliation");
        assert_eq!(registration["toolset"]["version"], 1);
        assert_eq!(
            registration["definitionsSchemaId"],
            "nucleus.toolset-definitions.v1"
        );
        assert_eq!(
            registration["definitions"]["tools"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(registration["definitions"]["tools"][0]["name"], TOOL_NAME);
        assert_eq!(
            registration["definitions"]["tools"][0]["inputSchemaId"],
            INPUT_SCHEMA_ID
        );
        write_json(
            &mut stream,
            "200 OK",
            &json!({
                "version": 1,
                "toolset": registration["toolset"],
                "definitionsSchemaId": registration["definitionsSchemaId"],
                "digest": registration["digest"],
                "registeredAt": "2026-09-01T00:00:00Z"
            }),
        )?;
        Ok(())
    }

    fn ready_health() -> Value {
        json!({
            "version": 1,
            "status": "ok",
            "daemonVersion": "test",
            "acceptingJobs": true,
            "checkedAt": "2026-09-01T00:00:00Z",
            "supportedProtocolVersions": [1],
            "harness": {
                "harness": "codex",
                "harnessVersion": "test",
                "adapterVersion": "test"
            },
            "capabilities": [
                "exact-model",
                "reasoning-effort",
                "workspace-none",
                "dynamic-client-tools",
                "developer-instructions",
                "persistent-file-authentication"
            ],
            "authentication": {
                "codexHome": "/tmp/codex-home",
                "configured": true,
                "authenticated": true
            }
        })
    }

    fn proposal() -> Value {
        json!({
            "base_revision": 0,
            "summary": "Define and ground authority",
            "effects": [
                {
                    "type": "define",
                    "concept_id": "c000001",
                    "label": "Authority",
                    "meaning": "The maintained source of project meaning."
                },
                {
                    "type": "ground",
                    "concept_id": "c000001",
                    "source": {
                        "kind": "decision",
                        "event_id": "event-1",
                        "decision_id": "decision-1"
                    },
                    "statement": "Keep vocabulary authoritative."
                }
            ]
        })
    }

    fn pending_calls(job_id: &str, proposal: &Value) -> Value {
        json!({
            "version": 1,
            "jobId": job_id,
            "calls": [{
                "version": 1,
                "call": tool_call(job_id, proposal),
                "state": "pending",
                "createdAt": "2026-09-01T00:00:00Z"
            }],
            "nextSequence": 1
        })
    }

    fn answered_call(job_id: &str, proposal: &Value) -> Value {
        json!({
            "version": 1,
            "call": tool_call(job_id, proposal),
            "state": "answered",
            "createdAt": "2026-09-01T00:00:00Z",
            "answeredAt": "2026-09-01T00:00:01Z"
        })
    }

    fn tool_call(job_id: &str, proposal: &Value) -> Value {
        json!({
            "version": 1,
            "id": "call-1",
            "jobId": job_id,
            "attemptId": "attempt-1",
            "requestSequence": 1,
            "toolName": TOOL_NAME,
            "argumentsSchemaId": INPUT_SCHEMA_ID,
            "arguments": proposal
        })
    }

    fn terminal_job(request: &JobRequestV1, state: &str) -> Value {
        json!({
            "version": 1,
            "summary": {
                "version": 1,
                "id": request.id.as_str(),
                "label": request.label,
                "requester": request.requester,
                "state": state,
                "requestDigest": "sha256:test",
                "createdAt": "2026-09-01T00:00:00Z",
                "updatedAt": "2026-09-01T00:00:01Z",
                "completedAt": "2026-09-01T00:00:01Z"
            },
            "request": request,
            "attempts": []
        })
    }

    fn accept_request(listener: &UnixListener) -> ServerResult<(UnixStream, String, Vec<u8>)> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((stream, _address)) => {
                    stream.set_nonblocking(false)?;
                    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                    let (line, body) = read_request(&stream)?;
                    return Ok((stream, line, body));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "fake Nucleus request timed out",
                        )
                        .into());
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn read_request(stream: &UnixStream) -> std::io::Result<(String, Vec<u8>)> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut request_line = String::new();
        reader.read_line(&mut request_line)?;
        let mut content_length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            if line == "\r\n" {
                break;
            }
            if let Some((_name, value)) = line
                .split_once(':')
                .filter(|(name, _value)| name.eq_ignore_ascii_case("content-length"))
            {
                content_length = value
                    .trim()
                    .parse()
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            }
        }
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body)?;
        Ok((request_line, body))
    }

    fn write_json(stream: &mut UnixStream, status: &str, value: &Value) -> std::io::Result<()> {
        let body = serde_json::to_vec(value).map_err(std::io::Error::other)?;
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
        stream.write_all(&body)?;
        stream.flush()
    }
}
