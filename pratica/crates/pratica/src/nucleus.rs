#![allow(dead_code)]

use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{
    AbsolutePath, AgentInvocationV1, AttemptState, BuiltinToolsV1, HarnessCapability, JobId,
    JobRequestV1, JobState, ModelId, PROTOCOL_VERSION_V1, ReasoningEffort, Requester, SchemaId,
    TimeoutSeconds, ToolCallId, ToolCallState, ToolCallsQueryV1, ToolResultV1, ToolsetRef,
    WorkspaceAccess,
};
use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;
use tokio::runtime::Builder;
use uuid::Uuid;

use crate::agent_contracts::{
    AgentCall, AgentStage, CompositionOutcome as AgentCompositionOutcome, CompositionSubmission,
    ConformanceOutcome as AgentConformanceOutcome, ConformanceSubmission, StewardSubmission,
    decode_call, schema_registrations, tool_for_name, toolset_registration,
};
use crate::error::{AppError, AppResult};
use crate::model::{
    AgentAttemptView, AttemptKind, AttemptSourceInput, BasisKind, CompositionAgreementRef,
    CompositionOutcome as StoredCompositionOutcome, ConformanceOutcome as StoredConformanceOutcome,
    FrozenBasisView, IntegrationView, NegotiationStatus, NewAgentAttempt, NewFrozenBasis,
    NewFrozenSource, OpaqueMarkdown, RosterView, RuntimeState, StewardResponse, StewardScopeView,
    ToolReceiptView, sha256_hex,
};
use crate::source_catalog::{
    FrozenSource, FrozenSourceCatalog, MANIFEST_SCHEMA_VERSION, MAX_CATALOG_BYTES,
    MAX_SOURCE_BYTES, MAX_SOURCES,
};
use crate::store::{Store, StoreError, StoreErrorKind};

const MODEL: &str = "gpt-5.6-terra";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const TRANSPORT_RETRY_DELAY: Duration = Duration::from_millis(100);
const CANCELLATION_CONFIRM_TIMEOUT: Duration = Duration::from_secs(30);
const COMPOSITION_VERIFIER_VERSION: &str = "pratica-composition-v1";

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AgentRunOutcome {
    pub(crate) attempt: AgentAttemptView,
    pub(crate) result_kind: String,
    pub(crate) result_id: String,
}

#[derive(Clone, Debug)]
pub(crate) struct AgentRunner {
    socket: Option<PathBuf>,
}

impl AgentRunner {
    #[must_use]
    pub(crate) const fn for_current_user() -> Self {
        Self { socket: None }
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_socket(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: Some(socket.into()),
        }
    }

    pub(crate) fn doctor(&self) -> AppResult<()> {
        runtime()?.block_on(async {
            let client = self.client()?;
            require_health(&client).await?;
            for stage in stages() {
                register_contract(&client, stage).await?;
            }
            Ok(())
        })
    }

    pub(crate) fn steward_response(
        &self,
        store: &mut Store,
        negotiation_id: &str,
    ) -> AppResult<AgentRunOutcome> {
        if let Some(attempt) =
            store_result(store.active_attempt(AttemptKind::StewardResponse, negotiation_id))?
        {
            return self.run_attempt(store, &attempt);
        }

        let negotiation = store_result(store.negotiation(negotiation_id))?;
        if negotiation.status != NegotiationStatus::Open {
            return Err(AppError::new(
                "negotiation_not_open",
                format!("negotiation {negotiation_id} is not open"),
            ));
        }
        let head = negotiation.head.as_ref().ok_or_else(|| {
            AppError::new(
                "negotiation_head_missing",
                format!("negotiation {negotiation_id} has no current offer"),
            )
        })?;
        let track = store_result(store.track(&negotiation.track_id))?;
        let scope = store_result(store.steward_scope(&track.scope_id, track.scope_version))?;
        let basis = store_result(store.steward_basis(&track.scope_id, track.scope_version))?;
        let catalog = catalog_from_steward(&scope, &basis)?;
        catalog.verify_sources_current().map_err(catalog_error)?;
        reject_implicit_retry(
            store_result(store.latest_attempt(AttemptKind::StewardResponse, negotiation_id))?
                .as_ref(),
            |latest| {
                latest.expected_offer_id.as_deref() == Some(head.offer_id.as_str())
                    && latest.basis_id.as_deref() == Some(basis.basis_id.as_str())
                    && latest.basis_digest == basis.manifest_sha256
                    && latest.catalog_sha256 == catalog.catalog_sha256
            },
        )?;
        let prompt = encode_prompt(json!({
            "stage": "steward_response",
            "negotiation_id": negotiation.negotiation_id,
            "track_id": track.track_id,
            "entrant_party": negotiation.entrant.party,
            "steward_party": negotiation.steward.party,
            "expected_offer_id": head.offer_id,
            "terms_sha256": head.terms_sha256,
            "terms_markdown": head.terms_markdown,
            "scope": {
                "scope_id": scope.scope_id,
                "version": scope.version,
                "title": scope.title,
                "charter_markdown": scope.charter_markdown,
            },
            "basis": basis_prompt(&basis),
            "catalog_sha256": catalog.catalog_sha256,
        }))?;
        let attempt = Self::prepare_attempt(
            store,
            None,
            None,
            AgentStage::StewardResponse,
            negotiation_id,
            &prompt,
            Some(head.offer_id.clone()),
            None,
            Some(basis.basis_id.clone()),
            basis.manifest_sha256.clone(),
            &catalog,
        )?;
        self.run_attempt(store, &attempt)
    }

    pub(crate) fn composition_review(
        &self,
        store: &mut Store,
        integration_id: &str,
    ) -> AppResult<AgentRunOutcome> {
        if let Some(attempt) =
            store_result(store.active_attempt(AttemptKind::CompositionReview, integration_id))?
        {
            return self.run_attempt(store, &attempt);
        }

        let integration = store_result(store.integration(integration_id))?;
        let (roster, agreements, composition_digest) =
            store_result(store.composition_basis(integration_id))?;
        if agreements.is_empty() {
            return Err(AppError::new(
                "composition_basis_empty",
                "composition review requires at least one sealed active agreement",
            ));
        }
        let catalog = composition_catalog(store, &integration, &roster, &agreements)?;
        reject_implicit_retry(
            store_result(store.latest_attempt(AttemptKind::CompositionReview, integration_id))?
                .as_ref(),
            |latest| {
                latest.expected_roster_digest.as_deref() == Some(composition_digest.as_str())
                    && latest.catalog_sha256 == catalog.catalog_sha256
            },
        )?;
        let prompt = encode_prompt(json!({
            "stage": "composition_review",
            "integration": integration,
            "roster": roster,
            "composition_digest": composition_digest,
            "agreements": agreements,
            "basis": {
                "verifier_version": COMPOSITION_VERIFIER_VERSION,
                "observed_at": catalog.observed_at,
            },
            "catalog_sha256": catalog.catalog_sha256,
        }))?;
        let attempt = Self::prepare_attempt(
            store,
            None,
            None,
            AgentStage::CompositionReview,
            integration_id,
            &prompt,
            None,
            Some(composition_digest.clone()),
            None,
            composition_digest,
            &catalog,
        )?;
        self.run_attempt(store, &attempt)
    }

    pub(crate) fn conformance_review(
        &self,
        store: &mut Store,
        agreement_id: &str,
        candidate: &FrozenSourceCatalog,
    ) -> AppResult<AgentRunOutcome> {
        if let Some(attempt) =
            store_result(store.active_attempt(AttemptKind::ConformanceReview, agreement_id))?
        {
            if attempt.catalog_sha256 != candidate.catalog_sha256 {
                return Err(AppError::new(
                    "attempt_candidate_conflict",
                    "an active conformance attempt already uses a different candidate basis",
                ));
            }
            return self.run_attempt(store, &attempt);
        }

        candidate.verify_sources_current().map_err(catalog_error)?;
        let agreement = store_result(store.agreement(agreement_id))?;
        reject_implicit_retry(
            store_result(store.latest_attempt(AttemptKind::ConformanceReview, agreement_id))?
                .as_ref(),
            |latest| latest.catalog_sha256 == candidate.catalog_sha256,
        )?;
        let frozen_input = crate::app::basis_from_catalog(candidate, BasisKind::Candidate)?;
        let basis = store_result(store.freeze_candidate_basis(&frozen_input))?;
        let prompt = encode_prompt(json!({
            "stage": "conformance_review",
            "agreement_id": agreement.agreement_id,
            "track_id": agreement.track_id,
            "offer_id": agreement.offer.offer_id,
            "terms_sha256": agreement.offer.terms_sha256,
            "terms_markdown": agreement.offer.terms_markdown,
            "candidate": {
                "scope": candidate.scope,
                "version": candidate.version,
                "party": candidate.party,
                "title": candidate.title,
                "charter_markdown": candidate.charter_markdown,
            },
            "basis": basis_prompt(&basis),
            "catalog_sha256": candidate.catalog_sha256,
        }))?;
        let attempt = Self::prepare_attempt(
            store,
            None,
            None,
            AgentStage::ConformanceReview,
            agreement_id,
            &prompt,
            None,
            None,
            Some(basis.basis_id.clone()),
            basis.manifest_sha256.clone(),
            candidate,
        )?;
        self.run_attempt(store, &attempt)
    }

    pub(crate) fn retry(&self, store: &mut Store, attempt_id: &str) -> AppResult<AgentRunOutcome> {
        let attempt = store_result(store.attempt(attempt_id))?;
        if attempt.domain_result_id.is_some() {
            return Err(AppError::new(
                "attempt_already_committed",
                "an attempt with a committed Pratica result cannot be retried",
            ));
        }
        if attempt.active {
            return Err(AppError::new(
                "attempt_still_active",
                "an active attempt cannot be replaced; resume its owning command",
            ));
        }
        if !matches!(
            attempt.runtime_state,
            RuntimeState::Completed
                | RuntimeState::Failed
                | RuntimeState::Cancelled
                | RuntimeState::Lost
                | RuntimeState::TimedOut
        ) {
            return Err(AppError::new(
                "attempt_not_retryable",
                "only completed-without-result, failed, cancelled, lost, or timed-out attempts may be retried",
            ));
        }

        let stage = stage_for_kind(attempt.kind);
        let request = persisted_request(&attempt)?;
        let catalog = catalog_from_attempt(&attempt)?;
        revalidate_retry_target(store, &attempt, &catalog)?;
        let replacement = Self::prepare_attempt(
            store,
            Some(attempt.attempt_id.clone()),
            Some(attempt.requester_id.clone()),
            stage,
            &attempt.subject_id,
            &request.prompt,
            attempt.expected_offer_id.clone(),
            attempt.expected_roster_digest.clone(),
            attempt.basis_id.clone(),
            attempt.basis_digest.clone(),
            &catalog,
        )?;
        self.run_attempt(store, &replacement)
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_attempt(
        store: &mut Store,
        predecessor_attempt_id: Option<String>,
        requester_id: Option<String>,
        stage: AgentStage,
        subject_id: &str,
        prompt: &str,
        expected_offer_id: Option<String>,
        expected_roster_digest: Option<String>,
        basis_id: Option<String>,
        basis_digest: String,
        catalog: &FrozenSourceCatalog,
    ) -> AppResult<AgentAttemptView> {
        verify_catalog_snapshot(catalog)?;
        let (requester_id, nucleus_job_id) = new_attempt_identity(stage, requester_id);
        let neutral = neutral_cwd(&nucleus_job_id)?;
        let registration = toolset_registration(stage).map_err(contract_error)?;
        let request = build_request(
            stage,
            &requester_id,
            &nucleus_job_id,
            prompt,
            registration.toolset,
            &neutral,
        )?;
        let request_bytes = serde_json::to_vec(&request).map_err(json_error)?;
        let request_sha256 = digest(&request_bytes);
        let sources = catalog
            .sources
            .iter()
            .map(|source| {
                if digest(&source.content) != source.content_sha256 {
                    return Err(AppError::new(
                        "catalog_digest_mismatch",
                        format!("frozen source {} does not match its digest", source.id),
                    ));
                }
                let origin_path = source.origin_path.to_str().ok_or_else(|| {
                    AppError::new(
                        "source_path_not_utf8",
                        "frozen source origins must be representable as UTF-8",
                    )
                })?;
                Ok(AttemptSourceInput {
                    source_id: source.id.clone(),
                    kind: source.kind.clone(),
                    locator: source.locator.clone(),
                    origin_path: origin_path.to_owned(),
                    revision: source.revision.clone(),
                    content: source.content.clone(),
                    content_sha256: source.content_sha256.clone(),
                    observed_at: source.observed_at,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let charter = OpaqueMarkdown::from_text(catalog.charter_markdown.clone())?;
        let new_attempt = NewAgentAttempt {
            predecessor_attempt_id,
            kind: kind_for_stage(stage),
            subject_id: subject_id.to_owned(),
            requester_id,
            nucleus_job_id: nucleus_job_id.clone(),
            request_bytes,
            request_sha256,
            toolset_name: stage.slug().to_owned(),
            toolset_version: 1,
            expected_offer_id,
            expected_roster_digest,
            basis_id,
            basis_digest,
            catalog_scope: catalog.scope.clone(),
            catalog_version: catalog.version,
            catalog_verifier_version: catalog.verifier_version.clone(),
            catalog_observed_at: catalog.observed_at,
            catalog_party: catalog.party.clone(),
            catalog_title: catalog.title.clone(),
            catalog_charter_markdown: charter,
            catalog_charter_sha256: catalog.charter_sha256.clone(),
            catalog_sha256: catalog.catalog_sha256.clone(),
            sources,
        };
        match store.begin_or_resume_attempt(&new_attempt) {
            Ok(attempt) => {
                if attempt.nucleus_job_id != nucleus_job_id {
                    cleanup_neutral_cwd(&nucleus_job_id);
                }
                Ok(attempt)
            }
            Err(error) => {
                cleanup_neutral_cwd(&nucleus_job_id);
                Err(store_error(error))
            }
        }
    }

    pub(crate) fn run_attempt(
        &self,
        store: &mut Store,
        attempt: &AgentAttemptView,
    ) -> AppResult<AgentRunOutcome> {
        let attempt = store_result(store.attempt(&attempt.attempt_id))?;
        if committed_outcome_is_acknowledged(&attempt) {
            return outcome(attempt);
        }
        if !attempt.active {
            return Err(AppError::new(
                "attempt_terminal",
                "the attempt is terminal without a committed Pratica result; use attempt retry",
            ));
        }
        let request = persisted_request(&attempt)?;
        let catalog = catalog_from_attempt(&attempt)?;
        let neutral = neutral_cwd(&attempt.nucleus_job_id)?;
        if request.invocation.cwd.as_path() != neutral {
            cleanup_neutral_cwd(&attempt.nucleus_job_id);
            return Err(AppError::new(
                "attempt_cwd_conflict",
                "persisted Nucleus request does not use its deterministic neutral directory",
            ));
        }
        let result =
            runtime()?.block_on(self.run_attempt_async(store, &attempt, &request, &catalog));
        cleanup_neutral_cwd(&attempt.nucleus_job_id);
        result
    }

    async fn run_attempt_async(
        &self,
        store: &mut Store,
        attempt: &AgentAttemptView,
        request: &JobRequestV1,
        catalog: &FrozenSourceCatalog,
    ) -> AppResult<AgentRunOutcome> {
        let client = self.client()?;
        require_health(&client).await?;
        let stage = stage_for_kind(attempt.kind);
        let registration = register_contract(&client, stage).await?;
        if request.invocation.toolset.as_ref() != Some(&registration.toolset) {
            return Err(AppError::new(
                "attempt_toolset_conflict",
                "persisted request does not reference the exact registered Pratica toolset",
            ));
        }

        if attempt.admitted {
            let job_id = JobId::new(&attempt.nucleus_job_id);
            let job = match client.get_job(&job_id).await {
                Ok(job) => job,
                Err(ClientError::Api { status: 404, .. }) => {
                    return Err(AppError::new(
                        "attempt_admission_ambiguous",
                        "the admitted Nucleus job is unavailable; resubmission could create duplicate work",
                    ));
                }
                Err(error) => return Err(client_error(error)),
            };
            verify_job_identity(attempt, request, &job)?;
        } else {
            let accepted = submit_stably(&client, request).await?;
            if accepted.version != PROTOCOL_VERSION_V1
                || accepted.job_id.as_str() != attempt.nucleus_job_id
            {
                return Err(AppError::new(
                    "nucleus_job_mismatch",
                    "Nucleus admitted a different job identity",
                ));
            }
            verify_accepted_digest(attempt, &accepted.request_digest)?;
            store_result(store.mark_attempt_admitted(
                &attempt.attempt_id,
                accepted.job_id.as_str(),
                &attempt.request_sha256,
            ))?;
        }

        serve_mailbox(&client, store, attempt, catalog).await
    }

    fn client(&self) -> AppResult<NucleusClient> {
        match &self.socket {
            Some(socket) => NucleusClient::new(socket).map_err(client_error),
            None => NucleusClient::for_current_user().map_err(client_error),
        }
    }
}

fn new_attempt_identity(
    stage: AgentStage,
    preserved_requester_id: Option<String>,
) -> (String, String) {
    let suffix = Uuid::now_v7();
    let requester_id = preserved_requester_id.unwrap_or_else(|| format!("pratica-{suffix}"));
    let nucleus_job_id = format!("pratica-{}-{suffix}", stage.slug());
    (requester_id, nucleus_job_id)
}

fn committed_outcome_is_acknowledged(attempt: &AgentAttemptView) -> bool {
    attempt.domain_result_id.is_some() && !attempt.active
}

fn build_request(
    stage: AgentStage,
    requester_id: &str,
    job_id: &str,
    prompt: &str,
    toolset: ToolsetRef,
    neutral_cwd: &Path,
) -> AppResult<JobRequestV1> {
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
        format!("{} for {job_id}", stage.label()),
        Requester {
            program: "pratica".to_owned(),
            id: requester_id.to_owned(),
        },
        stage.instructions(),
        prompt,
        invocation,
    );
    request.developer_instructions = Some(AgentStage::developer_instructions().to_owned());
    request.validate().map_err(|error| {
        AppError::new(
            "nucleus_request_invalid",
            format!("unable to construct constrained Nucleus request: {error}"),
        )
    })?;
    Ok(request)
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
    let codex = health
        .harness
        .as_ref()
        .is_some_and(|identity| identity.harness.as_str() == "codex");
    if health.version != PROTOCOL_VERSION_V1
        || health.status != "ok"
        || !health.accepting_jobs
        || !health.authentication.configured
        || !health.authentication.authenticated
        || !health
            .supported_protocol_versions
            .contains(&PROTOCOL_VERSION_V1)
        || !codex
        || health.harness_executable.is_none()
        || !missing.is_empty()
    {
        return Err(AppError::new(
            "nucleus_not_ready",
            format!(
                "Nucleus is not ready: status={}, accepting_jobs={}, configured={}, authenticated={}, codex_harness={}, missing_capabilities={}",
                health.status,
                health.accepting_jobs,
                health.authentication.configured,
                health.authentication.authenticated,
                codex,
                missing.join(",")
            ),
        ));
    }
    Ok(())
}

async fn register_contract(
    client: &NucleusClient,
    stage: AgentStage,
) -> AppResult<nucleus_core::ToolsetRegistrationV1> {
    for schema in schema_registrations(stage).map_err(contract_error)? {
        let registered = client
            .register_schema(&schema)
            .await
            .map_err(client_error)?;
        if registered.id != schema.id || registered.digest != schema.digest {
            return Err(AppError::new(
                "nucleus_schema_conflict",
                format!(
                    "Nucleus registered a different schema for {}",
                    schema.id.as_str()
                ),
            ));
        }
    }
    let registration = toolset_registration(stage).map_err(contract_error)?;
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
            format!("Nucleus registered a different {} toolset", stage.slug()),
        ));
    }
    Ok(registration)
}

async fn submit_stably(
    client: &NucleusClient,
    request: &JobRequestV1,
) -> AppResult<nucleus_core::JobAcceptedV1> {
    let mut transport_failures = 0_u8;
    loop {
        match client.submit_job(request).await {
            Ok(accepted) => return Ok(accepted),
            Err(ClientError::Transport { .. }) if transport_failures < 2 => {
                transport_failures += 1;
                tokio::time::sleep(TRANSPORT_RETRY_DELAY).await;
            }
            Err(error @ ClientError::Validation(_)) => {
                return Err(AppError::new(
                    "nucleus_admission_rejected",
                    format!("Nucleus rejected the immutable request: {error}"),
                ));
            }
            Err(error @ ClientError::Api { status, .. })
                if explicit_nonretryable_rejection(status) =>
            {
                return Err(AppError::new(
                    "nucleus_admission_rejected",
                    format!("Nucleus rejected the immutable request: {error}"),
                ));
            }
            Err(error) => return Err(client_error(error)),
        }
    }
}

const fn explicit_nonretryable_rejection(status: u16) -> bool {
    (status >= 400 && status < 500) && !matches!(status, 408 | 409 | 425 | 429)
}

#[allow(clippy::too_many_lines)]
async fn serve_mailbox(
    client: &NucleusClient,
    store: &mut Store,
    initial_attempt: &AgentAttemptView,
    catalog: &FrozenSourceCatalog,
) -> AppResult<AgentRunOutcome> {
    let job_id = JobId::new(&initial_attempt.nucleus_job_id);
    let stage = stage_for_kind(initial_attempt.kind);
    let mut tool_after = initial_attempt.tool_after;
    for receipt in store_result(store.attempt_receipts(&initial_attempt.attempt_id))? {
        if terminal_receipt_error(&receipt)?.is_some() {
            let (code, message) = reconcile_terminal_error_receipt(
                client,
                store,
                initial_attempt,
                stage,
                &receipt,
                None,
            )
            .await?;
            return Err(AppError::new(code, message));
        }
    }
    loop {
        let prior_tool_after = tool_after;
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
                "Nucleus returned a mailbox page outside the admitted Pratica job",
            ));
        }
        for pending in calls.calls {
            let call = pending.call;
            let Some(tool) = tool_for_name(stage, &call.tool_name) else {
                return Err(AppError::new(
                    "nucleus_tool_contract_mismatch",
                    format!("Nucleus returned an unregistered tool {}", call.tool_name),
                ));
            };
            if pending.version != PROTOCOL_VERSION_V1
                || pending.state != ToolCallState::Pending
                || pending.answered_at.is_some()
                || call.version != PROTOCOL_VERSION_V1
                || call.job_id != job_id
                || call.request_sequence <= tool_after
                || call.arguments_schema_id.as_str() != tool.input_schema_id()
            {
                return Err(AppError::new(
                    "nucleus_tool_contract_mismatch",
                    "Nucleus returned a call outside the admitted Pratica tool contract",
                ));
            }
            let arguments_sha256 = digest(call.arguments.get().as_bytes());
            let receipt = match store_result(store.tool_receipt(job_id.as_str(), call.id.as_str()))?
            {
                Some(receipt) => cached_receipt(receipt, &arguments_sha256)?,
                None => dispatch_tool(
                    store,
                    initial_attempt,
                    catalog,
                    stage,
                    &call.tool_name,
                    call.id.as_str(),
                    &arguments_sha256,
                    call.arguments.get(),
                )?,
            };
            if terminal_receipt_error(&receipt)?.is_some() {
                let (code, message) = reconcile_terminal_error_receipt(
                    client,
                    store,
                    initial_attempt,
                    stage,
                    &receipt,
                    Some(call.request_sequence),
                )
                .await?;
                return Err(AppError::new(code, message));
            }
            let result = tool_result_from_receipt(initial_attempt, stage, &call.id, &receipt)?;
            let posted_sequence = post_result_stably(client, &job_id, &call.id, &result).await?;
            if posted_sequence != call.request_sequence {
                return Err(AppError::new(
                    "nucleus_tool_contract_mismatch",
                    "Nucleus acknowledged a different mailbox sequence",
                ));
            }
            tool_after = tool_after.max(posted_sequence);
        }

        if tool_after > prior_tool_after {
            store_result(
                store.advance_attempt_tool_after(&initial_attempt.attempt_id, tool_after),
            )?;
        }

        let refreshed = store_result(store.attempt(&initial_attempt.attempt_id))?;
        let job = client.get_job(&job_id).await.map_err(client_error)?;
        verify_job_identity(&refreshed, &persisted_request(&refreshed)?, &job)?;
        if job.summary.state.is_terminal() {
            let detail = job
                .attempts
                .last()
                .and_then(|attempt| attempt.terminal_message.as_deref());
            let runtime_state = runtime_state_for_job(&job);
            let terminal = store_result(store.mark_attempt_runtime_state(
                &refreshed.attempt_id,
                runtime_state,
                detail,
            ))?;
            if terminal.domain_result_id.is_some() {
                return outcome(terminal);
            }
            let message = detail.unwrap_or(match job.summary.state {
                JobState::Completed => {
                    "Nucleus completed without an accepted Pratica terminal submission"
                }
                JobState::Failed => "Nucleus failed without an accepted Pratica result",
                JobState::Cancelled => "Nucleus was cancelled without an accepted Pratica result",
                JobState::Accepted | JobState::Running | JobState::WaitingOnRequester => {
                    "Nucleus reported an invalid terminal state"
                }
            });
            return Err(AppError::new("nucleus_job_terminal", message));
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn dispatch_tool(
    store: &mut Store,
    attempt: &AgentAttemptView,
    catalog: &FrozenSourceCatalog,
    stage: AgentStage,
    tool_name: &str,
    call_id: &str,
    arguments_sha256: &str,
    arguments: &str,
) -> AppResult<ToolReceiptView> {
    let current = store_result(store.attempt(&attempt.attempt_id))?;
    if current.domain_result_id.is_some() {
        return record_attempt_closed_error(store, &current, call_id, arguments_sha256);
    }
    let call = match decode_call(stage, tool_name, arguments) {
        Ok(call) => call,
        Err(error) => {
            return record_managed_error(
                store,
                attempt,
                call_id,
                arguments_sha256,
                error.code(),
                error.message(),
            );
        }
    };
    match call {
        AgentCall::SourceCatalog(request) => record_catalog_result(
            store,
            attempt,
            call_id,
            arguments_sha256,
            catalog.catalog_page(&request),
        ),
        AgentCall::SourceRead(request) => record_catalog_result(
            store,
            attempt,
            call_id,
            arguments_sha256,
            catalog.read(&request),
        ),
        AgentCall::SourceSearch(request) => record_catalog_result(
            store,
            attempt,
            call_id,
            arguments_sha256,
            catalog.search(&request),
        ),
        AgentCall::SubmitStewardResponse(submission) => {
            let cited = submission.cited_source_refs().to_vec();
            if let Some(missing) = missing_citation(store, &attempt.attempt_id, &cited)? {
                return record_managed_error(
                    store,
                    attempt,
                    call_id,
                    arguments_sha256,
                    "citation_not_emitted",
                    &format!("source reference was not emitted by a managed read: {missing}"),
                );
            }
            if let Err(error) = catalog.verify_sources_current() {
                return record_managed_error(
                    store,
                    attempt,
                    call_id,
                    arguments_sha256,
                    "attempt_stale",
                    error.message(),
                );
            }
            let response = stored_steward_response(submission)?;
            match store.commit_steward_tool_response(
                &attempt.attempt_id,
                call_id,
                arguments_sha256,
                &attempt.basis_digest,
                &response,
                &cited,
            ) {
                Ok(receipt) => Ok(receipt),
                Err(error) => {
                    handle_terminal_store_error(store, attempt, call_id, arguments_sha256, error)
                }
            }
        }
        AgentCall::SubmitCompositionReview(submission) => {
            if let Some(missing) =
                missing_citation(store, &attempt.attempt_id, &submission.cited_source_refs)?
            {
                return record_managed_error(
                    store,
                    attempt,
                    call_id,
                    arguments_sha256,
                    "citation_not_emitted",
                    &format!("source reference was not emitted by a managed read: {missing}"),
                );
            }
            commit_composition(store, attempt, call_id, arguments_sha256, submission)
        }
        AgentCall::SubmitConformanceReview(submission) => {
            if let Some(missing) =
                missing_citation(store, &attempt.attempt_id, &submission.cited_source_refs)?
            {
                return record_managed_error(
                    store,
                    attempt,
                    call_id,
                    arguments_sha256,
                    "citation_not_emitted",
                    &format!("source reference was not emitted by a managed read: {missing}"),
                );
            }
            if let Err(error) = catalog.verify_sources_current() {
                return record_managed_error(
                    store,
                    attempt,
                    call_id,
                    arguments_sha256,
                    "attempt_stale",
                    error.message(),
                );
            }
            commit_conformance(store, attempt, call_id, arguments_sha256, submission)
        }
    }
}

fn record_catalog_result(
    store: &mut Store,
    attempt: &AgentAttemptView,
    call_id: &str,
    arguments_sha256: &str,
    result: Result<crate::source_catalog::CatalogToolOutput, crate::source_catalog::CatalogError>,
) -> AppResult<ToolReceiptView> {
    match result {
        Ok(result) => {
            let bytes = serde_json::to_vec(&result.value).map_err(json_error)?;
            store_result(store.record_tool_receipt(
                &attempt.attempt_id,
                call_id,
                arguments_sha256,
                &bytes,
                false,
                &result.evidence_refs,
                None,
            ))
        }
        Err(error) => record_managed_error(
            store,
            attempt,
            call_id,
            arguments_sha256,
            error.code(),
            error.message(),
        ),
    }
}

fn record_managed_error(
    store: &mut Store,
    attempt: &AgentAttemptView,
    call_id: &str,
    arguments_sha256: &str,
    code: &str,
    message: &str,
) -> AppResult<ToolReceiptView> {
    let bytes = managed_error_bytes(code, message)?;
    store_result(store.record_tool_receipt(
        &attempt.attempt_id,
        call_id,
        arguments_sha256,
        &bytes,
        true,
        &[],
        None,
    ))
}

fn record_attempt_closed_error(
    store: &mut Store,
    attempt: &AgentAttemptView,
    call_id: &str,
    arguments_sha256: &str,
) -> AppResult<ToolReceiptView> {
    let bytes = managed_error_bytes(
        "attempt_closed",
        "the attempt already has an atomically committed terminal result",
    )?;
    if attempt.active {
        return store_result(store.record_tool_receipt(
            &attempt.attempt_id,
            call_id,
            arguments_sha256,
            &bytes,
            true,
            &[],
            None,
        ));
    }
    store_result(store.record_closed_tool_receipt(
        &attempt.attempt_id,
        call_id,
        arguments_sha256,
        &bytes,
    ))
}

fn managed_error_bytes(code: &str, message: &str) -> AppResult<Vec<u8>> {
    let message = message.chars().take(1_000).collect::<String>();
    serde_json::to_vec(&json!({
        "error": {"code": code, "message": message}
    }))
    .map_err(json_error)
}

fn handle_terminal_store_error(
    store: &mut Store,
    attempt: &AgentAttemptView,
    call_id: &str,
    arguments_sha256: &str,
    error: StoreError,
) -> AppResult<ToolReceiptView> {
    match error.kind() {
        StoreErrorKind::InvalidInput | StoreErrorKind::Conflict => record_managed_error(
            store,
            attempt,
            call_id,
            arguments_sha256,
            "attempt_conflict",
            &error.to_string(),
        ),
        StoreErrorKind::Stale => record_managed_error(
            store,
            attempt,
            call_id,
            arguments_sha256,
            "attempt_stale",
            &error.to_string(),
        ),
        StoreErrorKind::NotFound
        | StoreErrorKind::Database
        | StoreErrorKind::Filesystem
        | StoreErrorKind::CorruptState => Err(store_error(error)),
    }
}

fn commit_composition(
    store: &mut Store,
    attempt: &AgentAttemptView,
    call_id: &str,
    arguments_sha256: &str,
    submission: CompositionSubmission,
) -> AppResult<ToolReceiptView> {
    let cited = submission.cited_source_refs.clone();
    let review = OpaqueMarkdown::from_text(submission.review_markdown)?;
    let observed_basis_digests = match current_composition_basis_digests(store, &attempt.subject_id)
    {
        Ok(observed) => observed,
        Err(error) => {
            return record_managed_error(
                store,
                attempt,
                call_id,
                arguments_sha256,
                "attempt_stale",
                error.message(),
            );
        }
    };
    let outcome = match submission.outcome {
        AgentCompositionOutcome::Compatible => StoredCompositionOutcome::Compatible,
        AgentCompositionOutcome::Conflicts => StoredCompositionOutcome::Conflicts,
        AgentCompositionOutcome::Blocked => StoredCompositionOutcome::Blocked,
    };
    match store.commit_composition_tool_response(
        &attempt.attempt_id,
        call_id,
        arguments_sha256,
        &observed_basis_digests,
        outcome,
        &review,
        &cited,
    ) {
        Ok(receipt) => Ok(receipt),
        Err(error) => handle_terminal_store_error(store, attempt, call_id, arguments_sha256, error),
    }
}

fn commit_conformance(
    store: &mut Store,
    attempt: &AgentAttemptView,
    call_id: &str,
    arguments_sha256: &str,
    submission: ConformanceSubmission,
) -> AppResult<ToolReceiptView> {
    let cited = submission.cited_source_refs.clone();
    let review = OpaqueMarkdown::from_text(submission.review_markdown)?;
    let outcome = match submission.outcome {
        AgentConformanceOutcome::Conforms => StoredConformanceOutcome::Conforms,
        AgentConformanceOutcome::DoesNotConform => StoredConformanceOutcome::DoesNotConform,
        AgentConformanceOutcome::Blocked => StoredConformanceOutcome::Blocked,
    };
    match store.commit_conformance_tool_response(
        &attempt.attempt_id,
        call_id,
        arguments_sha256,
        &attempt.basis_digest,
        outcome,
        &review,
        &cited,
    ) {
        Ok(receipt) => Ok(receipt),
        Err(error) => handle_terminal_store_error(store, attempt, call_id, arguments_sha256, error),
    }
}

fn stored_steward_response(submission: StewardSubmission) -> AppResult<StewardResponse> {
    match submission {
        StewardSubmission::Assent {
            review_markdown, ..
        } => Ok(StewardResponse::Assent {
            review_markdown: OpaqueMarkdown::from_text(review_markdown)?,
        }),
        StewardSubmission::Counterproposal {
            terms_markdown,
            review_markdown,
            ..
        } => Ok(StewardResponse::Counterproposal {
            terms_markdown: OpaqueMarkdown::from_text(terms_markdown)?,
            review_markdown: OpaqueMarkdown::from_text(review_markdown)?,
        }),
        StewardSubmission::Blocked {
            review_markdown, ..
        } => Ok(StewardResponse::Blocked {
            review_markdown: OpaqueMarkdown::from_text(review_markdown)?,
        }),
    }
}

fn missing_citation(
    store: &Store,
    attempt_id: &str,
    cited: &[String],
) -> AppResult<Option<String>> {
    let emitted = store_result(store.attempt_emitted_source_refs(attempt_id))?;
    Ok(cited
        .iter()
        .find(|reference| !emitted.contains(*reference))
        .cloned())
}

fn stages() -> [AgentStage; 3] {
    [
        AgentStage::StewardResponse,
        AgentStage::CompositionReview,
        AgentStage::ConformanceReview,
    ]
}

const fn kind_for_stage(stage: AgentStage) -> AttemptKind {
    match stage {
        AgentStage::StewardResponse => AttemptKind::StewardResponse,
        AgentStage::CompositionReview => AttemptKind::CompositionReview,
        AgentStage::ConformanceReview => AttemptKind::ConformanceReview,
    }
}

const fn stage_for_kind(kind: AttemptKind) -> AgentStage {
    match kind {
        AttemptKind::StewardResponse => AgentStage::StewardResponse,
        AttemptKind::CompositionReview => AgentStage::CompositionReview,
        AttemptKind::ConformanceReview => AgentStage::ConformanceReview,
    }
}

fn outcome(attempt: AgentAttemptView) -> AppResult<AgentRunOutcome> {
    let result_kind = attempt.domain_result_kind.clone().ok_or_else(|| {
        AppError::new(
            "attempt_result_missing",
            "agent attempt has no committed domain result kind",
        )
    })?;
    let result_id = attempt.domain_result_id.clone().ok_or_else(|| {
        AppError::new(
            "attempt_result_missing",
            "agent attempt has no committed domain result identity",
        )
    })?;
    Ok(AgentRunOutcome {
        attempt,
        result_kind,
        result_id,
    })
}

fn persisted_request(attempt: &AgentAttemptView) -> AppResult<JobRequestV1> {
    if digest(&attempt.request_bytes) != attempt.request_sha256 {
        return Err(AppError::new(
            "attempt_request_digest_mismatch",
            "persisted Nucleus request bytes do not match their digest",
        ));
    }
    let request: JobRequestV1 =
        serde_json::from_slice(&attempt.request_bytes).map_err(json_error)?;
    let stage = stage_for_kind(attempt.kind);
    if request.version != PROTOCOL_VERSION_V1
        || request.id.as_str() != attempt.nucleus_job_id
        || request.label != format!("{} for {}", stage.label(), attempt.nucleus_job_id)
        || request.requester.program != "pratica"
        || request.requester.id != attempt.requester_id
        || request.parent.is_some()
        || request.instructions != stage.instructions()
        || request.developer_instructions.as_deref() != Some(AgentStage::developer_instructions())
        || request.invocation.version != PROTOCOL_VERSION_V1
        || request.invocation.harness.as_str() != "codex"
        || request.invocation.model.as_str() != MODEL
        || request.invocation.reasoning_effort != Some(ReasoningEffort::Medium)
        || request.invocation.workspace_access != WorkspaceAccess::None
        || request.invocation.builtin_tools.local_execution
        || request.invocation.builtin_tools.web_search
        || request.invocation.timeout_seconds != TimeoutSeconds::new(REQUEST_TIMEOUT.as_secs())
        || request.invocation.launch_context.is_some()
        || request.invocation.toolset.as_ref()
            != Some(&ToolsetRef {
                provider: "pratica".to_owned(),
                name: attempt.toolset_name.clone(),
                version: attempt.toolset_version,
            })
        || attempt.toolset_name != stage.slug()
        || attempt.toolset_version != 1
    {
        return Err(AppError::new(
            "attempt_request_contract_mismatch",
            "persisted request does not match its constrained Pratica attempt",
        ));
    }
    request.validate().map_err(|error| {
        AppError::new(
            "attempt_request_invalid",
            format!("persisted Nucleus request is invalid: {error}"),
        )
    })?;
    Ok(request)
}

fn verify_accepted_digest(attempt: &AgentAttemptView, accepted: &str) -> AppResult<()> {
    let expected = format!("sha256:{}", attempt.request_sha256);
    if accepted != expected {
        return Err(AppError::new(
            "nucleus_request_digest_mismatch",
            "Nucleus admitted different immutable request bytes",
        ));
    }
    Ok(())
}

fn verify_job_identity(
    attempt: &AgentAttemptView,
    request: &JobRequestV1,
    job: &nucleus_core::JobV1,
) -> AppResult<()> {
    verify_accepted_digest(attempt, &job.summary.request_digest)?;
    if job.version != PROTOCOL_VERSION_V1
        || job.summary.version != PROTOCOL_VERSION_V1
        || job.summary.id.as_str() != attempt.nucleus_job_id
        || job.summary.requester.program != "pratica"
        || job.summary.requester.id != attempt.requester_id
        || job.request != *request
        || job.request.id.as_str() != attempt.nucleus_job_id
        || job.request.requester.program != "pratica"
        || job.request.requester.id != attempt.requester_id
    {
        return Err(AppError::new(
            "nucleus_job_identity_mismatch",
            "the Nucleus job does not match the persisted Pratica correlation",
        ));
    }
    Ok(())
}

fn runtime_state_for_job(job: &nucleus_core::JobV1) -> RuntimeState {
    match job.attempts.last().map(|attempt| attempt.state) {
        Some(AttemptState::TimedOut) => RuntimeState::TimedOut,
        Some(AttemptState::Lost) => RuntimeState::Lost,
        Some(AttemptState::Cancelled) => RuntimeState::Cancelled,
        _ => match job.summary.state {
            JobState::Completed => RuntimeState::Completed,
            JobState::Cancelled => RuntimeState::Cancelled,
            JobState::Failed => RuntimeState::Failed,
            JobState::Accepted | JobState::Running | JobState::WaitingOnRequester => {
                RuntimeState::Failed
            }
        },
    }
}

fn basis_prompt(basis: &FrozenBasisView) -> Value {
    json!({
        "basis_id": basis.basis_id,
        "manifest_sha256": basis.manifest_sha256,
        "verifier_version": basis.verifier_version,
        "observed_at": basis.observed_at,
        "sources": basis.sources.iter().map(|source| json!({
            "source_id": source.source_id,
            "content_sha256": source.content_sha256,
            "observed_at": source.observed_at,
        })).collect::<Vec<_>>(),
    })
}

fn catalog_from_steward(
    scope: &StewardScopeView,
    basis: &FrozenBasisView,
) -> AppResult<FrozenSourceCatalog> {
    if basis.kind != BasisKind::Steward
        || basis.scope_id.as_deref() != Some(scope.scope_id.as_str())
        || basis.scope_version != Some(scope.version)
    {
        return Err(AppError::new(
            "steward_basis_conflict",
            "registered steward scope and basis do not match",
        ));
    }
    let sources = basis
        .sources
        .iter()
        .map(|source| {
            let origin = source.origin_path.as_deref().ok_or_else(|| {
                AppError::new(
                    "basis_origin_missing",
                    format!(
                        "steward source {} has no verification origin",
                        source.source_id
                    ),
                )
            })?;
            Ok(FrozenSource {
                id: source.source_id.clone(),
                kind: source.kind.clone(),
                locator: source.locator.clone(),
                origin_path: PathBuf::from(origin),
                revision: source.revision.clone(),
                content: source.content.clone(),
                content_sha256: source.content_sha256.clone(),
                observed_at: source.observed_at,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let catalog = FrozenSourceCatalog {
        schema_version: MANIFEST_SCHEMA_VERSION,
        verifier_version: basis.verifier_version.clone(),
        observed_at: basis.observed_at,
        scope: scope.scope_id.clone(),
        version: scope.version,
        party: scope.steward_party.clone(),
        title: scope.title.clone(),
        charter_markdown: scope.charter_markdown.as_str().to_owned(),
        charter_sha256: scope.charter_sha256.clone(),
        manifest_path: PathBuf::new(),
        catalog_sha256: scope.descriptor_sha256.clone(),
        sources,
    };
    verify_catalog_snapshot(&catalog)?;
    Ok(catalog)
}

fn catalog_from_attempt(attempt: &AgentAttemptView) -> AppResult<FrozenSourceCatalog> {
    let sources = attempt
        .sources
        .iter()
        .map(|source| FrozenSource {
            id: source.source_id.clone(),
            kind: source.kind.clone(),
            locator: source.locator.clone(),
            origin_path: PathBuf::from(&source.origin_path),
            revision: source.revision.clone(),
            content: source.content.clone(),
            content_sha256: source.content_sha256.clone(),
            observed_at: source.observed_at,
        })
        .collect();
    let catalog = FrozenSourceCatalog {
        schema_version: MANIFEST_SCHEMA_VERSION,
        verifier_version: attempt.catalog_verifier_version.clone(),
        observed_at: attempt.catalog_observed_at,
        scope: attempt.catalog_scope.clone(),
        version: attempt.catalog_version,
        party: attempt.catalog_party.clone(),
        title: attempt.catalog_title.clone(),
        charter_markdown: attempt.catalog_charter_markdown.as_str().to_owned(),
        charter_sha256: attempt.catalog_charter_sha256.clone(),
        manifest_path: PathBuf::new(),
        catalog_sha256: attempt.catalog_sha256.clone(),
        sources,
    };
    verify_catalog_snapshot(&catalog)?;
    Ok(catalog)
}

fn verify_catalog_snapshot(catalog: &FrozenSourceCatalog) -> AppResult<()> {
    if catalog.schema_version != MANIFEST_SCHEMA_VERSION
        || catalog.version == 0
        || catalog.verifier_version.trim().is_empty()
        || catalog.sources.is_empty()
        || catalog.sources.len() > MAX_SOURCES
    {
        return Err(AppError::new(
            "catalog_bounds_invalid",
            "frozen source catalog violates its version, verifier, or source-count bound",
        ));
    }
    if sha256_hex(catalog.charter_markdown.as_bytes()) != catalog.charter_sha256 {
        return Err(AppError::new(
            "catalog_digest_mismatch",
            "frozen catalog charter does not match its digest",
        ));
    }
    let mut total_bytes = 0_u64;
    for source in &catalog.sources {
        let source_bytes = u64::try_from(source.content.len()).unwrap_or(u64::MAX);
        if source_bytes > MAX_SOURCE_BYTES {
            return Err(AppError::new(
                "catalog_bounds_invalid",
                format!(
                    "frozen source {} exceeds the per-source byte bound",
                    source.id
                ),
            ));
        }
        total_bytes = total_bytes.checked_add(source_bytes).ok_or_else(|| {
            AppError::new(
                "catalog_bounds_invalid",
                "frozen catalog byte count overflowed",
            )
        })?;
        if total_bytes > MAX_CATALOG_BYTES {
            return Err(AppError::new(
                "catalog_bounds_invalid",
                "frozen source catalog exceeds its total byte bound",
            ));
        }
        if std::str::from_utf8(&source.content).is_err() {
            return Err(AppError::new(
                "catalog_encoding_invalid",
                format!("frozen source {} is not exact UTF-8 text", source.id),
            ));
        }
        if digest(&source.content) != source.content_sha256 {
            return Err(AppError::new(
                "catalog_digest_mismatch",
                format!("frozen source {} does not match its digest", source.id),
            ));
        }
    }
    let digest = catalog.recompute_catalog_sha256().map_err(catalog_error)?;
    if digest != catalog.catalog_sha256 {
        return Err(AppError::new(
            "catalog_digest_mismatch",
            "frozen source catalog does not match its digest",
        ));
    }
    Ok(())
}

fn composition_catalog(
    store: &Store,
    integration: &IntegrationView,
    roster: &RosterView,
    references: &[CompositionAgreementRef],
) -> AppResult<FrozenSourceCatalog> {
    let observed_at = OffsetDateTime::now_utc().unix_timestamp();
    let mut sources = Vec::new();
    if let Some(context) = &integration.context_markdown {
        let locator = format!("pratica:integration:{}:context", integration.integration_id);
        sources.push(FrozenSource {
            id: "integration-context".to_owned(),
            kind: "integration_context".to_owned(),
            origin_path: PathBuf::from(&locator),
            locator,
            revision: integration.context_sha256.clone(),
            content: context.as_bytes().to_vec(),
            content_sha256: context.sha256(),
            observed_at,
        });
    }
    for reference in references {
        let agreement = store_result(store.agreement(&reference.agreement_id))?;
        let track = store_result(store.track(&reference.track_id))?;
        let scope = store_result(store.steward_scope(&track.scope_id, track.scope_version))?;
        let agreement_locator = format!("pratica:agreement:{}", agreement.agreement_id);
        sources.push(FrozenSource {
            id: format!("agreement-{}", reference.ordinal),
            kind: "agreement_terms".to_owned(),
            origin_path: PathBuf::from(&agreement_locator),
            locator: agreement_locator,
            revision: Some(agreement.offer.terms_sha256.clone()),
            content: agreement.offer.terms_markdown.as_bytes().to_vec(),
            content_sha256: agreement.offer.terms_sha256,
            observed_at,
        });
        let scope_locator = format!("pratica:steward:{}:{}", scope.scope_id, scope.version);
        sources.push(FrozenSource {
            id: format!("scope-charter-{}", reference.ordinal),
            kind: "steward_charter".to_owned(),
            origin_path: PathBuf::from(&scope_locator),
            locator: scope_locator,
            revision: Some(scope.descriptor_sha256.clone()),
            content: scope.charter_markdown.as_bytes().to_vec(),
            content_sha256: scope.charter_sha256,
            observed_at,
        });
    }
    let charter_markdown = "Review the exact active bilateral agreements for internal cross-track compatibility. This catalog does not establish exhaustive system coverage.".to_owned();
    let charter_sha256 = digest(charter_markdown.as_bytes());
    let mut catalog = FrozenSourceCatalog {
        schema_version: MANIFEST_SCHEMA_VERSION,
        verifier_version: COMPOSITION_VERIFIER_VERSION.to_owned(),
        observed_at,
        scope: integration.integration_id.clone(),
        version: roster.revision,
        party: "pratica-composition-reviewer".to_owned(),
        title: format!("{} composition", integration.title),
        charter_markdown,
        charter_sha256,
        manifest_path: PathBuf::new(),
        catalog_sha256: String::new(),
        sources,
    };
    catalog.catalog_sha256 = catalog.recompute_catalog_sha256().map_err(catalog_error)?;
    Ok(catalog)
}

fn observe_basis_digest(basis: &FrozenBasisView) -> AppResult<String> {
    let sources = basis
        .sources
        .iter()
        .map(|source| {
            let origin = source.origin_path.as_deref().ok_or_else(|| {
                AppError::new(
                    "basis_unavailable",
                    format!(
                        "frozen source {} has no verification origin",
                        source.source_id
                    ),
                )
            })?;
            let path = Path::new(origin);
            let metadata = fs::symlink_metadata(path).map_err(|error| {
                AppError::new(
                    "basis_unavailable",
                    format!(
                        "unable to inspect current source {}: {error}",
                        source.locator
                    ),
                )
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(AppError::new(
                    "basis_stale",
                    format!("source identity changed: {}", source.locator),
                ));
            }
            let content = read_bounded(path, crate::model::MAX_FROZEN_SOURCE_BYTES as u64)?;
            Ok(NewFrozenSource {
                source_id: source.source_id.clone(),
                kind: source.kind.clone(),
                locator: source.locator.clone(),
                origin_path: source.origin_path.clone(),
                revision: source.revision.clone(),
                content,
                observed_at: OffsetDateTime::now_utc().unix_timestamp(),
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let current = NewFrozenBasis {
        kind: basis.kind,
        label: basis.label.clone(),
        scope_id: basis.scope_id.clone(),
        scope_version: basis.scope_version,
        verifier_version: basis.verifier_version.clone(),
        observed_at: OffsetDateTime::now_utc().unix_timestamp(),
        sources,
    };
    Store::basis_manifest_sha256(&current).map_err(store_error)
}

fn current_composition_basis_digests(
    store: &Store,
    integration_id: &str,
) -> AppResult<Vec<(String, String)>> {
    let (_, agreements, _) = store_result(store.composition_basis(integration_id))?;
    let mut observed = Vec::with_capacity(agreements.len());
    for reference in agreements {
        let agreement = store_result(store.agreement(&reference.agreement_id))?;
        let basis = store_result(store.frozen_basis(&agreement.basis_id))?;
        let digest = observe_basis_digest(&basis)?;
        if digest != basis.manifest_sha256 {
            return Err(AppError::new(
                "basis_stale",
                format!("agreement {} steward basis changed", agreement.agreement_id),
            ));
        }
        observed.push((basis.basis_id, digest));
    }
    Ok(observed)
}

fn reject_implicit_retry(
    latest: Option<&AgentAttemptView>,
    same_immutable_target: impl FnOnce(&AgentAttemptView) -> bool,
) -> AppResult<()> {
    if latest.is_some_and(|attempt| {
        !attempt.active
            && attempt.domain_result_id.is_none()
            && attempt.runtime_state.is_terminal()
            && same_immutable_target(attempt)
    }) {
        return Err(AppError::new(
            "attempt_retry_required",
            "the latest attempt failed for this same immutable target; use attempt retry to preserve lineage",
        ));
    }
    Ok(())
}

fn revalidate_retry_target(
    store: &Store,
    attempt: &AgentAttemptView,
    catalog: &FrozenSourceCatalog,
) -> AppResult<()> {
    match attempt.kind {
        AttemptKind::StewardResponse => {
            let negotiation = store_result(store.negotiation(&attempt.subject_id))?;
            if negotiation.status != NegotiationStatus::Open {
                return Err(AppError::new(
                    "attempt_stale",
                    "negotiation is no longer open",
                ));
            }
            let current_head = negotiation.head.as_ref().ok_or_else(|| {
                AppError::new("attempt_stale", "negotiation no longer has a current offer")
            })?;
            if attempt.expected_offer_id.as_deref() != Some(current_head.offer_id.as_str()) {
                return Err(AppError::new(
                    "attempt_stale",
                    "negotiation head changed after the failed attempt",
                ));
            }
            let track = store_result(store.track(&negotiation.track_id))?;
            let basis = store_result(store.steward_basis(&track.scope_id, track.scope_version))?;
            if attempt.basis_id.as_deref() != Some(basis.basis_id.as_str())
                || attempt.basis_digest != basis.manifest_sha256
            {
                return Err(AppError::new(
                    "attempt_stale",
                    "registered steward basis changed after the failed attempt",
                ));
            }
            catalog.verify_sources_current().map_err(catalog_error)?;
            if observe_basis_digest(&basis)? != basis.manifest_sha256 {
                return Err(AppError::new(
                    "basis_stale",
                    "registered steward source content changed after the failed attempt",
                ));
            }
        }
        AttemptKind::CompositionReview => {
            let (_, _, current_digest) =
                store_result(store.composition_basis(&attempt.subject_id))?;
            if attempt.expected_roster_digest.as_deref() != Some(current_digest.as_str()) {
                return Err(AppError::new(
                    "attempt_stale",
                    "integration composition changed after the failed attempt",
                ));
            }
            current_composition_basis_digests(store, &attempt.subject_id)?;
        }
        AttemptKind::ConformanceReview => {
            store_result(store.agreement(&attempt.subject_id))?;
            let basis_id = attempt.basis_id.as_deref().ok_or_else(|| {
                AppError::new(
                    "corrupt_state",
                    "conformance attempt has no frozen candidate basis",
                )
            })?;
            let basis = store_result(store.frozen_basis(basis_id))?;
            if basis.kind != BasisKind::Candidate || basis.manifest_sha256 != attempt.basis_digest {
                return Err(AppError::new(
                    "attempt_stale",
                    "candidate basis changed after the failed attempt",
                ));
            }
            catalog.verify_sources_current().map_err(catalog_error)?;
            if observe_basis_digest(&basis)? != basis.manifest_sha256 {
                return Err(AppError::new(
                    "basis_stale",
                    "candidate source content changed after the failed attempt",
                ));
            }
        }
    }
    Ok(())
}

fn read_bounded(path: &Path, maximum: u64) -> AppResult<Vec<u8>> {
    use std::io::Read as _;

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path).map_err(|error| {
        AppError::new(
            "basis_unavailable",
            format!("unable to open {}: {error}", path.display()),
        )
    })?;
    let metadata = file.metadata().map_err(|error| {
        AppError::new(
            "basis_unavailable",
            format!(
                "unable to inspect opened source {}: {error}",
                path.display()
            ),
        )
    })?;
    if !metadata.is_file() {
        return Err(AppError::new(
            "basis_stale",
            format!("source is no longer a regular file: {}", path.display()),
        ));
    }
    if metadata.len() > maximum {
        return Err(AppError::new(
            "basis_stale",
            format!("source now exceeds the admitted bound: {}", path.display()),
        ));
    }
    let mut content = Vec::new();
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut content)
        .map_err(|error| {
            AppError::new(
                "basis_unavailable",
                format!("unable to read {}: {error}", path.display()),
            )
        })?;
    if content.len() > usize::try_from(maximum).unwrap_or(usize::MAX) {
        return Err(AppError::new(
            "basis_stale",
            format!("source now exceeds the admitted bound: {}", path.display()),
        ));
    }
    Ok(content)
}

#[allow(clippy::needless_pass_by_value)]
fn encode_prompt(value: Value) -> AppResult<String> {
    serde_json::to_string(&value).map_err(json_error)
}

fn digest(bytes: &[u8]) -> String {
    let value = Sha256::digest(bytes);
    format!("{value:x}")
}

fn neutral_cwd(job_id: &str) -> AppResult<PathBuf> {
    let temporary = fs::canonicalize(std::env::temp_dir()).map_err(|error| {
        AppError::new(
            "nucleus_cwd_failed",
            format!("unable to resolve the temporary directory: {error}"),
        )
    })?;
    let base = temporary.join("pratica-nucleus");
    create_or_verify_private_directory(&base)?;
    if fs::canonicalize(&base).map_err(cwd_error)? != base {
        return Err(AppError::new(
            "nucleus_cwd_unsafe",
            "neutral Nucleus directory resolved through a symlink",
        ));
    }
    let path = base.join(digest(job_id.as_bytes()));
    create_or_verify_private_directory(&path)?;
    if !path.is_absolute() {
        return Err(AppError::new(
            "nucleus_cwd_relative",
            "neutral Nucleus working directory is not absolute",
        ));
    }
    if fs::canonicalize(&path).map_err(cwd_error)? != path {
        return Err(AppError::new(
            "nucleus_cwd_unsafe",
            "neutral Nucleus job directory resolved through a symlink",
        ));
    }
    Ok(path)
}

fn cleanup_neutral_cwd(job_id: &str) {
    let Ok(temporary) = fs::canonicalize(std::env::temp_dir()) else {
        return;
    };
    let base = temporary.join("pratica-nucleus");
    let Ok(base_metadata) = fs::symlink_metadata(&base) else {
        return;
    };
    if base_metadata.file_type().is_symlink()
        || !base_metadata.is_dir()
        || fs::canonicalize(&base).ok().as_ref() != Some(&base)
    {
        return;
    }
    let path = base.join(digest(job_id.as_bytes()));
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return;
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || fs::canonicalize(&path).ok().as_ref() != Some(&path)
    {
        return;
    }
    let _result = fs::remove_dir(path);
}

fn create_or_verify_private_directory(path: &Path) -> AppResult<()> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => return Err(cwd_error(error)),
    }
    let metadata = fs::symlink_metadata(path).map_err(cwd_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AppError::new(
            "nucleus_cwd_unsafe",
            format!(
                "neutral Nucleus path is not a nonsymlink directory: {}",
                path.display()
            ),
        ));
    }

    #[cfg(unix)]
    {
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(path)
            .map_err(cwd_error)?;
        directory
            .set_permissions(fs::Permissions::from_mode(0o700))
            .map_err(cwd_error)?;
        let mode = directory
            .metadata()
            .map_err(cwd_error)?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o700 {
            return Err(AppError::new(
                "nucleus_cwd_unsafe",
                format!(
                    "neutral Nucleus directory does not have mode 0700: {}",
                    path.display()
                ),
            ));
        }
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn cwd_error(error: std::io::Error) -> AppError {
    AppError::new("nucleus_cwd_failed", error.to_string())
}

fn runtime() -> AppResult<tokio::runtime::Runtime> {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| AppError::new("nucleus_runtime_failed", error.to_string()))
}

#[allow(clippy::needless_pass_by_value)]
fn json_error(error: serde_json::Error) -> AppError {
    AppError::new("json_serialization_failed", error.to_string())
}

#[allow(clippy::needless_pass_by_value)]
fn client_error(error: ClientError) -> AppError {
    AppError::new("nucleus_failed", error.to_string())
}

#[allow(clippy::needless_pass_by_value)]
fn contract_error(error: crate::agent_contracts::ContractError) -> AppError {
    AppError::new(error.code(), error.message())
}

#[allow(clippy::needless_pass_by_value)]
fn catalog_error(error: crate::source_catalog::CatalogError) -> AppError {
    AppError::new(error.code(), error.message())
}

#[allow(clippy::needless_pass_by_value)]
fn store_error(error: StoreError) -> AppError {
    let code = match error.kind() {
        StoreErrorKind::InvalidInput => "invalid_input",
        StoreErrorKind::NotFound => "not_found",
        StoreErrorKind::Conflict => "conflict",
        StoreErrorKind::Stale => "stale",
        StoreErrorKind::Database => "database_failed",
        StoreErrorKind::Filesystem => "filesystem_failed",
        StoreErrorKind::CorruptState => "corrupt_state",
    };
    AppError::new(code, error.to_string())
}

fn store_result<T>(result: Result<T, StoreError>) -> AppResult<T> {
    result.map_err(store_error)
}

fn cached_receipt(receipt: ToolReceiptView, arguments_sha256: &str) -> AppResult<ToolReceiptView> {
    if receipt.arguments_sha256 != arguments_sha256 {
        return Err(AppError::new(
            "mailbox_arguments_conflict",
            "a persisted Nucleus call was replayed with different arguments",
        ));
    }
    Ok(receipt)
}

fn terminal_receipt_error(receipt: &ToolReceiptView) -> AppResult<Option<(String, String)>> {
    if !receipt.is_error {
        return Ok(None);
    }
    let value: Value = serde_json::from_slice(&receipt.result_json).map_err(json_error)?;
    let Some(code) = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
    else {
        return Err(AppError::new(
            "tool_receipt_corrupt",
            "persisted error receipt has no error code",
        ));
    };
    if !matches!(code, "attempt_stale" | "attempt_conflict") {
        return Ok(None);
    }
    let message = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("the immutable attempt is no longer applicable")
        .to_owned();
    Ok(Some((code.to_owned(), message)))
}

fn tool_result_from_receipt(
    attempt: &AgentAttemptView,
    stage: AgentStage,
    call_id: &ToolCallId,
    receipt: &ToolReceiptView,
) -> AppResult<ToolResultV1> {
    if receipt.attempt_id != attempt.attempt_id
        || receipt.nucleus_job_id != attempt.nucleus_job_id
        || receipt.call_id != call_id.as_str()
    {
        return Err(AppError::new(
            "tool_receipt_corrupt",
            "persisted tool receipt does not belong to the expected attempt and call",
        ));
    }
    let result_text = String::from_utf8(receipt.result_json.clone()).map_err(|error| {
        AppError::new(
            "tool_receipt_corrupt",
            format!("persisted tool result is not UTF-8 JSON: {error}"),
        )
    })?;
    Ok(ToolResultV1 {
        version: PROTOCOL_VERSION_V1,
        call_id: call_id.clone(),
        requester: Requester {
            program: "pratica".to_owned(),
            id: attempt.requester_id.clone(),
        },
        result_schema_id: SchemaId::new(stage.result_schema_id()),
        result: RawValue::from_string(result_text).map_err(|error| {
            AppError::new(
                "tool_receipt_corrupt",
                format!("persisted tool result is not valid JSON: {error}"),
            )
        })?,
        is_error: receipt.is_error,
    })
}

async fn reconcile_terminal_error_receipt(
    client: &NucleusClient,
    store: &mut Store,
    attempt: &AgentAttemptView,
    stage: AgentStage,
    receipt: &ToolReceiptView,
    expected_sequence: Option<u64>,
) -> AppResult<(String, String)> {
    let classified = terminal_receipt_error(receipt)?.ok_or_else(|| {
        AppError::new(
            "tool_receipt_corrupt",
            "terminal-error recovery was requested for a nonterminal receipt",
        )
    })?;
    let job_id = JobId::new(&attempt.nucleus_job_id);
    let call_id = ToolCallId::new(&receipt.call_id);
    let result = tool_result_from_receipt(attempt, stage, &call_id, receipt)?;
    let posted_sequence = post_result_stably(client, &job_id, &call_id, &result).await?;
    if expected_sequence.is_some_and(|expected| expected != posted_sequence) {
        return Err(AppError::new(
            "nucleus_tool_contract_mismatch",
            "Nucleus acknowledged a different terminal-error mailbox sequence",
        ));
    }
    cancel_after_terminal_tool_error(client, &job_id).await?;
    store_result(store.advance_attempt_tool_after(&attempt.attempt_id, posted_sequence))?;
    store_result(store.mark_attempt_runtime_state(
        &attempt.attempt_id,
        RuntimeState::Failed,
        Some(&classified.1),
    ))?;
    Ok(classified)
}

async fn cancel_after_terminal_tool_error(client: &NucleusClient, job_id: &JobId) -> AppResult<()> {
    let response = match client.cancel_job(job_id).await {
        Ok(response) => response,
        Err(ClientError::Api { status: 409, .. }) => {
            let job = client.get_job(job_id).await.map_err(client_error)?;
            if job.version != PROTOCOL_VERSION_V1
                || job.summary.version != PROTOCOL_VERSION_V1
                || job.summary.id != *job_id
                || !job.summary.state.is_terminal()
            {
                return Err(AppError::new(
                    "nucleus_cancel_ambiguous",
                    "Nucleus rejected cancellation while the stale attempt remained active",
                ));
            }
            return Ok(());
        }
        Err(error) => return Err(client_error(error)),
    };
    if response.version != PROTOCOL_VERSION_V1 || response.job_id != *job_id {
        return Err(AppError::new(
            "nucleus_cancel_contract_mismatch",
            "Nucleus returned a cancellation response for a different protocol or job",
        ));
    }
    if response.state.is_terminal() {
        return Ok(());
    }
    if !response.cancellation_requested {
        return Err(AppError::new(
            "nucleus_cancel_ambiguous",
            "Nucleus did not confirm cancellation for the stale attempt",
        ));
    }

    tokio::time::timeout(CANCELLATION_CONFIRM_TIMEOUT, async {
        loop {
            let job = client.get_job(job_id).await.map_err(client_error)?;
            if job.version != PROTOCOL_VERSION_V1
                || job.summary.version != PROTOCOL_VERSION_V1
                || job.summary.id != *job_id
            {
                return Err(AppError::new(
                    "nucleus_cancel_contract_mismatch",
                    "Nucleus returned a different job while confirming cancellation",
                ));
            }
            if job.summary.state.is_terminal() {
                return Ok(());
            }
            tokio::time::sleep(TRANSPORT_RETRY_DELAY).await;
        }
    })
    .await
    .map_err(|_| {
        AppError::new(
            "nucleus_cancel_ambiguous",
            "Nucleus cancellation was not confirmed; the stale attempt remains active",
        )
    })?
}

async fn post_result_stably(
    client: &NucleusClient,
    job_id: &JobId,
    call_id: &nucleus_core::ToolCallId,
    result: &ToolResultV1,
) -> AppResult<u64> {
    let mut transport_failures = 0_u8;
    loop {
        match client.post_tool_result(job_id, call_id, result).await {
            Ok(answered)
                if answered.version == PROTOCOL_VERSION_V1
                    && answered.state == ToolCallState::Answered
                    && answered.answered_at.is_some()
                    && answered.call.version == PROTOCOL_VERSION_V1
                    && answered.call.job_id == *job_id
                    && answered.call.id == *call_id =>
            {
                return Ok(answered.call.request_sequence);
            }
            Ok(_) => {
                return Err(AppError::new(
                    "nucleus_tool_contract_mismatch",
                    "Nucleus returned an invalid acknowledgement for the posted tool result",
                ));
            }
            Err(ClientError::Transport { .. }) if transport_failures < 2 => {
                transport_failures += 1;
                tokio::time::sleep(TRANSPORT_RETRY_DELAY).await;
            }
            Err(error) => return Err(client_error(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BasisGuard, NewStewardScope};
    use crate::source_catalog::FILE_VERIFIER_VERSION;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn attempt_fixture() -> TestResult<AgentAttemptView> {
        Ok(AgentAttemptView {
            attempt_id: "att_test".to_owned(),
            kind: AttemptKind::StewardResponse,
            subject_id: "neg_test".to_owned(),
            requester_id: "pratica-test".to_owned(),
            nucleus_job_id: "pratica-steward-response-test".to_owned(),
            request_bytes: Vec::new(),
            request_sha256: "a".repeat(64),
            toolset_name: "steward-response".to_owned(),
            toolset_version: 1,
            predecessor_attempt_id: None,
            expected_offer_id: Some("off_test".to_owned()),
            expected_roster_digest: None,
            basis_id: Some("bas_test".to_owned()),
            basis_digest: "b".repeat(64),
            catalog_scope: "scope".to_owned(),
            catalog_version: 1,
            catalog_party: "party".to_owned(),
            catalog_title: "title".to_owned(),
            catalog_verifier_version: FILE_VERIFIER_VERSION.to_owned(),
            catalog_observed_at: 1,
            catalog_charter_markdown: OpaqueMarkdown::from_text("charter")?,
            catalog_charter_sha256: "c".repeat(64),
            catalog_sha256: "d".repeat(64),
            sources: Vec::new(),
            tool_after: 0,
            admitted: false,
            accepted_job_id: None,
            accepted_request_sha256: None,
            active: true,
            runtime_state: RuntimeState::Prepared,
            runtime_detail: None,
            domain_result_kind: None,
            domain_result_id: None,
            created_at: 1,
            updated_at: 1,
        })
    }

    #[test]
    fn all_stage_requests_are_workspace_and_builtin_tool_free() -> TestResult {
        let directory = tempfile::tempdir()?;
        for stage in stages() {
            let registration = toolset_registration(stage)?;
            let request = build_request(
                stage,
                "pratica-requester-test",
                &format!("pratica-{}-test", stage.slug()),
                "{}",
                registration.toolset,
                directory.path(),
            )?;
            assert_eq!(request.invocation.workspace_access, WorkspaceAccess::None);
            assert!(!request.invocation.builtin_tools.local_execution);
            assert!(!request.invocation.builtin_tools.web_search);
            assert!(request.invocation.launch_context.is_none());
            assert_eq!(request.requester.program, "pratica");
        }
        Ok(())
    }

    #[test]
    fn nucleus_request_digest_requires_explicit_algorithm_prefix() -> TestResult {
        let attempt = attempt_fixture()?;
        assert!(verify_accepted_digest(&attempt, &format!("sha256:{}", "a".repeat(64))).is_ok());
        assert!(verify_accepted_digest(&attempt, &"a".repeat(64)).is_err());
        Ok(())
    }

    #[test]
    fn committed_outcome_is_returned_only_after_terminal_acknowledgement() -> TestResult {
        let mut attempt = attempt_fixture()?;
        attempt.domain_result_kind = Some("steward_response".to_owned());
        attempt.domain_result_id = Some("off_result".to_owned());
        assert!(!committed_outcome_is_acknowledged(&attempt));
        attempt.active = false;
        assert!(committed_outcome_is_acknowledged(&attempt));
        Ok(())
    }

    #[test]
    fn failed_same_target_requires_explicit_retry_but_changed_target_does_not() -> TestResult {
        let mut attempt = attempt_fixture()?;
        attempt.active = false;
        attempt.runtime_state = RuntimeState::Completed;
        let error = reject_implicit_retry(Some(&attempt), |_| true)
            .err()
            .ok_or_else(|| std::io::Error::other("same target was accepted"))?;
        assert_eq!(error.code(), "attempt_retry_required");
        assert!(reject_implicit_retry(Some(&attempt), |_| false).is_ok());
        Ok(())
    }

    #[test]
    fn retry_identity_preserves_requester_and_allocates_a_new_job() {
        let (requester, first_job) = new_attempt_identity(AgentStage::StewardResponse, None);
        let (replacement_requester, replacement_job) =
            new_attempt_identity(AgentStage::StewardResponse, Some(requester.clone()));
        assert_eq!(replacement_requester, requester);
        assert_ne!(replacement_job, first_job);
    }

    #[test]
    fn composition_sources_prepare_and_persist_with_internal_provenance() -> TestResult {
        let source_content = b"Implemented behavior.\n".to_vec();
        let mut store = Store::open_in_memory()?;
        let scope = NewStewardScope {
            scope_id: "crm.test".to_owned(),
            version: 1,
            steward_party: "crm-steward".to_owned(),
            title: "CRM test scope".to_owned(),
            charter_markdown: OpaqueMarkdown::from_text("Steward the CRM contract.\n")?,
            descriptor_sha256: digest(b"crm test descriptor"),
        };
        let basis_input = NewFrozenBasis {
            kind: BasisKind::Steward,
            label: "CRM test basis".to_owned(),
            scope_id: Some(scope.scope_id.clone()),
            scope_version: Some(scope.version),
            verifier_version: FILE_VERIFIER_VERSION.to_owned(),
            observed_at: 1,
            sources: vec![NewFrozenSource {
                source_id: "implementation".to_owned(),
                kind: "implementation".to_owned(),
                locator: "implementation.md".to_owned(),
                origin_path: Some("/private/test/implementation.md".to_owned()),
                revision: Some("test".to_owned()),
                content: source_content,
                observed_at: 1,
            }],
        };
        let (_, basis) = store.register_steward(&scope, &basis_input)?;
        let context = OpaqueMarkdown::from_text("Integrate the CRM contracts.\n")?;
        let integration = store.create_integration("crm", "CRM integration", Some(&context))?;
        let terms = OpaqueMarkdown::from_text("CRM expects stable identifiers.\n")?;
        let (_, negotiation) = store.open_track(
            &integration.integration_id,
            &scope.scope_id,
            scope.version,
            &terms,
        )?;
        let head = negotiation
            .head
            .as_ref()
            .ok_or_else(|| std::io::Error::other("test negotiation has no head"))?;
        let mutation = store.apply_steward_response(
            &negotiation.negotiation_id,
            &head.offer_id,
            &BasisGuard {
                basis_id: basis.basis_id,
                observed_manifest_sha256: basis.manifest_sha256,
            },
            &StewardResponse::Assent {
                review_markdown: OpaqueMarkdown::from_text("Compatible.\n")?,
            },
            None,
        )?;
        assert!(mutation.agreement_id.is_some());

        let (roster, agreements, composition_digest) =
            store.composition_basis(&integration.integration_id)?;
        let catalog = composition_catalog(&store, &integration, &roster, &agreements)?;
        assert!(catalog.sources.iter().all(|source| {
            !source.origin_path.as_os_str().is_empty()
                && source.origin_path.to_str() == Some(source.locator.as_str())
        }));
        let attempt = AgentRunner::prepare_attempt(
            &mut store,
            None,
            None,
            AgentStage::CompositionReview,
            &integration.integration_id,
            "{}",
            None,
            Some(composition_digest.clone()),
            None,
            composition_digest,
            &catalog,
        )?;
        let persisted = store.attempt(&attempt.attempt_id)?;
        assert_eq!(persisted.sources.len(), catalog.sources.len());
        assert!(
            persisted
                .sources
                .iter()
                .all(|source| source.origin_path == source.locator)
        );
        cleanup_neutral_cwd(&attempt.nucleus_job_id);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn neutral_working_directory_is_private_and_nonsymlink() -> TestResult {
        let job_id = format!("pratica-cwd-test-{}", Uuid::now_v7());
        let path = neutral_cwd(&job_id)?;
        let metadata = fs::symlink_metadata(&path)?;
        assert!(metadata.is_dir());
        assert!(!metadata.file_type().is_symlink());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        cleanup_neutral_cwd(&job_id);
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn reconstructed_catalogs_cannot_exceed_the_closed_source_bound() {
        let sources = (0..=MAX_SOURCES)
            .map(|index| FrozenSource {
                id: format!("source-{index}"),
                kind: "contract".to_owned(),
                locator: format!("source-{index}.md"),
                origin_path: PathBuf::new(),
                revision: None,
                content: b"x".to_vec(),
                content_sha256: digest(b"x"),
                observed_at: 1,
            })
            .collect();
        let catalog = FrozenSourceCatalog {
            schema_version: MANIFEST_SCHEMA_VERSION,
            verifier_version: FILE_VERIFIER_VERSION.to_owned(),
            observed_at: 1,
            scope: "scope".to_owned(),
            version: 1,
            party: "party".to_owned(),
            title: "title".to_owned(),
            charter_markdown: "charter".to_owned(),
            charter_sha256: digest(b"charter"),
            manifest_path: PathBuf::new(),
            catalog_sha256: "0".repeat(64),
            sources,
        };
        let Err(error) = verify_catalog_snapshot(&catalog) else {
            panic!("over-limit catalog was accepted");
        };
        assert_eq!(error.code(), "catalog_bounds_invalid");
    }

    #[test]
    fn stale_terminal_receipt_is_classified_for_post_then_cancel() -> TestResult {
        let exact = br#"{"error":{"code":"attempt_stale","message":"head changed"}}"#.to_vec();
        let receipt = ToolReceiptView {
            receipt_id: "rcp_test".to_owned(),
            attempt_id: "att_test".to_owned(),
            nucleus_job_id: "pratica-steward-response-test".to_owned(),
            call_id: "call_test".to_owned(),
            arguments_sha256: "a".repeat(64),
            result_json: exact.clone(),
            is_error: true,
            domain_result_kind: None,
            domain_result_id: None,
            emitted_source_refs: Vec::new(),
            recorded_at: 1,
        };
        let classified = terminal_receipt_error(&receipt)?
            .ok_or_else(|| std::io::Error::other("receipt was not classified"))?;
        assert_eq!(classified.0, "attempt_stale");
        assert_eq!(classified.1, "head changed");
        assert_eq!(receipt.result_json, exact);
        let attempt = attempt_fixture()?;
        let call_id = ToolCallId::new("call_test");
        let replay =
            tool_result_from_receipt(&attempt, AgentStage::StewardResponse, &call_id, &receipt)?;
        assert_eq!(replay.result.get().as_bytes(), receipt.result_json);
        assert!(replay.is_error);
        assert_eq!(replay.requester.id, attempt.requester_id);
        Ok(())
    }
}
