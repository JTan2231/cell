use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
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

use crate::error::{AppError, AppResult};
use crate::model::{
    AuthorityMessageVerdict, AuthorityVerdict, Candidate, Confidence, MessageRole,
    ObservationClassification, SourceMessage, SubmittedCandidate, SubmittedClassification,
    SubmittedObservationClassification, ThreadTranscript,
};
use crate::store::RunJobCorrelation;

const INVOCATION_TIMEOUT: Duration = Duration::from_mins(20);
const OBSERVATION_TIMEOUT: Duration = Duration::from_mins(22);
const ABANDON_OBSERVATION_TIMEOUT: Duration = Duration::from_mins(2);
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(30);
const MAILBOX_WAIT_SECONDS: u32 = 1;
const TOOLSET_DEFINITIONS_SCHEMA_ID: &str = "nucleus.toolset-definitions.v1";
const INPUT_SCHEMA_ID: &str = "decisions.tool.submit-daily-classification.input.v1";
const RESULT_SCHEMA_ID: &str = "decisions.tool.daily-classification.result.v1";
const TOOL_NAME: &str = "submit_daily_classification";
const OBSERVATION_INPUT_SCHEMA_ID: &str = "decisions.tool.submit-turn-classification.input.v1";
const OBSERVATION_RESULT_SCHEMA_ID: &str = "decisions.tool.turn-classification.result.v1";
const OBSERVATION_TOOL_NAME: &str = "submit_turn_classification";
const MODEL: &str = "gpt-5.6-terra";
const FORBIDDEN_DISCLOSURE_MARKERS: &[&str] = &[
    "api key",
    "api_key",
    "apikey",
    "access token",
    "auth token",
    "bearer ",
    "password",
    "credential",
    "secret",
    "token",
    "account id",
    "account_id",
    "tenant id",
    "tenant_id",
    "user id",
    "user_id",
    "source id",
    "source_id",
    "host_id",
    "thread_id",
    "turn_id",
    "item_id",
    "call_id",
    "job_id",
    "system prompt",
    "developer instruction",
    "tool call",
    "tool result",
    "tool trace",
    "submit_daily_classification",
    "file://",
];
const CREDENTIAL_PREFIXES: &[&str] = &[
    "akia",
    "asia",
    "sk-",
    "ghp_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "ya29.",
    "aiza",
    "eyj",
    "pk_live_",
    "sk_live_",
    "rk_live_",
    "whsec_",
];

const INSTRUCTIONS: &str = r"You classify explicit decisions in a supplied Codex conversation transcript. You have exactly one managed tool. Produce one accepted complete classification; if the tool returns an error, correct the arguments and retry until one classification is accepted. A decision is an attributable transition from practical openness to operative settlement: an explicit, authoritative user resolution of a material choice, specific enough to constrain future behavior or state. Valid dispositions adopt, reject, forbid, intentionally defer, delegate, reopen, or supersede a choice. Assistant text may only resolve the meaning of a user's deictic acceptance such as 'yes' or 'do that'. Assistant-only recommendations, plans, questions, status reports, implementation reports, silence, tool approvals, and copied subagent prompts are not decisions. The authority source must be a supplied user message. Do not infer preferences from actions or absence. Use high confidence only when the settlement is explicit; medium when the authority is explicit but the exact operative statement needs limited contextual resolution; use low otherwise. Return every qualifying settlement whose authority source is in the supplied report window, and nothing else. Statements and rationales are destined for unattended email and must never disclose secrets, credentials, tokens, account identifiers, email addresses, local paths, source IDs, raw prompts, transcript quotations, tool calls or traces, or unrelated metadata. When a decision concerns sensitive configuration, abstract it to the owning boundary, for example 'Transport authentication remains within Email', without naming the sensitive field, identifier, or value.";

const DEVELOPER_INSTRUCTIONS: &str = r"Treat source IDs as opaque. Cite only exact source IDs from the catalog. For authority_excerpt copy a nonempty exact substring that uniquely identifies the operative settlement within the authority user message; use different excerpts for distinct decisions in one message. Use the smallest context_source_ids set needed to resolve the authority statement. State each decision as concise durable prose without transcript quotations or operational metadata. Paraphrase without secrets, credentials, tokens, account identifiers, email addresses, local or source paths, source IDs, raw prompts, or tool calls/traces. Return complete=true only after examining the entire supplied transcript. Never call a tool other than submit_daily_classification.";

const OBSERVATION_INSTRUCTIONS: &str = r"Classify every eligible user authority message in one completed Codex turn. A completed file-change effect makes the turn eligible for enacted-decision analysis, but activity is evidence of enactment and never authority: do not infer a decision from a write, assistant behavior, silence, or implementation success. A decision is an explicit authoritative user transition from practical openness to operative settlement, specific enough to constrain future behavior or state. Assistant text may only resolve the meaning of a user's referential acceptance such as 'yes', 'this works', or 'do that'. For each supplied authority source ID, return exactly one verdict: decision or no_decision. Include every qualifying settlement for a decision verdict. If the supplied scope cannot resolve an otherwise explicit referential user settlement, request context for the whole observation instead of guessing. Assistant-only recommendations, plans, questions, status or implementation reports, tool approvals, and copied subagent prompts are not decisions. Statements and rationales are destined for unattended email and must not disclose secrets, credentials, tokens, account identifiers, email addresses, local paths, source IDs, prompts, quotations, tool traces, or unrelated metadata.";

const OBSERVATION_DEVELOPER_INSTRUCTIONS: &str = r"Treat source IDs as opaque and cite only supplied IDs. Every authority_source_id listed by the prompt must appear exactly once in verdicts. A decision verdict must contain at least one decision and no_decision must contain none. Each nested decision must repeat its owning authority source ID. Copy a nonempty exact unique authority_excerpt. Use the smallest context_source_ids set necessary. Set needs_context=true only when the prompt says context expansion is available, and then return no verdicts. Return complete=true. Never call a tool other than submit_turn_classification.";

#[derive(Debug, Clone)]
pub(crate) struct ClassificationResult {
    pub(crate) candidates: Vec<Candidate>,
    pub(crate) authority_verdicts: Vec<AuthorityMessageVerdict>,
    pub(crate) needs_context: bool,
}

impl From<ObservationClassification> for ClassificationResult {
    fn from(classification: ObservationClassification) -> Self {
        Self {
            candidates: classification.candidates,
            authority_verdicts: classification.authority_verdicts,
            needs_context: classification.needs_context,
        }
    }
}

impl ClassificationResult {
    pub(crate) fn as_observation(&self) -> ObservationClassification {
        ObservationClassification {
            candidates: self.candidates.clone(),
            authority_verdicts: self.authority_verdicts.clone(),
            needs_context: self.needs_context,
        }
    }
}

#[derive(Clone, Copy)]
struct ObservationContract<'a> {
    authority_turn_id: &'a str,
    allow_needs_context: bool,
    file_change_count: usize,
}

pub(crate) struct Runner {
    socket: Option<std::path::PathBuf>,
}

impl Runner {
    pub(crate) const fn for_current_user() -> Self {
        Self { socket: None }
    }

    #[cfg(test)]
    pub(crate) fn classify(
        &self,
        store: &mut crate::store::Store,
        requester_id: &str,
        job_id: &str,
        transcript: &ThreadTranscript,
        window_start: i64,
        window_end: i64,
    ) -> AppResult<ClassificationResult> {
        self.classify_with(
            store,
            requester_id,
            job_id,
            transcript,
            window_start,
            window_end,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn classify_observation(
        &self,
        store: &mut crate::store::Store,
        requester_id: &str,
        job_id: &str,
        transcript: &ThreadTranscript,
        window_start: i64,
        window_end: i64,
        authority_turn_id: &str,
        allow_needs_context: bool,
        file_change_count: usize,
    ) -> AppResult<ClassificationResult> {
        self.classify_with(
            store,
            requester_id,
            job_id,
            transcript,
            window_start,
            window_end,
            Some(ObservationContract {
                authority_turn_id,
                allow_needs_context,
                file_change_count,
            }),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn classify_with(
        &self,
        store: &mut crate::store::Store,
        requester_id: &str,
        job_id: &str,
        transcript: &ThreadTranscript,
        window_start: i64,
        window_end: i64,
        observation: Option<ObservationContract<'_>>,
    ) -> AppResult<ClassificationResult> {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                AppError::new(
                    "nucleus_runtime_failed",
                    format!("unable to initialize Nucleus runtime: {error}"),
                )
            })?;
        runtime
            .block_on(async {
                tokio::time::timeout(
                    OBSERVATION_TIMEOUT,
                    self.run(
                        store,
                        requester_id,
                        job_id,
                        transcript,
                        window_start,
                        window_end,
                        observation,
                    ),
                )
                .await
            })
            .map_err(|_| {
                AppError::new(
                    "nucleus_timeout",
                    format!(
                        "classification observation for {job_id} exceeded 22 minutes; the same run and job remain resumable"
                    ),
                )
            })?
    }

    pub(crate) fn doctor(&self) -> AppResult<()> {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                AppError::new(
                    "nucleus_runtime_failed",
                    format!("unable to initialize Nucleus runtime: {error}"),
                )
            })?;
        runtime.block_on(async {
            let client = match &self.socket {
                Some(socket) => NucleusClient::new(socket),
                None => NucleusClient::for_current_user(),
            }
            .map_err(client_error)?;
            require_health(&client).await
        })
    }

    pub(crate) fn reconcile_abandonment(&self, jobs: &[RunJobCorrelation]) -> AppResult<()> {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_error| {
                AppError::new(
                    "nucleus_runtime_failed",
                    "unable to initialize Nucleus runtime",
                )
            })?;
        runtime
            .block_on(async {
                tokio::time::timeout(ABANDON_OBSERVATION_TIMEOUT, self.stop_jobs(jobs)).await
            })
            .map_err(|_| {
                AppError::new(
                    "abandonment_timeout",
                    "Nucleus jobs did not reach terminal state within two minutes; the run remains abandoning and can be retried",
                )
            })?
    }

    async fn stop_jobs(&self, jobs: &[RunJobCorrelation]) -> AppResult<()> {
        let client = match &self.socket {
            Some(socket) => NucleusClient::new(socket),
            None => NucleusClient::for_current_user(),
        }
        .map_err(client_error)?;
        let mut observed_jobs = Vec::with_capacity(jobs.len());
        for correlation in jobs {
            let job_id = JobId::new(&correlation.nucleus_job_id);
            let observed = observe_job_for_abandon(&client, &job_id).await?;
            let action = abandonment_action(observed.as_ref(), correlation.admitted)?;
            observed_jobs.push((job_id, action));
        }
        for (job_id, action) in observed_jobs {
            match action {
                AbandonmentAction::Terminal => continue,
                AbandonmentAction::Cancel => {
                    retry_transport(|| client.cancel_job(&job_id)).await?;
                }
            }
            loop {
                let job = retry_transport(|| client.get_job(&job_id)).await?;
                if job.summary.state.is_terminal() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    async fn run(
        &self,
        store: &mut crate::store::Store,
        requester_id: &str,
        job_id: &str,
        transcript: &ThreadTranscript,
        window_start: i64,
        window_end: i64,
        observation: Option<ObservationContract<'_>>,
    ) -> AppResult<ClassificationResult> {
        let client = match &self.socket {
            Some(socket) => NucleusClient::new(socket),
            None => NucleusClient::for_current_user(),
        }
        .map_err(client_error)?;
        require_health(&client).await?;
        let registration = register_contract(&client, observation.is_some()).await?;
        let request = build_request(
            requester_id,
            job_id,
            transcript,
            registration.toolset,
            window_start,
            window_end,
            observation,
        )?;
        let request_digest = request.digest().map_err(|error| {
            AppError::new(
                "classification_request_invalid",
                format!("unable to digest typed Nucleus request: {error}"),
            )
        })?;
        store.persist_job_request_digest(job_id, &request_digest)?;
        let admission_lock = store.lock_run_operations()?;
        // This intent and the run-status fence commit before the first network
        // byte. A killed or uncertain submit therefore cannot be mistaken for
        // an attempt that was never capable of arriving.
        store.begin_job_admission(job_id)?;
        let accepted = tokio::time::timeout(SUBMIT_TIMEOUT, client.submit_job(&request))
            .await
            .map_err(|_| {
                AppError::new(
                    "nucleus_submit_timeout",
                    "Nucleus admission did not resolve within 30 seconds; the same attempt remains resumable",
                )
            })?
            .map_err(client_error)?;
        drop(admission_lock);
        if accepted.job_id.as_str() != job_id {
            return Err(AppError::new(
                "nucleus_protocol_error",
                "Nucleus admitted a different job identity",
            ));
        }
        let job_id = accepted.job_id;
        let requester = request.requester;
        let by_id = transcript
            .messages
            .iter()
            .map(|message| (message.item_id.as_str(), message))
            .collect::<BTreeMap<_, _>>();
        let mut after = 0_u64;
        let mut accepted_classification = if observation.is_some() {
            store
                .persisted_observation_classification(job_id.as_str())?
                .map(ClassificationResult::from)
        } else {
            store
                .persisted_classification(job_id.as_str())?
                .map(|candidates| ClassificationResult {
                    candidates,
                    authority_verdicts: Vec::new(),
                    needs_context: false,
                })
        };
        let expected_tool = if observation.is_some() {
            OBSERVATION_TOOL_NAME
        } else {
            TOOL_NAME
        };
        let expected_input_schema = if observation.is_some() {
            OBSERVATION_INPUT_SCHEMA_ID
        } else {
            INPUT_SCHEMA_ID
        };
        let result_schema_id = if observation.is_some() {
            OBSERVATION_RESULT_SCHEMA_ID
        } else {
            RESULT_SCHEMA_ID
        };

        loop {
            let query = ToolCallsQueryV1 {
                after,
                wait_seconds: MAILBOX_WAIT_SECONDS,
            };
            let calls = retry_transport(|| client.pending_tool_calls(&job_id, &query)).await?;
            for pending in calls.calls {
                let call = pending.call;
                if call.job_id != job_id {
                    return Err(AppError::new(
                        "nucleus_protocol_error",
                        "Nucleus returned a tool call for another job",
                    ));
                }
                let cache_key = call.id.to_string();
                let (result_json, is_error, classification) = if observation.is_some() {
                    if let Some(receipt) =
                        store.observation_classification_receipt(job_id.as_str(), &cache_key)?
                    {
                        (
                            receipt.result_json,
                            receipt.is_error,
                            receipt.classification.map(ClassificationResult::from),
                        )
                    } else {
                        let result = if call.tool_name != expected_tool
                            || call.arguments_schema_id.as_str() != expected_input_schema
                        {
                            Err("tool call is outside the admitted Decisions contract".to_owned())
                        } else if accepted_classification.is_some() {
                            Err("the complete classification was already submitted".to_owned())
                        } else {
                            match observation {
                                Some(contract) => decode_and_validate_observation(
                                    call.arguments.get(),
                                    &by_id,
                                    window_start,
                                    window_end,
                                    contract,
                                ),
                                None => {
                                    Err("observation contract disappeared during classification"
                                        .to_owned())
                                }
                            }
                        };
                        let (value, is_error, classification) = match result {
                            Ok(classification) => (
                                json!({
                                    "accepted": true,
                                    "candidate_count": classification.candidates.len(),
                                    "needs_context": classification.needs_context
                                }),
                                false,
                                Some(classification),
                            ),
                            Err(_message) => (invalid_classification_result(), true, None),
                        };
                        let result_json = value.to_string();
                        let receipt = store.persist_observation_classification_receipt(
                            job_id.as_str(),
                            &cache_key,
                            &result_json,
                            is_error,
                            classification.as_ref(),
                        )?;
                        (
                            receipt.result_json,
                            receipt.is_error,
                            receipt.classification.map(ClassificationResult::from),
                        )
                    }
                } else {
                    if let Some(receipt) =
                        store.classification_receipt(job_id.as_str(), &cache_key)?
                    {
                        (
                            receipt.result_json,
                            receipt.is_error,
                            receipt
                                .classification
                                .map(|candidates| ClassificationResult {
                                    candidates,
                                    authority_verdicts: Vec::new(),
                                    needs_context: false,
                                }),
                        )
                    } else {
                        let result = if call.tool_name != expected_tool
                            || call.arguments_schema_id.as_str() != expected_input_schema
                        {
                            Err("tool call is outside the admitted Decisions contract".to_owned())
                        } else if accepted_classification.is_some() {
                            Err("the complete classification was already submitted".to_owned())
                        } else {
                            decode_and_validate(
                                call.arguments.get(),
                                &by_id,
                                window_start,
                                window_end,
                            )
                        };
                        let (value, is_error, classification) = match result {
                            Ok(candidates) => (
                                json!({
                                    "accepted": true,
                                    "candidate_count": candidates.len()
                                }),
                                false,
                                Some(candidates),
                            ),
                            Err(_message) => (invalid_classification_result(), true, None),
                        };
                        let result_json = value.to_string();
                        let receipt = store.persist_classification_receipt(
                            job_id.as_str(),
                            &cache_key,
                            &result_json,
                            is_error,
                            classification.as_deref(),
                        )?;
                        (
                            receipt.result_json,
                            receipt.is_error,
                            receipt
                                .classification
                                .map(|candidates| ClassificationResult {
                                    candidates,
                                    authority_verdicts: Vec::new(),
                                    needs_context: false,
                                }),
                        )
                    }
                };
                if let Some(classification) = &classification {
                    accepted_classification = Some(classification.clone());
                }
                let payload = RawValue::from_string(result_json).map_err(|error| {
                    AppError::new(
                        "classification_receipt_invalid",
                        format!("persisted result bytes are invalid JSON: {error}"),
                    )
                })?;
                let result = ToolResultV1 {
                    version: PROTOCOL_VERSION_V1,
                    call_id: call.id.clone(),
                    requester: requester.clone(),
                    result_schema_id: SchemaId::new(result_schema_id),
                    result: payload,
                    is_error,
                };
                if let Some(classification) = post_result_or_resolve_terminal(
                    &client,
                    store,
                    &job_id,
                    &call.id,
                    &result,
                    observation.is_some(),
                )
                .await?
                {
                    return Ok(classification);
                }
                after = after.max(call.request_sequence);
            }
            let job = retry_transport(|| client.get_job(&job_id)).await?;
            if !job.summary.state.is_terminal() {
                continue;
            }
            return resolve_terminal_classification(
                store,
                &job_id,
                job.summary.state,
                observation.is_some(),
            );
        }
    }
}

async fn post_result_or_resolve_terminal(
    client: &NucleusClient,
    store: &crate::store::Store,
    job_id: &JobId,
    call_id: &nucleus_core::ToolCallId,
    result: &ToolResultV1,
    observation: bool,
) -> AppResult<Option<ClassificationResult>> {
    loop {
        match client.post_tool_result(job_id, call_id, result).await {
            Ok(_) => return Ok(None),
            Err(ClientError::Transport { .. }) => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(ClientError::Api {
                status: 409, code, ..
            }) if code == "job_terminal" => {
                let job = retry_transport(|| client.get_job(job_id)).await?;
                if !job.summary.state.is_terminal() {
                    return Err(AppError::new(
                        "nucleus_protocol_error",
                        "Nucleus rejected a tool result as terminal but reported a nonterminal job",
                    ));
                }
                return resolve_terminal_classification(
                    store,
                    job_id,
                    job.summary.state,
                    observation,
                )
                .map(Some);
            }
            Err(error) => return Err(client_error(error)),
        }
    }
}

fn resolve_terminal_classification(
    store: &crate::store::Store,
    job_id: &JobId,
    state: JobState,
    observation: bool,
) -> AppResult<ClassificationResult> {
    if observation {
        if let Some(classification) = store.persisted_observation_classification(job_id.as_str())? {
            return Ok(classification.into());
        }
    } else if let Some(candidates) = store.persisted_classification(job_id.as_str())? {
        return Ok(ClassificationResult {
            candidates,
            authority_verdicts: Vec::new(),
            needs_context: false,
        });
    }
    match state {
        JobState::Completed => Err(AppError::new(
            "classification_incomplete",
            "model completed without a valid durable classification receipt",
        )),
        JobState::Failed | JobState::Cancelled => Err(AppError::new(
            "nucleus_job_failed",
            format!(
                "Nucleus job {job_id} ended without a valid classification; inspect Nucleus diagnostics"
            ),
        )),
        JobState::Accepted | JobState::Running | JobState::WaitingOnRequester => Err(
            AppError::new("nucleus_protocol_error", "job is not terminal"),
        ),
    }
}

async fn observe_job_for_abandon(
    client: &NucleusClient,
    job_id: &JobId,
) -> AppResult<Option<JobState>> {
    loop {
        match client.get_job(job_id).await {
            Ok(job) => return Ok(Some(job.summary.state)),
            Err(ClientError::Transport { .. }) => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(ClientError::Api { status: 404, .. }) => return Ok(None),
            Err(error) => return Err(client_error(error)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AbandonmentAction {
    Terminal,
    Cancel,
}

fn abandonment_action(state: Option<&JobState>, admitted: bool) -> AppResult<AbandonmentAction> {
    match state {
        Some(state) if state.is_terminal() => Ok(AbandonmentAction::Terminal),
        Some(_) => Ok(AbandonmentAction::Cancel),
        None if !admitted => Ok(AbandonmentAction::Terminal),
        None => Err(AppError::new(
            "abandonment_job_unavailable",
            "an admission intent has no observable Nucleus job; the run was restored for an exact build resume before abandonment can be retried",
        )),
    }
}

async fn require_health(client: &NucleusClient) -> AppResult<()> {
    let health = client.health().await.map_err(client_error)?;
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
        return Err(AppError::new(
            "nucleus_not_ready",
            format!(
                "Nucleus is not ready for Decisions: status={}, accepting_jobs={}, authenticated={}, missing_capabilities={}",
                health.status,
                health.accepting_jobs,
                health.authentication.authenticated,
                missing.join(",")
            ),
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn register_contract(
    client: &NucleusClient,
    observation: bool,
) -> AppResult<ToolsetRegistrationV1> {
    let input_schema = if observation {
        observation_schema_value()
    } else {
        schema_value()
    };
    let success_schema = if observation {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["accepted", "candidate_count", "needs_context"],
            "properties": {
                "accepted": {"const": true},
                "candidate_count": {"type": "integer", "minimum": 0},
                "needs_context": {"type": "boolean"}
            }
        })
    } else {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["accepted", "candidate_count"],
            "properties": {
                "accepted": {"const": true},
                "candidate_count": {"type": "integer", "minimum": 0}
            }
        })
    };
    let result_schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "oneOf": [
            success_schema,
            {
                "type": "object",
                "additionalProperties": false,
                "required": ["error"],
                "properties": {
                    "error": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["code", "message"],
                        "properties": {
                            "code": {"const": "invalid_classification"},
                            "message": {"type": "string", "minLength": 1, "maxLength": 160}
                        }
                    }
                }
            }
        ]
    });
    let input_schema_id = if observation {
        OBSERVATION_INPUT_SCHEMA_ID
    } else {
        INPUT_SCHEMA_ID
    };
    let result_schema_id = if observation {
        OBSERVATION_RESULT_SCHEMA_ID
    } else {
        RESULT_SCHEMA_ID
    };
    for (id, name, schema) in [
        (
            input_schema_id,
            if observation {
                "Decisions turn classification input"
            } else {
                "Decisions daily classification input"
            },
            input_schema.clone(),
        ),
        (
            result_schema_id,
            if observation {
                "Decisions turn classification result"
            } else {
                "Decisions daily classification result"
            },
            result_schema,
        ),
    ] {
        let schema = LogSchemaV1::new(
            id,
            name,
            "1",
            "application/schema+json",
            "decisions",
            to_raw_value(&schema).map_err(|error| {
                AppError::new("classification_contract_invalid", error.to_string())
            })?,
        );
        client
            .register_schema(&schema)
            .await
            .map_err(client_error)?;
    }
    let definitions = ToolsetDefinitionsV1 {
        version: PROTOCOL_VERSION_V1,
        tools: vec![ToolDefinitionV1 {
            name: if observation {
                OBSERVATION_TOOL_NAME
            } else {
                TOOL_NAME
            }
            .to_owned(),
            description: "Submit one complete structured classification; after an error, retry with corrected arguments until one is accepted."
                .to_owned(),
            input_schema_id: SchemaId::new(input_schema_id),
            input_schema: to_raw_value(&input_schema).map_err(|error| {
                AppError::new("classification_contract_invalid", error.to_string())
            })?,
        }],
    };
    let registration = ToolsetRegistrationV1::new(
        ToolsetRef {
            provider: "decisions".to_owned(),
            name: if observation {
                "turn-classification"
            } else {
                "daily-classification"
            }
            .to_owned(),
            version: 1,
        },
        TOOLSET_DEFINITIONS_SCHEMA_ID,
        definitions,
    )
    .map_err(|error| AppError::new("classification_contract_invalid", error.to_string()))?;
    client
        .register_toolset(&registration)
        .await
        .map_err(client_error)?;
    Ok(registration)
}

fn schema_value() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "required": ["decisions", "complete"],
        "properties": {
            "complete": {"const": true},
            "decisions": {
                "type": "array",
                "maxItems": 100,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["authority_source_id", "authority_excerpt", "context_source_ids", "statement", "disposition", "confidence"],
                    "properties": {
                        "authority_source_id": {"type": "string", "minLength": 1, "maxLength": 255},
                        "authority_excerpt": {"type": "string", "minLength": 1, "maxLength": 500},
                        "context_source_ids": {"type": "array", "uniqueItems": true, "items": {"type": "string", "minLength": 1, "maxLength": 255}},
                        "statement": {"type": "string", "minLength": 1, "maxLength": 1000},
                        "disposition": {"enum": ["adopt", "reject", "forbid", "defer", "delegate", "reopen", "supersede"]},
                        "confidence": {"enum": ["high", "medium", "low"]},
                        "rationale": {"type": ["string", "null"], "maxLength": 1000},
                        "supersedes_decision_id": {"type": ["string", "null"], "maxLength": 128}
                    }
                }
            }
        }
    })
}

fn observation_schema_value() -> Value {
    let candidate = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["authority_source_id", "authority_excerpt", "context_source_ids", "statement", "disposition", "confidence"],
        "properties": {
            "authority_source_id": {"type": "string", "minLength": 1, "maxLength": 255},
            "authority_excerpt": {"type": "string", "minLength": 1, "maxLength": 500},
            "context_source_ids": {"type": "array", "uniqueItems": true, "items": {"type": "string", "minLength": 1, "maxLength": 255}},
            "statement": {"type": "string", "minLength": 1, "maxLength": 1000},
            "disposition": {"enum": ["adopt", "reject", "forbid", "defer", "delegate", "reopen", "supersede"]},
            "confidence": {"enum": ["high", "medium"]},
            "rationale": {"type": ["string", "null"], "maxLength": 1000},
            "supersedes_decision_id": {"type": ["string", "null"], "maxLength": 128}
        }
    });
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "required": ["verdicts", "needs_context", "complete"],
        "properties": {
            "complete": {"const": true},
            "needs_context": {"type": "boolean"},
            "verdicts": {
                "type": "array",
                "maxItems": 100,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["authority_source_id", "verdict", "decisions"],
                    "properties": {
                        "authority_source_id": {"type": "string", "minLength": 1, "maxLength": 255},
                        "verdict": {"enum": ["decision", "no_decision"]},
                        "decisions": {"type": "array", "maxItems": 100, "items": candidate}
                    }
                }
            }
        }
    })
}

#[allow(clippy::too_many_lines)]
fn build_request(
    requester_id: &str,
    job_id: &str,
    transcript: &ThreadTranscript,
    toolset: ToolsetRef,
    window_start: i64,
    window_end: i64,
    observation: Option<ObservationContract<'_>>,
) -> AppResult<JobRequestV1> {
    let prompt_value = if let Some(observation) = observation {
        let authority_source_ids = transcript
            .messages
            .iter()
            .filter(|message| {
                message.role == MessageRole::User
                    && message.turn_id == observation.authority_turn_id
                    && message.occurred_at >= window_start
                    && message.occurred_at < window_end
            })
            .map(|message| message.item_id.as_str())
            .collect::<BTreeSet<_>>();
        let preceding_context = transcript
            .messages
            .iter()
            .enumerate()
            .filter(|(_, message)| authority_source_ids.contains(message.item_id.as_str()))
            .filter_map(|(index, _)| {
                transcript.messages[..index]
                    .iter()
                    .rev()
                    .find(|message| message.role == MessageRole::Assistant)
                    .map(|message| message.item_id.as_str())
            })
            .collect::<BTreeSet<_>>();
        let result_context = transcript
            .messages
            .iter()
            .rev()
            .find(|message| {
                message.role == MessageRole::Assistant
                    && message.turn_id == observation.authority_turn_id
                    && !message.text.trim().is_empty()
            })
            .map(|message| message.item_id.as_str());
        let messages = transcript
            .messages
            .iter()
            .map(|message| {
                let relation = if authority_source_ids.contains(message.item_id.as_str()) {
                    "authority_turn"
                } else if Some(message.item_id.as_str()) == result_context {
                    "result_context"
                } else if preceding_context.contains(message.item_id.as_str()) {
                    "preceding_context"
                } else {
                    "earlier_context"
                };
                json!({
                    "source_id": message.item_id,
                    "relation": relation,
                    "role": message.role.as_str(),
                    "occurred_at": message.occurred_at,
                    "timestamp_precision": message.precision.as_str(),
                    "text": message.text
                })
            })
            .collect::<Vec<_>>();
        json!({
            "scope": if observation.allow_needs_context {
                "level_0_current_authority_with_bounded_context"
            } else {
                "level_1_complete_thread_prefix"
            },
            "context_expansion_available": observation.allow_needs_context,
            "authority_turn_id": observation.authority_turn_id,
            "authority_source_ids": authority_source_ids,
            "durable_effects": [{"kind": "file_change", "count": observation.file_change_count}],
            "thread": {"host_id": transcript.host_id, "id": transcript.thread_id},
            "messages": messages
        })
    } else {
        let messages = transcript
            .messages
            .iter()
            .map(|message| {
                json!({
                    "source_id": message.item_id,
                    "role": message.role.as_str(),
                    "occurred_at": message.occurred_at,
                    "timestamp_precision": message.precision.as_str(),
                    "text": message.text
                })
            })
            .collect::<Vec<_>>();
        json!({
            "report_window": {"start": window_start, "end_exclusive": window_end},
            "thread": {"host_id": transcript.host_id, "id": transcript.thread_id},
            "messages": messages
        })
    };
    let prompt = serde_json::to_string(&prompt_value)
        .map_err(|error| AppError::new("classification_prompt_invalid", error.to_string()))?;
    let mut invocation = AgentInvocationV1::new(
        "codex",
        ModelId::new(MODEL),
        AbsolutePath::new(classifier_cwd()?),
        WorkspaceAccess::None,
        BuiltinToolsV1 {
            local_execution: false,
            web_search: false,
        },
        TimeoutSeconds::new(INVOCATION_TIMEOUT.as_secs()),
    );
    invocation.reasoning_effort = Some(ReasoningEffort::Medium);
    invocation.toolset = Some(toolset);
    let mut request = JobRequestV1::new(
        JobId::new(job_id),
        format!(
            "Classify {} in {}",
            if observation.is_some() {
                "one enacted turn"
            } else {
                "decisions"
            },
            transcript.thread_id
        ),
        Requester {
            program: "decisions".to_owned(),
            id: requester_id.to_owned(),
        },
        if observation.is_some() {
            OBSERVATION_INSTRUCTIONS
        } else {
            INSTRUCTIONS
        },
        prompt,
        invocation,
    );
    request.developer_instructions = Some(
        if observation.is_some() {
            OBSERVATION_DEVELOPER_INSTRUCTIONS
        } else {
            DEVELOPER_INSTRUCTIONS
        }
        .to_owned(),
    );
    Ok(request)
}

fn classifier_cwd() -> AppResult<std::path::PathBuf> {
    for path in [Path::new("/private/tmp"), Path::new("/tmp")] {
        if path.is_absolute() && path.is_dir() {
            return Ok(path.to_path_buf());
        }
    }
    Err(AppError::new(
        "classification_cwd_invalid",
        "no fixed private temporary working directory is available",
    ))
}

fn decode_and_validate(
    raw: &str,
    sources: &BTreeMap<&str, &SourceMessage>,
    window_start: i64,
    window_end: i64,
) -> Result<Vec<Candidate>, String> {
    let submitted: SubmittedClassification =
        serde_json::from_str(raw).map_err(|error| format!("invalid JSON arguments: {error}"))?;
    if !submitted.complete {
        return Err("complete must be true".to_owned());
    }
    if submitted.decisions.len() > 100 {
        return Err("at most 100 decisions may be submitted".to_owned());
    }
    let mut seen = BTreeSet::new();
    submitted
        .decisions
        .into_iter()
        .map(|submitted| {
            validate_candidate(submitted, sources, window_start, window_end, &mut seen)
        })
        .collect()
}

fn decode_and_validate_observation(
    raw: &str,
    sources: &BTreeMap<&str, &SourceMessage>,
    window_start: i64,
    window_end: i64,
    contract: ObservationContract<'_>,
) -> Result<ObservationClassification, String> {
    let submitted: SubmittedObservationClassification =
        serde_json::from_str(raw).map_err(|error| format!("invalid JSON arguments: {error}"))?;
    if !submitted.complete {
        return Err("complete must be true".to_owned());
    }
    let eligible = sources
        .values()
        .copied()
        .filter(|source| {
            source.role == MessageRole::User
                && source.turn_id == contract.authority_turn_id
                && source.occurred_at >= window_start
                && source.occurred_at < window_end
        })
        .map(|source| (source.item_id.as_str(), source))
        .collect::<BTreeMap<_, _>>();
    if eligible.is_empty() {
        return Err("the observation has no eligible authority sources".to_owned());
    }
    if submitted.needs_context {
        if !contract.allow_needs_context {
            return Err("needs_context is unavailable at this scope".to_owned());
        }
        if !submitted.verdicts.is_empty() {
            return Err("needs_context requires an empty verdict set".to_owned());
        }
        return Ok(ObservationClassification {
            candidates: Vec::new(),
            authority_verdicts: Vec::new(),
            needs_context: true,
        });
    }
    if submitted.verdicts.len() != eligible.len() {
        return Err("every eligible authority source must have exactly one verdict".to_owned());
    }
    let mut seen_authorities = BTreeSet::new();
    let mut seen_candidates = BTreeSet::new();
    let mut candidates = Vec::new();
    let mut authority_verdicts = Vec::new();
    for submitted_verdict in submitted.verdicts {
        if !seen_authorities.insert(submitted_verdict.authority_source_id.clone()) {
            return Err("duplicate authority verdict".to_owned());
        }
        let authority = eligible
            .get(submitted_verdict.authority_source_id.as_str())
            .copied()
            .ok_or_else(|| "verdict authority is outside the eligible authority set".to_owned())?;
        match submitted_verdict.verdict {
            AuthorityVerdict::Decision if submitted_verdict.decisions.is_empty() => {
                return Err("a decision verdict requires at least one decision".to_owned());
            }
            AuthorityVerdict::NoDecision if !submitted_verdict.decisions.is_empty() => {
                return Err("a no_decision verdict cannot contain decisions".to_owned());
            }
            AuthorityVerdict::Decision | AuthorityVerdict::NoDecision => {}
        }
        for decision in submitted_verdict.decisions {
            if decision.authority_source_id != authority.item_id {
                return Err("a nested decision must repeat its verdict authority".to_owned());
            }
            if decision.confidence == Confidence::Low {
                return Err("observation decisions must use high or medium confidence".to_owned());
            }
            candidates.push(validate_candidate(
                decision,
                sources,
                window_start,
                window_end,
                &mut seen_candidates,
            )?);
        }
        authority_verdicts.push(AuthorityMessageVerdict {
            authority: authority.clone(),
            verdict: submitted_verdict.verdict,
        });
    }
    if seen_authorities.len() != eligible.len()
        || eligible
            .keys()
            .any(|source_id| !seen_authorities.contains(*source_id))
    {
        return Err("every eligible authority source must have exactly one verdict".to_owned());
    }
    if candidates.len() > 100 {
        return Err("at most 100 decisions may be submitted".to_owned());
    }
    Ok(ObservationClassification {
        candidates,
        authority_verdicts,
        needs_context: false,
    })
}

fn validate_candidate(
    submitted: SubmittedCandidate,
    sources: &BTreeMap<&str, &SourceMessage>,
    window_start: i64,
    window_end: i64,
    seen: &mut BTreeSet<String>,
) -> Result<Candidate, String> {
    let authority = sources
        .get(submitted.authority_source_id.as_str())
        .copied()
        .ok_or_else(|| "unknown authority source".to_owned())?;
    if authority.role != MessageRole::User {
        return Err("authority source must be a user message".to_owned());
    }
    if authority.occurred_at < window_start || authority.occurred_at >= window_end {
        return Err("authority source is outside the report window".to_owned());
    }
    let statement = submitted.statement.trim();
    if statement.is_empty() || statement.len() > 1000 {
        return Err("statement must contain 1 to 1000 bytes".to_owned());
    }
    let rationale = submitted
        .rationale
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if rationale.as_ref().is_some_and(|value| value.len() > 1000) {
        return Err("rationale may contain at most 1000 bytes".to_owned());
    }
    validate_disclosure_text("statement", statement, sources.values().copied())?;
    if let Some(rationale) = &rationale {
        validate_disclosure_text("rationale", rationale, sources.values().copied())?;
    }
    if submitted.authority_excerpt.is_empty() || submitted.authority_excerpt.len() > 500 {
        return Err("authority_excerpt must contain 1 to 500 exact bytes".to_owned());
    }
    let mut matches = authority.text.match_indices(&submitted.authority_excerpt);
    let (authority_start, _) = matches.next().ok_or_else(|| {
        "authority_excerpt is not an exact substring of the authority message".to_owned()
    })?;
    if matches.next().is_some() {
        return Err(
            "authority_excerpt must occur exactly once in the authority message".to_owned(),
        );
    }
    let authority_end = authority_start + submitted.authority_excerpt.len();
    let mut context = Vec::new();
    let mut context_seen = BTreeSet::new();
    for source_id in submitted.context_source_ids {
        if !context_seen.insert(source_id.clone()) {
            return Err("duplicate context source".to_owned());
        }
        let source = sources
            .get(source_id.as_str())
            .copied()
            .ok_or_else(|| "unknown context source".to_owned())?;
        if source.thread_id != authority.thread_id {
            return Err("context source must be in the authority thread".to_owned());
        }
        context.push(source.clone());
    }
    let identity = format!(
        "{}\n{}\n{}\n{}",
        authority.host_id, authority.item_id, authority_start, authority_end
    );
    if !seen.insert(identity.clone()) {
        return Err("duplicate decision candidate".to_owned());
    }
    let digest = format!("{:x}", Sha256::digest(identity.as_bytes()));
    if submitted.supersedes_decision_id.as_ref().is_some_and(|id| {
        id.len() != 22
            || !id.starts_with("d_")
            || !id[2..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err("supersedes_decision_id is not a decision ID".to_owned());
    }
    Ok(Candidate {
        id: format!("d_{}", &digest[..20]),
        decided_at: authority.occurred_at,
        precision: authority.precision,
        statement: statement.to_owned(),
        disposition: submitted.disposition,
        confidence: submitted.confidence,
        rationale,
        supersedes_id: submitted.supersedes_decision_id,
        authority_start,
        authority_end,
        authority: authority.clone(),
        context,
    })
}

fn validate_disclosure_text<'a>(
    field: &str,
    value: &str,
    sources: impl Iterator<Item = &'a SourceMessage>,
) -> Result<(), String> {
    if value.chars().any(char::is_control) {
        return Err(format!("{field} contains control characters"));
    }
    let lowercase = value.to_ascii_lowercase();
    if value.contains('@')
        || FORBIDDEN_DISCLOSURE_MARKERS
            .iter()
            .any(|marker| lowercase.contains(marker))
        || contains_local_path(value)
        || disclosure_words(value).any(looks_sensitive_literal)
    {
        return Err(format!("{field} contains disclosure-sensitive content"));
    }
    for source in sources {
        if [
            source.host_id.as_str(),
            source.thread_id.as_str(),
            source.turn_id.as_str(),
            source.item_id.as_str(),
        ]
        .iter()
        .any(|identifier| !identifier.is_empty() && value.contains(identifier))
        {
            return Err(format!("{field} contains a source identifier"));
        }
        if shares_raw_fragment(value, &source.text) {
            return Err(format!("{field} contains a raw transcript fragment"));
        }
        if disclosure_words(value).any(|word| {
            let canonical_word = canonical_identifier(word);
            disclosure_words(&source.text).any(|source_word| {
                !canonical_word.is_empty()
                    && canonical_word == canonical_identifier(source_word)
                    && (looks_copied_identifier(word) || looks_copied_identifier(source_word))
            })
        }) {
            return Err(format!("{field} contains a copied sensitive identifier"));
        }
    }
    Ok(())
}

fn canonical_identifier(value: &str) -> String {
    value
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| char::from(byte.to_ascii_lowercase()))
        .collect()
}

fn disclosure_words(value: &str) -> impl Iterator<Item = &str> {
    value
        .split_whitespace()
        .map(|word| word.trim_matches(|character: char| !character.is_ascii_alphanumeric()))
}

fn looks_sensitive_literal(word: &str) -> bool {
    let lowercase = word.to_ascii_lowercase();
    (word.len() >= 12
        && CREDENTIAL_PREFIXES
            .iter()
            .any(|prefix| lowercase.starts_with(prefix)))
        || looks_like_uuid(word)
        || (word.len() >= 10 && word.bytes().all(|byte| byte.is_ascii_digit()))
        || looks_high_entropy_literal(word)
}

fn looks_high_entropy_literal(word: &str) -> bool {
    if word.len() < 12 {
        return false;
    }
    let has_lower = word.bytes().any(|byte| byte.is_ascii_lowercase());
    let has_upper = word.bytes().any(|byte| byte.is_ascii_uppercase());
    let has_digit = word.bytes().any(|byte| byte.is_ascii_digit());
    (has_digit && (has_lower || has_upper))
        || (word.contains('_') && has_upper)
        || (word.len() >= 20 && has_lower && has_upper)
}

fn looks_copied_identifier(word: &str) -> bool {
    let has_lower = word.bytes().any(|byte| byte.is_ascii_lowercase());
    let has_upper = word.bytes().any(|byte| byte.is_ascii_uppercase());
    let has_digit = word.bytes().any(|byte| byte.is_ascii_digit());
    let has_symbol = word
        .bytes()
        .any(|byte| matches!(byte, b'_' | b'-' | b'.' | b'/' | b'+' | b'='));
    (word.len() >= 7 && has_digit && (has_lower || has_upper))
        || (word.len() >= 12 && has_symbol && (has_lower || has_upper || has_digit))
        || (word.len() >= 20 && has_lower && has_upper)
}

fn looks_like_uuid(word: &str) -> bool {
    word.len() == 36
        && word.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn contains_local_path(value: &str) -> bool {
    value.split_whitespace().any(|word| {
        let word = word.trim_matches(|character: char| {
            matches!(
                character,
                '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | '"' | '\''
            )
        });
        word.contains('/')
            || word.contains('\\')
            || (word.len() >= 3
                && word.as_bytes()[0].is_ascii_alphabetic()
                && word.as_bytes()[1] == b':'
                && matches!(word.as_bytes()[2], b'/' | b'\\'))
    })
}

fn shares_raw_fragment(value: &str, transcript: &str) -> bool {
    const MIN_FRAGMENT_CHARS: usize = 24;
    let value = value.to_lowercase();
    let transcript = transcript.to_lowercase();
    let boundaries = value
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(value.len()))
        .collect::<Vec<_>>();
    if boundaries.len() <= MIN_FRAGMENT_CHARS {
        return false;
    }
    boundaries
        .windows(MIN_FRAGMENT_CHARS + 1)
        .any(|window| transcript.contains(&value[window[0]..window[MIN_FRAGMENT_CHARS]]))
}

fn invalid_classification_result() -> Value {
    json!({
        "error": {
            "code": "invalid_classification",
            "message": "classification arguments were invalid; use catalog IDs, the exact schema, and privacy-normalized prose without sensitive values or metadata"
        }
    })
}

async fn retry_transport<T, F, Fut>(mut operation: F) -> AppResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, ClientError>>,
{
    loop {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(ClientError::Transport { .. }) => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => return Err(client_error(error)),
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn client_error(_error: ClientError) -> AppError {
    AppError::new(
        "nucleus_request_failed",
        "Nucleus request failed; inspect Nucleus diagnostics",
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nucleus_client::NucleusClient;
    use nucleus_core::{
        JobId, JobState, ReasoningEffort, Requester, SchemaId, ToolCallId, ToolResultV1, ToolsetRef,
    };
    use serde_json::value::RawValue;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    use crate::model::{MessageRole, Precision, SourceMessage, ThreadTranscript};
    use crate::store::Store;

    use super::{
        AbandonmentAction, ClassificationResult, ObservationContract, Runner, abandonment_action,
        build_request, decode_and_validate, decode_and_validate_observation,
        invalid_classification_result, post_result_or_resolve_terminal,
    };

    async fn read_http_request(connection: &mut tokio::net::UnixStream) -> std::io::Result<String> {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4_096];
        loop {
            let read = connection.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        Ok(String::from_utf8_lossy(&request).into_owned())
    }

    async fn serve_http_response(
        listener: &tokio::net::UnixListener,
        expected_request: &str,
        status: &str,
        body: &str,
    ) -> std::io::Result<()> {
        let (mut connection, _) = listener.accept().await?;
        let request = read_http_request(&mut connection).await?;
        assert!(
            request.starts_with(expected_request),
            "request was {request:?}"
        );
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        connection.write_all(response.as_bytes()).await
    }

    #[allow(clippy::too_many_lines)]
    async fn terminal_owner_resolution(
        with_success_receipt: bool,
    ) -> Result<
        Result<Option<ClassificationResult>, crate::error::AppError>,
        Box<dyn std::error::Error>,
    > {
        let directory = tempfile::tempdir()?;
        let socket = directory.path().join("nucleus.sock");
        let listener = tokio::net::UnixListener::bind(&socket)?;
        let transcript = ThreadTranscript {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            messages: Vec::new(),
        };
        let request = build_request(
            "requester",
            "job",
            &transcript,
            ToolsetRef {
                provider: "decisions".to_owned(),
                name: "daily-classification".to_owned(),
                version: 1,
            },
            0,
            1,
            None,
        )?;
        let terminal_job = serde_json::json!({
            "version": 1,
            "summary": {
                "version": 1,
                "id": "job",
                "label": "test",
                "requester": {"program": "decisions", "id": "requester"},
                "state": "completed",
                "requestDigest": "digest",
                "createdAt": "2026-08-31T00:00:00Z",
                "updatedAt": "2026-08-31T00:00:01Z",
                "completedAt": "2026-08-31T00:00:01Z"
            },
            "request": request,
            "attempts": []
        })
        .to_string();
        let terminal_error = serde_json::json!({
            "version": 1,
            "code": "job_terminal",
            "message": "job is terminal",
            "issues": []
        })
        .to_string();
        let server = tokio::spawn(async move {
            serve_http_response(
                &listener,
                "POST /v1/jobs/job/tool-calls/call/result HTTP/1.1",
                "409 Conflict",
                &terminal_error,
            )
            .await?;
            serve_http_response(
                &listener,
                "GET /v1/jobs/job HTTP/1.1",
                "200 OK",
                &terminal_job,
            )
            .await
        });

        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let run = store.begin_or_resume_run("2026-08-31", 0, 1, "manifest")?;
        store.plan_job(&run.id, "thread", "job")?;
        store.begin_job_admission("job")?;
        let (result_json, is_error, classification) = if with_success_receipt {
            (
                r#"{"accepted":true,"candidate_count":0}"#,
                false,
                Some(&[][..]),
            )
        } else {
            (
                r#"{"error":{"code":"invalid_classification","message":"invalid"}}"#,
                true,
                None,
            )
        };
        store.persist_classification_receipt(
            "job",
            "call",
            result_json,
            is_error,
            classification,
        )?;
        let result = ToolResultV1 {
            version: 1,
            call_id: ToolCallId::from("call"),
            requester: Requester {
                program: "decisions".to_owned(),
                id: "requester".to_owned(),
            },
            result_schema_id: SchemaId::new(super::RESULT_SCHEMA_ID),
            result: RawValue::from_string(result_json.to_owned())?,
            is_error,
        };
        let resolution = post_result_or_resolve_terminal(
            &NucleusClient::new(&socket)?,
            &store,
            &JobId::new("job"),
            &ToolCallId::from("call"),
            &result,
            false,
        )
        .await;
        server.await??;
        Ok(resolution)
    }

    #[test]
    fn classification_request_uses_bounded_medium_effort() -> Result<(), Box<dyn std::error::Error>>
    {
        let transcript = ThreadTranscript {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            messages: Vec::new(),
        };
        let request = build_request(
            "requester",
            "job",
            &transcript,
            ToolsetRef {
                provider: "decisions".to_owned(),
                name: "daily-classification".to_owned(),
                version: 1,
            },
            0,
            1,
            None,
        )?;
        assert_eq!(
            request.invocation.reasoning_effort,
            Some(ReasoningEffort::Medium)
        );
        Ok(())
    }

    #[test]
    fn terminal_owner_rejection_recovers_accepted_durable_receipt()
    -> Result<(), Box<dyn std::error::Error>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let resolution = runtime.block_on(terminal_owner_resolution(true))??;
        let classification = resolution.ok_or("missing terminal classification")?;
        assert!(classification.candidates.is_empty());
        Ok(())
    }

    #[test]
    fn terminal_owner_rejection_without_success_is_terminal_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let resolution = runtime.block_on(terminal_owner_resolution(false))?;
        let Err(error) = resolution else {
            return Err("terminal owner without success unexpectedly completed".into());
        };
        assert_eq!(error.code, "classification_incomplete");
        Ok(())
    }

    #[test]
    fn classify_enters_its_runtime_before_creating_timers() -> Result<(), Box<dyn std::error::Error>>
    {
        assert!(tokio::runtime::Handle::try_current().is_err());
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(&directory.path().join("decisions.db"))?;
        let runner = Runner {
            socket: Some(directory.path().join("missing-nucleus.sock")),
        };
        let transcript = ThreadTranscript {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            messages: Vec::new(),
        };

        let result = runner.classify(&mut store, "requester", "job", &transcript, 0, 1);
        let Err(error) = result else {
            return Err("classification unexpectedly reached Nucleus".into());
        };
        assert_eq!(error.code, "nucleus_request_failed");
        Ok(())
    }

    #[test]
    fn refuses_assistant_authority() {
        let message = SourceMessage {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            turn_id: "turn".to_owned(),
            item_id: "item".to_owned(),
            role: MessageRole::Assistant,
            text: "Use it.".to_owned(),
            occurred_at: 10,
            precision: Precision::Item,
        };
        let sources = BTreeMap::from([("item", &message)]);
        let raw = r#"{"decisions":[{"authority_source_id":"item","authority_excerpt":"Use it.","context_source_ids":[],"statement":"Use it.","disposition":"adopt","confidence":"high"}],"complete":true}"#;
        assert!(decode_and_validate(raw, &sources, 0, 20).is_err());
    }

    #[test]
    fn accepts_explicit_user_authority() -> Result<(), String> {
        let message = SourceMessage {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            turn_id: "turn".to_owned(),
            item_id: "item".to_owned(),
            role: MessageRole::User,
            text: "Use it.".to_owned(),
            occurred_at: 10,
            precision: Precision::Turn,
        };
        let sources = BTreeMap::from([("item", &message)]);
        let raw = r#"{"decisions":[{"authority_source_id":"item","authority_excerpt":"Use it.","context_source_ids":[],"statement":"Use the proposed design.","disposition":"adopt","confidence":"high"}],"complete":true}"#;
        let result = decode_and_validate(raw, &sources, 0, 20)?;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].precision, Precision::Turn);
        Ok(())
    }

    #[test]
    fn observation_requires_one_verdict_for_each_eligible_authority() -> Result<(), String> {
        let first = SourceMessage {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            turn_id: "turn".to_owned(),
            item_id: "first".to_owned(),
            role: MessageRole::User,
            text: "Use it.".to_owned(),
            occurred_at: 10,
            precision: Precision::Item,
        };
        let second = SourceMessage {
            item_id: "second".to_owned(),
            text: "No decision here.".to_owned(),
            occurred_at: 11,
            ..first.clone()
        };
        let sources = BTreeMap::from([("first", &first), ("second", &second)]);
        let contract = ObservationContract {
            authority_turn_id: "turn",
            allow_needs_context: true,
            file_change_count: 1,
        };
        let complete = r#"{
            "verdicts":[
                {"authority_source_id":"first","verdict":"decision","decisions":[{
                    "authority_source_id":"first","authority_excerpt":"Use it.",
                    "context_source_ids":[],"statement":"Use the proposed design.",
                    "disposition":"adopt","confidence":"high"
                }]},
                {"authority_source_id":"second","verdict":"no_decision","decisions":[]}
            ],
            "needs_context":false,
            "complete":true
        }"#;
        let result = decode_and_validate_observation(complete, &sources, 10, 20, contract)?;
        assert_eq!(result.authority_verdicts.len(), 2);
        assert_eq!(result.candidates.len(), 1);
        let missing = r#"{
            "verdicts":[
                {"authority_source_id":"first","verdict":"no_decision","decisions":[]}
            ],
            "needs_context":false,
            "complete":true
        }"#;
        assert!(decode_and_validate_observation(missing, &sources, 10, 20, contract).is_err());
        let context = r#"{"verdicts":[],"needs_context":true,"complete":true}"#;
        assert!(
            decode_and_validate_observation(context, &sources, 10, 20, contract)?.needs_context
        );
        assert!(
            decode_and_validate_observation(
                context,
                &sources,
                10,
                20,
                ObservationContract {
                    allow_needs_context: false,
                    ..contract
                }
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn observation_rejects_low_confidence_candidate() {
        let authority = SourceMessage {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            turn_id: "turn".to_owned(),
            item_id: "authority".to_owned(),
            role: MessageRole::User,
            text: "Use it.".to_owned(),
            occurred_at: 10,
            precision: Precision::Item,
        };
        let sources = BTreeMap::from([("authority", &authority)]);
        let raw = r#"{
            "verdicts":[{"authority_source_id":"authority","verdict":"decision","decisions":[{
                "authority_source_id":"authority","authority_excerpt":"Use it.",
                "context_source_ids":[],"statement":"Use the proposed design.",
                "disposition":"adopt","confidence":"low"
            }]}],
            "needs_context":false,
            "complete":true
        }"#;
        assert!(
            decode_and_validate_observation(
                raw,
                &sources,
                10,
                20,
                ObservationContract {
                    authority_turn_id: "turn",
                    allow_needs_context: true,
                    file_change_count: 1,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn identity_uses_only_canonical_item_and_span() -> Result<(), String> {
        let message = SourceMessage {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            turn_id: "turn".to_owned(),
            item_id: "item".to_owned(),
            role: MessageRole::User,
            text: "Use it.".to_owned(),
            occurred_at: 10,
            precision: Precision::Item,
        };
        let sources = BTreeMap::from([("item", &message)]);
        let adopted = decode_and_validate(
            r#"{"decisions":[{"authority_source_id":"item","authority_excerpt":"Use it.","context_source_ids":[],"statement":"Use the design.","disposition":"adopt","confidence":"high"}],"complete":true}"#,
            &sources,
            0,
            20,
        )?;
        let rejected = decode_and_validate(
            r#"{"decisions":[{"authority_source_id":"item","authority_excerpt":"Use it.","context_source_ids":[],"statement":"Reject the alternative.","disposition":"reject","confidence":"medium"}],"complete":true}"#,
            &sources,
            0,
            20,
        )?;
        assert_eq!(adopted[0].id, rejected[0].id);
        let mut forked_message = message;
        forked_message.thread_id = "newest-fork".to_owned();
        forked_message.turn_id = "fork-turn".to_owned();
        let forked_sources = BTreeMap::from([("item", &forked_message)]);
        let forked = decode_and_validate(
            r#"{"decisions":[{"authority_source_id":"item","authority_excerpt":"Use it.","context_source_ids":[],"statement":"Use the design.","disposition":"adopt","confidence":"high"}],"complete":true}"#,
            &forked_sources,
            0,
            20,
        )?;
        assert_eq!(adopted[0].id, forked[0].id);
        Ok(())
    }

    #[test]
    fn invalid_result_never_echoes_model_controlled_source_text() {
        let secret = "SECRET_TRANSCRIPT_TEXT_AS_A_FAKE_SOURCE_ID";
        let raw = format!(
            r#"{{"decisions":[{{"authority_source_id":"{secret}","authority_excerpt":"x","context_source_ids":[],"statement":"Use it.","disposition":"adopt","confidence":"high"}}],"complete":true}}"#
        );
        let sources = BTreeMap::new();
        assert!(decode_and_validate(&raw, &sources, 0, 20).is_err());
        assert!(!invalid_classification_result().to_string().contains(secret));
    }

    #[test]
    fn rejects_unattended_disclosure_content() {
        let message = SourceMessage {
            host_id: "host-secret".to_owned(),
            thread_id: "thread-secret".to_owned(),
            turn_id: "turn-secret".to_owned(),
            item_id: "item-secret".to_owned(),
            role: MessageRole::User,
            text: "Use the private deployment configuration from yesterday.".to_owned(),
            occurred_at: 10,
            precision: Precision::Item,
        };
        let sources = BTreeMap::from([("item-secret", &message)]);
        for statement in [
            "Use API key SECRET_SENTINEL.",
            "Use /Users/person/private/config.json.",
            "Update decisions/src/main.rs.",
            "Set password huntertwo.",
            "Notify person@example.com.",
            "Use source_id item-secret.",
            "Include the tool result in the digest.",
            "Use the private deployment configuration from yesterday.",
            "Use this.\nSECRET_SENTINEL",
        ] {
            let raw = serde_json::json!({
                "decisions": [{
                    "authority_source_id": "item-secret",
                    "authority_excerpt": "Use the private deployment configuration",
                    "context_source_ids": [],
                    "statement": statement,
                    "disposition": "adopt",
                    "confidence": "high"
                }],
                "complete": true
            })
            .to_string();
            assert!(
                decode_and_validate(&raw, &sources, 0, 20).is_err(),
                "unexpectedly accepted {statement:?}"
            );
        }
    }

    #[test]
    fn rejects_short_copied_token_and_account_identifiers() {
        let message = SourceMessage {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            turn_id: "turn".to_owned(),
            item_id: "item".to_owned(),
            role: MessageRole::User,
            text: "Use AKIAIOSFODNN7EXAMPLE with account 123456789012 and tenant 123e4567-e89b-12d3-a456-426614174000.".to_owned(),
            occurred_at: 10,
            precision: Precision::Item,
        };
        let sources = BTreeMap::from([("item", &message)]);
        for statement in [
            "Retain AKIAIOSFODNN7EXAMPLE.",
            "Use account 123456789012.",
            "Use tenant 123e4567-e89b-12d3-a456-426614174000.",
        ] {
            let raw = serde_json::json!({
                "decisions": [{
                    "authority_source_id": "item",
                    "authority_excerpt": "Use AKIAIOSFODNN7EXAMPLE",
                    "context_source_ids": [],
                    "statement": statement,
                    "disposition": "adopt",
                    "confidence": "high"
                }],
                "complete": true
            })
            .to_string();
            assert!(
                decode_and_validate(&raw, &sources, 0, 20).is_err(),
                "unexpectedly accepted {statement:?}"
            );
        }
    }

    #[test]
    fn rejects_sensitive_rationale_and_noncanonical_supersession() {
        let message = SourceMessage {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            turn_id: "turn".to_owned(),
            item_id: "item".to_owned(),
            role: MessageRole::User,
            text: "Use it.".to_owned(),
            occurred_at: 10,
            precision: Precision::Item,
        };
        let sources = BTreeMap::from([("item", &message)]);
        let rationale = r#"{"decisions":[{"authority_source_id":"item","authority_excerpt":"Use it.","context_source_ids":[],"statement":"Use the proposed design.","disposition":"adopt","confidence":"high","rationale":"Password SECRET_SENTINEL"}],"complete":true}"#;
        assert!(decode_and_validate(rationale, &sources, 0, 20).is_err());
        let supersession = r#"{"decisions":[{"authority_source_id":"item","authority_excerpt":"Use it.","context_source_ids":[],"statement":"Use the proposed design.","disposition":"supersede","confidence":"high","supersedes_decision_id":"d_AAAAAAAAAAAAAAAAAAAA"}],"complete":true}"#;
        assert!(decode_and_validate(supersession, &sources, 0, 20).is_err());
    }

    #[test]
    fn rejects_case_folded_copied_identifier() {
        let message = SourceMessage {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            turn_id: "turn".to_owned(),
            item_id: "item".to_owned(),
            role: MessageRole::User,
            text: "Use AbCdEfGhIjKlMnOpQrStUvWx for the deployment.".to_owned(),
            occurred_at: 10,
            precision: Precision::Item,
        };
        let sources = BTreeMap::from([("item", &message)]);
        let raw = r#"{"decisions":[{"authority_source_id":"item","authority_excerpt":"Use AbCdEfGhIjKlMnOpQrStUvWx","context_source_ids":[],"statement":"Retain abcdefghijklmnopqrstuvwx.","disposition":"adopt","confidence":"high"}],"complete":true}"#;
        assert!(decode_and_validate(raw, &sources, 0, 20).is_err());
    }

    #[test]
    fn rejects_punctuation_normalized_copied_identifier() {
        let message = SourceMessage {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            turn_id: "turn".to_owned(),
            item_id: "item".to_owned(),
            role: MessageRole::User,
            text: "Use AbCd-EfGhIjKlMnOpQrStUvWx for the deployment.".to_owned(),
            occurred_at: 10,
            precision: Precision::Item,
        };
        let sources = BTreeMap::from([("item", &message)]);
        let raw = r#"{"decisions":[{"authority_source_id":"item","authority_excerpt":"Use AbCd-EfGhIjKlMnOpQrStUvWx","context_source_ids":[],"statement":"Retain abcdefghijklmnopqrstuvwx.","disposition":"adopt","confidence":"high"}],"complete":true}"#;
        assert!(decode_and_validate(raw, &sources, 0, 20).is_err());
    }

    #[test]
    fn rejects_wrapped_case_folded_short_token() {
        let message = SourceMessage {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            turn_id: "turn".to_owned(),
            item_id: "item".to_owned(),
            role: MessageRole::User,
            text: "Set password hunter2.".to_owned(),
            occurred_at: 10,
            precision: Precision::Item,
        };
        let sources = BTreeMap::from([("item", &message)]);
        let raw = r#"{"decisions":[{"authority_source_id":"item","authority_excerpt":"Set password hunter2.","context_source_ids":[],"statement":"Retain -HUNTER2-.","disposition":"adopt","confidence":"high"}],"complete":true}"#;
        assert!(decode_and_validate(raw, &sources, 0, 20).is_err());
    }

    #[test]
    fn abandonment_requires_terminal_or_cancellable_job_state()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            abandonment_action(Some(&JobState::Completed), true)?,
            AbandonmentAction::Terminal
        );
        assert_eq!(
            abandonment_action(Some(&JobState::Running), true)?,
            AbandonmentAction::Cancel
        );
        assert_eq!(
            abandonment_action(None, false)?,
            AbandonmentAction::Terminal
        );
        assert!(abandonment_action(None, true).is_err());
        Ok(())
    }
}
