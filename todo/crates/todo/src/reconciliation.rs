use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::model::{ConcernId, DesignId, ModelQuality, TodoId, TodoView};
use crate::model_runner::{ModelSettings, RunIdentity, Runner};
use crate::reconciliation_store::{
    AssessmentBase, AssessmentReturnView, AssessmentSnapshot, DesignCorrection, DesignView,
    RoutingCandidate, RoutingProposalView, RoutingSnapshot, SituationAssessmentView,
};
use crate::todo_store;
use crate::tool_server::contracts::{
    AssessmentReturn, CandidateReadRequest, ConcernRoutingProposal, PageRequest,
    SituationAssessment, SourceReadRequest, SourceSearchRequest,
};
use crate::tool_server::{Backend, Call, Stage, ToolFailure, ToolSuccess};
use crate::{db, reconciliation_store as store};

const SOURCE_PAGE: usize = 100;
const CANDIDATE_PAGE: usize = 50;
const CANDIDATE_TITLE_CHARS: usize = 512;
const READ_PAGE_CHARS: usize = 24_000;
const SEARCH_MATCHES: usize = 40;
const MAX_WORKSPACE_FILES: usize = 4_000;
const MAX_SEARCHABLE_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) struct StageOutput<T> {
    pub(crate) artifact: T,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) fn route_concern(
    path: &Path,
    config: &Config,
    concern_id: ConcernId,
    quality: Option<ModelQuality>,
    model: Option<&str>,
) -> AppResult<StageOutput<RoutingProposalView>> {
    let mut connection = db::open_write(path)?;
    let snapshot = store::routing_snapshot(&connection, concern_id)?;
    let working_directory = working_directory()?;
    let sources = BoundedSources::explicit_only([PathBuf::from(&snapshot.concern.source_path)])?;
    let canonical_evidence_refs = routing_prompt_refs(&snapshot);
    let identity = new_identity(Stage::ConcernRouting);
    let job = store::record_agent_job(
        &mut connection,
        "concern_routing",
        Some(concern_id),
        None,
        &snapshot.base_digest,
        identity.requester_id(),
        identity.job_id(),
    )?;
    let prompt = serde_json::to_string_pretty(&json!({
        "concern": snapshot.concern,
        "candidate_snapshot_digest": snapshot.base_digest,
        "candidate_count": snapshot.candidates.len(),
        "canonical_evidence_refs": canonical_evidence_refs,
        "instruction": "Use the managed source and candidate tools, then submit exactly one pending routing proposal.",
    }))?;
    let settings = model_settings(config, quality, model);
    let mut backend = RoutingBackend {
        connection,
        snapshot,
        sources,
        canonical_evidence_refs,
        emitted_candidate_refs: BTreeSet::new(),
        agent_job_id: job.id,
        recorded: None,
        failure: None,
    };
    let run = Runner::for_current_user().run_stage(
        Stage::ConcernRouting,
        &identity,
        &settings,
        &prompt,
        &working_directory,
        &mut backend,
    );
    finish_stage(backend.recorded, backend.failure, run, "routing proposal")
}

pub(crate) fn assess_todo(
    path: &Path,
    config: &Config,
    todo_id: TodoId,
    quality: Option<ModelQuality>,
    model: Option<&str>,
) -> AppResult<StageOutput<SituationAssessmentView>> {
    let mut connection = db::open_write(path)?;
    let snapshot = store::assessment_snapshot(&connection, todo_id)?;
    let view = todo_store::show(&connection, todo_id)?;
    let source_paths = store::effective_concerns(&connection, todo_id)?
        .into_iter()
        .map(|concern| PathBuf::from(concern.source_path))
        .collect::<Vec<_>>();
    let working_directory = working_directory()?;
    let sources = match nearest_git_workspace(&working_directory) {
        Some(root) => BoundedSources::with_workspace(&root, source_paths)?,
        None => BoundedSources::explicit_only(source_paths)?,
    };
    let canonical_evidence_refs = assessment_prompt_refs(&snapshot, &view);
    let identity = new_identity(Stage::SituationAssessment);
    let job = store::record_agent_job(
        &mut connection,
        "situation_assessment",
        None,
        Some(todo_id),
        &snapshot.base_digest,
        identity.requester_id(),
        identity.job_id(),
    )?;
    let prompt = serde_json::to_string_pretty(&json!({
        "todo": view,
        "direction": snapshot.direction,
        "assessment_snapshot_digest": snapshot.base_digest,
        "canonical_evidence_refs": canonical_evidence_refs,
        "instruction": "Use managed source reads to describe the present situation. Map every supplied direction boundary exactly once and submit one immutable assessment.",
    }))?;
    let settings = model_settings(config, quality, model);
    let mut backend = AssessmentBackend {
        connection,
        snapshot,
        sources,
        canonical_evidence_refs,
        agent_job_id: job.id,
        recorded: None,
        failure: None,
    };
    let run = Runner::for_current_user().run_stage(
        Stage::SituationAssessment,
        &identity,
        &settings,
        &prompt,
        &working_directory,
        &mut backend,
    );
    finish_stage(
        backend.recorded,
        backend.failure,
        run,
        "situation assessment",
    )
}

pub(crate) fn propose_design(
    path: &Path,
    config: &Config,
    todo_id: TodoId,
    quality: Option<ModelQuality>,
    model: Option<&str>,
) -> AppResult<StageOutput<DesignView>> {
    let connection = db::open_write(path)?;
    let assessment = store::latest_current_ready_assessment(&connection, todo_id)?;
    let based_on = assessment.based_on_design_id;
    run_design(
        connection, config, assessment, based_on, None, quality, model,
    )
}

pub(crate) fn correct_design(
    path: &Path,
    config: &Config,
    design_id: DesignId,
    feedback: &str,
    quality: Option<ModelQuality>,
    model: Option<&str>,
) -> AppResult<StageOutput<DesignView>> {
    if feedback.trim().is_empty() {
        return Err(AppError::invalid(
            "blank_design_feedback",
            "design correction feedback must not be blank",
        ));
    }
    let connection = db::open_write(path)?;
    let design = store::get_design(&connection, design_id)?;
    if design.state == "open" {
        return Err(AppError::conflict(
            "design_still_open",
            "an open design must finish its original liaison run before correction",
        ));
    }
    if design.state == "authorized" {
        return Err(AppError::conflict(
            "accepted_design_requires_reassessment",
            "an accepted design changed the authoritative basis; assess the todo again before proposing a correction",
        ));
    }
    if !matches!(design.state.as_str(), "ready" | "rejected" | "abandoned") {
        return Err(AppError::conflict(
            "design_not_correctable",
            format!("design cannot be corrected from state {}", design.state),
        ));
    }
    let assessment_id = design.assessment_id.ok_or_else(|| {
        AppError::conflict(
            "legacy_design_has_no_assessment",
            "a legacy design must be reassessed before it can be corrected",
        )
    })?;
    let assessment = store::get_assessment(&connection, assessment_id)?;
    if !assessment.current || assessment.disposition != "ready" {
        return Err(AppError::conflict(
            "design_basis_changed",
            "the design's situation assessment is no longer current and ready",
        ));
    }
    run_design(
        connection,
        config,
        assessment,
        Some(design_id),
        Some((&design, feedback)),
        quality,
        model,
    )
}

fn run_design(
    mut connection: Connection,
    config: &Config,
    assessment: SituationAssessmentView,
    based_on_design_id: Option<DesignId>,
    correction: Option<(&DesignView, &str)>,
    quality: Option<ModelQuality>,
    model: Option<&str>,
) -> AppResult<StageOutput<DesignView>> {
    let snapshot = store::assessment_snapshot(&connection, assessment.todo_id)?;
    let working_directory = working_directory()?;
    let identity = new_identity(Stage::DesignReconciliation);
    let job = store::record_agent_job(
        &mut connection,
        "design_reconciliation",
        None,
        Some(assessment.todo_id),
        &snapshot.base_digest,
        identity.requester_id(),
        identity.job_id(),
    )?;
    let prior = match based_on_design_id {
        Some(id) => Some(store::get_design(&connection, id)?),
        None => None,
    };
    let correction_record = correction
        .map(|(design, feedback)| {
            store::record_design_correction(&mut connection, job.id, design.id, feedback)
        })
        .transpose()?;
    let basis_catalog = design_basis_catalog(
        &snapshot,
        &assessment,
        prior.as_ref(),
        correction_record.as_ref(),
    );
    let correction_prompt = correction_record.as_ref().map(|record| {
        json!({
            "basis_ref": record.basis_ref,
            "feedback": record.feedback,
        })
    });
    let prompt = serde_json::to_string_pretty(&json!({
        "todo_id": assessment.todo_id,
        "direction": snapshot.direction,
        "situation_assessment": assessment,
        "based_on_design": prior,
        "correction": correction_prompt,
        "basis_catalog": basis_catalog,
        "instruction": "Submit a desired-state design only. Do not include implementation steps, work planning, deployment actions, or execution state.",
    }))?;
    let settings = model_settings(config, quality, model);
    let mut backend = DesignBackend {
        connection,
        assessment,
        based_on_design_id,
        agent_job_id: job.id,
        draft: None,
        failure: None,
        returned_for_assessment: None,
    };
    let run = Runner::for_current_user().run_stage(
        Stage::DesignReconciliation,
        &identity,
        &settings,
        &prompt,
        &working_directory,
        &mut backend,
    );
    abandon_unfinished_design(&mut backend);
    finish_design_stage(
        backend.draft,
        backend.failure,
        backend.returned_for_assessment,
        run,
    )
}

fn abandon_unfinished_design(backend: &mut DesignBackend) {
    let unfinished_id = backend
        .draft
        .as_ref()
        .filter(|draft| draft.state == "open")
        .map(|draft| draft.id);
    if let (None, Some(id)) = (&backend.returned_for_assessment, unfinished_id) {
        match store::abandon_open_design(
            &mut backend.connection,
            id,
            "the design liaison ended before producing a ready design",
        ) {
            Ok(draft) => backend.draft = Some(draft),
            Err(error) => {
                backend.draft = None;
                backend.failure = Some(error);
            }
        }
    }
}

fn finish_design_stage(
    draft: Option<DesignView>,
    failure: Option<AppError>,
    returned_for_assessment: Option<AssessmentReturnView>,
    run: AppResult<String>,
) -> AppResult<StageOutput<DesignView>> {
    if let Some(returned) = returned_for_assessment {
        return Err(AppError::conflict(
            "assessment_research_needed",
            format!(
                "{}; missing or stale references: {}",
                returned.reason,
                returned.missing_or_stale_refs.join(", ")
            ),
        ));
    }
    if let Some(draft) = draft {
        let run_error = run.err();
        let diagnostic = if draft.state == "abandoned" {
            Some(match run_error {
                Some(error) => format!(
                    "the liaison ended before producing a ready design; the incomplete draft was retained as abandoned: {error}"
                ),
                None => "the liaison ended before producing a ready design; the incomplete draft was retained as abandoned".to_owned(),
            })
        } else {
            run_error.map(|error| {
                format!("the design was retained, but the liaison ended afterward: {error}")
            })
        };
        return Ok(StageOutput {
            artifact: draft,
            diagnostic,
        });
    }
    if let Some(error) = failure {
        return Err(error);
    }
    match run {
        Err(error) => Err(error),
        Ok(_) => Err(AppError::unexpected(
            "model_did_not_produce_design",
            "the design liaison exited without producing or explicitly returning a design",
        )),
    }
}

fn finish_stage<T>(
    artifact: Option<T>,
    failure: Option<AppError>,
    run: AppResult<String>,
    kind: &str,
) -> AppResult<StageOutput<T>> {
    if let Some(artifact) = artifact {
        let diagnostic = run.err().map(|error| {
            format!("the {kind} was retained, but the liaison ended afterward: {error}")
        });
        return Ok(StageOutput {
            artifact,
            diagnostic,
        });
    }
    if let Some(error) = failure {
        return Err(error);
    }
    match run {
        Err(error) => Err(error),
        Ok(_) => Err(AppError::unexpected(
            "model_omitted_terminal_artifact",
            format!("the liaison exited without recording a {kind}"),
        )),
    }
}

fn model_settings(
    config: &Config,
    quality: Option<ModelQuality>,
    model: Option<&str>,
) -> ModelSettings {
    ModelSettings::new(
        quality.unwrap_or(config.liaison.quality),
        model.or(config.liaison.model.as_deref()),
    )
}

fn new_identity(stage: Stage) -> RunIdentity {
    let suffix = Uuid::now_v7();
    RunIdentity::new(
        format!("todo-{}-request-{suffix}", stage.slug()),
        format!("todo-{}-{suffix}", stage.slug()),
    )
}

fn working_directory() -> AppResult<PathBuf> {
    let current = std::env::current_dir().map_err(|error| {
        AppError::unexpected(
            "working_directory_unavailable",
            format!("unable to determine the caller's working directory: {error}"),
        )
    })?;
    fs::canonicalize(&current).map_err(|error| {
        AppError::unexpected(
            "working_directory_unavailable",
            format!("unable to resolve the caller's working directory: {error}"),
        )
    })
}

fn nearest_git_workspace(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

fn routing_prompt_refs(snapshot: &RoutingSnapshot) -> BTreeSet<String> {
    BTreeSet::from([
        format!("concern:{}", snapshot.concern.id),
        format!("routing-snapshot:{}", snapshot.base_digest),
    ])
}

fn assessment_prompt_refs(snapshot: &AssessmentSnapshot, view: &TodoView) -> BTreeSet<String> {
    let direction = &snapshot.direction;
    let direction_ref = format!(
        "direction:{}@revision:{}",
        direction.todo_id, direction.revision
    );
    let mut refs = BTreeSet::from([
        format!("todo:{}", snapshot.todo_id),
        format!("assessment-snapshot:{}", snapshot.base_digest),
        format!("concern-set:{}", snapshot.concern_set_digest),
        direction_ref.clone(),
    ]);
    for boundary in &direction.boundaries {
        refs.insert(format!("{direction_ref}#boundary:{}", boundary.local_ref));
        refs.extend(boundary.source_refs.iter().cloned());
    }
    refs.extend(
        view.concerns
            .iter()
            .map(|concern| format!("concern:{}", concern.id)),
    );
    refs.extend(
        view.working_notes
            .iter()
            .map(|note| format!("note:{}", note.id)),
    );
    if let Some(design_id) = snapshot.based_on_design_id {
        refs.insert(format!("design:{design_id}"));
    }
    refs
}

fn design_basis_catalog(
    snapshot: &AssessmentSnapshot,
    assessment: &SituationAssessmentView,
    predecessor: Option<&DesignView>,
    correction: Option<&DesignCorrection>,
) -> Vec<String> {
    let mut refs = BTreeSet::from([
        "direction:body".to_owned(),
        format!("assessment:{}", assessment.id),
    ]);
    refs.extend(
        snapshot
            .direction
            .boundaries
            .iter()
            .map(|boundary| format!("direction:{}", boundary.local_ref)),
    );
    refs.extend(assessment.findings.iter().filter_map(|finding| {
        finding
            .get("ref")
            .and_then(Value::as_str)
            .map(|local_ref| format!("assessment:{}:finding:{local_ref}", assessment.id))
    }));
    refs.extend(assessment.jurisdictions.iter().filter_map(|jurisdiction| {
        jurisdiction
            .get("key")
            .and_then(Value::as_str)
            .map(|key| format!("assessment:{}:jurisdiction:{key}", assessment.id))
    }));
    if let Some(predecessor) = predecessor {
        for operation in predecessor
            .jurisdiction_changes
            .iter()
            .chain(predecessor.clauses.iter())
            .chain(predecessor.unresolved_choices.iter())
        {
            if operation.get("status").and_then(Value::as_str) == Some("active")
                && let Some(operation_id) = operation.get("operation_id").and_then(Value::as_str)
            {
                refs.insert(format!("design:{}:{operation_id}", predecessor.id));
            }
        }
    }
    if let Some(correction) = correction {
        refs.insert(correction.basis_ref.clone());
    }
    refs.into_iter().collect()
}

fn candidate_summary_evidence_ref(candidate: &RoutingCandidate) -> String {
    format!(
        "candidate:{}@direction:{}#summary",
        candidate.id, candidate.direction_revision
    )
}

fn candidate_page_evidence_ref(candidate: &RoutingCandidate, offset: usize, end: usize) -> String {
    format!(
        "candidate:{}@direction:{}@chars:{offset}-{end}",
        candidate.id, candidate.direction_revision
    )
}

fn validate_routing_evidence_refs(
    proposal: &ConcernRoutingProposal,
    allowed: &BTreeSet<String>,
) -> Result<(), ToolFailure> {
    validate_evidence_refs(proposal.evidence_refs.iter(), allowed)?;
    if let Some(direction) = &proposal.proposed_direction {
        validate_evidence_refs(
            direction
                .boundaries
                .iter()
                .flat_map(|boundary| boundary.basis_refs.iter()),
            allowed,
        )?;
    }
    Ok(())
}

fn validate_assessment_evidence_refs(
    assessment: &SituationAssessment,
    allowed: &BTreeSet<String>,
) -> Result<(), ToolFailure> {
    validate_evidence_refs(assessment.subject.identity_refs.iter(), allowed)?;
    validate_evidence_refs(
        assessment
            .findings
            .iter()
            .flat_map(|finding| finding.evidence_refs.iter()),
        allowed,
    )?;
    validate_evidence_refs(
        assessment
            .jurisdictions
            .iter()
            .flat_map(|jurisdiction| jurisdiction.evidence_refs.iter()),
        allowed,
    )?;
    validate_evidence_refs(
        assessment
            .unresolved
            .iter()
            .flat_map(|unresolved| unresolved.evidence_refs.iter()),
        allowed,
    )
}

fn validate_evidence_refs<'a>(
    refs: impl IntoIterator<Item = &'a String>,
    allowed: &BTreeSet<String>,
) -> Result<(), ToolFailure> {
    for evidence_ref in refs {
        if !allowed.contains(evidence_ref) {
            return Err(ToolFailure::new(
                "evidence_ref_not_in_scope",
                format!(
                    "evidence reference was neither emitted by a managed read nor supplied as a canonical prompt reference: {evidence_ref}"
                ),
            ));
        }
    }
    Ok(())
}

struct RoutingBackend {
    connection: Connection,
    snapshot: RoutingSnapshot,
    sources: BoundedSources,
    canonical_evidence_refs: BTreeSet<String>,
    emitted_candidate_refs: BTreeSet<String>,
    agent_job_id: i64,
    recorded: Option<RoutingProposalView>,
    failure: Option<AppError>,
}

impl Backend for RoutingBackend {
    fn call(&mut self, tool_call_id: &str, call: Call) -> Result<ToolSuccess, ToolFailure> {
        match call {
            Call::RoutingSourceOverview(request) => self.sources.overview(&request),
            Call::RoutingSourceRead(request) => self.sources.read(&request),
            Call::RoutingSourceSearch(request) => self.sources.search(&request),
            Call::RoutingCandidates(request) => self.candidates(&request),
            Call::RoutingCandidateInspect(request) => self.inspect_candidate(&request),
            Call::SubmitConcernRouting(proposal) => {
                if self.recorded.is_some() {
                    return Err(ToolFailure::new(
                        "routing_already_submitted",
                        "this run has already recorded its routing proposal",
                    ));
                }
                self.validate_evidence(&proposal)?;
                match store::create_routing_proposal(
                    &mut self.connection,
                    &self.snapshot,
                    self.agent_job_id,
                    tool_call_id,
                    &proposal,
                ) {
                    Ok(view) => {
                        let id = view.id.to_string();
                        self.recorded = Some(view);
                        Ok(ToolSuccess::recorded("routing_proposal", &id, 1, "pending"))
                    }
                    Err(error) => self.store_failure(error),
                }
            }
            _ => Err(unsupported("concern-routing")),
        }
    }
}

impl RoutingBackend {
    fn candidates(&mut self, request: &PageRequest) -> Result<ToolSuccess, ToolFailure> {
        let offset = parse_cursor(request.cursor.as_deref())?;
        if offset > self.snapshot.candidates.len() {
            return Err(invalid_cursor());
        }
        let end = (offset + CANDIDATE_PAGE).min(self.snapshot.candidates.len());
        let candidates = self.snapshot.candidates[offset..end]
            .iter()
            .map(|candidate| {
                let title = candidate
                    .title
                    .chars()
                    .take(CANDIDATE_TITLE_CHARS)
                    .collect::<String>();
                let evidence_ref = candidate_summary_evidence_ref(candidate);
                self.emitted_candidate_refs.insert(evidence_ref.clone());
                json!({
                    "id": candidate.id,
                    "title": title,
                    "title_truncated": candidate.title.chars().count() > CANDIDATE_TITLE_CHARS,
                    "direction_revision": candidate.direction_revision,
                    "status": candidate.status,
                    "boundary_count": candidate.boundaries.len(),
                    "evidence_ref": evidence_ref,
                })
            })
            .collect::<Vec<_>>();
        Ok(ToolSuccess::data(json!({
            "candidates": candidates,
            "next_cursor": next_cursor(end, self.snapshot.candidates.len()),
            "snapshot_digest": self.snapshot.base_digest,
        })))
    }

    fn inspect_candidate(
        &mut self,
        request: &CandidateReadRequest,
    ) -> Result<ToolSuccess, ToolFailure> {
        let todo_id = request.candidate_id.parse::<TodoId>().map_err(|error| {
            ToolFailure::new(
                "invalid_candidate_id",
                format!("invalid candidate ID: {error}"),
            )
        })?;
        let candidate = self
            .snapshot
            .candidates
            .iter()
            .find(|candidate| candidate.id == todo_id)
            .ok_or_else(|| {
                ToolFailure::new(
                    "candidate_not_in_snapshot",
                    "candidate is not part of this frozen routing snapshot",
                )
            })?;
        let offset = parse_cursor(request.cursor.as_deref())?;
        let content = serde_json::to_string_pretty(candidate)
            .map_err(|error| ToolFailure::new("candidate_encode_failed", error.to_string()))?;
        let total = content.chars().count();
        if offset > total {
            return Err(invalid_cursor());
        }
        let page = content
            .chars()
            .skip(offset)
            .take(READ_PAGE_CHARS)
            .collect::<String>();
        let end = offset + page.chars().count();
        let evidence_ref = candidate_page_evidence_ref(candidate, offset, end);
        self.emitted_candidate_refs.insert(evidence_ref.clone());
        Ok(ToolSuccess::data(json!({
            "candidate_id": todo_id,
            "direction_revision": candidate.direction_revision,
            "content": page,
            "evidence_ref": evidence_ref,
            "next_cursor": next_cursor(end, total),
        })))
    }

    fn validate_evidence(&self, proposal: &ConcernRoutingProposal) -> Result<(), ToolFailure> {
        let mut allowed = self.canonical_evidence_refs.clone();
        allowed.extend(self.emitted_candidate_refs.iter().cloned());
        allowed.extend(self.sources.emitted_evidence_refs().iter().cloned());
        validate_routing_evidence_refs(proposal, &allowed)
    }

    fn store_failure<T>(&mut self, error: AppError) -> Result<T, ToolFailure> {
        let failure = app_tool_failure_ref(&error);
        if self.failure.is_none() {
            self.failure = Some(error);
        }
        Err(failure)
    }
}

struct AssessmentBackend {
    connection: Connection,
    snapshot: AssessmentSnapshot,
    sources: BoundedSources,
    canonical_evidence_refs: BTreeSet<String>,
    agent_job_id: i64,
    recorded: Option<SituationAssessmentView>,
    failure: Option<AppError>,
}

impl Backend for AssessmentBackend {
    fn call(&mut self, tool_call_id: &str, call: Call) -> Result<ToolSuccess, ToolFailure> {
        match call {
            Call::SituationSources(request) => self.sources.overview(&request),
            Call::SituationSourceRead(request) => self.sources.read(&request),
            Call::SituationSourceSearch(request) => self.sources.search(&request),
            Call::SubmitSituationAssessment(assessment) => {
                if self.recorded.is_some() {
                    return Err(ToolFailure::new(
                        "assessment_already_submitted",
                        "this run has already recorded its situation assessment",
                    ));
                }
                self.validate_evidence(&assessment)?;
                let mut bases = self.sources.accessed_bases();
                bases.push(AssessmentBase {
                    source_ref: "todo-snapshot".to_owned(),
                    kind: "todo".to_owned(),
                    locator: self.snapshot.todo_id.to_string(),
                    revision: self.snapshot.base_digest.clone(),
                    observed_at: observed_now(),
                });
                match store::create_situation_assessment(
                    &mut self.connection,
                    &self.snapshot,
                    self.agent_job_id,
                    tool_call_id,
                    &assessment,
                    &bases,
                ) {
                    Ok(view) => {
                        let id = view.id.to_string();
                        self.recorded = Some(view);
                        Ok(ToolSuccess::recorded(
                            "situation_assessment",
                            &id,
                            1,
                            "recorded",
                        ))
                    }
                    Err(error) => self.store_failure(error),
                }
            }
            _ => Err(unsupported("situation-assessment")),
        }
    }
}

impl AssessmentBackend {
    fn validate_evidence(&self, assessment: &SituationAssessment) -> Result<(), ToolFailure> {
        let mut allowed = self.canonical_evidence_refs.clone();
        allowed.extend(self.sources.emitted_evidence_refs().iter().cloned());
        validate_assessment_evidence_refs(assessment, &allowed)
    }

    fn store_failure<T>(&mut self, error: AppError) -> Result<T, ToolFailure> {
        let failure = app_tool_failure_ref(&error);
        if self.failure.is_none() {
            self.failure = Some(error);
        }
        Err(failure)
    }
}

struct DesignBackend {
    connection: Connection,
    assessment: SituationAssessmentView,
    based_on_design_id: Option<DesignId>,
    agent_job_id: i64,
    draft: Option<DesignView>,
    failure: Option<AppError>,
    returned_for_assessment: Option<AssessmentReturnView>,
}

impl Backend for DesignBackend {
    fn call(&mut self, tool_call_id: &str, call: Call) -> Result<ToolSuccess, ToolFailure> {
        match call {
            Call::SubmitDesignReconciliation(submission) => {
                self.require_active_run()?;
                if self.draft.is_some() {
                    return Err(ToolFailure::new(
                        "design_already_submitted",
                        "this run already has a design draft; revise it by operation ID",
                    ));
                }
                match store::create_design(
                    &mut self.connection,
                    &self.assessment,
                    self.based_on_design_id,
                    self.agent_job_id,
                    tool_call_id,
                    &submission,
                ) {
                    Ok(view) => self.record_design(view),
                    Err(error) => self.store_failure(error),
                }
            }
            Call::ReviseDesignReconciliation(revision) => {
                self.require_active_run()?;
                let id = self
                    .draft
                    .as_ref()
                    .ok_or_else(|| {
                        ToolFailure::new(
                            "design_not_submitted",
                            "submit the initial design before revising it",
                        )
                    })?
                    .id;
                match store::revise_design(&mut self.connection, id, &revision) {
                    Ok(view) => self.record_design(view),
                    Err(error) => self.store_failure(error),
                }
            }
            Call::DesignReconciliationStatus(_) => {
                self.require_active_run()?;
                let view = self.draft.as_ref().ok_or_else(|| {
                    ToolFailure::new(
                        "design_not_submitted",
                        "no design has been submitted in this run",
                    )
                })?;
                serde_json::to_value(view)
                    .map(ToolSuccess::data)
                    .map_err(|error| ToolFailure::new("design_encode_failed", error.to_string()))
            }
            Call::DiscardDesignReconciliation(discard) => {
                self.require_active_run()?;
                let id = self
                    .draft
                    .as_ref()
                    .ok_or_else(|| {
                        ToolFailure::new(
                            "design_not_submitted",
                            "no design has been submitted in this run",
                        )
                    })?
                    .id;
                match store::discard_design(
                    &mut self.connection,
                    id,
                    discard.expected_version,
                    &discard.reason,
                ) {
                    Ok(view) => self.record_design(view),
                    Err(error) => self.store_failure(error),
                }
            }
            Call::ReturnForAssessment(request) => {
                self.return_for_assessment(tool_call_id, &request)
            }
            _ => Err(unsupported("design-reconciliation")),
        }
    }
}

impl DesignBackend {
    fn return_for_assessment(
        &mut self,
        tool_call_id: &str,
        request: &AssessmentReturn,
    ) -> Result<ToolSuccess, ToolFailure> {
        let returned = match store::record_assessment_return(
            &mut self.connection,
            self.agent_job_id,
            self.assessment.id,
            tool_call_id,
            &request.reason,
            &request.missing_or_stale_refs,
        ) {
            Ok(returned) => returned,
            Err(error) => return self.store_failure(error),
        };
        let design_id = returned.design_id;
        self.returned_for_assessment = Some(returned);
        if let Some(design_id) = design_id {
            match store::get_design(&self.connection, design_id) {
                Ok(view) => self.draft = Some(view),
                Err(error) => return self.store_failure(error),
            }
        }
        Ok(ToolSuccess::recorded(
            "assessment_return",
            &self.assessment.id.to_string(),
            1,
            "returned",
        ))
    }

    fn require_active_run(&self) -> Result<(), ToolFailure> {
        if self.returned_for_assessment.is_some() {
            Err(ToolFailure::new(
                "assessment_already_returned",
                "this run already returned for more situation assessment",
            ))
        } else {
            Ok(())
        }
    }

    fn record_design(&mut self, view: DesignView) -> Result<ToolSuccess, ToolFailure> {
        let id = view.id.to_string();
        let version = u64::try_from(view.draft_version).map_err(|_| {
            ToolFailure::new("invalid_design_version", "stored design version is invalid")
        })?;
        let state = view.state.clone();
        self.draft = Some(view);
        Ok(ToolSuccess::recorded("design", &id, version, &state))
    }

    fn store_failure<T>(&mut self, error: AppError) -> Result<T, ToolFailure> {
        let failure = app_tool_failure_ref(&error);
        if self.failure.is_none() {
            self.failure = Some(error);
        }
        Err(failure)
    }
}

#[derive(Clone)]
struct SourceRecord {
    id: String,
    path: PathBuf,
    locator: String,
    revision: String,
    observed_at: String,
}

struct BoundedSources {
    workspace_root: Option<PathBuf>,
    order: Vec<String>,
    records: BTreeMap<String, SourceRecord>,
    accessed: BTreeSet<String>,
    emitted_evidence_refs: BTreeSet<String>,
}

impl BoundedSources {
    fn explicit_only(explicit: impl IntoIterator<Item = PathBuf>) -> AppResult<Self> {
        Self::build(None, explicit)
    }

    fn with_workspace(root: &Path, explicit: impl IntoIterator<Item = PathBuf>) -> AppResult<Self> {
        let root = fs::canonicalize(root)?;
        Self::build(Some(&root), explicit)
    }

    fn build(
        workspace_root: Option<&Path>,
        explicit: impl IntoIterator<Item = PathBuf>,
    ) -> AppResult<Self> {
        let mut paths = Vec::new();
        let mut seen = BTreeSet::new();
        for path in explicit {
            if let Ok(path) = fs::canonicalize(path)
                && path.is_file()
                && seen.insert(path.clone())
            {
                paths.push(path);
            }
        }
        if let Some(root) = workspace_root {
            let mut workspace = Vec::new();
            enumerate_files(root, &mut workspace);
            workspace.sort();
            workspace.truncate(MAX_WORKSPACE_FILES);
            for path in workspace {
                if seen.insert(path.clone()) {
                    paths.push(path);
                }
            }
        }
        let mut order = Vec::new();
        let mut records = BTreeMap::new();
        for path in paths {
            let id = stable_source_id(&path);
            let metadata = fs::metadata(&path)?;
            let locator = path.to_string_lossy().into_owned();
            let record = SourceRecord {
                id: id.clone(),
                path,
                locator,
                revision: metadata_revision(&metadata),
                observed_at: observed_now(),
            };
            order.push(id.clone());
            records.insert(id, record);
        }
        Ok(Self {
            workspace_root: workspace_root.map(Path::to_path_buf),
            order,
            records,
            accessed: BTreeSet::new(),
            emitted_evidence_refs: BTreeSet::new(),
        })
    }

    fn overview(&self, request: &PageRequest) -> Result<ToolSuccess, ToolFailure> {
        let offset = parse_cursor(request.cursor.as_deref())?;
        let workspace_offset = usize::from(self.workspace_root.is_some());
        let total = self.order.len() + workspace_offset;
        if offset > total {
            return Err(invalid_cursor());
        }
        let end = (offset + SOURCE_PAGE).min(total);
        let mut sources = Vec::new();
        for position in offset..end {
            if position == 0
                && let Some(root) = &self.workspace_root
            {
                sources.push(json!({
                    "source_id": "workspace",
                    "kind": "workspace_index",
                    "locator": root,
                    "description": "Search across the frozen workspace file catalog; search results return exact source IDs for reading.",
                }));
            } else if let Some(record) = self
                .order
                .get(position - workspace_offset)
                .and_then(|id| self.records.get(id))
            {
                sources.push(source_summary(record));
            }
        }
        Ok(ToolSuccess::data(json!({
            "sources": sources,
            "next_cursor": next_cursor(end, total),
        })))
    }

    fn read(&mut self, request: &SourceReadRequest) -> Result<ToolSuccess, ToolFailure> {
        if request.source_id == "workspace" {
            if self.workspace_root.is_none() {
                return Err(ToolFailure::new(
                    "source_not_in_snapshot",
                    "the workspace catalog is not available in this bounded source scope",
                ));
            }
            return Err(ToolFailure::new(
                "workspace_is_search_only",
                "search the workspace index, then read an exact returned source ID",
            ));
        }
        let offset = parse_cursor(request.cursor.as_deref())?;
        let record = self.record(&request.source_id)?.clone();
        let content = read_frozen(&record)?;
        let total = content.chars().count();
        if offset > total {
            return Err(invalid_cursor());
        }
        let text = content
            .chars()
            .skip(offset)
            .take(READ_PAGE_CHARS)
            .collect::<String>();
        let end = offset + text.chars().count();
        self.accessed.insert(record.id.clone());
        let evidence_ref = format!("source:{}@chars:{offset}-{end}", record.id);
        self.emitted_evidence_refs.insert(evidence_ref.clone());
        Ok(ToolSuccess::data(json!({
            "source_id": record.id,
            "locator": record.locator,
            "revision": record.revision,
            "content": text,
            "evidence_ref": evidence_ref,
            "next_cursor": next_cursor(end, total),
        })))
    }

    fn search(&mut self, request: &SourceSearchRequest) -> Result<ToolSuccess, ToolFailure> {
        let query = request.query.to_lowercase();
        if request.source_id == "workspace" {
            return self.search_workspace(&query, request.cursor.as_deref());
        }
        let record = self.record(&request.source_id)?.clone();
        let line_offset = parse_cursor(request.cursor.as_deref())?;
        let content = read_frozen(&record)?;
        let lines = content.lines().collect::<Vec<_>>();
        if line_offset > lines.len() {
            return Err(invalid_cursor());
        }
        let mut matches = Vec::new();
        let mut next = lines.len();
        for (index, line) in lines.iter().enumerate().skip(line_offset) {
            if line.to_lowercase().contains(&query) {
                self.emitted_evidence_refs
                    .insert(source_line_evidence_ref(&record, index + 1));
                matches.push(search_match(&record, index + 1, line));
                if matches.len() == SEARCH_MATCHES {
                    next = index + 1;
                    break;
                }
            }
        }
        self.accessed.insert(record.id.clone());
        Ok(ToolSuccess::data(json!({
            "source_id": record.id,
            "query": request.query,
            "matches": matches,
            "next_cursor": next_cursor(next, lines.len()),
        })))
    }

    fn search_workspace(
        &mut self,
        query: &str,
        cursor: Option<&str>,
    ) -> Result<ToolSuccess, ToolFailure> {
        if self.workspace_root.is_none() {
            return Err(ToolFailure::new(
                "source_not_in_snapshot",
                "the workspace catalog is not available in this bounded source scope",
            ));
        }
        let offset = parse_cursor(cursor)?;
        if offset > self.order.len() {
            return Err(invalid_cursor());
        }
        let mut matches = Vec::new();
        let mut next = self.order.len();
        for (position, id) in self.order.iter().enumerate().skip(offset) {
            let Some(record) = self.records.get(id) else {
                continue;
            };
            let Ok(metadata) = fs::metadata(&record.path) else {
                continue;
            };
            if metadata.len() > MAX_SEARCHABLE_BYTES {
                continue;
            }
            let Ok(content) = read_frozen(record) else {
                continue;
            };
            let mut matched_record = false;
            for (line_index, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(query) {
                    self.emitted_evidence_refs
                        .insert(source_line_evidence_ref(record, line_index + 1));
                    matches.push(search_match(record, line_index + 1, line));
                    matched_record = true;
                    if matches.len() == SEARCH_MATCHES {
                        next = position + 1;
                        break;
                    }
                }
            }
            if matched_record {
                self.accessed.insert(id.clone());
            }
            if matches.len() == SEARCH_MATCHES {
                break;
            }
        }
        Ok(ToolSuccess::data(json!({
            "source_id": "workspace",
            "query": query,
            "matches": matches,
            "next_cursor": next_cursor(next, self.order.len()),
        })))
    }

    fn record(&self, id: &str) -> Result<&SourceRecord, ToolFailure> {
        self.records.get(id).ok_or_else(|| {
            ToolFailure::new(
                "source_not_in_snapshot",
                format!("source ID is not part of this frozen snapshot: {id}"),
            )
        })
    }

    fn accessed_bases(&self) -> Vec<AssessmentBase> {
        self.accessed
            .iter()
            .filter_map(|id| self.records.get(id))
            .map(|record| AssessmentBase {
                source_ref: format!("source:{}", record.id),
                kind: "document".to_owned(),
                locator: record.locator.clone(),
                revision: record.revision.clone(),
                observed_at: record.observed_at.clone(),
            })
            .collect()
    }

    fn emitted_evidence_refs(&self) -> &BTreeSet<String> {
        &self.emitted_evidence_refs
    }
}

fn source_summary(record: &SourceRecord) -> Value {
    json!({
        "source_id": record.id,
        "kind": "document",
        "locator": record.locator,
        "revision": record.revision,
    })
}

fn stable_source_id(path: &Path) -> String {
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    format!("s-{digest:x}")
}

fn search_match(record: &SourceRecord, line: usize, text: &str) -> Value {
    let snippet = text.chars().take(2_000).collect::<String>();
    json!({
        "source_id": record.id,
        "locator": record.locator,
        "line": line,
        "text": snippet,
        "evidence_ref": source_line_evidence_ref(record, line),
    })
}

fn source_line_evidence_ref(record: &SourceRecord, line: usize) -> String {
    format!("source:{}@line:{line}", record.id)
}

fn read_frozen(record: &SourceRecord) -> Result<String, ToolFailure> {
    let metadata = fs::metadata(&record.path).map_err(|error| {
        ToolFailure::new(
            "source_unreadable",
            format!("unable to inspect {}: {error}", record.locator),
        )
    })?;
    if metadata_revision(&metadata) != record.revision {
        return Err(ToolFailure::new(
            "source_changed",
            format!("source changed during the frozen run: {}", record.locator),
        ));
    }
    fs::read_to_string(&record.path).map_err(|error| {
        ToolFailure::new(
            "source_unreadable",
            format!(
                "source must be readable UTF-8 text {}: {error}",
                record.locator
            ),
        )
    })
}

fn enumerate_files(root: &Path, output: &mut Vec<PathBuf>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if !skip_directory(&path) {
                    stack.push(path);
                }
            } else if file_type.is_file()
                && let Ok(path) = fs::canonicalize(path)
                && path.starts_with(root)
                && admissible_workspace_file(&path)
            {
                output.push(path);
                if output.len() >= MAX_WORKSPACE_FILES * 2 {
                    return;
                }
            }
        }
    }
}

fn skip_directory(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    name.starts_with('.')
        || matches!(
            name.to_ascii_lowercase().as_str(),
            "target"
                | "node_modules"
                | "vendor"
                | "logs"
                | "log"
                | "secrets"
                | "secret"
                | "credentials"
        )
}

fn admissible_workspace_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name.starts_with('.') || sensitive_file_name(name) || excluded_file_extension(path) {
        return false;
    }
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if metadata.len() > MAX_SEARCHABLE_BYTES {
        return false;
    }
    likely_utf8_text(path)
}

fn sensitive_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "id_rsa"
            | "id_dsa"
            | "id_ecdsa"
            | "id_ed25519"
            | "passwd"
            | "shadow"
            | "credentials"
            | "secrets"
    ) {
        return true;
    }
    lower
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|part| {
            matches!(
                part,
                "secret"
                    | "secrets"
                    | "credential"
                    | "credentials"
                    | "password"
                    | "passwords"
                    | "privatekey"
            )
        })
}

fn excluded_file_extension(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "db" | "db3"
            | "sqlite"
            | "sqlite3"
            | "sqlite-wal"
            | "sqlite-shm"
            | "wal"
            | "shm"
            | "log"
            | "pem"
            | "key"
            | "p12"
            | "pfx"
            | "der"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "bmp"
            | "ico"
            | "pdf"
            | "zip"
            | "gz"
            | "xz"
            | "bz2"
            | "7z"
            | "tar"
            | "wasm"
            | "o"
            | "a"
            | "so"
            | "dylib"
            | "dll"
            | "exe"
            | "bin"
            | "class"
            | "jar"
            | "pyc"
            | "pyo"
            | "woff"
            | "woff2"
            | "ttf"
            | "otf"
            | "mp3"
            | "mp4"
            | "mov"
            | "wav"
            | "flac"
    )
}

fn likely_utf8_text(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut sample = [0_u8; 8 * 1024];
    let Ok(length) = file.read(&mut sample) else {
        return false;
    };
    let sample = &sample[..length];
    if sample
        .iter()
        .any(|byte| *byte == 0 || (*byte < b'\t') || (*byte > b'\r' && *byte < b' '))
    {
        return false;
    }
    match std::str::from_utf8(sample) {
        Ok(_) => true,
        Err(error) => error.error_len().is_none() && error.valid_up_to() + 4 >= sample.len(),
    }
}

fn metadata_revision(metadata: &fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    format!("size:{};mtime-ns:{modified}", metadata.len())
}

fn observed_now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_secs());
            format!("unix:{now}")
        })
}

fn parse_cursor(cursor: Option<&str>) -> Result<usize, ToolFailure> {
    match cursor {
        None => Ok(0),
        Some(value) => value.parse().map_err(|_| invalid_cursor()),
    }
}

fn next_cursor(position: usize, total: usize) -> Value {
    if position < total {
        Value::String(position.to_string())
    } else {
        Value::Null
    }
}

fn invalid_cursor() -> ToolFailure {
    ToolFailure::new(
        "invalid_cursor",
        "cursor does not belong to this bounded result",
    )
}

fn unsupported(stage: &str) -> ToolFailure {
    ToolFailure::new(
        "unsupported_tool",
        format!("tool is not available in the {stage} backend"),
    )
}

fn app_tool_failure_ref(error: &AppError) -> ToolFailure {
    ToolFailure::new(error.code(), error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;

    use rusqlite::params;
    use serde_json::json;

    use super::{
        BoundedSources, DesignBackend, READ_PAGE_CHARS, RoutingBackend, abandon_unfinished_design,
        finish_design_stage, nearest_git_workspace, validate_assessment_evidence_refs,
        validate_routing_evidence_refs,
    };
    use crate::db;
    use crate::model::{ConcernId, SituationAssessmentId, TodoId, TodoStatus};
    use crate::reconciliation_store::{
        self as store, Concern, ConcernStatus, DirectionBoundary, RoutingCandidate, RoutingSnapshot,
    };
    use crate::tool_server::contracts::{
        AssessmentReturn, CandidateReadRequest, ConcernRoutingProposal, DesignSubmission,
        PageRequest, SituationAssessment, SourceReadRequest, SourceSearchRequest,
    };
    use crate::tool_server::{Backend, Call, ToolFailure};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn explicit_source_scope_never_exposes_a_workspace_catalog() -> TestResult {
        let directory = tempfile::tempdir()?;
        let origin = directory.path().join("captured.txt");
        fs::write(&origin, "captured evidence\nsecond line")?;
        fs::write(directory.path().join("unrelated.txt"), "must stay out")?;
        let mut sources = BoundedSources::explicit_only([origin.clone()])?;

        let overview = sources
            .overview(&PageRequest { cursor: None })
            .map_err(|failure| tool_failure_error(&failure))?;
        let listed = overview.output()["data"]["sources"]
            .as_array()
            .ok_or_else(|| io::Error::other("source overview was not an array"))?;
        assert_eq!(listed.len(), 1);
        let source_id = listed[0]["source_id"]
            .as_str()
            .ok_or_else(|| io::Error::other("source overview omitted its stable ID"))?
            .to_owned();
        assert!(source_id.starts_with("s-"));

        let Err(workspace_error) = sources.search(&SourceSearchRequest {
            source_id: "workspace".to_owned(),
            query: "must stay out".to_owned(),
            cursor: None,
        }) else {
            return Err(io::Error::other("explicit-only scope exposed the workspace").into());
        };
        assert_eq!(workspace_error.code(), "source_not_in_snapshot");

        let read = sources
            .read(&SourceReadRequest {
                source_id,
                cursor: None,
            })
            .map_err(|failure| tool_failure_error(&failure))?;
        let evidence_ref = read.output()["data"]["evidence_ref"]
            .as_str()
            .ok_or_else(|| io::Error::other("source read omitted its evidence ref"))?
            .to_owned();
        assert!(sources.emitted_evidence_refs().contains(&evidence_ref));
        let repeated = BoundedSources::explicit_only([origin])?
            .overview(&PageRequest { cursor: None })
            .map_err(|failure| tool_failure_error(&failure))?;
        assert_eq!(
            repeated.output()["data"]["sources"][0]["source_id"],
            listed[0]["source_id"]
        );
        Ok(())
    }

    #[test]
    fn routing_candidates_are_frozen_summaries_with_paginated_inspection() -> TestResult {
        let candidate_id = TodoId::from_storage(1)?;
        let candidate = RoutingCandidate {
            id: candidate_id,
            title: "Frozen candidate".to_owned(),
            direction: "frozen direction ".repeat(READ_PAGE_CHARS / 8),
            direction_revision: 7,
            status: TodoStatus::Open,
            boundaries: vec![DirectionBoundary {
                id: 1,
                local_ref: "b1".to_owned(),
                kind: "required".to_owned(),
                statement: "Preserve frozen state.".to_owned(),
                attribution: "explicit_user".to_owned(),
                source_refs: vec!["concern:c1".to_owned()],
            }],
        };
        let mut backend = RoutingBackend {
            connection: rusqlite::Connection::open_in_memory()?,
            snapshot: RoutingSnapshot {
                concern: Concern {
                    id: ConcernId::from_storage(1)?,
                    body: "route this".to_owned(),
                    source_path: "/tmp/source".to_owned(),
                    source_thread_id: None,
                    source_turn_id: None,
                    source_item_id: None,
                    status: ConcernStatus::Pending,
                    created_at: "2026-08-28T00:00:00Z".to_owned(),
                    resolved_at: None,
                },
                base_digest: "frozen-digest".to_owned(),
                candidates: vec![candidate],
            },
            sources: BoundedSources::explicit_only(Vec::new())?,
            canonical_evidence_refs: std::collections::BTreeSet::new(),
            emitted_candidate_refs: std::collections::BTreeSet::new(),
            agent_job_id: 1,
            recorded: None,
            failure: None,
        };

        let list = backend
            .candidates(&PageRequest { cursor: None })
            .map_err(|failure| tool_failure_error(&failure))?;
        let summary = &list.output()["data"]["candidates"][0];
        assert_eq!(summary["id"], json!("t1"));
        assert_eq!(summary["direction_revision"], json!(7));
        assert_eq!(summary["boundary_count"], json!(1));
        assert!(summary.get("direction").is_none());
        assert!(
            summary["evidence_ref"]
                .as_str()
                .is_some_and(|reference| reference.ends_with("#summary"))
        );

        let first = backend
            .inspect_candidate(&CandidateReadRequest {
                candidate_id: candidate_id.to_string(),
                cursor: None,
            })
            .map_err(|failure| tool_failure_error(&failure))?;
        let first_data = &first.output()["data"];
        let cursor = first_data["next_cursor"]
            .as_str()
            .ok_or_else(|| io::Error::other("long frozen candidate was not paginated"))?;
        assert!(
            first_data["content"]
                .as_str()
                .is_some_and(|content| { content.chars().count() <= READ_PAGE_CHARS })
        );
        let first_ref = first_data["evidence_ref"]
            .as_str()
            .ok_or_else(|| io::Error::other("candidate page omitted its evidence ref"))?
            .to_owned();

        let second = backend
            .inspect_candidate(&CandidateReadRequest {
                candidate_id: candidate_id.to_string(),
                cursor: Some(cursor.to_owned()),
            })
            .map_err(|failure| tool_failure_error(&failure))?;
        let second_ref = second.output()["data"]["evidence_ref"]
            .as_str()
            .ok_or_else(|| io::Error::other("second candidate page omitted its evidence ref"))?;
        assert_ne!(first_ref, second_ref);
        assert!(backend.emitted_candidate_refs.contains(&first_ref));
        Ok(())
    }

    #[test]
    fn assessment_workspace_is_git_anchored_and_filters_unsafe_files() -> TestResult {
        let directory = tempfile::tempdir()?;
        let root = directory.path();
        fs::create_dir(root.join(".git"))?;
        let nested = root.join("src/nested");
        fs::create_dir_all(&nested)?;
        fs::write(root.join("visible.txt"), "visible evidence")?;
        fs::write(root.join(".env"), "PASSWORD=secret")?;
        fs::write(root.join("service-secrets.txt"), "secret")?;
        fs::write(root.join("state.sqlite"), "not really sqlite")?;
        fs::write(root.join("agent.log"), "log output")?;
        fs::write(root.join("binary.dat"), b"text\0binary")?;

        assert_eq!(nearest_git_workspace(&nested), Some(root.to_path_buf()));
        let sources = BoundedSources::with_workspace(root, Vec::new())?;
        let overview = sources
            .overview(&PageRequest { cursor: None })
            .map_err(|failure| tool_failure_error(&failure))?;
        let listed = overview.output()["data"]["sources"]
            .as_array()
            .ok_or_else(|| io::Error::other("source overview was not an array"))?;
        let locators = listed
            .iter()
            .filter_map(|source| source["locator"].as_str())
            .collect::<Vec<_>>();
        assert!(
            locators
                .iter()
                .any(|locator| locator.ends_with("visible.txt"))
        );
        for excluded in [
            ".env",
            "service-secrets.txt",
            "state.sqlite",
            "agent.log",
            "binary.dat",
        ] {
            assert!(
                locators.iter().all(|locator| !locator.ends_with(excluded)),
                "unsafe workspace file was cataloged: {excluded}"
            );
        }
        Ok(())
    }

    #[test]
    fn routing_checks_top_level_and_boundary_evidence_refs() -> TestResult {
        let allowed = std::collections::BTreeSet::from(["concern:c1".to_owned()]);
        let valid: ConcernRoutingProposal = serde_json::from_value(json!({
            "disposition": "create",
            "targets": [],
            "proposed_direction": {
                "title": "Retain the concern",
                "body": "Preserve the explicit boundary.",
                "boundaries": [{
                    "ref": "b1",
                    "kind": "required",
                    "text": "Keep the captured behavior.",
                    "attribution": "explicit_user",
                    "basis_refs": ["concern:c1"]
                }]
            },
            "unify": null,
            "rationale": "The concern is distinct.",
            "evidence_refs": ["concern:c1"],
            "limitations": []
        }))?;
        validate_routing_evidence_refs(&valid, &allowed)
            .map_err(|failure| tool_failure_error(&failure))?;

        let mut forged_top = valid.clone();
        forged_top.evidence_refs = vec!["source:never-emitted".to_owned()];
        assert_out_of_scope(validate_routing_evidence_refs(&forged_top, &allowed))?;
        let mut forged_boundary = valid;
        if let Some(direction) = &mut forged_boundary.proposed_direction {
            direction.boundaries[0].basis_refs = vec!["source:never-emitted".to_owned()];
        }
        assert_out_of_scope(validate_routing_evidence_refs(&forged_boundary, &allowed))
    }

    #[test]
    fn assessment_checks_every_evidence_bearing_section() -> TestResult {
        let allowed = std::collections::BTreeSet::from(["todo:t1".to_owned()]);
        let base = json!({
            "disposition": "needs_user_choice",
            "summary": "A choice remains.",
            "subject": {"label": "subject", "identity_refs": ["todo:t1"]},
            "findings": [{
                "ref": "f1", "kind": "gap", "claim": "A gap remains.",
                "evidence_refs": ["todo:t1"]
            }],
            "jurisdictions": [{
                "key": "j1", "concern": "Ownership",
                "assignments": [{"party": "user", "role": "owner", "responsibility": "Choose."}],
                "evidence_refs": ["todo:t1"]
            }],
            "direction_mappings": [],
            "unresolved": [{
                "ref": "u1", "kind": "user_choice", "description": "Choose.",
                "materiality": "Changes ownership.", "evidence_refs": ["todo:t1"]
            }]
        });
        let valid: SituationAssessment = serde_json::from_value(base.clone())?;
        validate_assessment_evidence_refs(&valid, &allowed)
            .map_err(|failure| tool_failure_error(&failure))?;

        for pointer in [
            "/subject/identity_refs/0",
            "/findings/0/evidence_refs/0",
            "/jurisdictions/0/evidence_refs/0",
            "/unresolved/0/evidence_refs/0",
        ] {
            let mut forged = base.clone();
            let value = forged
                .pointer_mut(pointer)
                .ok_or_else(|| io::Error::other("test evidence pointer disappeared"))?;
            *value = json!("source:never-emitted");
            let assessment: SituationAssessment = serde_json::from_value(forged)?;
            assert_out_of_scope(validate_assessment_evidence_refs(&assessment, &allowed))?;
        }
        Ok(())
    }

    fn assert_out_of_scope(result: Result<(), ToolFailure>) -> TestResult {
        let Err(error) = result else {
            return Err(io::Error::other("forged evidence reference was accepted").into());
        };
        assert_eq!(error.code(), "evidence_ref_not_in_scope");
        Ok(())
    }

    fn tool_failure_error(failure: &ToolFailure) -> io::Error {
        io::Error::other(format!("{}: {}", failure.code(), failure.message()))
    }

    #[test]
    fn design_backend_records_a_return_before_submission_and_stops_the_run() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut backend = design_backend_fixture(&database, None)?;

        let success = backend
            .call(
                "return-before-submit",
                Call::ReturnForAssessment(AssessmentReturn {
                    reason: "The ownership evidence is incomplete.".to_owned(),
                    missing_or_stale_refs: vec!["source:ownership".to_owned()],
                }),
            )
            .map_err(|failure| {
                io::Error::other(format!("{}: {}", failure.code(), failure.message()))
            })?;
        assert_eq!(
            success.output()["artifact"]["status"],
            serde_json::json!("returned")
        );
        let Some(returned) = backend.returned_for_assessment.as_ref() else {
            return Err(io::Error::other("backend lost the recorded return").into());
        };
        assert_eq!(returned.design_id, None);
        assert_eq!(
            store::get_assessment_return_for_job(&backend.connection, backend.agent_job_id)?,
            Some(returned.clone())
        );

        let Err(blocked) = backend.call(
            "late-submit",
            Call::SubmitDesignReconciliation(DesignSubmission {
                summary: "late".to_owned(),
                jurisdiction_changes: vec![],
                clauses: vec![],
                unresolved_choices: vec![],
            }),
        ) else {
            return Err(io::Error::other(
                "the backend accepted a design after its terminal return",
            )
            .into());
        };
        assert_eq!(blocked.code(), "assessment_already_returned");
        Ok(())
    }

    #[test]
    fn design_run_reports_a_return_instead_of_its_abandoned_draft() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut backend = design_backend_fixture(&database, Some("open"))?;

        backend
            .call(
                "return-after-submit",
                Call::ReturnForAssessment(AssessmentReturn {
                    reason: "The observed configuration changed.".to_owned(),
                    missing_or_stale_refs: vec!["source:configuration".to_owned()],
                }),
            )
            .map_err(|failure| {
                io::Error::other(format!("{}: {}", failure.code(), failure.message()))
            })?;
        assert_eq!(
            backend.draft.as_ref().map(|draft| draft.state.as_str()),
            Some("abandoned")
        );

        let result = finish_design_stage(
            backend.draft,
            backend.failure,
            backend.returned_for_assessment,
            Ok("liaison completed".to_owned()),
        );
        let Err(result) = result else {
            return Err(io::Error::other(
                "run result exposed the abandoned draft instead of its return",
            )
            .into());
        };
        assert_eq!(result.code(), "assessment_research_needed");
        assert!(result.to_string().contains("source:configuration"));
        Ok(())
    }

    #[test]
    fn design_run_abandons_an_unfinished_draft() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut backend = design_backend_fixture(&database, Some("open"))?;

        abandon_unfinished_design(&mut backend);

        let draft = backend
            .draft
            .take()
            .ok_or_else(|| io::Error::other("unfinished draft disappeared"))?;
        assert_eq!(draft.state, "abandoned");
        assert_eq!(
            draft.decision_reason.as_deref(),
            Some("the design liaison ended before producing a ready design")
        );
        let output = finish_design_stage(
            Some(draft),
            backend.failure,
            backend.returned_for_assessment,
            Ok("liaison completed".to_owned()),
        )?;
        assert_eq!(output.artifact.state, "abandoned");
        assert!(
            output
                .diagnostic
                .as_deref()
                .is_some_and(|message| message.contains("retained as abandoned"))
        );
        Ok(())
    }

    fn design_backend_fixture(
        database: &std::path::Path,
        design_state: Option<&str>,
    ) -> Result<DesignBackend, Box<dyn std::error::Error>> {
        let mut connection = db::init(database)?;
        connection.execute(
            "INSERT INTO concerns(body, source_path, status, resolved_at)
             VALUES('fixture concern','/tmp/fixture-source.md','attached',
                    '2026-08-28T00:00:00.000Z')",
            [],
        )?;
        let concern_id = connection.last_insert_rowid();
        connection.execute("INSERT INTO todos DEFAULT VALUES", [])?;
        let todo_id = TodoId::from_storage(connection.last_insert_rowid())?;
        connection.execute(
            "INSERT INTO todo_direction_revisions(
                 todo_id, revision, title, body, source_concern_id, provenance_kind
             ) VALUES(?1,1,'Fixture','Retain return provenance',?2,'legacy_v1')",
            params![todo_id.storage_id(), concern_id],
        )?;
        connection.execute(
            "INSERT INTO todo_concerns(todo_id, concern_id) VALUES(?1,?2)",
            params![todo_id.storage_id(), concern_id],
        )?;

        let assessment_snapshot = store::assessment_snapshot(&connection, todo_id)?;
        let assessment_job = store::record_agent_job(
            &mut connection,
            "situation_assessment",
            None,
            Some(todo_id),
            &assessment_snapshot.base_digest,
            "assessment-requester",
            "assessment-job",
        )?;
        connection.execute(
            "INSERT INTO todo_situation_assessments(
                 todo_id, agent_job_id, direction_revision_id, concern_set_digest,
                 disposition, summary, subject_label, observed_at, producer_tool_call_id
             ) VALUES(?1,?2,?3,?4,'ready','Fixture assessment','Fixture subject',
                      '2026-08-28T00:00:00.000Z','assessment-call')",
            params![
                todo_id.storage_id(),
                assessment_job.id,
                assessment_snapshot.direction.id,
                assessment_snapshot.concern_set_digest,
            ],
        )?;
        let assessment_id = SituationAssessmentId::from_storage(connection.last_insert_rowid())?;
        let assessment = store::get_assessment(&connection, assessment_id)?;
        let design_snapshot = store::assessment_snapshot(&connection, todo_id)?;
        let design_job = store::record_agent_job(
            &mut connection,
            "design_reconciliation",
            None,
            Some(todo_id),
            &design_snapshot.base_digest,
            "design-requester",
            "design-job",
        )?;
        let draft = if let Some(state) = design_state {
            connection.execute(
                "INSERT INTO todo_designs(
                     todo_id, revision, assessment_id, agent_job_id, state, summary,
                     producer_tool_call_id
                 ) VALUES(?1,1,?2,?3,?4,'Open fixture','design-call')",
                params![
                    todo_id.storage_id(),
                    assessment_id.storage_id(),
                    design_job.id,
                    state,
                ],
            )?;
            let id = crate::model::DesignId::from_storage(connection.last_insert_rowid())?;
            Some(store::get_design(&connection, id)?)
        } else {
            None
        };
        Ok(DesignBackend {
            connection,
            assessment,
            based_on_design_id: None,
            agent_job_id: design_job.id,
            draft,
            failure: None,
            returned_for_assessment: None,
        })
    }
}
