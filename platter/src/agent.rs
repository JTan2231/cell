//! Constrained Nucleus execution; accepted content is retained as run artifacts.
use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::{resume::Project, store::Store};
use anyhow::{Context, Result, bail, ensure};
use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{
    AbsolutePath, AgentInvocationV1, BuiltinToolsV1, HarnessCapability, HealthResponseV1,
    JobRequestV1, JobV1, LogSchemaV1, ReasoningEffort, Requester, TimeoutSeconds, ToolCallV1,
    ToolCallsQueryV1, ToolDefinitionV1, ToolResultV1, ToolsetDefinitionsV1, ToolsetRef,
    ToolsetRegistrationV1, WorkspaceAccess,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json, value::to_raw_value};
use uuid::Uuid;

pub const MODEL: &str = "gpt-5.6-sol";
const TOOL_NAMESPACE: &str = "platter";
const LEGACY_TOOL_NAMESPACE: &str = "job-packets";
pub(crate) const RESUME_EDITORIAL: &str = "<bazaar:platter.resume.editorial>";
const RESUME_WRITER: &str = "<bazaar:platter.legacy.resume.writer>";
const RESUME_REVIEWER: &str = "<bazaar:platter.legacy.resume.reviewer>";
#[cfg(test)]
const PROJECT_RESOURCES: &str = "<bazaar:platter.legacy.project.resources>";
const DRAFT: &str = "<bazaar:platter.legacy.draft.instructions>";
const WEAVER_DRAFT: &str = "<bazaar:platter.draft.instructions>";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CareerEntry {
    pub id: String,
    pub title: String,
    pub markdown: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageInputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_selection: Option<i64>,
    pub packet_id: String,
    pub posting: String,
    pub career_entries: Vec<CareerEntry>,
    pub guidance: String,
    pub original_jackson_bullets: Vec<String>,
    pub brief: Option<Brief>,
    // Omit absent additions to preserve historical input fingerprints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editorial_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_draft: Option<Resume>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editorial_review: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_resources: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_projects: Option<crate::projects::ProjectBullets>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Draft,
    Brief,
    Resume,
    ResumeDraft,
    ResumeReview,
    ResumeRevision,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Brief => "brief",
            Self::Resume => "resume",
            Self::ResumeDraft => "resume-draft",
            Self::ResumeReview => "resume-review",
            Self::ResumeRevision => "resume-revision",
        }
    }

    fn submit_tool(self) -> &'static str {
        match self {
            Self::Draft => "submit_draft",
            Self::Brief => "submit_brief",
            Self::Resume | Self::ResumeDraft | Self::ResumeRevision => "submit_resume",
            Self::ResumeReview => "submit_review",
        }
    }

    fn toolset_name(self) -> &'static str {
        match self {
            Self::ResumeDraft | Self::ResumeRevision => "resume-writer",
            _ => self.name(),
        }
    }

    pub(crate) fn resume_artifacts(self) -> (&'static str, &'static str, &'static str) {
        if self == Self::ResumeDraft {
            ("resume-draft", "resume-draft-source", "resume-draft-pdf")
        } else {
            ("resume-content", "resume-source", "resume-pdf")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brief {
    /// Frozen display text: labeled sections for new briefs, a paragraph for legacy briefs.
    pub paragraph: String,
    pub pursue: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BriefSubmission {
    why_it_works: String,
    role: String,
    #[serde(default)]
    culture: Option<String>,
    pursue: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftSubmission {
    brief: BriefSubmission,
    resume: Option<Resume>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub brief: Brief,
    pub resume: Option<Resume>,
}

impl BriefSubmission {
    fn into_brief(self) -> Result<Brief> {
        if !self.pursue {
            return Ok(Brief {
                paragraph: String::new(),
                pursue: false,
            });
        }
        let why = self.why_it_works.trim();
        let role = self.role.trim();
        let culture = self
            .culture
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        validate_brief_sections(why, role, culture)?;
        let mut paragraph = format!("Why it works: {why}\n\nRole: {role}");
        if let Some(culture) = culture {
            paragraph.push_str("\n\nCulture: ");
            paragraph.push_str(culture);
        }
        Ok(Brief {
            paragraph,
            pursue: true,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BulletEvidence {
    /// Zero-based index into `jackson_bullets`. These references stay private.
    pub bullet_index: usize,
    pub career_entry_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resume {
    pub jackson_bullets: Vec<String>,
    pub evidence: Vec<BulletEvidence>,
    /// None retains the historical Jackson-only payload and rendering boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projects: Option<Vec<Project>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_bullets: Option<crate::projects::ProjectBullets>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", content = "result", rename_all = "snake_case")]
pub enum StageResult {
    Draft(Draft),
    Brief(Brief),
    Resume(Resume),
    Review(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StageState {
    version: u32,
    stage: Stage,
    request: JobRequestV1,
    input_sha256: String,
    runtime: Option<RuntimeState>,
    #[serde(skip)]
    inputs: StageInputs,
    #[serde(skip)]
    accepted: Option<StageResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeState {
    #[serde(default)]
    quota_exhausted: bool,
    state: nucleus_core::JobState,
    attempt_id: Option<nucleus_core::AttemptId>,
}

pub async fn run_stage(
    client: &NucleusClient,
    store: &Store,
    stage: Stage,
    inputs: StageInputs,
    deadline: Option<Instant>,
) -> Result<StageResult> {
    run_stage_impl(client, store, stage, inputs, deadline, None).await
}

pub async fn run_stage_with_validator(
    client: &NucleusClient,
    store: &Store,
    stage: Stage,
    inputs: StageInputs,
    deadline: Option<Instant>,
    validator: &dyn Fn(&StageResult) -> Result<()>,
) -> Result<StageResult> {
    run_stage_impl(client, store, stage, inputs, deadline, Some(validator)).await
}

type SubmissionValidator<'a> = Option<&'a dyn Fn(&StageResult) -> Result<()>>;

async fn run_stage_impl(
    client: &NucleusClient,
    store: &Store,
    kind: Stage,
    inputs: StageInputs,
    deadline: Option<Instant>,
    validator: SubmissionValidator<'_>,
) -> Result<StageResult> {
    let mut state = load_or_create(store, kind, inputs)?;
    let operation = run_stage_inner(client, store, &mut state, validator);
    let result = if let Some(deadline) = deadline {
        if let Ok(result) = tokio::time::timeout(
            deadline.saturating_duration_since(Instant::now()),
            operation,
        )
        .await
        {
            result
        } else {
            let _ =
                tokio::time::timeout(Duration::from_secs(2), client.cancel_job(&state.request.id))
                    .await;
            bail!("work deadline reached; exact Nucleus job cancellation requested; run retained")
        }
    } else {
        operation.await
    };
    if result
        .as_ref()
        .is_err_and(anyhow::Error::is::<crate::resume::RendererFailure>)
    {
        tokio::time::timeout(
            Duration::from_secs(10),
            client.cancel_job(&state.request.id),
        )
        .await
        .context("renderer failed; exact Nucleus job cancellation timed out")
        .context(crate::resume::RendererFailure)?
        .context(crate::resume::RendererFailure)?;
    }
    result
}

pub fn retained_request(store: &Store, run: &str, selected: Stage) -> Result<Option<JobRequestV1>> {
    let Some(value) = store.execution(run, selected.name())? else {
        return Ok(None);
    };
    let state: StageState = serde_json::from_value(value)?;
    ensure!(state.version == 2, "unsupported execution state version");
    retained_tool_namespace(&state)?;
    state.request.validate()?;
    Ok(Some(state.request))
}

pub fn retained_guidance(store: &Store, run: &str) -> Result<Option<String>> {
    retained_request(store, run, Stage::Draft)?
        .or(retained_request(store, run, Stage::Brief)?)
        .map(|request| {
            let prompt: Value = serde_json::from_str(&request.prompt)?;
            Ok(prompt
                .get("preferences_and_disclosure_guidance")
                .and_then(Value::as_str)
                .context("retained guidance missing")?
                .to_owned())
        })
        .transpose()
}

fn load_or_create(store: &Store, kind: Stage, inputs: StageInputs) -> Result<StageState> {
    validate_inputs(&inputs)?;
    validate_handoff(store, kind, &inputs)?;
    let input_sha256 = crate::store::digest(&serde_json::to_vec(&inputs)?);
    let mut state = if let Some(value) = store.execution(&inputs.packet_id, kind.name())? {
        let state: StageState = serde_json::from_value(value)?;
        ensure!(
            state.version == 2 && state.stage == kind && state.input_sha256 == input_sha256,
            "stage input conflict: retained job inputs are immutable"
        );
        retained_tool_namespace(&state)?;
        state
    } else {
        StageState {
            version: 2,
            stage: kind,
            request: if kind == Stage::ResumeRevision {
                revision_request(store, &inputs)?
            } else {
                build_request(kind, &inputs, store.root())?
            },
            input_sha256,
            runtime: None,
            inputs: inputs.clone(),
            accepted: None,
        }
    };
    state.inputs = inputs;
    state.accepted = match kind {
        Stage::Draft => retained_draft(store, &state.inputs.packet_id)?.map(StageResult::Draft),
        Stage::Brief => store
            .content(&state.inputs.packet_id, "brief")?
            .map(StageResult::Brief),
        Stage::Resume | Stage::ResumeDraft | Stage::ResumeRevision => store
            .content(&state.inputs.packet_id, kind.resume_artifacts().0)?
            .map(StageResult::Resume),
        Stage::ResumeReview => {
            retained_review(store, &state.inputs.packet_id)?.map(StageResult::Review)
        }
    };
    persist(store, &state)?;
    Ok(state)
}

fn retained_draft(store: &Store, run: &str) -> Result<Option<Draft>> {
    let Some(brief) = store.content::<Brief>(run, "brief")? else {
        return Ok(None);
    };
    let resume = if brief.pursue {
        ensure!(
            store.run_artifact(run, "resume-source")?.is_some()
                && store.run_artifact(run, "resume-pdf")?.is_some(),
            "accepted draft is missing rendered artifacts"
        );
        Some(
            store
                .content(run, "resume-content")?
                .context("accepted draft is missing resume content")?,
        )
    } else {
        None
    };
    Ok(Some(Draft { brief, resume }))
}

fn retained_review(store: &Store, run: &str) -> Result<Option<String>> {
    store
        .run_artifact(run, "resume-review")?
        .map(|artifact| String::from_utf8(artifact.content).context("review is not UTF-8"))
        .transpose()
}

fn writer_basis(store: &Store, inputs: &StageInputs) -> Result<JobRequestV1> {
    let value = store
        .execution(&inputs.packet_id, Stage::ResumeDraft.name())?
        .context("draft writer request is missing")?;
    let state: StageState = serde_json::from_value(value)?;
    let mut basis = inputs.clone();
    basis.proposed_draft = None;
    basis.editorial_review = None;
    ensure!(
        state.version == 2
            && state.stage == Stage::ResumeDraft
            && state.input_sha256 == crate::store::digest(&serde_json::to_vec(&basis)?),
        "writing setup differs from the retained draft request"
    );
    retained_tool_namespace(&state)?;
    state.request.validate()?;
    Ok(state.request)
}

fn validate_handoff(store: &Store, stage: Stage, inputs: &StageInputs) -> Result<()> {
    match stage {
        Stage::Draft => ensure!(
            inputs.editorial_policy.is_some()
                && (inputs.project_resources.is_some() || inputs.fixed_projects.is_some())
                && inputs.brief.is_none()
                && inputs.proposed_draft.is_none()
                && inputs.editorial_review.is_none(),
            "draft requires editorial policy and project resources without prior stage content"
        ),
        Stage::ResumeDraft => ensure!(
            inputs.editorial_policy.is_some()
                && inputs.proposed_draft.is_none()
                && inputs.editorial_review.is_none(),
            "draft requires a captured editorial policy and no handoff"
        ),
        Stage::ResumeReview | Stage::ResumeRevision => {
            writer_basis(store, inputs)?;
            let draft: Resume = store
                .content(&inputs.packet_id, "resume-draft")?
                .context("accepted draft is missing")?;
            ensure!(
                inputs.proposed_draft.as_ref() == Some(&draft),
                "draft handoff conflict"
            );
            if stage == Stage::ResumeRevision {
                let review =
                    retained_review(store, &inputs.packet_id)?.context("review is missing")?;
                ensure!(
                    inputs.editorial_review.as_ref() == Some(&review),
                    "review handoff conflict"
                );
            } else {
                ensure!(
                    inputs.editorial_review.is_none(),
                    "reviewer must not receive a prior review"
                );
            }
        }
        Stage::Brief | Stage::Resume => {}
    }
    Ok(())
}

fn revision_request(store: &Store, inputs: &StageInputs) -> Result<JobRequestV1> {
    let mut request = writer_basis(store, inputs)?;
    // Clone every writer setting. Only execution identity and these two inputs change.
    request.id = format!("platter-resume-revision-{}", Uuid::now_v7()).into();
    let mut prompt: Value = serde_json::from_str(&request.prompt)?;
    prompt["proposed_draft"] = serde_json::to_value(&inputs.proposed_draft)?;
    prompt["editorial_review"] = serde_json::to_value(&inputs.editorial_review)?;
    request.prompt = serde_json::to_string(&prompt)?;
    request.validate()?;
    Ok(request)
}

/// Import only correlation and execution progress from predecessor stage files.
/// Accepted content is imported separately; tool history remains Nucleus-owned.
pub(crate) fn import_execution(value: &Value) -> Result<Value> {
    let inputs: StageInputs =
        serde_json::from_value(value.get("inputs").context("stage inputs missing")?.clone())?;
    let job: Option<JobV1> =
        serde_json::from_value(value.get("runtime").cloned().unwrap_or(Value::Null))?;
    let state = StageState {
        version: 2,
        stage: serde_json::from_value(value["stage"].clone())?,
        request: serde_json::from_value(value["request"].clone())?,
        input_sha256: crate::store::digest(&serde_json::to_vec(&inputs)?),
        runtime: job.map(|job| RuntimeState {
            quota_exhausted: job.quota_exhausted(),
            state: job.summary.state,
            attempt_id: job.summary.current_attempt_id,
        }),
        inputs,
        accepted: None,
    };
    retained_tool_namespace(&state)?;
    state.request.validate()?;
    Ok(serde_json::to_value(state)?)
}

fn validate_inputs(inputs: &StageInputs) -> Result<()> {
    ensure!(!inputs.packet_id.trim().is_empty(), "packet id is required");
    ensure!(
        !inputs.posting.trim().is_empty(),
        "full posting is required"
    );
    ensure!(!inputs.career_entries.is_empty(), "career library is empty");
    let mut ids = BTreeSet::new();
    for entry in &inputs.career_entries {
        ensure!(
            !entry.id.is_empty()
                && !entry.title.trim().is_empty()
                && !entry.markdown.trim().is_empty(),
            "career entries must include identity, title, and complete content"
        );
        ensure!(ids.insert(&entry.id), "duplicate career entry id");
    }
    Ok(())
}

fn build_request(stage: Stage, inputs: &StageInputs, cwd: &Path) -> Result<JobRequestV1> {
    let prompts = cell_prompts::Prompts::at("platter", inputs.prompt_selection.unwrap_or(1))?;
    let mut request = build_legacy_request(stage, inputs, cwd)?;
    if inputs.fixed_projects.is_some() {
        ensure!(
            stage == Stage::Draft && inputs.project_resources.is_none(),
            "fixed projects require the Weaver draft workflow"
        );
        request.invocation.toolset = Some(ToolsetRef {
            provider: TOOL_NAMESPACE.into(),
            name: "draft".into(),
            version: 2,
        });
        request.instructions = format!(
            "{}\n\n{}",
            prompts.expand(WEAVER_DRAFT)?,
            inputs
                .editorial_policy
                .as_deref()
                .context("editorial policy missing")?
        );
        let mut prompt: Value = serde_json::from_str(&request.prompt)?;
        prompt["fixed_projects"] = serde_json::to_value(&inputs.fixed_projects)?;
        request.prompt = prompt.to_string();
        if inputs.prompt_selection.is_some() {
            request.invocation.toolset = Some(
                prompts.toolset(
                    request
                        .invocation
                        .toolset
                        .take()
                        .context("draft toolset missing")?,
                    3,
                )?,
            );
        }
        return Ok(request);
    }
    let Some(resources) = &inputs.project_resources else {
        return Ok(request);
    };
    ensure!(
        stage != Stage::Resume,
        "legacy resume stage cannot author projects"
    );
    request.invocation.cwd = AbsolutePath::new("/Users/joey");
    request.invocation.workspace_access = WorkspaceAccess::ReadOnly;
    request.invocation.builtin_tools.local_execution = true;
    let mut toolset = toolset_ref(stage, TOOL_NAMESPACE);
    if stage != Stage::Draft {
        toolset.version += 1;
    }
    request.invocation.toolset = Some(toolset);
    let task = match stage {
        Stage::Draft => DRAFT,
        Stage::Brief => "<bazaar:platter.legacy.project.brief.instructions>",
        Stage::ResumeReview => "<bazaar:platter.legacy.project.review.instructions>",
        _ => "<bazaar:platter.legacy.project.resume.instructions>",
    };
    request.instructions = prompts.render(
        "platter.legacy.project.instructions-template",
        &[
            ("task", prompts.expand(task)?),
            ("resources", resources.clone()),
        ],
    )?;
    if stage != Stage::Brief {
        request.instructions.push_str("\n\n# Editorial policy\n\n");
        request
            .instructions
            .push_str(&match &inputs.editorial_policy {
                Some(policy) => policy.clone(),
                None => prompts.expand(RESUME_EDITORIAL)?,
            });
    }
    request.validate()?;
    Ok(request)
}

fn build_legacy_request(stage: Stage, inputs: &StageInputs, cwd: &Path) -> Result<JobRequestV1> {
    let prompts = cell_prompts::Prompts::at("platter", inputs.prompt_selection.unwrap_or(1))?;
    let mut invocation = AgentInvocationV1::new(
        "codex",
        MODEL,
        AbsolutePath::new(cwd),
        WorkspaceAccess::None,
        BuiltinToolsV1 {
            local_execution: false,
            web_search: false,
        },
        TimeoutSeconds::new(3600),
    );
    invocation.reasoning_effort = Some(ReasoningEffort::Max);
    invocation.toolset = Some(toolset_ref(stage, TOOL_NAMESPACE));
    let index: Vec<Value> = inputs
        .career_entries
        .iter()
        .map(|entry| json!({"id":entry.id,"title":entry.title}))
        .collect();
    let task = match stage {
        Stage::Draft => DRAFT,
        Stage::Brief => "<bazaar:platter.legacy.brief.instructions>",
        Stage::Resume | Stage::ResumeDraft | Stage::ResumeRevision => {
            "<bazaar:platter.legacy.resume.instructions>"
        }
        Stage::ResumeReview => RESUME_REVIEWER,
    };
    let mut instructions = prompts.render(
        "platter.legacy.packet.instructions-template",
        &[("task", prompts.expand(task)?)],
    )?;
    if stage != Stage::Brief {
        instructions.push_str(&prompts.text("platter.legacy.resume.evidence")?);
        instructions.push_str(&match &inputs.editorial_policy {
            Some(policy) => policy.clone(),
            None => prompts.expand(RESUME_EDITORIAL)?,
        });
    }
    if matches!(
        stage,
        Stage::Resume | Stage::ResumeDraft | Stage::ResumeRevision
    ) {
        instructions.push_str(&prompts.expand(RESUME_WRITER)?);
        instructions.push_str(&prompts.text("platter.legacy.resume.artifact-contract")?);
    }
    let mut prompt = json!({
        "packet_id":inputs.packet_id,
        "complete_posting": inputs.posting,
        "career_entry_index": index,
        "preferences_and_disclosure_guidance":inputs.guidance,
        "original_jackson_bullets":inputs.original_jackson_bullets,
        "accepted_brief_positioning_only":inputs.brief,
    });
    if stage == Stage::Draft {
        prompt
            .as_object_mut()
            .context("invalid draft prompt")?
            .remove("accepted_brief_positioning_only");
    }
    if stage == Stage::ResumeReview {
        prompt
            .as_object_mut()
            .context("invalid review prompt")?
            .remove("accepted_brief_positioning_only");
        prompt["proposed_draft"] = serde_json::to_value(&inputs.proposed_draft)?;
    }
    let prompt = serde_json::to_string(&prompt)?;
    let request = JobRequestV1::new(
        format!("platter-{}-{}", stage.name(), Uuid::now_v7()),
        format!("Platter {}: {}", stage.toolset_name(), inputs.packet_id),
        Requester {
            program: "platter".to_owned(),
            id: inputs.packet_id.clone(),
        },
        instructions,
        prompt,
        invocation,
    );
    request.validate()?;
    Ok(request)
}

/// Read-only proof of required runtime semantics. Exact model support remains
/// admission-owned; an unsupported model/effort error is returned unchanged.
///
/// # Errors
/// Returns an error when the service is unavailable or required proof is missing.
pub async fn check_readiness(client: &NucleusClient) -> Result<HealthResponseV1> {
    let health = client.health().await?;
    validate_health(&health, true)?;
    Ok(health)
}

/// Prove installation readiness while the exact deployment owner holds Nucleus.
/// This never authorizes ordinary stage admission while the service is held.
///
/// # Errors
/// Returns errors unless the owner's sole hold, full drain, and required runtime
/// capabilities are proven by read-only Nucleus observations.
pub async fn check_deployment_readiness(
    client: &NucleusClient,
    owner: &str,
) -> Result<HealthResponseV1> {
    let health = client.health_for_deployment(owner).await?;
    validate_health(&health, false)?;
    Ok(health)
}

fn validate_health(health: &HealthResponseV1, require_admission: bool) -> Result<()> {
    ensure!(
        health.version == 1
            && (!require_admission || (health.status == "ok" && health.accepting_jobs))
            && health.supported_protocol_versions.contains(&1)
            && health.authentication.configured
            && health.authentication.authenticated
            && health
                .harness
                .as_ref()
                .is_some_and(|h| h.harness.as_str() == "codex"),
        "Nucleus strict readiness is not satisfied"
    );
    let capacity = health
        .execution
        .context("Nucleus execution-capacity proof is missing")?;
    ensure!(
        capacity.max_active_jobs == 8
            && capacity.active_jobs <= 8
            && capacity.available_slots == 8 - capacity.active_jobs,
        "Nucleus capacity contract does not match eight shared slots"
    );
    for capability in [
        HarnessCapability::ExactModel,
        HarnessCapability::ReasoningEffort,
        HarnessCapability::WorkspaceNone,
        HarnessCapability::BuiltinLocalExecution,
        HarnessCapability::BuiltinWebSearch,
        HarnessCapability::DynamicClientTools,
    ] {
        ensure!(
            health.capabilities.contains(&capability),
            "Nucleus capability missing: {capability:?}"
        );
    }
    Ok(())
}

async fn run_stage_inner(
    client: &NucleusClient,
    store: &Store,
    state: &mut StageState,
    validator: SubmissionValidator<'_>,
) -> Result<StageResult> {
    if state
        .runtime
        .as_ref()
        .is_some_and(|job| job.state.is_terminal())
    {
        return terminal_result(state);
    }
    // Reads and pending-call recovery remain possible while new admission is held.
    match client.get_job(&state.request.id).await {
        Ok(job) => ensure!(
            job.request == state.request,
            "Nucleus correlation conflicts with retained exact request"
        ),
        Err(ClientError::Api { status: 404, .. }) => {
            let health = client.health_for_work().await?;
            validate_health(&health, true)?;
            ensure!(
                !project_tools(state)
                    || health
                        .capabilities
                        .contains(&HarnessCapability::WorkspaceReadOnly),
                "Nucleus read-only workspace capability is missing"
            );
            register_tools(client, state).await?;
            // Exact request was committed before any potentially ambiguous submit.
            client.submit_job(&state.request).await?;
        }
        Err(error) => return Err(error.into()),
    }
    loop {
        let job = client.get_job_for_work(&state.request.id).await?;
        ensure!(job.request == state.request, "Nucleus request changed");
        let quota_exhausted = job.quota_exhausted();
        let terminal = job.summary.state.is_terminal();
        state.runtime = Some(RuntimeState {
            quota_exhausted,
            state: job.summary.state,
            attempt_id: job.summary.current_attempt_id,
        });
        persist(store, state)?;
        if terminal {
            if state.accepted.is_none() && quota_exhausted {
                return Err(nucleus_core::QuotaExhausted.into());
            }
            return terminal_result(state);
        }
        let pending = client
            .pending_tool_calls(
                &state.request.id,
                &ToolCallsQueryV1 {
                    after: 0,
                    wait_seconds: 10,
                },
            )
            .await?;
        ensure!(
            pending.version == 1 && pending.job_id == state.request.id,
            "mailbox correlation mismatch"
        );
        for pending_call in pending.calls {
            let call = pending_call.call;
            let response = bind_tool_result_validated(store, state, &call, validator)?;
            client
                .post_tool_result(&state.request.id, &call.id, &response)
                .await?;
        }
    }
}

fn terminal_result(state: &StageState) -> Result<StageResult> {
    if state.accepted.is_none()
        && state
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.quota_exhausted)
    {
        return Err(nucleus_core::QuotaExhausted.into());
    }
    state.accepted.clone().context("Nucleus job ended without an accepted domain result; inspect retained runtime state before authorizing a new attempt")
}

fn toolset_ref(stage: Stage, namespace: &str) -> ToolsetRef {
    ToolsetRef {
        provider: namespace.into(),
        name: stage.toolset_name().into(),
        version: if stage == Stage::Brief && namespace == TOOL_NAMESPACE {
            2
        } else {
            1
        },
    }
}

// Recovery uses the exact namespace and version frozen in each request.
fn retained_tool_namespace(state: &StageState) -> Result<&'static str> {
    let namespace = match state.request.requester.program.as_str() {
        TOOL_NAMESPACE => TOOL_NAMESPACE,
        LEGACY_TOOL_NAMESPACE => LEGACY_TOOL_NAMESPACE,
        _ => bail!("stage request has an unsupported requester identity"),
    };
    let toolset = state
        .request
        .invocation
        .toolset
        .as_ref()
        .context("stage toolset is missing")?;
    ensure!(
        toolset.provider.as_str() == namespace
            && toolset.name.as_str() == state.stage.toolset_name()
            && (namespace == TOOL_NAMESPACE || matches!(state.stage, Stage::Brief | Stage::Resume))
            && (toolset.version == 1
                || (namespace == TOOL_NAMESPACE
                    && state.stage == Stage::Brief
                    && toolset.version == 2)
                || fixed_project_tools(state)
                || project_tools(state)),
        "stage request has a conflicting retained toolset identity"
    );
    Ok(namespace)
}

fn sectioned_brief(state: &StageState) -> bool {
    state.stage == Stage::Brief
        && state
            .request
            .invocation
            .toolset
            .as_ref()
            .is_some_and(|toolset| toolset.version >= 2)
}

fn fixed_project_tools(state: &StageState) -> bool {
    state.stage == Stage::Draft
        && state.request.invocation.toolset.as_ref().is_some_and(|t| {
            t.provider.as_str() == TOOL_NAMESPACE
                && t.name.as_str() == "draft"
                // Inputs are reconstructed after recovery validation. The frozen
                // toolset identifies fixed-project drafts: legacy 2 or selection + 3.
                && (t.version == 2 || t.version >= 4)
        })
}

fn project_tools(state: &StageState) -> bool {
    state
        .request
        .invocation
        .toolset
        .as_ref()
        .is_some_and(|toolset| {
            toolset.provider.as_str() == TOOL_NAMESPACE
                && match state.stage {
                    Stage::Draft => toolset.version == 1,
                    Stage::Brief => toolset.version == 3,
                    Stage::ResumeDraft | Stage::ResumeReview | Stage::ResumeRevision => {
                        toolset.version == 2
                    }
                    Stage::Resume => false,
                }
        })
}

fn call_schema_id(state: &StageState, name: &str) -> Result<String> {
    let namespace = retained_tool_namespace(state)?;
    if fixed_project_tools(state) && name == "submit_draft" {
        return Ok("platter.submit-draft.arguments.v2".into());
    }
    if project_tools(state) && name == "submit_resume" {
        return Ok(format!("{namespace}.submit-resume.arguments.v2"));
    }
    Ok(argument_schema_id(namespace, name, sectioned_brief(state)))
}

fn argument_schema_id(namespace: &str, name: &str, sectioned: bool) -> String {
    let version = if sectioned && name == "submit_brief" {
        2
    } else {
        1
    };
    format!(
        "{namespace}.{}.arguments.v{version}",
        name.replace('_', "-")
    )
}

fn result_schema_id(namespace: &str) -> String {
    format!("{namespace}.tool-result.v1")
}

fn tool_definitions(
    stage: Stage,
    namespace: &str,
    sectioned: bool,
) -> Result<ToolsetDefinitionsV1> {
    let mut definitions = vec![
        (
            "list_career_entries",
            "<bazaar:platter.tools.list_career_entries.description>",
            json!({"type":"object","additionalProperties":false,"properties":{}}),
        ),
        (
            "read_career_entry",
            "<bazaar:platter.tools.read_career_entry.description>",
            json!({"type":"object","additionalProperties":false,"required":["id"],"properties":{"id":{"type":"string","minLength":1}}}),
        ),
    ];
    match stage {
        Stage::Draft => {
            let brief = tool_definitions(Stage::Brief, TOOL_NAMESPACE, true)?;
            let resume = project_tool_definitions(Stage::ResumeDraft)?;
            let schema = |definitions: &ToolsetDefinitionsV1, name: &str| -> Result<Value> {
                Ok(serde_json::from_str(definitions.tools.iter().find(|tool| tool.name == name).context("submission schema missing")?.input_schema.get())?)
            };
            let mut resume_schema = schema(&resume, "submit_resume")?;
            resume_schema["type"] = json!(["object", "null"]);
            definitions.push(("submit_draft", "<bazaar:platter.legacy.tools.submit_draft.description>", json!({
                "type":"object", "additionalProperties":false, "required":["brief","resume"],
                "properties":{"brief":schema(&brief,"submit_brief")?,"resume":resume_schema}
            })));
        }
        Stage::Brief if sectioned => definitions.push(("submit_brief", "<bazaar:platter.legacy.tools.submit_brief.sectioned.description>", json!({
            "type":"object","additionalProperties":false,"required":["why_it_works","role","pursue"],
            "properties":{
                "why_it_works":{"type":"string"},
                "role":{"type":"string"},
                "culture":{"type":["string","null"]},
                "pursue":{"type":"boolean"}
            }
        }))),
        Stage::Brief => definitions.push(("submit_brief", "<bazaar:platter.legacy.tools.submit_brief.paragraph.description>", json!({
            "type":"object","additionalProperties":false,"required":["paragraph","pursue"],
            "properties":{"paragraph":{"type":"string","minLength":1},"pursue":{"type":"boolean"}}
        }))),
        Stage::Resume | Stage::ResumeDraft | Stage::ResumeRevision => definitions.push(("submit_resume", "<bazaar:platter.legacy.tools.submit_resume.jackson.description>", json!({
            "type":"object","additionalProperties":false,"required":["jackson_bullets","evidence"],
            "properties":{
                "jackson_bullets":{"type":"array","minItems":1,"maxItems":12,"items":{"type":"string","minLength":1}},
                "evidence":{"type":"array","minItems":1,"items":{"type":"object","additionalProperties":false,"required":["bullet_index","career_entry_ids"],"properties":{
                    "bullet_index":{"type":"integer","minimum":0},"career_entry_ids":{"type":"array","minItems":1,"items":{"type":"string","minLength":1}}
                }}}
            }
        }))),
        Stage::ResumeReview => definitions.push(("submit_review", "<bazaar:platter.legacy.tools.submit_review.description>", json!({
            "type":"object","additionalProperties":false,"required":["markdown"],
            "properties":{"markdown":{"type":"string"}}
        }))),
    }
    Ok(ToolsetDefinitionsV1 {
        version: 1,
        tools: definitions
            .into_iter()
            .map(|(name, description, schema)| {
                Ok(ToolDefinitionV1 {
                    name: name.into(),
                    description: description.into(),
                    input_schema_id: argument_schema_id(namespace, name, sectioned).into(),
                    input_schema: to_raw_value(&schema)?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    })
}

fn project_tool_definitions(stage: Stage) -> Result<ToolsetDefinitionsV1> {
    let mut definitions = tool_definitions(stage, TOOL_NAMESPACE, stage == Stage::Brief)?;
    if let Some(tool) = definitions
        .tools
        .iter_mut()
        .find(|tool| tool.name == "submit_resume")
    {
        tool.description =
            "<bazaar:platter.legacy.tools.submit_resume.projects.description>".into();
        tool.input_schema_id = "platter.submit-resume.arguments.v2".into();
        let mut schema: Value = serde_json::from_str(tool.input_schema.get())?;
        schema["required"] = json!(["jackson_bullets", "evidence", "projects"]);
        schema["properties"]["projects"] = json!({
            "type":"array","minItems":1,"maxItems":2,
            "items":{"type":"object","additionalProperties":false,
                "required":["name","description","dates","bullets","sources"],
                "properties":{
                    "name":{"type":"string","enum":["Cell","Wrought"]},
                    "description":{"type":"string","minLength":1},
                    "dates":{"type":["string","null"]},
                    "bullets":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"string","minLength":1}},
                    "sources":{"type":"array","minItems":1,"items":{"type":"string","minLength":1}}
                }
            }
        });
        tool.input_schema = to_raw_value(&schema)?;
    }
    Ok(definitions)
}

fn fixed_draft_definitions() -> Result<ToolsetDefinitionsV1> {
    let mut definitions = tool_definitions(Stage::Draft, TOOL_NAMESPACE, false)?;
    let jackson = tool_definitions(Stage::Resume, TOOL_NAMESPACE, false)?;
    let mut schema: Value = serde_json::from_str(
        jackson
            .tools
            .iter()
            .find(|t| t.name == "submit_resume")
            .context("Jackson schema missing")?
            .input_schema
            .get(),
    )?;
    schema["type"] = json!(["object", "null"]);
    let tool = definitions
        .tools
        .iter_mut()
        .find(|t| t.name == "submit_draft")
        .context("draft tool missing")?;
    let mut combined: Value = serde_json::from_str(tool.input_schema.get())?;
    combined["properties"]["resume"] = schema;
    tool.input_schema = to_raw_value(&combined)?;
    tool.input_schema_id = "platter.submit-draft.arguments.v2".into();
    tool.description = "<bazaar:platter.tools.submit_draft.description>".into();
    Ok(definitions)
}

async fn register_tools(client: &NucleusClient, state: &StageState) -> Result<()> {
    let namespace = retained_tool_namespace(state)?;
    let sectioned = sectioned_brief(state);
    let reference = state
        .request
        .invocation
        .toolset
        .as_ref()
        .context("stage toolset is missing")?;
    let prompts = cell_prompts::Prompts::for_toolset("platter", reference, 3)?;
    let mut definitions = if fixed_project_tools(state) {
        fixed_draft_definitions()?
    } else if project_tools(state) {
        project_tool_definitions(state.stage)?
    } else {
        tool_definitions(state.stage, namespace, sectioned)?
    };
    for tool in &mut definitions.tools {
        tool.description = prompts.expand(&tool.description)?;
    }
    for tool in &definitions.tools {
        client
            .register_schema(&LogSchemaV1::new(
                tool.input_schema_id.clone(),
                &tool.name,
                if (sectioned && tool.name == "submit_brief")
                    || (project_tools(state) && tool.name == "submit_resume")
                    || (fixed_project_tools(state) && tool.name == "submit_draft")
                {
                    "2"
                } else {
                    "1"
                },
                "application/schema+json",
                namespace,
                tool.input_schema.clone(),
            ))
            .await?;
    }
    client
        .register_schema(&LogSchemaV1::new(
            result_schema_id(namespace),
            if namespace == LEGACY_TOOL_NAMESPACE {
                "Job packet tool result"
            } else {
                "Platter tool result"
            },
            "1",
            "application/schema+json",
            namespace,
            to_raw_value(&json!({"type":"object"}))?,
        ))
        .await?;
    let registration = ToolsetRegistrationV1::new(
        state
            .request
            .invocation
            .toolset
            .clone()
            .context("stage toolset is missing")?,
        "nucleus.toolset-definitions.v1",
        definitions,
    )?;
    let registered = client.register_toolset(&registration).await?;
    ensure!(
        registered.toolset == registration.toolset
            && registered.digest == registration.digest
            && registered.definitions_schema_id == registration.definitions_schema_id,
        "Nucleus immutable toolset registration mismatch"
    );
    Ok(())
}

fn bind_tool_result_validated(
    store: &Store,
    state: &mut StageState,
    call: &ToolCallV1,
    validator: SubmissionValidator<'_>,
) -> Result<ToolResultV1> {
    ensure!(
        call.version == 1 && call.job_id == state.request.id,
        "tool call belongs to another job/protocol"
    );
    if let Some(attempt_id) = state
        .runtime
        .as_ref()
        .and_then(|job| job.attempt_id.as_ref())
    {
        ensure!(
            &call.attempt_id == attempt_id,
            "tool call belongs to another attempt"
        );
    }
    let namespace = retained_tool_namespace(state)?;
    let expected = call_schema_id(state, &call.tool_name)?;
    let allowed = [
        "list_career_entries",
        "read_career_entry",
        state.stage.submit_tool(),
    ];
    ensure!(
        allowed.contains(&call.tool_name.as_str()) && call.arguments_schema_id.as_str() == expected,
        "unregistered tool or argument schema"
    );
    let (value, is_error) = match execute_tool(state, call, validator) {
        Ok(value) => (value, false),
        Err(error) if error.is::<crate::resume::RendererFailure>() => return Err(error),
        Err(error) => (json!({"error":format!("{error:#}")}), true),
    };
    let response = ToolResultV1 {
        version: 1,
        call_id: call.id.clone(),
        requester: state.request.requester.clone(),
        result_schema_id: result_schema_id(namespace).into(),
        result: to_raw_value(&value)?,
        is_error,
    };
    // Content records themselves make successful submissions idempotent. Nucleus
    // owns the mailbox response, including diagnostics and read-only calls.
    if !is_error && let Some(result) = &state.accepted {
        match result {
            // The draft validator commits brief, resume content and rendered bytes together.
            StageResult::Draft(_) => {}
            StageResult::Brief(brief) => {
                store.put_content(&state.inputs.packet_id, "brief", brief)?;
            }
            StageResult::Resume(resume) => {
                store.put_content(
                    &state.inputs.packet_id,
                    state.stage.resume_artifacts().0,
                    resume,
                )?;
            }
            StageResult::Review(markdown) => {
                store.put_artifact(
                    Some(&state.inputs.packet_id),
                    "resume-review",
                    &format!("{}-resume-review.md", state.inputs.packet_id),
                    "text/markdown",
                    markdown.as_bytes(),
                )?;
            }
        }
    }
    Ok(response)
}

fn execute_tool(
    state: &mut StageState,
    call: &ToolCallV1,
    validator: SubmissionValidator<'_>,
) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Read {
        id: String,
    }
    match call.tool_name.as_str() {
        "submit_draft" => execute_draft(state, call, validator),
        "list_career_entries" => {
            let _: Empty = serde_json::from_str(call.arguments.get())?;
            Ok(
                json!({"entries":state.inputs.career_entries.iter().map(|entry| json!({"id":entry.id,"title":entry.title})).collect::<Vec<_>>()}),
            )
        }
        "read_career_entry" => {
            let request: Read = serde_json::from_str(call.arguments.get())?;
            let entry = state
                .inputs
                .career_entries
                .iter()
                .find(|entry| entry.id == request.id)
                .context("unknown career entry ID")?;
            Ok(json!({"entry":entry}))
        }
        "submit_brief" => {
            ensure!(
                state.stage == Stage::Brief,
                "brief submission not allowed in this stage"
            );
            let brief = if sectioned_brief(state) {
                serde_json::from_str::<BriefSubmission>(call.arguments.get())?.into_brief()?
            } else {
                let brief: Brief = serde_json::from_str(call.arguments.get())?;
                validate_brief(&brief)?;
                brief
            };
            accept(state, StageResult::Brief(brief))
        }
        "submit_resume" => {
            ensure!(
                matches!(
                    state.stage,
                    Stage::Resume | Stage::ResumeDraft | Stage::ResumeRevision
                ),
                "resume submission not allowed in this stage"
            );
            let resume: Resume = serde_json::from_str(call.arguments.get())?;
            let raw: Value = serde_json::from_str(call.arguments.get())?;
            ensure!(
                raw.get("project_bullets").is_none(),
                "project bullets are requester-owned"
            );
            if !project_tools(state) {
                let submitted: Value = serde_json::from_str(call.arguments.get())?;
                ensure!(
                    submitted.get("projects").is_none(),
                    "legacy submissions cannot contain a projects field"
                );
            }
            ensure!(
                resume.projects.is_some() == project_tools(state),
                "projects are required only by the project-authoring toolset"
            );
            validate_resume(&resume, &state.inputs.career_entries)?;
            let result = StageResult::Resume(resume);
            if state.accepted.is_none()
                && let Some(validate) = validator
            {
                validate(&result).context("Resume rendering rejected the candidate; revise the editable resume content and submit again")?;
            }
            accept(state, result)
        }
        "submit_review" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct ReviewSubmission {
                markdown: String,
            }
            ensure!(
                state.stage == Stage::ResumeReview,
                "review submission not allowed in this stage"
            );
            let review: ReviewSubmission = serde_json::from_str(call.arguments.get())?;
            ensure!(!review.markdown.trim().is_empty(), "review text is empty");
            accept(state, StageResult::Review(review.markdown))
        }
        _ => bail!("unknown tool"),
    }
}

fn execute_draft(
    state: &mut StageState,
    call: &ToolCallV1,
    validator: SubmissionValidator<'_>,
) -> Result<Value> {
    ensure!(
        state.stage == Stage::Draft,
        "draft submission not allowed in this stage"
    );
    let mut submission: DraftSubmission = serde_json::from_str(call.arguments.get())?;
    if let Some(resume) = &submission.resume {
        let raw: Value = serde_json::from_str(call.arguments.get())?;
        ensure!(
            raw["resume"].get("project_bullets").is_none(),
            "project bullets are requester-owned"
        );
        if fixed_project_tools(state) {
            ensure!(
                raw["resume"].get("projects").is_none() && resume.projects.is_none(),
                "project fields are not writable"
            );
        }
    }
    if fixed_project_tools(state)
        && let Some(resume) = &mut submission.resume
    {
        resume.project_bullets = Some(
            state
                .inputs
                .fixed_projects
                .clone()
                .context("fixed project input missing")?,
        );
    }
    ensure!(
        submission.brief.pursue == submission.resume.is_some(),
        "pursued drafts require a resume; declined drafts must not contain one"
    );
    if !submission.brief.pursue {
        ensure!(
            submission.brief.why_it_works.trim().is_empty()
                && submission.brief.role.trim().is_empty()
                && submission
                    .brief
                    .culture
                    .as_deref()
                    .is_none_or(|text| text.trim().is_empty()),
            "declined drafts must have empty brief text"
        );
    }
    let brief = submission.brief.into_brief()?;
    if let Some(resume) = &submission.resume {
        ensure!(
            resume.projects.is_some() != fixed_project_tools(state),
            "draft project boundary differs"
        );
        if let Some(bullets) = &resume.project_bullets {
            bullets.validate()?;
        }
        validate_resume(resume, &state.inputs.career_entries)?;
    }
    let result = StageResult::Draft(Draft {
        brief,
        resume: submission.resume,
    });
    if state.accepted.is_none() {
        validator.context("draft acceptance validator is missing")?(&result)?;
    }
    accept(state, result)
}

fn validate_brief(brief: &Brief) -> Result<()> {
    ensure!(
        !brief.paragraph.trim().is_empty()
            && !brief.paragraph.contains(['\n', '\r'])
            && brief.paragraph.split_whitespace().count() <= 150,
        "brief must be one nonempty plain paragraph of at most 150 words"
    );
    ensure!(
        !brief.paragraph.starts_with(['#', '*', '-']),
        "brief must not have a heading or list marker"
    );
    Ok(())
}

fn validate_brief_sections(why: &str, role: &str, culture: Option<&str>) -> Result<()> {
    let mut total = 0;
    for (name, text, limit) in [("Why it works", why, 45), ("Role", role, 30)]
        .into_iter()
        .chain(culture.map(|text| ("Culture", text, 25)))
    {
        let words = text.split_whitespace().count();
        ensure!(
            !text.trim().is_empty()
                && !text.contains(['\n', '\r'])
                && !text.starts_with(['#', '*', '-', '•'])
                && words <= limit,
            "{name} must be plain nonempty single-line text without headings or list markers, at most {limit} words"
        );
        total += words;
    }
    ensure!(
        total <= 90,
        "brief content must be at most 90 words in total"
    );
    Ok(())
}

/// Validate frozen display text while retaining support for the old paragraph format.
pub(crate) fn validate_brief_text(text: &str) -> Result<()> {
    if !text.contains(['\n', '\r']) {
        ensure!(
            !text.trim().is_empty() && text.split_whitespace().count() <= 150,
            "legacy brief must be one nonempty paragraph of at most 150 words"
        );
        return Ok(());
    }
    let sections: Vec<_> = text.split("\n\n").collect();
    ensure!(
        (2..=3).contains(&sections.len()),
        "brief must contain two or three labeled sections"
    );
    let why = sections[0]
        .strip_prefix("Why it works: ")
        .context("missing Why it works section")?;
    let role = sections[1]
        .strip_prefix("Role: ")
        .context("missing Role section")?;
    let culture = sections
        .get(2)
        .map(|text| {
            text.strip_prefix("Culture: ")
                .context("invalid Culture section")
        })
        .transpose()?;
    validate_brief_sections(why, role, culture)
}

fn validate_resume(resume: &Resume, entries: &[CareerEntry]) -> Result<()> {
    if let Some(projects) = &resume.projects {
        crate::resume::validate_projects(projects)?;
    }
    ensure!(
        (1..=12).contains(&resume.jackson_bullets.len()),
        "resume must contain 1 to 12 Jackson bullets"
    );
    for bullet in &resume.jackson_bullets {
        ensure!(
            !bullet.trim().is_empty()
                && !bullet.contains(['\n', '\r'])
                && !bullet.starts_with(['•', '*', '-'])
                && bullet.chars().count() <= 1000,
            "Jackson bullets must be plain nonempty single-line text without bullet markers"
        );
    }
    let mut referenced = BTreeSet::new();
    for evidence in &resume.evidence {
        ensure!(
            evidence.bullet_index < resume.jackson_bullets.len()
                && referenced.insert(evidence.bullet_index)
                && !evidence.career_entry_ids.is_empty(),
            "every bullet requires one unique evidence record"
        );
        for id in &evidence.career_entry_ids {
            ensure!(
                entries.iter().any(|entry| &entry.id == id),
                "resume evidence cites an unknown career entry"
            );
        }
    }
    ensure!(
        referenced.len() == resume.jackson_bullets.len(),
        "every authored Jackson bullet must cite supporting career entries"
    );
    Ok(())
}

fn accept(state: &mut StageState, result: StageResult) -> Result<Value> {
    if let Some(existing) = &state.accepted {
        ensure!(
            existing == &result,
            "a different result is already committed; the accepted stage is immutable"
        );
    } else {
        state.accepted = Some(result);
    }
    let reported_stage = match state.stage {
        Stage::ResumeDraft | Stage::ResumeRevision => Stage::Resume,
        kind => kind,
    };
    Ok(json!({"accepted":true,"packet_id":state.inputs.packet_id,"stage":reported_stage}))
}

fn persist(store: &Store, state: &StageState) -> Result<()> {
    store.save_execution(&state.inputs.packet_id, state.stage.name(), state)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture_store(root: &Path) -> Store {
        let store = Store::open(root).unwrap();
        store
            .insert(&crate::store::PacketRecord {
                id: "packet-1".into(),
                opportunity: "fixture-role".into(),
                job_id: "cast-1".into(),
                company: "Example".into(),
                title: "Engineer".into(),
                status: "preparing".into(),
                directory: String::new(),
            })
            .unwrap();
        store
    }
    fn stage_bytes(store: &Store) -> Vec<u8> {
        serde_json::to_vec(&store.execution("packet-1", "brief").unwrap()).unwrap()
    }
    fn bind_tool_result(
        store: &Store,
        state: &mut StageState,
        call: &ToolCallV1,
    ) -> Result<ToolResultV1> {
        bind_tool_result_validated(store, state, call, None)
    }

    use nucleus_core::{AttemptId, SchemaId, ToolCallId};

    fn inputs() -> StageInputs {
        StageInputs {
            packet_id: "packet-1".into(),
            posting: "A complete job posting".into(),
            career_entries: vec![CareerEntry {
                id: "story-1".into(),
                title: "Jackson work".into(),
                markdown: "Jackson project details and disclosure notes".into(),
            }],
            guidance: "Never invent metrics".into(),
            original_jackson_bullets: vec!["Original bullet".into()],
            brief: None,
            ..StageInputs::default()
        }
    }

    fn call(state: &StageState, id: &str, name: &str, value: &Value) -> ToolCallV1 {
        ToolCallV1 {
            version: 1,
            id: ToolCallId::new(id),
            job_id: state.request.id.clone(),
            attempt_id: AttemptId::new("attempt-1"),
            request_sequence: 1,
            tool_name: name.into(),
            arguments_schema_id: SchemaId::new(call_schema_id(state, name).unwrap()),
            arguments: to_raw_value(value).unwrap(),
        }
    }

    fn use_legacy_identity(state: &mut StageState) {
        assert!(state.runtime.is_none());
        state.request.id = nucleus_core::JobId::new(state.request.id.as_str().replacen(
            "platter-",
            "job-packets-",
            1,
        ));
        state.request.label = state.request.label.replacen("Platter ", "Job packet ", 1);
        state.request.requester.program = LEGACY_TOOL_NAMESPACE.into();
        state.request.invocation.toolset = Some(toolset_ref(state.stage, LEGACY_TOOL_NAMESPACE));
        state.request.validate().unwrap();
    }

    #[test]
    fn exact_policy_and_stage_authority() {
        let request = build_request(Stage::Resume, &inputs(), Path::new("/tmp")).unwrap();
        assert_eq!(request.invocation.model.as_str(), MODEL);
        assert_eq!(
            request.invocation.reasoning_effort,
            Some(ReasoningEffort::Max)
        );
        assert_eq!(request.invocation.workspace_access, WorkspaceAccess::None);
        assert!(
            !request.invocation.builtin_tools.local_execution
                && !request.invocation.builtin_tools.web_search
        );
        assert!(request.invocation.launch_context.is_none());
        assert!(request.id.as_str().starts_with("platter-resume-"));
        assert_eq!(request.requester.program, TOOL_NAMESPACE);
        assert_eq!(
            request.invocation.toolset,
            Some(toolset_ref(Stage::Resume, TOOL_NAMESPACE))
        );
        assert_eq!(
            tool_definitions(Stage::Resume, TOOL_NAMESPACE, false)
                .unwrap()
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["list_career_entries", "read_career_entry", "submit_resume"]
        );
    }

    #[test]
    fn restart_reuses_exact_request_and_rejects_changed_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_store(dir.path());
        for stage in [Stage::Brief, Stage::Resume] {
            let mut state = load_or_create(&path, stage, inputs()).unwrap();
            state.request.instructions = "Previously retained instructions".into();
            persist(&path, &state).unwrap();
            let reloaded = load_or_create(&path, stage, inputs()).unwrap();
            assert_eq!(state.request, reloaded.request);
            let mut changed = inputs();
            changed.posting.push_str(" different");
            assert!(load_or_create(&path, stage, changed).is_err());
        }
    }

    #[test]
    fn legacy_pending_calls_keep_frozen_request_and_schema_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_store(dir.path());
        let mut state = load_or_create(&path, Stage::Brief, inputs()).unwrap();
        use_legacy_identity(&mut state);
        persist(&path, &state).unwrap();
        let original_bytes = stage_bytes(&path);
        let mut restarted = load_or_create(&path, Stage::Brief, inputs()).unwrap();
        assert_eq!(
            retained_request(&path, "packet-1", Stage::Brief)
                .unwrap()
                .unwrap(),
            state.request
        );
        assert_eq!(stage_bytes(&path), original_bytes);
        let pending = call(
            &restarted,
            "legacy-call",
            "submit_brief",
            &json!({"paragraph":"Supported fit with compensation unknown.","pursue":true}),
        );
        assert_eq!(
            pending.arguments_schema_id.as_str(),
            "job-packets.submit-brief.arguments.v1"
        );
        let response = bind_tool_result(&path, &mut restarted, &pending).unwrap();
        assert_eq!(
            response.result_schema_id.as_str(),
            "job-packets.tool-result.v1"
        );
        assert_eq!(response.requester, state.request.requester);
        let mut reloaded = load_or_create(&path, Stage::Brief, inputs()).unwrap();
        assert_eq!(reloaded.request, state.request);
        let accepted_bytes = stage_bytes(&path);
        assert_eq!(
            serde_json::to_vec(&bind_tool_result(&path, &mut reloaded, &pending).unwrap()).unwrap(),
            serde_json::to_vec(&response).unwrap()
        );
        assert_eq!(stage_bytes(&path), accepted_bytes);
        let mut foreign_schema = call(
            &reloaded,
            "wrong-namespace",
            "list_career_entries",
            &json!({}),
        );
        foreign_schema.arguments_schema_id = "platter.list-career-entries.arguments.v1".into();
        assert!(bind_tool_result(&path, &mut reloaded, &foreign_schema).is_err());
        assert_eq!(stage_bytes(&path), accepted_bytes);
    }

    #[test]
    fn conflicting_retained_toolset_is_rejected_without_rewriting_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_store(dir.path());
        let mut state = load_or_create(&path, Stage::Brief, inputs()).unwrap();
        state.request.invocation.toolset = Some(toolset_ref(Stage::Brief, LEGACY_TOOL_NAMESPACE));
        persist(&path, &state).unwrap();
        let before = stage_bytes(&path);
        assert!(load_or_create(&path, Stage::Brief, inputs()).is_err());
        assert!(retained_request(&path, "packet-1", Stage::Brief).is_err());
        assert_eq!(stage_bytes(&path), before);
    }

    #[test]
    fn domain_commit_survives_restart_and_exact_receipt_replay() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_store(dir.path());
        let mut state = load_or_create(&path, Stage::Brief, inputs()).unwrap();
        let first = call(
            &state,
            "call-1",
            "submit_brief",
            &json!({"why_it_works":"Your production service experience fits the team's backend ownership needs.","role":"Rust services, design reviews, and production support.","culture":null,"pursue":true}),
        );
        let response = bind_tool_result(&path, &mut state, &first).unwrap();
        let mut restarted = load_or_create(&path, Stage::Brief, inputs()).unwrap();
        assert!(restarted.accepted.is_some());
        assert_eq!(
            serde_json::to_vec(&response).unwrap(),
            serde_json::to_vec(&bind_tool_result(&path, &mut restarted, &first).unwrap()).unwrap()
        );
        let conflicting = call(
            &restarted,
            "call-1",
            "submit_brief",
            &json!({"why_it_works":"","role":"","culture":null,"pursue":false}),
        );
        assert!(
            bind_tool_result(&path, &mut restarted, &conflicting)
                .unwrap()
                .is_error
        );
        let replacement = call(
            &restarted,
            "call-2",
            "submit_brief",
            &json!({"why_it_works":"","role":"","culture":null,"pursue":false}),
        );
        assert!(
            bind_tool_result(&path, &mut restarted, &replacement)
                .unwrap()
                .is_error
        );
        assert_eq!(state.accepted, restarted.accepted);
    }

    #[test]
    fn generated_resume_cannot_author_other_sections_or_skip_evidence() {
        assert!(
            serde_json::from_value::<Resume>(
                json!({"jackson_bullets":["Supported"],"evidence":[],"education":"invented"})
            )
            .is_err()
        );
        let mut resume = Resume {
            projects: None,
            project_bullets: None,
            jackson_bullets: vec!["Supported work".into()],
            evidence: vec![],
        };
        assert!(validate_resume(&resume, &inputs().career_entries).is_err());
        resume.evidence.push(BulletEvidence {
            bullet_index: 0,
            career_entry_ids: vec!["story-1".into()],
        });
        validate_resume(&resume, &inputs().career_entries).unwrap();
        resume.evidence[0].career_entry_ids[0] = "missing".into();
        assert!(validate_resume(&resume, &inputs().career_entries).is_err());
    }

    #[test]
    fn malformed_brief_never_commits() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_store(dir.path());
        let mut state = load_or_create(&path, Stage::Brief, inputs()).unwrap();
        let invalid = call(
            &state,
            "call-1",
            "submit_brief",
            &json!({"why_it_works":"One paragraph\nAnother paragraph","role":"Backend services.","culture":null,"pursue":true}),
        );
        assert!(
            bind_tool_result(&path, &mut state, &invalid)
                .unwrap()
                .is_error
        );
        assert!(state.accepted.is_none());
    }

    #[test]
    fn career_tools_only_read_frozen_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_store(dir.path());
        let mut state = load_or_create(&path, Stage::Brief, inputs()).unwrap();
        let read = call(
            &state,
            "call-1",
            "read_career_entry",
            &json!({"id":"story-1"}),
        );
        let response = bind_tool_result(&path, &mut state, &read).unwrap();
        assert!(
            response
                .result
                .get()
                .contains("Jackson project details and disclosure notes")
        );
        let unknown = call(
            &state,
            "call-2",
            "read_career_entry",
            &json!({"id":"outside"}),
        );
        assert!(
            bind_tool_result(&path, &mut state, &unknown)
                .unwrap()
                .is_error
        );
        assert_eq!(state.inputs, inputs());
    }

    fn runtime_job(state: &StageState, status: nucleus_core::JobState) -> JobV1 {
        JobV1 {
            quota: None,
            version: 1,
            summary: nucleus_core::JobSummaryV1 {
                version: 1,
                id: state.request.id.clone(),
                label: state.request.label.clone(),
                requester: state.request.requester.clone(),
                parent: None,
                state: status,
                request_digest: state.request.digest().unwrap(),
                created_at: "2026-09-06T21:00:00Z".into(),
                updated_at: "2026-09-06T21:00:00Z".into(),
                completed_at: None,
                current_attempt_id: Some(AttemptId::new("attempt-1")),
            },
            request: state.request.clone(),
            attempts: vec![],
        }
    }

    fn writing_inputs() -> StageInputs {
        let mut value = inputs();
        value.editorial_policy = Some("Frozen policy: explain the consequence.".into());
        value.brief = Some(Brief {
            paragraph: "Writer positioning only".into(),
            pursue: true,
        });
        value.career_entries.push(CareerEntry {
            id: "other-story".into(),
            title: "Complementary work".into(),
            markdown: "Unselected supporting material".into(),
        });
        value
    }

    fn candidate() -> Resume {
        Resume {
            projects: None,
            project_bullets: None,
            jackson_bullets: vec![
                "Built a diagnostic service that made failures easier to investigate".into(),
            ],
            evidence: vec![BulletEvidence {
                bullet_index: 0,
                career_entry_ids: vec!["story-1".into()],
            }],
        }
    }

    fn single_draft_inputs() -> StageInputs {
        StageInputs {
            brief: None,
            project_resources: Some(
                cell_prompts::Prompts::at("platter", 1)
                    .unwrap()
                    .expand(PROJECT_RESOURCES)
                    .unwrap(),
            ),
            ..writing_inputs()
        }
    }

    fn single_draft_payload() -> Value {
        let mut resume = candidate();
        resume.projects = Some(vec![Project {
            name: "Cell".into(),
            description: "Rust applications".into(),
            dates: None,
            bullets: vec!["Built local tools".into()],
            sources: vec!["cell/README.md".into()],
        }]);
        json!({"brief":{"pursue":true,"why_it_works":"Your service ownership fits this role.","role":"Rust services and production support.","culture":null},"resume":resume})
    }

    fn retain_fixture_draft(store: &Store, result: &StageResult) -> Result<()> {
        let StageResult::Draft(draft) = result else {
            bail!("expected draft");
        };
        let rendered = draft
            .resume
            .as_ref()
            .map(|_| crate::resume::RenderedResume {
                source: "fixture LaTeX".into(),
                pdf: b"fixture PDF".to_vec(),
                pages: 1,
            });
        crate::workflow::retain_draft(store, "packet-1", draft, rendered.as_ref())
    }

    #[test]
    fn single_draft_uses_combined_prompt_and_submission_without_handoffs() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture_store(dir.path());
        let state = load_or_create(&store, Stage::Draft, single_draft_inputs()).unwrap();
        assert!(
            state.request.instructions.contains(
                &cell_prompts::Prompts::at("platter", 1)
                    .unwrap()
                    .expand(DRAFT)
                    .unwrap()
            )
        );
        assert!(
            state
                .request
                .instructions
                .contains("Frozen policy: explain the consequence.")
        );
        assert!(
            state.request.instructions.contains(
                &cell_prompts::Prompts::at("platter", 1)
                    .unwrap()
                    .expand(PROJECT_RESOURCES)
                    .unwrap()
            )
        );
        assert!(
            !state
                .request
                .prompt
                .contains("accepted_brief_positioning_only")
        );
        assert_eq!(
            state.request.invocation.workspace_access,
            WorkspaceAccess::ReadOnly
        );
        assert!(state.request.invocation.builtin_tools.local_execution);
        assert!(!state.request.invocation.builtin_tools.web_search);
        let definitions = project_tool_definitions(Stage::Draft).unwrap();
        assert_eq!(
            definitions
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            ["list_career_entries", "read_career_entry", "submit_draft"]
        );
        assert_eq!(
            definitions.tools[2].input_schema_id.as_str(),
            "platter.submit-draft.arguments.v1"
        );
        let schema: Value = serde_json::from_str(definitions.tools[2].input_schema.get()).unwrap();
        assert_eq!(
            schema["properties"]["resume"]["required"],
            json!(["jackson_bullets", "evidence", "projects"])
        );
        for stage in [
            Stage::Brief,
            Stage::ResumeDraft,
            Stage::ResumeReview,
            Stage::ResumeRevision,
        ] {
            assert!(store.execution("packet-1", stage.name()).unwrap().is_none());
        }
    }

    #[test]
    fn single_draft_rejects_content_and_layout_then_accepts_one_immutable_packet() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture_store(dir.path());
        let mut state = load_or_create(&store, Stage::Draft, single_draft_inputs()).unwrap();
        let original_request = state.request.clone();
        let payload = single_draft_payload();
        let retain = |result: &StageResult| retain_fixture_draft(&store, result);
        for invalid in [
            json!({"brief":payload["brief"],"resume":null}),
            json!({"brief":{"pursue":true,"why_it_works":"","role":"Rust"},"resume":payload["resume"]}),
            json!({"brief":payload["brief"],"resume":{"jackson_bullets":["Work"],"evidence":[],"projects":[]}}),
        ] {
            let submit = call(&state, "invalid", "submit_draft", &invalid);
            assert!(
                bind_tool_result_validated(&store, &mut state, &submit, Some(&retain))
                    .unwrap()
                    .is_error
            );
            assert!(store.run_artifact("packet-1", "brief").unwrap().is_none());
        }
        let submit = call(&state, "draft-submit", "submit_draft", &payload);
        let rejected =
            bind_tool_result_validated(&store, &mut state, &submit, Some(&|_| bail!("two pages")))
                .unwrap();
        assert!(rejected.is_error && rejected.result.get().contains("two pages"));
        assert!(state.accepted.is_none());
        assert!(store.run_artifact("packet-1", "brief").unwrap().is_none());
        assert!(
            bind_tool_result_validated(
                &store,
                &mut state,
                &submit,
                Some(&|_| Err(crate::resume::RendererFailure.into()))
            )
            .unwrap_err()
            .is::<crate::resume::RendererFailure>()
        );
        assert!(
            !bind_tool_result_validated(&store, &mut state, &submit, Some(&retain))
                .unwrap()
                .is_error
        );
        assert_eq!(state.request, original_request);
        for kind in ["brief", "resume-content", "resume-source", "resume-pdf"] {
            assert!(store.run_artifact("packet-1", kind).unwrap().is_some());
        }
        for kind in [
            "resume-draft",
            "resume-draft-source",
            "resume-draft-pdf",
            "resume-review",
        ] {
            assert!(store.run_artifact("packet-1", kind).unwrap().is_none());
        }
        let mut restored = load_or_create(&store, Stage::Draft, single_draft_inputs()).unwrap();
        assert_eq!(restored.accepted, state.accepted);
        assert!(
            !bind_tool_result_validated(
                &store,
                &mut restored,
                &submit,
                Some(&|_| panic!("accepted packet must not render again"))
            )
            .unwrap()
            .is_error
        );
        let mut changed = payload;
        changed["brief"]["role"] = json!("Different role");
        let conflicting = call(&restored, "conflict", "submit_draft", &changed);
        assert!(
            bind_tool_result(&store, &mut restored, &conflicting)
                .unwrap()
                .is_error
        );
    }

    #[test]
    fn single_draft_decline_and_failed_commit_do_not_leave_partial_packets() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture_store(dir.path());
        let mut state = load_or_create(&store, Stage::Draft, single_draft_inputs()).unwrap();
        // Force the last artifact write to fail after the brief and resume writes.
        store
            .put_artifact(
                Some("packet-1"),
                "resume-pdf",
                "packet-1-resume.pdf",
                "application/pdf",
                b"conflict",
            )
            .unwrap();
        let submit = call(
            &state,
            "failed-commit",
            "submit_draft",
            &single_draft_payload(),
        );
        assert!(
            bind_tool_result_validated(
                &store,
                &mut state,
                &submit,
                Some(&|result| retain_fixture_draft(&store, result))
            )
            .unwrap()
            .is_error
        );
        for kind in ["brief", "resume-content", "resume-source"] {
            assert!(store.run_artifact("packet-1", kind).unwrap().is_none());
        }
        assert!(state.accepted.is_none());

        let declined_dir = tempfile::tempdir().unwrap();
        let declined = fixture_store(declined_dir.path());
        let mut state = load_or_create(&declined, Stage::Draft, single_draft_inputs()).unwrap();
        let submit = call(
            &state,
            "decline",
            "submit_draft",
            &json!({"brief":{"pursue":false,"why_it_works":"","role":"","culture":null},"resume":null}),
        );
        assert!(
            !bind_tool_result_validated(
                &declined,
                &mut state,
                &submit,
                Some(&|result| retain_fixture_draft(&declined, result))
            )
            .unwrap()
            .is_error
        );
        assert_eq!(
            load_or_create(&declined, Stage::Draft, single_draft_inputs())
                .unwrap()
                .accepted,
            state.accepted
        );
        assert!(
            declined
                .run_artifact("packet-1", "resume-pdf")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)] // Keep the complete handoff and legacy-boundary check together.
    fn project_workflow_keeps_direct_research_and_versioned_submissions() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture_store(dir.path());
        let mut inputs = writing_inputs();
        inputs.project_resources = Some(
            cell_prompts::Prompts::at("platter", 1)
                .unwrap()
                .expand(PROJECT_RESOURCES)
                .unwrap(),
        );
        let brief = build_request(Stage::Brief, &inputs, dir.path()).unwrap();
        assert_eq!(brief.invocation.toolset.unwrap().version, 3);
        let mut draft = load_or_create(&store, Stage::ResumeDraft, inputs.clone()).unwrap();
        assert_eq!(
            draft.request.invocation.workspace_access,
            WorkspaceAccess::ReadOnly
        );
        assert!(draft.request.invocation.builtin_tools.local_execution);
        assert!(!draft.request.invocation.builtin_tools.web_search);
        assert!(
            draft
                .request
                .instructions
                .contains("/Users/joey/Library/Application Support/Annals/decisions/config.toml")
        );
        for removed in [
            "/Users/joey/rust/cell",
            "/Users/joey/ts/wrought-private",
            "https://github.com/jtan2231/cell",
            "repositories",
            "working-tree",
        ] {
            assert!(!draft.request.instructions.contains(removed));
        }
        let missing = call(
            &draft,
            "missing-projects",
            "submit_resume",
            &json!(candidate()),
        );
        assert!(
            bind_tool_result(&store, &mut draft, &missing)
                .unwrap()
                .is_error
        );
        let mut resume = candidate();
        resume.projects = Some(vec![Project {
            name: "Cell".into(),
            description: "Rust applications".into(),
            dates: None,
            bullets: vec!["Built local tools".into()],
            sources: vec!["cell/README.md".into()],
        }]);
        let submit = call(&draft, "project-draft", "submit_resume", &json!(resume));
        assert_eq!(
            submit.arguments_schema_id.as_str(),
            "platter.submit-resume.arguments.v2"
        );
        assert!(
            !bind_tool_result(&store, &mut draft, &submit)
                .unwrap()
                .is_error
        );
        assert!(
            !bind_tool_result(&store, &mut draft, &submit)
                .unwrap()
                .is_error
        );
        assert_eq!(
            load_or_create(&store, Stage::ResumeDraft, inputs.clone())
                .unwrap()
                .request,
            draft.request
        );
        inputs.proposed_draft = Some(resume.clone());
        let mut reviewer = load_or_create(&store, Stage::ResumeReview, inputs.clone()).unwrap();
        assert!(
            reviewer.request.instructions.contains(
                &cell_prompts::Prompts::at("platter", 1)
                    .unwrap()
                    .expand(PROJECT_RESOURCES)
                    .unwrap()
            )
        );
        assert!(reviewer.request.invocation.builtin_tools.local_execution);
        assert!(!reviewer.request.prompt.contains("Writer positioning only"));
        let review = call(
            &reviewer,
            "project-review",
            "submit_review",
            &json!({"markdown":"Keep the project detail."}),
        );
        assert!(
            !bind_tool_result(&store, &mut reviewer, &review)
                .unwrap()
                .is_error
        );
        inputs.editorial_review = Some("Keep the project detail.".into());
        let revision = load_or_create(&store, Stage::ResumeRevision, inputs).unwrap();
        assert_eq!(revision.request.instructions, draft.request.instructions);
        assert_eq!(revision.request.invocation, draft.request.invocation);
        assert_eq!(
            project_tool_definitions(Stage::ResumeDraft)
                .unwrap()
                .tools
                .last()
                .unwrap()
                .input_schema_id
                .as_str(),
            "platter.submit-resume.arguments.v2"
        );

        let legacy_dir = tempfile::tempdir().unwrap();
        let legacy_store = fixture_store(legacy_dir.path());
        let mut legacy = load_or_create(&legacy_store, Stage::Resume, self::inputs()).unwrap();
        let submit = call(&legacy, "legacy-project", "submit_resume", &json!(resume));
        assert!(
            bind_tool_result(&legacy_store, &mut legacy, &submit)
                .unwrap()
                .is_error
        );
        assert!(
            serde_json::to_value(candidate())
                .unwrap()
                .get("projects")
                .is_none()
        );
    }

    fn submitted_draft(store: &Store) -> (StageState, StageInputs) {
        let mut state = load_or_create(store, Stage::ResumeDraft, writing_inputs()).unwrap();
        let submit = call(&state, "draft-call", "submit_resume", &json!(candidate()));
        let response =
            bind_tool_result_validated(store, &mut state, &submit, Some(&|_| Ok(()))).unwrap();
        assert!(!response.is_error);
        assert!(
            store
                .run_artifact("packet-1", "resume-content")
                .unwrap()
                .is_none()
        );
        let mut next = writing_inputs();
        next.proposed_draft = Some(candidate());
        (state, next)
    }

    #[test]
    fn revision_adds_only_draft_and_review_to_the_exact_retained_writer() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture_store(dir.path());
        let (mut draft, mut next) = submitted_draft(&store);
        // Simulate a request retained by an earlier build of the writer.
        draft
            .request
            .instructions
            .push_str("\nRetained writer wording.");
        draft.request.developer_instructions = Some("Retained developer instructions".into());
        persist(&store, &draft).unwrap();
        let mut reviewer = load_or_create(&store, Stage::ResumeReview, next.clone()).unwrap();
        let markdown =
            "A free-form review, without the suggested headings.\n\nKeep the diagnosis detail.\n";
        let submit = call(
            &reviewer,
            "review-call",
            "submit_review",
            &json!({"markdown":markdown}),
        );
        assert!(
            !bind_tool_result(&store, &mut reviewer, &submit)
                .unwrap()
                .is_error
        );
        next.editorial_review = Some(markdown.into());
        let revision = load_or_create(&store, Stage::ResumeRevision, next.clone()).unwrap();
        assert_ne!(revision.request.id, draft.request.id);
        let mut prompt: Value = serde_json::from_str(&revision.request.prompt).unwrap();
        assert_eq!(
            prompt.as_object_mut().unwrap().remove("proposed_draft"),
            Some(json!(candidate()))
        );
        assert_eq!(
            prompt.as_object_mut().unwrap().remove("editorial_review"),
            Some(json!(markdown))
        );
        assert_eq!(
            prompt,
            serde_json::from_str::<Value>(&draft.request.prompt).unwrap()
        );
        let mut comparable = revision.request.clone();
        comparable.id = draft.request.id.clone();
        comparable.prompt = draft.request.prompt.clone();
        assert_eq!(comparable, draft.request);
        assert_eq!(
            serde_json::to_value(
                tool_definitions(Stage::ResumeDraft, TOOL_NAMESPACE, false).unwrap()
            )
            .unwrap(),
            serde_json::to_value(
                tool_definitions(Stage::ResumeRevision, TOOL_NAMESPACE, false).unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            load_or_create(&store, Stage::ResumeRevision, next)
                .unwrap()
                .request,
            revision.request
        );
    }

    #[test]
    fn reviewer_gets_frozen_sources_and_policy_without_writer_context_or_authority() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture_store(dir.path());
        let (mut draft, next) = submitted_draft(&store);
        draft
            .request
            .instructions
            .push_str("\nPrivate writer rationale sentinel");
        draft.request.developer_instructions = Some("Private developer sentinel".into());
        persist(&store, &draft).unwrap();
        let mut reviewer = load_or_create(&store, Stage::ResumeReview, next).unwrap();
        let serialized = serde_json::to_string(&reviewer.request).unwrap();
        assert!(!serialized.contains("sentinel"));
        assert!(!serialized.contains("Writer positioning only"));
        assert!(
            reviewer
                .request
                .instructions
                .contains("Frozen policy: explain the consequence.")
        );
        assert!(
            !reviewer
                .request
                .instructions
                .contains("If a proposed draft and editorial review are supplied")
        );
        assert!(reviewer.request.parent.is_none());
        assert_eq!(
            reviewer.request.invocation.workspace_access,
            WorkspaceAccess::None
        );
        assert!(!reviewer.request.invocation.builtin_tools.local_execution);
        assert!(!reviewer.request.invocation.builtin_tools.web_search);
        let read = call(
            &reviewer,
            "read-other",
            "read_career_entry",
            &json!({"id":"other-story"}),
        );
        assert!(
            bind_tool_result(&store, &mut reviewer, &read)
                .unwrap()
                .result
                .get()
                .contains("Unselected supporting material")
        );
        let forbidden = call(
            &reviewer,
            "write-final",
            "submit_resume",
            &json!(candidate()),
        );
        assert!(bind_tool_result(&store, &mut reviewer, &forbidden).is_err());
    }

    #[test]
    fn markdown_review_is_exact_immutable_and_survives_a_lost_acknowledgement() {
        for markdown in [
            "Looks good.",
            "  **Keep this**\n\n- Consider a clearer consequence.\n",
            "# My own format\r\n\r\nNo numbered findings — no verdict.\r\n",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let store = fixture_store(dir.path());
            let (_, next) = submitted_draft(&store);
            let mut review = load_or_create(&store, Stage::ResumeReview, next.clone()).unwrap();
            let submit = call(
                &review,
                "review-call",
                "submit_review",
                &json!({"markdown":markdown}),
            );
            let response = bind_tool_result(&store, &mut review, &submit).unwrap();
            assert!(!response.is_error);
            let artifact = store
                .run_artifact("packet-1", "resume-review")
                .unwrap()
                .unwrap();
            assert_eq!(artifact.content, markdown.as_bytes());
            assert_eq!(artifact.media_type, "text/markdown");
            let mut restarted = load_or_create(&store, Stage::ResumeReview, next).unwrap();
            assert_eq!(restarted.request, review.request);
            assert_eq!(
                serde_json::to_value(bind_tool_result(&store, &mut restarted, &submit).unwrap())
                    .unwrap(),
                serde_json::to_value(response).unwrap()
            );
            let conflict = call(
                &restarted,
                "conflict",
                "submit_review",
                &json!({"markdown":"Different review"}),
            );
            assert!(
                bind_tool_result(&store, &mut restarted, &conflict)
                    .unwrap()
                    .is_error
            );
            restarted.runtime = Some(RuntimeState {
                quota_exhausted: false,
                state: nucleus_core::JobState::Failed,
                attempt_id: None,
            });
            assert_eq!(
                terminal_result(&restarted).unwrap(),
                StageResult::Review(markdown.into())
            );
        }
    }

    #[test]
    fn revision_requires_exact_handoffs_but_no_finding_responses() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture_store(dir.path());
        let mut next = writing_inputs();
        next.proposed_draft = Some(candidate());
        assert!(load_or_create(&store, Stage::ResumeReview, next.clone()).is_err());
        assert!(load_or_create(&store, Stage::ResumeRevision, next.clone()).is_err());
        submitted_draft(&store);
        assert!(load_or_create(&store, Stage::ResumeRevision, next.clone()).is_err());
        let mut review = load_or_create(&store, Stage::ResumeReview, next.clone()).unwrap();
        let markdown = "This needs substantial revision.\n\nExplain the consequence.";
        let submit = call(
            &review,
            "review-call",
            "submit_review",
            &json!({"markdown":markdown}),
        );
        bind_tool_result(&store, &mut review, &submit).unwrap();
        next.editorial_review = Some(markdown.into());
        let mut changed = next.clone();
        changed.editorial_policy = Some("Newer policy".into());
        assert!(load_or_create(&store, Stage::ResumeRevision, changed).is_err());
        let mut changed = next.clone();
        changed.editorial_review = Some("Substituted feedback".into());
        assert!(load_or_create(&store, Stage::ResumeRevision, changed).is_err());
        let mut changed = next.clone();
        changed.proposed_draft.as_mut().unwrap().jackson_bullets[0].push_str(" changed");
        assert!(load_or_create(&store, Stage::ResumeReview, changed).is_err());
        let mut revision = load_or_create(&store, Stage::ResumeRevision, next).unwrap();
        let submit = call(
            &revision,
            "final-call",
            "submit_resume",
            &json!(candidate()),
        );
        assert!(
            !bind_tool_result_validated(&store, &mut revision, &submit, Some(&|_| Ok(())))
                .unwrap()
                .is_error
        );
        assert_eq!(
            store
                .content::<Resume>("packet-1", "resume-content")
                .unwrap(),
            Some(candidate())
        );
    }

    #[test]
    fn both_writers_reject_bad_layout_before_accepting_content() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture_store(dir.path());
        let mut draft = load_or_create(&store, Stage::ResumeDraft, writing_inputs()).unwrap();
        let submit = call(&draft, "draft-call", "submit_resume", &json!(candidate()));
        let reject = |_: &StageResult| -> Result<()> { bail!("fixture overflow") };
        assert!(
            bind_tool_result_validated(&store, &mut draft, &submit, Some(&reject))
                .unwrap()
                .is_error
        );
        assert!(
            store
                .run_artifact("packet-1", "resume-draft")
                .unwrap()
                .is_none()
        );
        let (_, mut next) = submitted_draft(&store);
        let mut review = load_or_create(&store, Stage::ResumeReview, next.clone()).unwrap();
        let submit = call(
            &review,
            "review-call",
            "submit_review",
            &json!({"markdown":"Keep the useful detail."}),
        );
        bind_tool_result(&store, &mut review, &submit).unwrap();
        next.editorial_review = Some("Keep the useful detail.".into());
        let mut revision = load_or_create(&store, Stage::ResumeRevision, next).unwrap();
        let submit = call(
            &revision,
            "revision-call",
            "submit_resume",
            &json!(candidate()),
        );
        assert!(
            bind_tool_result_validated(&store, &mut revision, &submit, Some(&reject))
                .unwrap()
                .is_error
        );
        assert!(
            store
                .run_artifact("packet-1", "resume-content")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .run_artifact("packet-1", "resume-draft")
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .run_artifact("packet-1", "resume-review")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn legacy_input_bytes_do_not_gain_review_fields() {
        assert_eq!(
            serde_json::to_value(inputs()).unwrap(),
            json!({
                "packet_id":"packet-1", "posting":"A complete job posting",
                "career_entries":[{"id":"story-1", "title":"Jackson work", "markdown":"Jackson project details and disclosure notes"}],
                "guidance":"Never invent metrics", "original_jackson_bullets":["Original bullet"], "brief":null
            })
        );
    }

    fn fake_mailbox(
        socket: &Path,
        exchanges: Vec<(String, Value)>,
    ) -> tokio::task::JoinHandle<Vec<Value>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::UnixListener::bind(socket).unwrap();
        tokio::spawn(async move {
            let mut requests = Vec::new();
            for (expected_route, response) in exchanges {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let boundary = loop {
                    let mut buffer = [0; 4096];
                    let count = stream.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let headers = String::from_utf8(bytes[..boundary].to_vec()).unwrap();
                assert!(
                    headers.starts_with(&expected_route),
                    "unexpected request: {headers}"
                );
                let content_length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::parse)
                    })
                    .transpose()
                    .unwrap()
                    .unwrap_or(0);
                while bytes.len() < boundary + content_length {
                    let mut buffer = [0; 4096];
                    let count = stream.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                requests.push(if content_length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes[boundary..boundary + content_length]).unwrap()
                });
                let body = serde_json::to_vec(&response).unwrap();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
                stream.write_all(&body).await.unwrap();
            }
            requests
        })
    }

    #[tokio::test]
    async fn legacy_recovery_registers_legacy_schema_and_toolset_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_store(dir.path());
        let socket = dir.path().join("nucleus.sock");
        let mut state = load_or_create(&path, Stage::Brief, inputs()).unwrap();
        use_legacy_identity(&mut state);
        let mut definitions = tool_definitions(Stage::Brief, LEGACY_TOOL_NAMESPACE, false).unwrap();
        let prompts = cell_prompts::Prompts::at("platter", 1).unwrap();
        for tool in &mut definitions.tools {
            tool.description = prompts.expand(&tool.description).unwrap();
        }
        let mut exchanges = Vec::new();
        for tool in &definitions.tools {
            exchanges.push((
                "POST /v1/schemas ".into(),
                json!(LogSchemaV1::new(
                    tool.input_schema_id.clone(),
                    &tool.name,
                    "1",
                    "application/schema+json",
                    "job-packets",
                    tool.input_schema.clone(),
                )),
            ));
        }
        exchanges.push((
            "POST /v1/schemas ".into(),
            json!(LogSchemaV1::new(
                "job-packets.tool-result.v1",
                "Job packet tool result",
                "1",
                "application/schema+json",
                "job-packets",
                to_raw_value(&json!({"type":"object"})).unwrap(),
            )),
        ));
        let registration = ToolsetRegistrationV1::new(
            state.request.invocation.toolset.clone().unwrap(),
            "nucleus.toolset-definitions.v1",
            definitions,
        )
        .unwrap();
        exchanges.push((
            "POST /v1/toolsets ".into(),
            json!({
                "version":1,"toolset":registration.toolset,
                "definitionsSchemaId":registration.definitions_schema_id,
                "digest":registration.digest,"registeredAt":"2026-09-06T21:00:00Z"
            }),
        ));
        let expected_schemas: Vec<Value> = exchanges[..4]
            .iter()
            .map(|(_, response)| response.clone())
            .collect();
        let server = fake_mailbox(&socket, exchanges);
        register_tools(&NucleusClient::new(&socket).unwrap(), &state)
            .await
            .unwrap();
        let requests = server.await.unwrap();
        assert_eq!(requests[..4], expected_schemas);
        assert_eq!(requests[4], json!(registration));
    }

    #[tokio::test]
    async fn restart_replays_committed_receipt_and_runtime_failure_preserves_success() {
        for stage in [Stage::Brief, Stage::Draft] {
            let dir = tempfile::tempdir().unwrap();
            let path = fixture_store(dir.path());
            let socket = dir.path().join("nucleus.sock");
            let stage_inputs = if stage == Stage::Draft {
                single_draft_inputs()
            } else {
                inputs()
            };
            let mut state = load_or_create(&path, stage, stage_inputs.clone()).unwrap();
            let pending = call(
                &state,
                "call-1",
                stage.submit_tool(),
                &if stage == Stage::Draft {
                    single_draft_payload()
                } else {
                    json!({"why_it_works":"Your production service experience fits the team's backend ownership needs.","role":"Rust services, design reviews, and production support.","culture":null,"pursue":true})
                },
            );
            let response = bind_tool_result_validated(
                &path,
                &mut state,
                &pending,
                Some(&|result| retain_fixture_draft(&path, result)),
            )
            .unwrap();
            let running = runtime_job(&state, nucleus_core::JobState::WaitingOnRequester);
            let failed = runtime_job(&state, nucleus_core::JobState::Failed);
            let pending_record = nucleus_core::PendingToolCallV1 {
                version: 1,
                call: pending,
                state: nucleus_core::ToolCallState::Pending,
                created_at: "2026-09-06T21:00:00Z".into(),
                answered_at: None,
            };
            let id = state.request.id.to_string();
            let server = fake_mailbox(
                &socket,
                vec![
                    (format!("GET /v1/jobs/{id} "), json!(running)),
                    (format!("GET /v1/jobs/{id} "), json!(running)),
                    (
                        format!("GET /v1/jobs/{id}/tool-calls?"),
                        json!({"version":1,"jobId":id,"calls":[pending_record],"nextSequence":1}),
                    ),
                    (
                        format!("POST /v1/jobs/{id}/tool-calls/call-1/result "),
                        json!(pending_record),
                    ),
                    (format!("GET /v1/jobs/{id} "), json!(failed)),
                ],
            );
            let result = run_stage(
                &NucleusClient::new(&socket).unwrap(),
                &path,
                stage,
                stage_inputs.clone(),
                Some(Instant::now() + Duration::from_secs(5)),
            )
            .await
            .unwrap();
            assert_eq!(Some(result), state.accepted);
            let requests = server.await.unwrap();
            assert_eq!(requests[3], serde_json::to_value(response).unwrap());
            // With a terminal observation retained, no service or new attempt is needed.
            fs::remove_file(&socket).unwrap();
            assert_eq!(
                run_stage(
                    &NucleusClient::new(&socket).unwrap(),
                    &path,
                    stage,
                    stage_inputs,
                    None
                )
                .await
                .unwrap(),
                state.accepted.unwrap()
            );
        }
    }

    #[tokio::test]
    async fn legacy_runtime_pending_call_resumes_without_submitting_new_work() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_store(dir.path());
        let socket = dir.path().join("nucleus.sock");
        let mut state = load_or_create(&path, Stage::Brief, inputs()).unwrap();
        use_legacy_identity(&mut state);
        persist(&path, &state).unwrap();
        let pending = call(
            &state,
            "legacy-call",
            "submit_brief",
            &json!({"paragraph":"Supported fit, with location still unknown.","pursue":true}),
        );
        let running = runtime_job(&state, nucleus_core::JobState::WaitingOnRequester);
        let completed = runtime_job(&state, nucleus_core::JobState::Completed);
        let id = state.request.id.to_string();
        let pending_record = nucleus_core::PendingToolCallV1 {
            version: 1,
            call: pending,
            state: nucleus_core::ToolCallState::Pending,
            created_at: "2026-09-06T21:00:00Z".into(),
            answered_at: None,
        };
        let server = fake_mailbox(
            &socket,
            vec![
                (format!("GET /v1/jobs/{id} "), json!(running)),
                (format!("GET /v1/jobs/{id} "), json!(running)),
                (
                    format!("GET /v1/jobs/{id}/tool-calls?"),
                    json!({"version":1,"jobId":id,"calls":[pending_record],"nextSequence":1}),
                ),
                (
                    format!("POST /v1/jobs/{id}/tool-calls/legacy-call/result "),
                    json!(pending_record),
                ),
                (format!("GET /v1/jobs/{id} "), json!(completed)),
            ],
        );
        let accepted = run_stage(
            &NucleusClient::new(&socket).unwrap(),
            &path,
            Stage::Brief,
            inputs(),
            Some(Instant::now() + Duration::from_secs(5)),
        )
        .await
        .unwrap();
        let requests = server.await.unwrap();
        assert_eq!(requests[3]["resultSchemaId"], "job-packets.tool-result.v1");
        assert_eq!(requests[3]["requester"], json!(state.request.requester));
        let reloaded = load_or_create(&path, Stage::Brief, inputs()).unwrap();
        assert_eq!(reloaded.request, state.request);
        assert_eq!(reloaded.accepted, Some(accepted));
    }

    #[tokio::test]
    async fn runtime_completion_without_domain_commit_fails_and_never_resubmits() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_store(dir.path());
        let socket = dir.path().join("nucleus.sock");
        let state = load_or_create(&path, Stage::Brief, inputs()).unwrap();
        let completed = runtime_job(&state, nucleus_core::JobState::Completed);
        let id = state.request.id.to_string();
        let server = fake_mailbox(
            &socket,
            vec![
                (format!("GET /v1/jobs/{id} "), json!(completed)),
                (format!("GET /v1/jobs/{id} "), json!(completed)),
            ],
        );
        let error = run_stage(
            &NucleusClient::new(&socket).unwrap(),
            &path,
            Stage::Brief,
            inputs(),
            Some(Instant::now() + Duration::from_secs(5)),
        )
        .await
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("without an accepted domain result")
        );
        assert_eq!(server.await.unwrap().len(), 2);
    }

    #[test]
    fn strict_health_accepts_full_capacity_but_rejects_missing_or_inconsistent_proof() {
        let mut health: HealthResponseV1 = serde_json::from_value(json!({
            "version":1,"status":"ok","daemonVersion":"0.5.0","acceptingJobs":true,
            "checkedAt":"2026-09-06T21:00:00Z","supportedProtocolVersions":[1],
            "harness":{"harness":"codex","harnessVersion":"proved","adapterVersion":"1"},
            "capabilities":["exact-model","reasoning-effort","workspace-none","builtin-local-execution","builtin-web-search","dynamic-client-tools"],
            "authentication":{"codexHome":"/private/nucleus","configured":true,"authenticated":true},
            "execution":{"maxActiveJobs":8,"activeJobs":8,"availableSlots":0}
        })).unwrap();
        validate_health(&health, true).unwrap();
        health.execution.as_mut().unwrap().available_slots = 1;
        assert!(validate_health(&health, true).is_err());
        health.execution.as_mut().unwrap().available_slots = 0;
        health.capabilities.pop();
        assert!(validate_health(&health, true).is_err());
    }

    #[tokio::test]
    async fn deployment_readiness_requires_exact_hold_and_runtime_capabilities() {
        let health = json!({
            "version":1,"status":"maintenance","daemonVersion":"0.5.0","acceptingJobs":false,
            "checkedAt":"2026-09-06T21:00:00Z","supportedProtocolVersions":[1],
            "harness":{"harness":"codex","harnessVersion":"proved","adapterVersion":"1"},
            "harnessExecutable":"/fixture/codex",
            "capabilities":["exact-model","reasoning-effort","workspace-none","builtin-local-execution","builtin-web-search","dynamic-client-tools"],
            "authentication":{"codexHome":"/private/nucleus","configured":true,"authenticated":true},
            "execution":{"maxActiveJobs":8,"activeJobs":0,"availableSlots":8}
        });
        assert!(validate_health(&serde_json::from_value(health.clone()).unwrap(), true).is_err());
        for (owner, remove_capability, ready) in [
            ("deployment-1", false, true),
            ("other-owner", false, false),
            ("deployment-1", true, false),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let socket = dir.path().join("nucleus.sock");
            let mut observed_health = health.clone();
            if remove_capability {
                observed_health["capabilities"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            let server = fake_mailbox(
                &socket,
                vec![
                    ("GET /v1/health ".into(), observed_health),
                    (
                        "GET /v1/maintenance ".into(),
                        json!({"protocol_version":1,"holds":[owner],"drained":true,"nonterminal_jobs":0}),
                    ),
                ],
            );
            assert_eq!(
                check_deployment_readiness(&NucleusClient::new(&socket).unwrap(), "deployment-1")
                    .await
                    .is_ok(),
                ready
            );
            assert_eq!(server.await.unwrap().len(), 2);
        }
    }

    #[test]
    fn rendering_rejection_returns_feedback_without_committing_resume() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_store(dir.path());
        let mut state = load_or_create(&path, Stage::Resume, inputs()).unwrap();
        let payload = json!({"jackson_bullets":["Supported Jackson work"],"evidence":[{"bullet_index":0,"career_entry_ids":["story-1"]}]});
        let first = call(&state, "call-1", "submit_resume", &payload);
        let rejects =
            |_result: &StageResult| -> Result<()> { bail!("two pages; shorten Jackson bullets") };
        let response =
            bind_tool_result_validated(&path, &mut state, &first, Some(&rejects)).unwrap();
        assert!(response.is_error);
        assert!(response.result.get().contains("two pages"));
        assert!(state.accepted.is_none());
        let next = call(&state, "call-2", "submit_resume", &payload);
        let accepts = |_result: &StageResult| -> Result<()> { Ok(()) };
        assert!(
            !bind_tool_result_validated(&path, &mut state, &next, Some(&accepts))
                .unwrap()
                .is_error
        );
        assert!(state.accepted.is_some());
    }
    #[test]
    fn weaver_draft_has_no_project_write_authority() {
        for selection in [None, Some(1)] {
            let root = tempfile::tempdir().unwrap();
            let store = fixture_store(root.path());
            let mut input = writing_inputs();
            input.brief = None;
            input.prompt_selection = selection;
            let fixed = crate::projects::ProjectBullets {
                cell: vec!["Built Cell".into()],
                wrought: vec!["Built Wrought".into()],
            };
            input.fixed_projects = Some(fixed.clone());
            let mut state = load_or_create(&store, Stage::Draft, input.clone()).unwrap();
            let retained = retained_request(&store, "packet-1", Stage::Draft)
                .unwrap()
                .unwrap();
            assert_eq!(retained, state.request);
            state = load_or_create(&store, Stage::Draft, input).unwrap();
            assert_eq!(state.request, retained);
            assert!(fixed_project_tools(&state));
            assert_eq!(
                state.request.invocation.toolset.as_ref().unwrap().version,
                if selection.is_some() { 4 } else { 2 }
            );
            assert!(!state.request.instructions.contains("<bazaar:"));
            assert!(!state.request.invocation.builtin_tools.local_execution);
            assert_eq!(
                state.request.invocation.workspace_access,
                WorkspaceAccess::None
            );
            let definitions = fixed_draft_definitions().unwrap();
            let tool = definitions
                .tools
                .iter()
                .find(|t| t.name == "submit_draft")
                .unwrap();
            let schema: Value = serde_json::from_str(tool.input_schema.get()).unwrap();
            assert!(
                schema["properties"]["resume"]["properties"]
                    .get("projects")
                    .is_none()
            );
            let mut payload = single_draft_payload();
            payload["resume"]
                .as_object_mut()
                .unwrap()
                .remove("projects");
            let validate = |result: &StageResult| retain_fixture_draft(&store, result);
            let mut invalid = payload.clone();
            invalid["resume"]["projects"] = json!(null);
            let bad_call = call(&state, "bad", "submit_draft", &invalid);
            assert!(execute_draft(&mut state, &bad_call, Some(&validate)).is_err());
            let submission = call(&state, "good", "submit_draft", &payload);
            execute_draft(&mut state, &submission, Some(&validate)).unwrap();
            let retained: Resume = store
                .content("packet-1", "resume-content")
                .unwrap()
                .unwrap();
            assert_eq!(retained.project_bullets, Some(fixed));
            assert!(retained.projects.is_none());
        }
    }
}
