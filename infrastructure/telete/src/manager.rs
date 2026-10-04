//! Independent serial CI orchestration, with intent retained before effects.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::deployment::{
    self, DeploymentResult, DeploymentState, InstructionState, Preparation, PrepareError,
};
use crate::git;
use crate::inventory::Inventory;
use crate::model::{CommitId, JobId};
use crate::paths::Paths;
use crate::providers::{self, Observation, Prompts, Selection};
use crate::signing::{self, SigningPolicy};
use crate::store::{
    self, Cleanup, Config, DeploymentRequest, Job, Notification, Operation, Phase, RepairAttempt,
    SendAttempt, Store, Validation,
};
use crate::validation::{self, ValidationReport, ValidationState};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
// These independent caller choices are frozen together at admission.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct SubmitOptions {
    pub request_id: Option<String>,
    pub run_tests: bool,
    pub deploy: Vec<String>,
    pub repair: bool,
    pub notify: bool,
    pub no_deploy: bool,
}

impl Default for SubmitOptions {
    fn default() -> Self {
        Self {
            request_id: None,
            run_tests: false,
            deploy: Vec::new(),
            repair: true,
            notify: true,
            no_deploy: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    pub primary_attempts: u64,
    pub escalation_attempts: u64,
    pub primary_model: String,
    pub primary_reasoning: nucleus_core::ReasoningEffort,
    pub escalation_model: String,
    pub escalation_reasoning: nucleus_core::ReasoningEffort,
    pub model_timeout_seconds: u64,
    // Preserve the domain name in the journal alongside repair and provider policy.
    #[allow(clippy::struct_field_names)]
    pub signing_policy: SigningPolicy,
    pub providers: Selection,
    pub prompts: Option<Prompts>,
}

fn check_identity(value: &str, label: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control),
        "{label} must have 1 to 256 non-control characters"
    );
    Ok(())
}

fn configured(paths: &Paths) -> Result<(Store, Config)> {
    let store = Store::open(&paths.root, false)?;
    let config = store.config()?;
    ensure!(
        git::common(paths, &config.repository)? == config.common_git_dir,
        "configured Git repository identity changed"
    );
    Ok((store, config))
}

pub(crate) fn init(paths: &Paths, repo: &Path, accepted_baseline: &str) -> Result<Value> {
    let _admission = store::lock(&paths.root.join("admission.lock"), true)?;
    let store = Store::open(&paths.root, true)?;
    let repository = git::repository(paths, repo)?;
    let baseline = git::commit(paths, &repository, accepted_baseline)?;
    let config = Config {
        common_git_dir: git::common(paths, &repository)?,
        repository,
        accepted_baseline: baseline.clone(),
    };
    if let Some(previous) = store.get::<Config>("config")? {
        ensure!(
            previous == config,
            "initialization cannot replace Telete's repository or baseline"
        );
        ensure!(
            git::ref_value(paths, &config.repository, git::ACCEPTED)?.as_ref() == Some(&baseline),
            "accepted history already advanced; initialization cannot rewind it"
        );
        return Ok(
            json!({"state":"initialized","paused":store.paused()?,"accepted":baseline,"config":config}),
        );
    }
    git::advance(paths, &config.repository, git::ACCEPTED, None, &baseline)?;
    store.transaction(|| {
        store.set("config", &config)?;
        store.set("paused", &true)
    })?;
    Ok(json!({"state":"initialized","paused":true,"accepted":baseline,"config":config}))
}

pub(crate) fn submit(
    paths: &Paths,
    repo: &Path,
    revision: &str,
    mut options: SubmitOptions,
) -> Result<Value> {
    let (store, config) = configured(paths)?;
    ensure!(
        git::common(paths, repo)? == config.common_git_dir,
        "submit from a worktree of Telete's configured repository"
    );
    let input = git::commit(paths, repo, revision)?;
    options.deploy.sort();
    options.deploy.dedup();
    for product in &options.deploy {
        crate::model::ProductId::new(product)?;
    }
    ensure!(
        !options.deploy.iter().any(|product| product == "telete"),
        "Telete installation is separate; use telete install explicitly"
    );
    ensure!(
        !options.no_deploy || options.deploy.is_empty(),
        "--no-deploy conflicts with explicit deployment products"
    );
    let request_id = options
        .request_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    check_identity(&request_id, "submission request identity")?;
    options.request_id = Some(request_id.clone());
    let _admission = store::lock(&paths.root.join("admission.lock"), true)?;
    if let Some(previous) = store.by_request(&request_id)? {
        ensure!(
            previous.input == input && previous.options == options,
            "submission request identity belongs to different inputs"
        );
        return previous.output();
    }
    paths.require_capacity()?;
    let signing_policy = signing::selected(paths)?;
    signing::preflight(&signing_policy)?;
    let (providers, prompts) = providers::select(options.repair, options.notify)?;
    let policy = Policy {
        primary_attempts: 3,
        escalation_attempts: 1,
        primary_model: "gpt-5.6-terra".into(),
        primary_reasoning: nucleus_core::ReasoningEffort::Medium,
        escalation_model: "gpt-5.6-sol".into(),
        escalation_reasoning: nucleus_core::ReasoningEffort::High,
        model_timeout_seconds: 600,
        signing_policy,
        providers,
        prompts,
    };
    let id = JobId::new(uuid::Uuid::now_v7().to_string())?;
    let job = new_job(id, request_id, input, options, policy);
    git::advance(
        paths,
        repo,
        &git::private_ref(&job.id.0, "input")?,
        None,
        &job.input,
    )?;
    let admitted = store.transaction(|| store.insert(&job))?;
    admitted.output()
}

fn new_job(
    id: JobId,
    request_id: String,
    input: CommitId,
    options: SubmitOptions,
    policy: Policy,
) -> Job {
    Job {
        id,
        sequence: 0,
        revision: 0,
        request_id,
        phase: Phase::Queued,
        cancel_requested: false,
        created: store::now(),
        updated: store::now(),
        input,
        options,
        policy,
        base: None,
        candidate: None,
        accepted: false,
        attempts: Vec::new(),
        validations: Vec::new(),
        preparation: None,
        preparation_generation: 0,
        production_diagnostics: None,
        deployment_request: None,
        deployment_result: None,
        operation: None,
        outcome: None,
        outcome_message: None,
        stopped_phase: None,
        unresolved: false,
        model_unresolved: false,
        waiting_reason: None,
        last_error: None,
        notification: None,
        outcome_generation: 1,
        cleanup: Cleanup::default(),
    }
}

pub(crate) fn status(paths: &Paths, id: Option<&str>) -> Result<Value> {
    let (store, config) = configured(paths)?;
    if let Some(id) = id {
        return store.job(id)?.output();
    }
    let active = store.active()?.map(|job| job.output()).transpose()?;
    let queue = store
        .jobs()?
        .into_iter()
        .filter(|job| job.phase == Phase::Queued)
        .map(|job| job.output())
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"schema_version":1,"version":env!("CARGO_PKG_VERSION"),"paused":store.paused()?,
        "config":config,"accepted":git::ref_value(paths,&config.repository,git::ACCEPTED)?,"active":active,"queue":queue,"maintenance_owners":store.holds()?}),
    )
}

pub(crate) async fn wait(paths: &Paths, id: &str, timeout_seconds: u64) -> Result<Value> {
    ensure!(timeout_seconds > 0, "wait timeout must be positive");
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    loop {
        let (store, _) = configured(paths)?;
        let job = store.job(id)?;
        if job.phase.terminal() || job.phase == Phase::Blocked {
            return Ok(json!({"observation":"terminal","job":job.output()?}));
        }
        if Instant::now() >= deadline {
            return Ok(json!({"observation":"timeout","job":job.output()?}));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub(crate) fn pause(paths: &Paths) -> Result<Value> {
    let _admission = store::lock(&paths.root.join("admission.lock"), true)?;
    let (store, _) = configured(paths)?;
    store.set("paused", &true)?;
    status(paths, None)
}

pub(crate) fn resume(paths: &Paths) -> Result<Value> {
    let _admission = store::lock(&paths.root.join("admission.lock"), true)?;
    let (store, _) = configured(paths)?;
    if let Some(active) = store.active()? {
        ensure!(
            active.phase != Phase::Blocked && !active.unresolved,
            "recover the unresolved Telete job before resuming"
        );
    }
    store.set("paused", &false)?;
    status(paths, None)
}

pub(crate) fn cancel(paths: &Paths, id: &str) -> Result<Value> {
    let _admission = store::lock(&paths.root.join("admission.lock"), true)?;
    let (store, _) = configured(paths)?;
    store.transaction(|| {
        let mut job = store.job(id)?;
        store.cancel(id)?;
        if job.phase == Phase::Queued {
            job.cancel_requested = true;
            job.phase = Phase::Cancelled;
            job.outcome = Some(Phase::Cancelled);
            job.outcome_message = Some("Cancelled before queue admission".into());
            store.save(&mut job)?;
        }
        Ok(())
    })?;
    store.job(id)?.output()
}

pub(crate) fn maintenance_hold(paths: &Paths, owner: &str) -> Result<Value> {
    maintenance(paths, Some(owner), "hold")
}
pub(crate) fn maintenance_status(paths: &Paths) -> Result<Value> {
    maintenance(paths, None, "status")
}
pub(crate) fn maintenance_release(paths: &Paths, owner: &str) -> Result<Value> {
    maintenance(paths, Some(owner), "release")
}

fn maintenance(paths: &Paths, owner: Option<&str>, action: &str) -> Result<Value> {
    if let Some(owner) = owner {
        check_identity(owner, "maintenance owner")?;
    }
    let _admission = store::lock(&paths.root.join("admission.lock"), true)?;
    let (store, _) = configured(paths)?;
    if let Some(owner) = owner {
        if action == "hold" {
            store.hold(owner)?;
        } else if action == "release" {
            store.release(owner)?;
        }
    }
    let active = store.active()?;
    let owners = store.holds()?;
    let drained = active.is_none()
        && store
            .jobs()?
            .iter()
            .all(|job| !job.unresolved && !job.model_unresolved);
    Ok(json!({"held":!owners.is_empty(),"drained":drained,
        "owner_held":owner.map(|owner| owners.iter().any(|held| held == owner)),"owners":owners,"active_job":active.map(|job|job.id)}))
}

pub(crate) fn require_quiescent(paths: &Paths) -> Result<()> {
    let database = paths.root.join("queue.sqlite3");
    if !database.exists() {
        return Ok(());
    }
    let store = Store::open(&paths.root, false)?;
    ensure!(
        store.paused()?,
        "pause Telete before installation or signing maintenance"
    );
    ensure!(
        store
            .jobs()?
            .iter()
            .all(|job| job.phase.terminal() && !job.unresolved && !job.model_unresolved),
        "Telete has queued, active or unresolved work; settle it before maintenance"
    );
    Ok(())
}

// Service stop holds broker and deployment ownership for this recovery case.
// Installation and signing retain the stricter quiescent queue requirement.
pub(crate) fn require_service_stoppable(paths: &Paths) -> Result<()> {
    if require_quiescent(paths).is_ok() {
        return Ok(());
    }
    let (store, config) = configured(paths)?;
    ensure!(store.paused()?, "pause Telete before stopping its service");
    let job = store.active()?.context("Telete has unsettled queued work")?;
    ensure!(
        job.phase == Phase::Blocked
            && job.stopped_phase == Some(Phase::Deploying)
            && job.accepted
            && job.operation.is_none()
            && !job.model_unresolved
            && (!job.options.notify
                || job
                    .notification
                    .as_ref()
                    .is_some_and(|notice| notice.accepted && !notice.uncertain))
            && store.jobs()?.iter().all(|other| {
                other.id == job.id
                    || (other.phase.terminal() && !other.unresolved && !other.model_unresolved)
            }),
        "service stop requires settled work or one blocked completed deployment"
    );
    let request = job
        .deployment_request
        .as_ref()
        .context("blocked deployment has no saved request")?;
    ensure!(
        job.candidate.as_ref() == Some(&request.source)
            && git::ref_value(paths, &config.repository, git::ACCEPTED)?.as_ref()
                == Some(&request.source)
            && !paths.root.join("deployments/active.json").try_exists()?,
        "blocked deployment source or execution ownership is unsettled"
    );
    let result = deployment::observe(paths, &request.id)?
        .context("blocked deployment has no terminal receipt")?;
    correlate_deployment(&result, request)?;
    ensure!(
        result.state == DeploymentState::Succeeded,
        "service stop requires the blocked deployment to have completed successfully"
    );
    Ok(())
}

// Keep the authoritative recovery decisions together before the state transition.
#[allow(clippy::too_many_lines)]
pub(crate) async fn recover(paths: &Paths, id: &str) -> Result<Value> {
    let _worker = store::lock(&paths.root.join("worker.lock"), false)?;
    let (store, config) = configured(paths)?;
    let mut job = store.job(id)?;
    ensure!(
        job.phase == Phase::Blocked,
        "only a blocked Telete job can be recovered"
    );
    if job
        .notification
        .as_ref()
        .is_some_and(|notice| notice.uncertain)
        && !job.unresolved
    {
        bail!("Email acceptance remains uncertain; recovery cannot send a replacement message");
    }
    let phase = job
        .stopped_phase
        .context("blocked job has no retained operation phase")?;
    let settled_cancel = phase == Phase::Checking
        && job.cancel_requested
        && job.operation.is_none()
        && job
            .validations
            .last()
            .is_some_and(|validation| validation.report.state == ValidationState::Stale);
    match phase {
        Phase::Checking if settled_cancel => {}
        Phase::RepairWait => {
            let attempt = job
                .attempts
                .last_mut()
                .context("blocked repair has no saved request")?;
            let client = providers::client(&job.policy.providers)?;
            match providers::get_job(&client, &attempt.request.id).await {
                Ok(view) => {
                    if matches!(
                        providers::correlate(&view, &attempt.request)?,
                        Observation::Failed { lost: true, .. }
                    ) {
                        bail!(
                            "Nucleus still reports lost execution; the request cannot be replaced"
                        );
                    }
                    attempt.transport_failures = 0;
                    job.model_unresolved = true;
                }
                Err(error) if providers::absent(&error) => {
                    attempt.transport_failures = 0;
                    job.model_unresolved = false;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Phase::Checking | Phase::Preparing => {
            let operation = job
                .operation
                .as_ref()
                .context("blocked operation has no retained intent")?;
            if phase == Phase::Checking && !operation.result.is_file() {
                let provider_receipt = validation::receipt_path(
                    paths,
                    job.base.as_ref().context("fixed base absent")?,
                    job.candidate.as_ref().context("candidate absent")?,
                    job.options.run_tests,
                    true,
                );
                if provider_receipt.is_file() {
                    let report: ValidationReport =
                        serde_json::from_slice(&fs::read(provider_receipt)?)?;
                    store::atomic_json(&operation.result, &report)?;
                }
            }
            if phase == Phase::Preparing && !operation.result.is_file() {
                let output = paths
                    .job(&job.id.0)
                    .join(format!("production-{}", job.preparation_generation))
                    .join("result.json");
                if output.is_file() {
                    let prepared: Preparation = serde_json::from_slice(&fs::read(output)?)?;
                    let inspector = Worker {
                        paths,
                        store: Store::open(&paths.root, false)?,
                        config: config.clone(),
                    };
                    let products = inspector.production_products(&job)?;
                    correlate_preparation(
                        &prepared,
                        job.candidate.as_ref().context("candidate absent")?,
                        &products,
                        &job.policy.signing_policy,
                        paths,
                    )?;
                    store::atomic_json(
                        &operation.result,
                        &PreparationOutcome {
                            prepared: Some(prepared),
                            compilation_failure: None,
                        },
                    )?;
                }
            }
            ensure!(
                operation.result.is_file(),
                "operation has no authoritative terminal receipt; it cannot be repeated"
            );
        }
        Phase::Deploying => {
            let request = job
                .deployment_request
                .as_ref()
                .context("blocked deployment has no saved request")?;
            let observed = deployment::observe(paths, &request.id)?
                .context("deployment admission remains unknown")?;
            correlate_deployment(&observed, request)?;
            ensure!(
                observed.state != DeploymentState::Running
                    && (observed.state != DeploymentState::Interrupted || observed.acknowledged),
                "deployment has interrupted or uncertain instruction effects; inspect its owning operation"
            );
        }
        Phase::Accepting | Phase::Applying | Phase::Autofixing | Phase::Integrating => {}
        _ => bail!("recovery has no authoritative continuation for this phase"),
    }
    job.phase = phase;
    job.outcome_generation += 1;
    job.outcome = None;
    job.outcome_message = None;
    job.notification = None;
    job.unresolved = false;
    job.last_error = None;
    job.waiting_reason = None;
    store.save(&mut job)?;
    let mut worker = Worker {
        paths,
        store,
        config,
    };
    if settled_cancel {
        worker.finish(
            &mut job,
            Phase::Cancelled,
            "Cancelled after stale validation settled".into(),
            false,
        )?;
    } else {
        worker.tick().await?;
    }
    worker.store.job(id)?.output()
}

pub(crate) async fn worker_once(paths: &Paths) -> Result<Value> {
    let _worker = store::lock(&paths.root.join("worker.lock"), false)?;
    let (store, config) = configured(paths)?;
    let mut worker = Worker {
        paths,
        store,
        config,
    };
    worker.tick().await
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparationAcknowledgement {
    schema: u32,
    job: JobId,
    operation: Operation,
    acknowledged: u64,
}

// Keep the identity checks and durable abandonment transition in one place.
#[allow(clippy::too_many_lines)]
pub(crate) fn acknowledge_preparation(paths: &Paths, id: &str) -> Result<Value> {
    let _worker = store::lock(&paths.root.join("worker.lock"), false)?;
    let _admission = store::lock(&paths.root.join("admission.lock"), true)?;
    let (store, config) = configured(paths)?;
    ensure!(
        store.paused()?,
        "pause Telete before acknowledging preparation"
    );
    let mut job = store.job(id)?;
    let directory = paths.job(&job.id.0);
    let name = format!("production-{}", job.preparation_generation);
    let record = directory.join(format!("{name}.acknowledged.json"));
    let previous: Option<PreparationAcknowledgement> = if record.try_exists()? {
        Some(serde_json::from_slice(&fs::read(&record)?)?)
    } else {
        None
    };
    if let Some(acknowledgement) = &previous {
        ensure!(
            acknowledgement.schema == 1
                && acknowledgement.job == job.id
                && acknowledgement.operation.phase == Phase::Preparing
                && acknowledgement.operation.name == name
                && Some(&acknowledgement.operation.candidate) == job.candidate.as_ref()
                && acknowledgement.operation.result == directory.join(format!("{name}.json")),
            "preparation acknowledgement names another operation"
        );
        if job.phase == Phase::Failed && !job.unresolved && job.operation.is_none() {
            let worker = Worker {
                paths,
                store,
                config,
            };
            worker.cleanup(&mut job)?;
            return job.output();
        }
    }
    ensure!(
        job.phase == Phase::Blocked
            && job.stopped_phase == Some(Phase::Preparing)
            && job.outcome == Some(Phase::Failed)
            && job.unresolved
            && !job.model_unresolved
            && !job.accepted
            && job.preparation.is_none()
            && job.deployment_request.is_none()
            && job.deployment_result.is_none()
            && (!job.options.notify
                || job
                    .notification
                    .as_ref()
                    .is_some_and(|notice| { notice.accepted && !notice.uncertain })),
        "only an unaccepted blocked preparation can be acknowledged"
    );
    let operation = job
        .operation
        .as_ref()
        .context("preparation intent absent")?;
    ensure!(
        operation.phase == Phase::Preparing
            && operation.name == name
            && Some(&operation.candidate) == job.candidate.as_ref()
            && operation.result == directory.join(format!("{name}.json")),
        "preparation intent does not match the job"
    );
    ensure!(
        !operation.result.try_exists()?
            && !directory.join(&name).join("result.json").try_exists()?,
        "preparation has a terminal receipt; use recover"
    );
    ensure!(
        git::ref_value(paths, &config.repository, git::ACCEPTED)?.as_ref() == job.base.as_ref(),
        "accepted source changed after preparation"
    );
    crate::broker::require_settled(paths)?;
    if previous.is_none() {
        store::atomic_json(
            &record,
            &PreparationAcknowledgement {
                schema: 1,
                job: job.id.clone(),
                operation: operation.clone(),
                acknowledged: store::now(),
            },
        )?;
    } else {
        ensure!(
            serde_json::to_value(&previous.context("acknowledgement absent")?.operation)?
                == serde_json::to_value(operation)?,
            "retained preparation intent changed"
        );
    }
    job.operation = None;
    job.unresolved = false;
    job.phase = Phase::Failed;
    store.save(&mut job)?;
    let worker = Worker {
        paths,
        store,
        config,
    };
    worker.cleanup(&mut job)?;
    job.output()
}

pub(crate) async fn worker(paths: &Paths) -> Result<()> {
    let _worker = store::lock(&paths.root.join("worker.lock"), false)?;
    let (store, config) = configured(paths)?;
    let mut worker = Worker {
        paths,
        store,
        config,
    };
    loop {
        worker.tick().await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

struct Worker<'a> {
    paths: &'a Paths,
    store: Store,
    config: Config,
}

#[derive(Serialize, Deserialize)]
struct PreparationOutcome {
    prepared: Option<Preparation>,
    compilation_failure: Option<String>,
}

impl Worker<'_> {
    fn directory(&self, job: &Job) -> Result<PathBuf> {
        let directory = self.paths.job(&job.id.0);
        store::private_directory(&directory)?;
        Ok(directory)
    }

    fn worktree(&self, job: &Job) -> PathBuf {
        self.paths.job(&job.id.0).join("worktree")
    }
    fn candidate(job: &Job) -> Result<&CommitId> {
        job.candidate
            .as_ref()
            .context("active job has no candidate")
    }
    fn base(job: &Job) -> Result<&CommitId> {
        job.base.as_ref().context("active job has no fixed base")
    }

    fn save(&self, job: &mut Job, phase: Phase) -> Result<()> {
        job.phase = phase;
        self.store.save(job)
    }

    fn claim(&self) -> Result<Option<Job>> {
        self.store.transaction(|| {
            if let Some(job) = self.store.active()? {
                return Ok(Some(job));
            }
            if self.store.paused()? || !self.store.holds()?.is_empty() {
                return Ok(None);
            }
            let Some(mut job) = self.store.queued()? else {
                return Ok(None);
            };
            self.paths.require_capacity()?;
            job.base = Some(git::commit(
                self.paths,
                &self.config.repository,
                git::ACCEPTED,
            )?);
            self.save(&mut job, Phase::Integrating)?;
            Ok(Some(job))
        })
    }

    async fn tick(&mut self) -> Result<Value> {
        self.paths.require_capacity()?;
        for mut job in self.store.jobs()? {
            if (job.phase.terminal() || job.outcome.is_some())
                && !job.unresolved
                && !job.model_unresolved
            {
                self.cleanup(&mut job)?;
            }
        }
        let Some(mut job) = self.claim()? else {
            return Ok(json!({"state":"idle","paused":self.store.paused()?}));
        };
        if job.phase != Phase::Blocked
            && let Err(error) = self.step(&mut job).await
        {
            let message = format!("{error:#}");
            job.last_error = Some(message.clone());
            if job.phase == Phase::Notifying {
                job.phase = Phase::Blocked;
                self.store.set("paused", &true)?;
                self.store.save(&mut job)?;
            } else {
                let unresolved = job.model_unresolved
                    || job.operation.is_some()
                    || matches!(job.phase, Phase::Deploying);
                self.finish(&mut job, Phase::Failed, message, unresolved)?;
            }
        }
        self.store.job(&job.id.0)?.output()
    }

    async fn step(&mut self, job: &mut Job) -> Result<()> {
        job.cancel_requested = self.store.job(&job.id.0)?.cancel_requested;
        if job.cancel_requested
            && job.operation.is_none()
            && !matches!(
                job.phase,
                Phase::RepairWait | Phase::Deploying | Phase::Notifying | Phase::Blocked
            )
        {
            return self.finish(
                job,
                Phase::Cancelled,
                "Cancelled before the next operation".into(),
                false,
            );
        }
        match job.phase {
            Phase::Integrating => self.integrating(job),
            Phase::Checking => self.checking(job),
            Phase::Autofixing => self.autofixing(job),
            Phase::RepairPrepare => self.repair_prepare(job),
            Phase::RepairWait => self.repair_wait(job).await,
            Phase::Applying => self.applying(job),
            Phase::Preparing => self.preparing(job),
            Phase::Accepting => self.accepting(job),
            Phase::Deploying => self.deploying(job),
            Phase::Notifying => self.notifying(job).await,
            Phase::Blocked => Ok(()),
            _ => bail!("unexpected active Telete phase: {:?}", job.phase),
        }
    }

    fn assert_policy(&self, job: &Job) -> Result<()> {
        ensure!(
            signing::selected(self.paths)? == job.policy.signing_policy,
            "host signing selection changed after admission"
        );
        Ok(())
    }

    fn finish(
        &self,
        job: &mut Job,
        outcome: Phase,
        message: String,
        unresolved: bool,
    ) -> Result<()> {
        ensure!(outcome.terminal(), "invalid terminal CI outcome");
        job.stopped_phase = Some(job.phase);
        job.outcome = Some(outcome);
        job.outcome_message = Some(message);
        job.unresolved = unresolved;
        if !matches!(outcome, Phase::Succeeded | Phase::AlreadyIncluded) {
            self.store.set("paused", &true)?;
        }
        if job.options.notify && job.notification.is_none() {
            let (subject, body) = render_notification(job);
            job.notification = Some(Notification {
                message: email::api::Message {
                    subject,
                    body,
                    idempotency_key: Some(format!(
                        "telete/{}/outcome/{}",
                        job.id, job.outcome_generation
                    )),
                },
                created: store::now(),
                attempts: Vec::new(),
                accepted: false,
                uncertain: false,
            });
        }
        let phase = if job.options.notify {
            Phase::Notifying
        } else if unresolved {
            Phase::Blocked
        } else {
            outcome
        };
        self.save(job, phase)?;
        self.cleanup(job)
    }

    fn cleanup(&self, job: &mut Job) -> Result<()> {
        if job.outcome.is_none()
            || job.unresolved
            || job.model_unresolved
            || job.cleanup.state == "removed"
        {
            return Ok(());
        }
        let worktree = self.worktree(job);
        if !job.cleanup.recorded {
            match git::registration(self.paths, &self.config.repository, &worktree) {
                Ok(registration) => {
                    job.cleanup.registration = registration;
                    job.cleanup.recorded = true;
                }
                Err(error) => {
                    job.cleanup.state = "failed".into();
                    job.cleanup.error = Some(format!("{error:#}"));
                    job.cleanup.updated = store::now();
                    return self.store.save(job);
                }
            }
            self.store.save(job)?;
        }
        match git::remove_worktree(
            self.paths,
            &self.config.repository,
            &worktree,
            job.cleanup.registration.as_deref(),
        ) {
            Ok(()) => {
                job.cleanup.state = "removed".into();
                job.cleanup.error = None;
            }
            Err(error) => {
                job.cleanup.state = "failed".into();
                job.cleanup.error = Some(format!("{error:#}"));
            }
        }
        job.cleanup.updated = store::now();
        self.store.save(job)
    }

    fn integrating(&self, job: &mut Job) -> Result<()> {
        let base = Self::base(job)?.clone();
        if git::ancestor(self.paths, &self.config.repository, &job.input, &base)? {
            job.candidate = Some(base);
            return self.finish(
                job,
                Phase::AlreadyIncluded,
                "The submitted changes are already in accepted history".into(),
                false,
            );
        }
        let worktree = self.worktree(job);
        git::ensure_worktree(self.paths, &self.config.repository, &worktree, &base)?;
        let merge = git::merge(self.paths, &worktree, &job.input)?;
        store::atomic_bytes(
            &self.directory(job)?.join("merge.log"),
            format!("{}{}", merge.stdout, merge.stderr).as_bytes(),
        )?;
        if !merge.success() {
            return self.finish(
                job,
                Phase::Failed,
                "Integration failed. Merge conflicts require source changes".into(),
                false,
            );
        }
        let tree = git::value(self.paths, &worktree, &["write-tree"])?;
        let candidate = git::commit_tree(
            self.paths,
            &self.config.repository,
            &tree,
            &[base, job.input.clone()],
            &job.id.0,
            job.created,
            "Integrate Telete submission",
        )?;
        job.candidate = Some(candidate.clone());
        self.store.save(job)?;
        git::advance(
            self.paths,
            &self.config.repository,
            &git::private_ref(&job.id.0, "candidate")?,
            None,
            &candidate,
        )?;
        git::ensure_worktree(self.paths, &self.config.repository, &worktree, &candidate)?;
        self.save(job, Phase::Checking)
    }

    fn begin_operation(&self, job: &mut Job, name: String, result: PathBuf) -> Result<bool> {
        let candidate = Self::candidate(job)?.clone();
        if let Some(operation) = &job.operation {
            ensure!(
                operation.name == name
                    && operation.candidate == candidate
                    && operation.phase == job.phase
                    && operation.result == result,
                "retained operation does not match current stage"
            );
            ensure!(
                result.is_file(),
                "interrupted operation has no terminal receipt; recovery is required"
            );
            return Ok(false);
        }
        ensure!(
            !result.exists(),
            "unattributed operation receipt already exists"
        );
        job.operation = Some(Operation {
            phase: job.phase,
            name,
            candidate,
            result,
            started: store::now(),
        });
        self.store.save(job)?;
        Ok(true)
    }

    // Receipt correlation, cancellation, and the next phase form one durable decision.
    #[allow(clippy::too_many_lines)]
    fn checking(&self, job: &mut Job) -> Result<()> {
        self.assert_policy(job)?;
        let candidate = Self::candidate(job)?.clone();
        let base = Self::base(job)?.clone();
        let worktree = self.worktree(job);
        let ordinal = job.validations.len();
        let directory = self.directory(job)?;
        let receipt = directory.join(format!("validation-{ordinal}.json"));
        let fresh = self.begin_operation(job, format!("validation-{ordinal}"), receipt.clone())?;
        if fresh {
            git::ensure_worktree(self.paths, &self.config.repository, &worktree, &candidate)?;
            let report = validation::run_candidate(
                self.paths,
                &worktree,
                &base,
                &candidate,
                job.options.run_tests,
                true,
            )?;
            store::atomic_json(&receipt, &report)?;
        }
        let report: ValidationReport = serde_json::from_slice(&fs::read(&receipt)?)?;
        validation::verify_report(&report, &base, &candidate, job.options.run_tests, true)?;
        ensure!(
            report.schema == 1 && report.base == base && report.candidate == candidate,
            "validation receipt names another source"
        );
        if matches!(
            report.state,
            ValidationState::Passed | ValidationState::Autofix
        ) {
            ensure!(
                report.tests_run == job.options.run_tests,
                "validation receipt has another test policy"
            );
            ensure!(
                report.release_builds_deferred,
                "validation receipt has another release policy"
            );
        }
        job.cancel_requested = self.store.job(&job.id.0)?.cancel_requested;
        if job.cancel_requested {
            let diagnostics = directory.join(format!("validation-{ordinal}.log"));
            store::atomic_bytes(&diagnostics, report.diagnostics.as_bytes())?;
            job.validations.push(Validation {
                report,
                diagnostics,
                stamp: store::now(),
                patch_content: None,
                fixed_candidate: None,
            });
            job.operation = None;
            return self.finish(
                job,
                Phase::Cancelled,
                "Cancelled after validation settled".into(),
                false,
            );
        }
        git::clean_candidate(self.paths, &worktree, &candidate)?;
        let inventory = Inventory::load(&worktree)?;
        let selected = inventory.select(&report.products)?;
        let names = selected
            .iter()
            .map(|product| product.id.to_string())
            .collect::<BTreeSet<_>>();
        ensure!(
            names.len() == report.products.len()
                && names == report.products.iter().cloned().collect(),
            "validation selection is not a canonical product scope"
        );
        let diagnostics = directory.join(format!("validation-{ordinal}.log"));
        store::atomic_bytes(&diagnostics, report.diagnostics.as_bytes())?;
        let patch_content = if report.state == ValidationState::Autofix {
            let path = report
                .autofix_patch
                .as_ref()
                .context("autofix receipt has no patch")?;
            ensure!(
                path.is_absolute() && path.starts_with(&self.paths.root),
                "autofix patch is outside Telete state"
            );
            let raw = fs::read_to_string(path)?;
            ensure!(!raw.is_empty(), "autofix patch is empty");
            Some(raw)
        } else {
            None
        };
        job.validations.push(Validation {
            report: report.clone(),
            diagnostics,
            stamp: store::now(),
            patch_content,
            fixed_candidate: None,
        });
        job.operation = None;
        job.production_diagnostics = None;
        job.cancel_requested = self.store.job(&job.id.0)?.cancel_requested;
        if job.cancel_requested {
            return self.finish(
                job,
                Phase::Cancelled,
                "Cancelled after validation settled".into(),
                false,
            );
        }
        match report.state {
            ValidationState::Passed => self.save(job, Phase::Preparing),
            ValidationState::Autofix => self.save(job, Phase::Autofixing),
            ValidationState::Failed => self.save(job, Phase::RepairPrepare),
            ValidationState::Stale => self.finish(
                job,
                Phase::Failed,
                "Validation became stale; fresh validation requires a new submission".into(),
                true,
            ),
        }
    }

    fn autofixing(&self, job: &mut Job) -> Result<()> {
        self.assert_policy(job)?;
        let validation = job
            .validations
            .last()
            .context("autofix has no validation")?;
        let parent = validation.report.candidate.clone();
        ensure!(
            validation.report.state == ValidationState::Autofix && Self::candidate(job)? == &parent,
            "autofix names another candidate"
        );
        let raw = validation
            .patch_content
            .as_ref()
            .context("autofix patch was not retained")?;
        let path = validation
            .report
            .autofix_patch
            .as_ref()
            .context("autofix path absent")?;
        ensure!(
            fs::read_to_string(path)? == *raw,
            "retained autofix patch changed"
        );
        let tree = git::patch_tree(
            self.paths,
            &self.config.repository,
            &parent,
            raw,
            &self.directory(job)?.join("autofix.index"),
        )?;
        ensure!(
            tree != git::value(
                self.paths,
                &self.config.repository,
                &["rev-parse", &format!("{parent}^{{tree}}")]
            )?,
            "autofix made no source changes"
        );
        let candidate = git::commit_tree(
            self.paths,
            &self.config.repository,
            &tree,
            std::slice::from_ref(&parent),
            &job.id.0,
            validation.stamp,
            "Apply deterministic Telete fixes",
        )?;
        if let Some(recorded) = &validation.fixed_candidate {
            ensure!(recorded == &candidate, "autofix candidate changed");
        }
        job.validations
            .last_mut()
            .context("autofix record absent")?
            .fixed_candidate = Some(candidate.clone());
        self.store.save(job)?;
        git::advance(
            self.paths,
            &self.config.repository,
            &git::private_ref(&job.id.0, "candidate")?,
            Some(&parent),
            &candidate,
        )?;
        job.candidate = Some(candidate.clone());
        git::ensure_worktree(
            self.paths,
            &self.config.repository,
            &self.worktree(job),
            &candidate,
        )?;
        self.save(job, Phase::Checking)
    }

    // Freeze the complete request before recording its single charged invocation.
    #[allow(clippy::too_many_lines)]
    fn repair_prepare(&self, job: &mut Job) -> Result<()> {
        self.assert_policy(job)?;
        if !job.options.repair {
            return self.finish(
                job,
                Phase::Failed,
                "Required checks failed; automatic repair is disabled".into(),
                false,
            );
        }
        let used = job.charged_attempts() as u64;
        if used >= job.policy.primary_attempts + job.policy.escalation_attempts {
            return self.finish(
                job,
                Phase::Failed,
                "Automatic repair exhausted its unrefunded budget".into(),
                false,
            );
        }
        let _admission = store::lock(&self.paths.root.join("admission.lock"), true)?;
        if !self.store.holds()?.is_empty() {
            job.waiting_reason = Some("requester_maintenance".into());
            return self.store.save(job);
        }
        let number = job.attempts.len() as u64 + 1;
        let identity = format!("telete-{}-repair-{number}", job.id);
        let diagnostics = job
            .production_diagnostics
            .as_ref()
            .or_else(|| {
                job.validations
                    .last()
                    .map(|validation| &validation.diagnostics)
            })
            .context("repair has no diagnostics")?;
        let parent = Self::candidate(job)?.clone();
        let history = job
            .attempts
            .iter()
            .map(|attempt| {
                json!({"number":attempt.number,
            "nucleus_job_id":attempt.request.id,"parent":attempt.parent,"state":attempt.state,
            "candidate":attempt.candidate,"rejection":attempt.rejection})
            })
            .collect::<Vec<_>>();
        let prompts = job
            .policy
            .prompts
            .as_ref()
            .context("repair-enabled job has no frozen prompt")?;
        let components = BTreeMap::from([
            (
                prompts.instructions.id.clone(),
                prompts.instructions.version,
            ),
            (prompts.template.id.clone(), prompts.template.version),
        ]);
        let context = json!({"domain_job":job.id,"accepted_base":Self::base(job)?,"candidate_commit":parent,
            "candidate_directory":self.worktree(job),"diagnostic_path":diagnostics,"prior_attempts":history,
            "prompt_selection":{"id":prompts.selection.id,"version":prompts.selection.version,
                "components":components}});
        let primary = used < job.policy.primary_attempts;
        let request = providers::request(
            &identity,
            &job.id.0,
            &self.worktree(job),
            &context,
            job.policy
                .prompts
                .as_ref()
                .context("repair-enabled job has no frozen prompt")?,
            if primary {
                &job.policy.primary_model
            } else {
                &job.policy.escalation_model
            },
            if primary {
                job.policy.primary_reasoning
            } else {
                job.policy.escalation_reasoning
            },
            job.policy.model_timeout_seconds,
        )?;
        let directory = self.directory(job)?;
        store::atomic_json(
            &directory.join(format!("repair-{number}.request.json")),
            &request,
        )?;
        job.attempts.push(RepairAttempt {
            number,
            request,
            parent,
            stamp: store::now(),
            state: "prepared".into(),
            patch: None,
            patch_content: None,
            candidate: None,
            rejection: None,
            transport_failures: 0,
            terminal: None,
        });
        job.model_unresolved = true;
        job.waiting_reason = None;
        self.save(job, Phase::RepairWait)
    }

    // Keep one frozen request's observations and admission effects in one workflow.
    #[allow(clippy::too_many_lines)]
    async fn repair_wait(&self, job: &mut Job) -> Result<()> {
        let attempt = job
            .attempts
            .last()
            .context("repair wait has no saved attempt")?
            .clone();
        let saved: nucleus_core::JobRequestV1 = serde_json::from_slice(&fs::read(
            self.directory(job)?
                .join(format!("repair-{}.request.json", attempt.number)),
        )?)?;
        ensure!(saved == attempt.request, "saved Nucleus request changed");
        let client = providers::client(&job.policy.providers)?;
        let observed = providers::get_job(&client, &attempt.request.id).await;
        match observed {
            Ok(view) => {
                let observation = providers::correlate(&view, &attempt.request)?;
                job.attempts
                    .last_mut()
                    .context("attempt missing")?
                    .transport_failures = 0;
                job.model_unresolved = true;
                job.cancel_requested = self.store.job(&job.id.0)?.cancel_requested;
                if job.cancel_requested && !view.summary.state.is_terminal() {
                    let cancelled = match providers::cancel(&client, &attempt.request.id).await {
                        Ok(cancelled) => cancelled,
                        Err(error) => return self.transport_failure(job, &error),
                    };
                    ensure!(
                        cancelled.version == 1 && cancelled.job_id == attempt.request.id,
                        "Nucleus cancellation names another job"
                    );
                    job.waiting_reason = Some("cancelling_nucleus".into());
                    return self.store.save(job);
                }
                match observation {
                    Observation::Pending => {
                        job.waiting_reason = Some(
                            if view
                                .quota
                                .as_ref()
                                .is_some_and(nucleus_core::QuotaStatusV1::is_blocked)
                            {
                                "quota_deferred"
                            } else {
                                "nucleus"
                            }
                            .into(),
                        );
                        self.store.save(job)
                    }
                    Observation::Failed { lost, message } => {
                        store::atomic_json(
                            &self
                                .directory(job)?
                                .join(format!("repair-{}.result.json", attempt.number)),
                            &view,
                        )?;
                        let current = job.attempts.last_mut().context("attempt missing")?;
                        current.state = if lost { "lost" } else { "failed" }.into();
                        current.terminal = Some(serde_json::to_value(view)?);
                        job.model_unresolved = lost;
                        self.finish(
                            job,
                            if job.cancel_requested && !lost {
                                Phase::Cancelled
                            } else {
                                Phase::Failed
                            },
                            message,
                            lost,
                        )
                    }
                    Observation::Completed(patch) => {
                        store::atomic_json(
                            &self
                                .directory(job)?
                                .join(format!("repair-{}.result.json", attempt.number)),
                            &view,
                        )?;
                        job.model_unresolved = false;
                        let current = job.attempts.last_mut().context("attempt missing")?;
                        current.state = "completed".into();
                        current.terminal = Some(serde_json::to_value(view)?);
                        if job.cancel_requested {
                            return self.finish(
                                job,
                                Phase::Cancelled,
                                "Cancelled after model execution settled".into(),
                                false,
                            );
                        }
                        if let Some(raw) = patch {
                            let patch_file = self
                                .directory(job)?
                                .join(format!("repair-{}.patch", attempt.number));
                            store::atomic_bytes(&patch_file, raw.as_bytes())?;
                            let current = job.attempts.last_mut().context("attempt missing")?;
                            current.patch = Some(patch_file);
                            current.patch_content = Some(raw);
                            self.save(job, Phase::Applying)
                        } else {
                            job.attempts
                                .last_mut()
                                .context("attempt missing")?
                                .rejection = Some("No correlated final patch was returned".into());
                            self.save(job, Phase::RepairPrepare)
                        }
                    }
                }
            }
            Err(error) if providers::absent(&error) => {
                let _admission = store::lock(&self.paths.root.join("admission.lock"), true)?;
                job.model_unresolved = false;
                job.cancel_requested = self.store.job(&job.id.0)?.cancel_requested;
                if job.cancel_requested {
                    return self.finish(
                        job,
                        Phase::Cancelled,
                        "Cancelled before model admission".into(),
                        false,
                    );
                }
                if !self.store.holds()?.is_empty() {
                    job.waiting_reason = Some("requester_maintenance".into());
                    return self.store.save(job);
                }
                match providers::health(&client).await {
                    Ok(health) => ensure!(
                        health.version == 1
                            && health.status == "ok"
                            && health.accepting_jobs
                            && health.supported_protocol_versions.contains(&1)
                            && health.authentication.authenticated,
                        "Nucleus strict readiness was not proved"
                    ),
                    Err(error) if error.deferred() => {
                        job.waiting_reason = Some("quota_deferred".into());
                        return self.store.save(job);
                    }
                    Err(error) => return self.transport_failure(job, &error),
                }
                job.model_unresolved = true;
                self.store.save(job)?;
                match providers::submit(&client, &attempt.request).await {
                    Ok(accepted) => {
                        ensure!(
                            accepted.version == 1 && accepted.job_id == attempt.request.id,
                            "Nucleus admission names another request"
                        );
                        ensure!(
                            accepted.request_digest == attempt.request.request_digest()?,
                            "Nucleus admission has another request digest"
                        );
                        job.attempts.last_mut().context("attempt missing")?.state =
                            "submitted".into();
                        job.attempts
                            .last_mut()
                            .context("attempt missing")?
                            .transport_failures = 0;
                        job.waiting_reason = Some("nucleus".into());
                        self.store.save(job)
                    }
                    Err(error) if error.deferred() => {
                        job.model_unresolved = false;
                        job.waiting_reason = Some("quota_deferred".into());
                        self.store.save(job)
                    }
                    Err(error) => self.transport_failure(job, &error),
                }
            }
            Err(error) => self.transport_failure(job, &error),
        }
    }

    fn transport_failure(&self, job: &mut Job, error: &providers::CallError) -> Result<()> {
        job.waiting_reason = Some("nucleus_transport".into());
        job.last_error = Some(error.to_string());
        let attempt = job
            .attempts
            .last_mut()
            .context("transport failure has no saved attempt")?;
        attempt.transport_failures += 1;
        if attempt.transport_failures >= 5 {
            self.finish(
                job,
                Phase::Failed,
                "Nucleus observation remains unavailable; the exact request identity is retained"
                    .into(),
                true,
            )
        } else {
            self.store.save(job)
        }
    }

    fn applying(&self, job: &mut Job) -> Result<()> {
        self.assert_policy(job)?;
        let attempt = job
            .attempts
            .last()
            .context("patch application has no attempt")?
            .clone();
        ensure!(
            Self::candidate(job)? == &attempt.parent,
            "patch parent differs from current candidate"
        );
        let raw = attempt
            .patch_content
            .as_ref()
            .context("patch content was not retained")?;
        ensure!(
            fs::read_to_string(attempt.patch.as_ref().context("patch path absent")?)? == *raw,
            "saved model patch changed"
        );
        let tree = match git::patch_tree(
            self.paths,
            &self.config.repository,
            &attempt.parent,
            raw,
            &self.directory(job)?.join("repair.index"),
        ) {
            Ok(tree) => tree,
            Err(error) => {
                job.attempts.last_mut().context("attempt absent")?.rejection =
                    Some(error.to_string());
                return self.save(job, Phase::RepairPrepare);
            }
        };
        let candidate = git::commit_tree(
            self.paths,
            &self.config.repository,
            &tree,
            std::slice::from_ref(&attempt.parent),
            &job.id.0,
            attempt.stamp,
            &format!("Apply Telete repair {}", attempt.number),
        )?;
        if let Some(previous) = attempt.candidate {
            ensure!(previous == candidate, "recorded repair candidate changed");
        }
        job.attempts.last_mut().context("attempt absent")?.candidate = Some(candidate.clone());
        self.store.save(job)?;
        git::advance(
            self.paths,
            &self.config.repository,
            &git::private_ref(&job.id.0, "candidate")?,
            Some(&attempt.parent),
            &candidate,
        )?;
        job.candidate = Some(candidate.clone());
        git::ensure_worktree(
            self.paths,
            &self.config.repository,
            &self.worktree(job),
            &candidate,
        )?;
        self.save(job, Phase::Checking)
    }

    fn production_products(&self, job: &Job) -> Result<Vec<String>> {
        let report = &job
            .validations
            .last()
            .context("production has no successful validation")?
            .report;
        ensure!(
            report.state == ValidationState::Passed && Self::candidate(job)? == &report.candidate,
            "production validation names another candidate"
        );
        let mut names = report.products.clone();
        names.extend(report.platform_products.iter().cloned());
        names.extend(job.options.deploy.iter().cloned());
        names.sort();
        names.dedup();
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let mut selected = Inventory::load(&self.worktree(job))?
            .select(&names)?
            .iter()
            .map(|product| {
                if product.id.to_string() == "decisions" {
                    "krisis".into()
                } else {
                    product.id.to_string()
                }
            })
            .collect::<Vec<_>>();
        selected.sort();
        selected.dedup();
        Ok(selected)
    }

    fn deployment_products(&self, job: &Job) -> Result<Vec<String>> {
        if job.options.no_deploy {
            return Ok(Vec::new());
        }
        let names = if job.options.deploy.is_empty() {
            let report = &job
                .validations
                .last()
                .context("deployment has no validation")?
                .report;
            let mut names = report.products.clone();
            names.extend(report.platform_products.iter().cloned());
            names.retain(|product| product != "telete");
            names
        } else {
            job.options.deploy.clone()
        };
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let selected = Inventory::load(&self.worktree(job))?.select(&names)?;
        ensure!(
            !selected
                .iter()
                .any(|product| product.id.to_string() == "telete"),
            "Telete installation is a separate explicit operation"
        );
        let mut canonical = selected
            .iter()
            .map(|product| {
                if product.id.to_string() == "decisions" {
                    "krisis".into()
                } else {
                    product.id.to_string()
                }
            })
            .collect::<Vec<_>>();
        canonical.sort();
        canonical.dedup();
        Ok(canonical)
    }

    fn preparing(&self, job: &mut Job) -> Result<()> {
        self.assert_policy(job)?;
        let candidate = Self::candidate(job)?.clone();
        let products = self.production_products(job)?;
        let directory = self.directory(job)?;
        let generation = job.preparation_generation;
        let receipt = directory.join(format!("production-{generation}.json"));
        let fresh =
            self.begin_operation(job, format!("production-{generation}"), receipt.clone())?;
        if fresh {
            let prepared = if products.is_empty() {
                Ok(Preparation {
                    schema: 1,
                    source: candidate.clone(),
                    products: Vec::new(),
                    signing_policy: job.policy.signing_policy.clone(),
                    candidates: BTreeMap::new(),
                    release_check: true,
                })
            } else {
                deployment::prepare(
                    self.paths,
                    &self.worktree(job),
                    &candidate,
                    &products,
                    &job.policy.signing_policy,
                    &directory.join(format!("production-{generation}")),
                )
            };
            let outcome = match prepared {
                Ok(prepared) => PreparationOutcome {
                    prepared: Some(prepared),
                    compilation_failure: None,
                },
                Err(error) => match error.downcast_ref::<PrepareError>() {
                    Some(PrepareError::Compilation(diagnostic)) => PreparationOutcome {
                        prepared: None,
                        compilation_failure: Some(diagnostic.clone()),
                    },
                    None => return Err(error),
                },
            };
            store::atomic_json(&receipt, &outcome)?;
        }
        let outcome: PreparationOutcome = serde_json::from_slice(&fs::read(receipt)?)?;
        ensure!(
            outcome.prepared.is_some() != outcome.compilation_failure.is_some(),
            "production outcome is inconsistent"
        );
        job.operation = None;
        job.preparation_generation += 1;
        if let Some(diagnostic) = outcome.compilation_failure {
            let path = directory.join(format!("production-{generation}.log"));
            store::atomic_bytes(&path, diagnostic.as_bytes())?;
            job.production_diagnostics = Some(path);
            job.cancel_requested = self.store.job(&job.id.0)?.cancel_requested;
            if job.cancel_requested {
                return self.finish(
                    job,
                    Phase::Cancelled,
                    "Cancelled after release compilation settled".into(),
                    false,
                );
            }
            return self.save(job, Phase::RepairPrepare);
        }
        let prepared = outcome
            .prepared
            .context("production receipt has no prepared result")?;
        correlate_preparation(
            &prepared,
            &candidate,
            &products,
            &job.policy.signing_policy,
            self.paths,
        )?;
        job.preparation = Some(prepared);
        job.cancel_requested = self.store.job(&job.id.0)?.cancel_requested;
        if job.cancel_requested {
            return self.finish(
                job,
                Phase::Cancelled,
                "Cancelled after production preparation settled".into(),
                false,
            );
        }
        self.save(job, Phase::Accepting)
    }

    fn accepting(&self, job: &mut Job) -> Result<()> {
        self.assert_policy(job)?;
        let candidate = Self::candidate(job)?.clone();
        let products = self.production_products(job)?;
        let prepared = job
            .preparation
            .as_ref()
            .context("acceptance has no production receipt")?
            .clone();
        correlate_preparation(
            &prepared,
            &candidate,
            &products,
            &job.policy.signing_policy,
            self.paths,
        )?;
        git::clean_candidate(self.paths, &self.worktree(job), &candidate)?;
        git::advance(
            self.paths,
            &self.config.repository,
            git::ACCEPTED,
            Some(Self::base(job)?),
            &candidate,
        )?;
        job.accepted = true;
        self.store.save(job)?;
        let deploy_products = self.deployment_products(job)?;
        if deploy_products.is_empty() {
            return self.finish(
                job,
                Phase::Succeeded,
                "Required checks passed; no deployment was selected".into(),
                false,
            );
        }
        if job.deployment_request.is_none() {
            job.deployment_request = Some(DeploymentRequest {
                id: format!("telete-{}-deployment", job.id),
                source: candidate,
                products: deploy_products,
                prepared: prepared.clone(),
            });
        }
        self.save(job, Phase::Deploying)
    }

    fn deploying(&self, job: &mut Job) -> Result<()> {
        self.assert_policy(job)?;
        let request = job
            .deployment_request
            .as_ref()
            .context("deployment has no frozen request")?
            .clone();
        ensure!(
            git::ref_value(self.paths, &self.config.repository, git::ACCEPTED)?.as_ref()
                == Some(&request.source),
            "accepted source changed before deployment"
        );
        let observed = deployment::observe(self.paths, &request.id)?;
        if observed.is_none() && job.cancel_requested {
            return self.finish(
                job,
                Phase::Cancelled,
                "Cancelled after source acceptance and before deployment admission".into(),
                false,
            );
        }
        let result = match observed {
            Some(result) => result,
            None => match deployment::execute(
                self.paths,
                &self.worktree(job),
                &request.id,
                &request.prepared,
                &request.products,
            ) {
                Ok(result) => result,
                Err(error) => match deployment::observe(self.paths, &request.id)? {
                    Some(result) => result,
                    None => {
                        return self.finish(
                            job,
                            Phase::Failed,
                            format!("Deployment stopped before admission: {error}"),
                            false,
                        );
                    }
                },
            },
        };
        correlate_deployment(&result, &request)?;
        store::atomic_json(&self.directory(job)?.join("deployment.json"), &result)?;
        job.deployment_result = Some(result.clone());
        match result.state {
            DeploymentState::Succeeded => self.finish(
                job,
                Phase::Succeeded,
                "Deployment instructions completed for the accepted candidate".into(),
                false,
            ),
            DeploymentState::Failed => self.finish(
                job,
                Phase::Failed,
                "Deployment instructions failed; accepted source is retained".into(),
                false,
            ),
            DeploymentState::Interrupted if result.acknowledged => self.finish(
                job,
                Phase::Failed,
                "Interrupted deployment was explicitly acknowledged; no instruction was repeated"
                    .into(),
                false,
            ),
            DeploymentState::Interrupted | DeploymentState::Running => self.finish(
                job,
                Phase::Failed,
                "Deployment instruction effects remain unresolved".into(),
                true,
            ),
        }
    }

    async fn notifying(&self, job: &mut Job) -> Result<()> {
        let notice = job
            .notification
            .as_ref()
            .context("notification phase has no frozen message")?;
        if notice.accepted {
            return self.save(
                job,
                if job.unresolved {
                    Phase::Blocked
                } else {
                    job.outcome.context("notification has no CI outcome")?
                },
            );
        }
        let now = store::now();
        if notice
            .attempts
            .last()
            .is_some_and(|attempt| now.saturating_sub(attempt.started) < 300)
        {
            return Ok(());
        }
        if notice.attempts.len() >= 2 || now.saturating_sub(notice.created) >= 23 * 3600 {
            job.notification
                .as_mut()
                .context("notice absent")?
                .uncertain = true;
            self.store.set("paused", &true)?;
            return self.save(job, Phase::Blocked);
        }
        let message = notice.message.clone();
        job.notification
            .as_mut()
            .context("notice absent")?
            .attempts
            .push(SendAttempt {
                started: now,
                receipt: None,
                error: None,
            });
        self.store.save(job)?;
        let result = providers::email(&job.policy.providers, &message).await;
        let notice = job.notification.as_mut().context("notice absent")?;
        let attempt = notice.attempts.last_mut().context("send attempt absent")?;
        match result {
            Ok(receipt) => {
                attempt.receipt = Some(receipt);
                notice.accepted = true;
            }
            Err(error) => attempt.error = Some(error.to_string()),
        }
        self.store.save(job)
    }
}

fn correlate_preparation(
    prepared: &Preparation,
    candidate: &CommitId,
    products: &[String],
    policy: &SigningPolicy,
    paths: &Paths,
) -> Result<()> {
    ensure!(
        prepared.schema == 1
            && prepared.source == *candidate
            && prepared.products == products
            && prepared.signing_policy == *policy
            && prepared.release_check,
        "production preparation does not match source, scope, signing policy or release checks"
    );
    ensure!(
        prepared.candidates.keys().cloned().collect::<BTreeSet<_>>()
            == products.iter().cloned().collect(),
        "prepared candidate scope is incomplete"
    );
    for (product, artifact) in &prepared.candidates {
        ensure!(
            artifact.schema == 1
                && artifact.product == *product
                && artifact.source_commit == *candidate
                && artifact.signing_policy == *policy
                && artifact.source_key == candidate.to_string()
                && !artifact.candidate_id.is_empty()
                && artifact.candidate_dir.is_absolute()
                && artifact.candidate_dir.starts_with(&paths.root),
            "production candidate correlation is invalid"
        );
    }
    Ok(())
}

fn correlate_deployment(result: &DeploymentResult, request: &DeploymentRequest) -> Result<()> {
    let result_products = result.products.iter().collect::<BTreeSet<_>>();
    let request_products = request.products.iter().collect::<BTreeSet<_>>();
    ensure!(
        result.schema == 2
            && result.request_id == request.id
            && result.source == request.source
            && result_products.len() == result.products.len()
            && request_products.len() == request.products.len()
            && result_products == request_products,
        "deployment receipt does not match admitted request"
    );
    ensure!(
        !result.run_id.is_empty(),
        "deployment receipt has no owning run identity"
    );
    if result.state == DeploymentState::Succeeded {
        ensure!(
            result.exit_code == Some(0)
                && !result.instructions.is_empty()
                && result
                    .instructions
                    .iter()
                    .all(
                        |instruction| instruction.state == InstructionState::Succeeded
                            && instruction.exit_code == Some(0)
                    ),
            "deployment success has incomplete instruction results"
        );
    }
    Ok(())
}

fn render_notification(job: &Job) -> (String, String) {
    let products = job
        .deployment_result
        .as_ref()
        .map(|result| result.products.clone())
        .or_else(|| {
            job.validations
                .last()
                .map(|validation| validation.report.products.clone())
        })
        .unwrap_or_default();
    let scope = products
        .iter()
        .map(|name| name.replace('-', " "))
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if scope.is_empty() {
        String::new()
    } else {
        format!(" — {scope}")
    };
    let (title, mut body) = match job.outcome {
        Some(Phase::Succeeded)
            if job
                .deployment_result
                .as_ref()
                .is_some_and(|result| result.state == DeploymentState::Succeeded) =>
        {
            (
                "deployed",
                "Required checks passed and deployment instructions completed.".to_owned(),
            )
        }
        Some(Phase::Succeeded) => (
            "passed",
            "Required checks passed. No deployment was selected.".to_owned(),
        ),
        Some(Phase::AlreadyIncluded) => (
            "already included",
            "These changes are already in accepted history. No new checks or deployment ran."
                .to_owned(),
        ),
        Some(Phase::Cancelled) => (
            "cancelled",
            "CI was cancelled. Recorded source acceptance and completed effects are retained."
                .to_owned(),
        ),
        _ => {
            let phase = job
                .stopped_phase
                .map_or("processing", Phase::name)
                .replace('_', " ");
            let detail = notification_detail(
                job,
                job.outcome_message
                    .as_deref()
                    .unwrap_or("No specific cause was reported."),
            );
            (
                "failed",
                format!("What failed: {phase}.\n{detail}\nThe Telete queue is paused."),
            )
        }
    };
    if !job.options.run_tests {
        body.push_str("\n\nTests were skipped.");
    }
    if job.unresolved {
        body.push_str("\nThe recorded operation requires recovery.");
    }
    (format!("Telete: {title}{suffix}"), body)
}

fn notification_detail(job: &Job, value: &str) -> String {
    let first = value.lines().next().unwrap_or("");
    let mut words = Vec::new();
    for word in first.split_whitespace() {
        let reference = word.contains('/')
            || word.contains('\\')
            || word.contains(&job.id.0)
            || [
                &job.input,
                job.base.as_ref().unwrap_or(&job.input),
                job.candidate.as_ref().unwrap_or(&job.input),
            ]
            .iter()
            .any(|id| word.contains(&id.0))
            || word.starts_with("gpt-");
        if reference {
            if words
                .last()
                .is_none_or(|last| last != "[local details omitted]")
            {
                words.push("[local details omitted]".to_owned());
            }
        } else {
            words.push(word.to_owned());
        }
    }
    words.join(" ").chars().take(350).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::MacSigning;
    use bazaar::api::Record;
    use std::os::unix::fs::PermissionsExt;

    fn policy(paths: &Paths) -> Result<Policy> {
        let signing_policy = SigningPolicy {
            schema: 1,
            macos: MacSigning {
                profile: "local".into(),
                certificate_sha1: "a".repeat(40),
                keychain: paths.root.join("fixture.keychain"),
                identifier_namespace: "local.fixture".into(),
            },
        };
        store::atomic_json(&paths.root.join("signing.json"), &signing_policy)?;
        let record = |id: &str, content: &str| Record {
            id: id.into(),
            version: 1,
            content: content.into(),
        };
        Ok(Policy {
            primary_attempts: 3,
            escalation_attempts: 1,
            primary_model: "gpt-5.6-terra".into(),
            primary_reasoning: nucleus_core::ReasoningEffort::Medium,
            escalation_model: "gpt-5.6-sol".into(),
            escalation_reasoning: nucleus_core::ReasoningEffort::High,
            model_timeout_seconds: 600,
            signing_policy,
            providers: Selection {
                nucleus_socket: paths.root.join("not-a-live-socket"),
                email_executable: None,
                bazaar_database: paths.root.join("not-a-live-bazaar"),
            },
            prompts: Some(Prompts {
                selection: record(providers::PROMPT_SELECTION, "{}"),
                instructions: record(providers::PROMPT_INSTRUCTIONS, "Return a patch."),
                template: record(providers::PROMPT_TEMPLATE, "{context}"),
            }),
        })
    }

    fn fixture() -> Result<(tempfile::TempDir, Paths, PathBuf, CommitId)> {
        let temporary = tempfile::tempdir()?;
        let paths = Paths::for_test(temporary.path().join("telete"))?;
        let repo = temporary.path().join("repository");
        fs::create_dir(&repo)?;
        git::value(&paths, &repo, &["init", "--initial-branch=main"])?;
        fs::write(repo.join("file.txt"), "base\n")?;
        fs::create_dir_all(repo.join("pipeline/products"))?;
        for product in ["alpha", "beta", "decisions"] {
            fs::create_dir_all(repo.join(format!("products/{product}")))?;
            fs::write(repo.join(format!("products/{product}/marker")), product)?;
            fs::write(
                repo.join(format!("pipeline/products/{product}.sh")),
                format!(
                    "PIPELINE_SCHEMA=1\nPRODUCT_ID={product}\nPRODUCT_NAME={product}\nPRODUCT_DIR=products/{product}\nCARGO_PACKAGES={product}\nPROVIDERS='one|{product}|products/{product}/chancery|{product}'\n{}",
                    if product == "decisions" {
                        "PRODUCT_ALIASES=krisis\n"
                    } else {
                        ""
                    }
                ),
            )?;
        }
        fs::create_dir_all(repo.join("infrastructure/telete"))?;
        fs::write(
            repo.join("infrastructure/telete/product.sh"),
            "PIPELINE_SCHEMA=1\nPRODUCT_ID=telete\nPRODUCT_NAME=Telete\nPRODUCT_DIR=infrastructure/telete\nCARGO_PACKAGES=telete\nPROVIDERS='one|telete|infrastructure/telete/chancery|telete'\n",
        )?;
        git::value(&paths, &repo, &["add", "."])?;
        let tree = git::value(&paths, &repo, &["write-tree"])?;
        let base = git::commit_tree(
            &paths,
            &repo,
            &tree,
            &[],
            "fixture",
            1_700_000_000,
            "Fixture baseline",
        )?;
        git::value(&paths, &repo, &["update-ref", "HEAD", &base.0])?;
        init(&paths, &repo, &base.0)?;
        Ok((temporary, paths, repo, base))
    }

    fn job(paths: &Paths, base: &CommitId, id: &str) -> Result<Job> {
        let options = SubmitOptions {
            notify: false,
            repair: false,
            ..SubmitOptions::default()
        };
        Ok(new_job(
            JobId::new(id)?,
            format!("request-{id}"),
            base.clone(),
            options,
            policy(paths)?,
        ))
    }

    fn worker(paths: &Paths) -> Result<Worker<'_>> {
        let (store, config) = configured(paths)?;
        Ok(Worker {
            paths,
            store,
            config,
        })
    }

    fn report(base: &CommitId, candidate: &CommitId, products: &[&str]) -> ValidationReport {
        ValidationReport {
            schema: 1,
            base: base.clone(),
            candidate: candidate.clone(),
            state: ValidationState::Passed,
            products: products.iter().map(|value| (*value).into()).collect(),
            platform_products: Vec::new(),
            shared_suites: Vec::new(),
            tests_run: false,
            release_builds_deferred: true,
            required_gates: Vec::new(),
            gates: Vec::new(),
            diagnostics: String::new(),
            autofix_patch: None,
        }
    }

    fn repair(job: &Job, paths: &Paths, number: u64, raw: &str) -> Result<RepairAttempt> {
        let request = providers::request(
            &format!("telete-{}-repair-{number}", job.id),
            &job.id.0,
            &paths.job(&job.id.0).join("worktree"),
            &json!({}),
            job.policy.prompts.as_ref().context("fixture prompts")?,
            &job.policy.primary_model,
            job.policy.primary_reasoning,
            600,
        )?;
        let patch = paths.job(&job.id.0).join(format!("repair-{number}.patch"));
        store::atomic_bytes(&patch, raw.as_bytes())?;
        Ok(RepairAttempt {
            number,
            request,
            parent: job.candidate.clone().context("fixture candidate")?,
            stamp: 1_700_000_001,
            state: "completed".into(),
            patch: Some(patch),
            patch_content: Some(raw.into()),
            candidate: None,
            rejection: None,
            transport_failures: 0,
            terminal: None,
        })
    }

    #[test]
    fn store_preserves_cancellation_and_rejects_stale_history() -> Result<()> {
        let (_temporary, paths, _repo, base) = fixture()?;
        let store = Store::open(&paths.root, false)?;
        let queued = job(&paths, &base, "cas-fixture")?;
        let mut first = store.insert(&queued)?;
        let mut stale = first.clone();
        store.cancel(&first.id.0)?;
        first.waiting_reason = Some("retained".into());
        store.save(&mut first)?;
        assert!(first.cancel_requested);
        stale.waiting_reason = Some("must not overwrite".into());
        assert!(store.save(&mut stale).is_err());
        assert_eq!(
            store.job(&first.id.0)?.waiting_reason.as_deref(),
            Some("retained")
        );
        assert!(store.insert(&queued).is_err());
        assert_eq!(
            store
                .by_request(&queued.request_id)?
                .context("saved request")?
                .id,
            queued.id
        );
        Ok(())
    }

    #[test]
    fn fresh_integration_creates_private_job_directory_before_git() -> Result<()> {
        let (_temporary, paths, repo, base) = fixture()?;
        fs::write(repo.join("file.txt"), "submitted\n")?;
        git::value(&paths, &repo, &["add", "file.txt"])?;
        let tree = git::value(&paths, &repo, &["write-tree"])?;
        let input = git::commit_tree(
            &paths,
            &repo,
            &tree,
            std::slice::from_ref(&base),
            "fixture",
            1_700_000_001,
            "Fixture submission",
        )?;
        git::value(&paths, &repo, &["reset", "--hard", &input.0])?;
        let worker = worker(&paths)?;
        let mut job = job(&paths, &base, "fresh-integration")?;
        job.input = input.clone();
        job.base = Some(base.clone());
        job.phase = Phase::Integrating;
        let mut job = worker.store.insert(&job)?;
        let directory = paths.job(&job.id.0);
        assert!(!directory.exists());

        worker.integrating(&mut job)?;

        assert_eq!(job.phase, Phase::Checking);
        assert_eq!(
            fs::metadata(&directory)?.permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(directory.join("merge.log"))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let candidate = job.candidate.as_ref().context("integrated candidate")?;
        assert_eq!(
            git::commit(&paths, &worker.worktree(&job), "HEAD")?,
            *candidate
        );
        assert_eq!(
            git::ref_value(&paths, &repo, &git::private_ref(&job.id.0, "candidate")?)?,
            Some(candidate.clone())
        );
        assert_eq!(
            fs::read_to_string(worker.worktree(&job).join("file.txt"))?,
            "submitted\n"
        );
        assert_eq!(git::ref_value(&paths, &repo, git::ACCEPTED)?, Some(base));
        assert_eq!(git::commit(&paths, &repo, "HEAD")?, input);
        Ok(())
    }

    #[test]
    fn preparation_acknowledgement_abandons_without_replay_or_new_outcome() -> Result<()> {
        let (_temporary, paths, repo, base) = fixture()?;
        let store = Store::open(&paths.root, false)?;
        let mut blocked = job(&paths, &base, "abandon-preparation")?;
        blocked.base = Some(base.clone());
        blocked.candidate = Some(base.clone());
        blocked.phase = Phase::Blocked;
        blocked.stopped_phase = Some(Phase::Preparing);
        blocked.outcome = Some(Phase::Failed);
        blocked.outcome_message = Some("retained staging failure".into());
        blocked.last_error = blocked.outcome_message.clone();
        blocked.unresolved = true;
        blocked.operation = Some(Operation {
            phase: Phase::Preparing,
            candidate: base.clone(),
            name: "production-0".into(),
            result: paths.job(&blocked.id.0).join("production-0.json"),
            started: 1_700_000_002,
        });
        blocked.options.notify = true;
        blocked.notification = Some(Notification {
            message: email::api::Message {
                subject: "retained failure".into(),
                body: "retained body".into(),
                idempotency_key: Some("retained-key".into()),
            },
            accepted: true,
            uncertain: false,
            attempts: Vec::new(),
            created: 1_700_000_003,
        });
        let blocked = store.insert(&blocked)?;
        let directory = paths.job(&blocked.id.0);
        git::ensure_worktree(&paths, &repo, &directory.join("worktree"), &base)?;
        store.set("paused", &false)?;
        assert!(acknowledge_preparation(&paths, &blocked.id.0).is_err());
        store.set("paused", &true)?;
        let active_child = store::lock(&paths.root.join("worker.lock"), false)?;
        assert!(acknowledge_preparation(&paths, &blocked.id.0).is_err());
        drop(active_child);

        acknowledge_preparation(&paths, &blocked.id.0)?;
        let settled = store.job(&blocked.id.0)?;
        assert_eq!(settled.phase, Phase::Failed);
        assert!(!settled.unresolved);
        assert!(settled.operation.is_none());
        assert_eq!(settled.last_error, blocked.last_error);
        assert_eq!(settled.outcome_message, blocked.outcome_message);
        assert_eq!(settled.outcome_generation, blocked.outcome_generation);
        assert_eq!(
            serde_json::to_value(settled.notification)?,
            serde_json::to_value(blocked.notification)?
        );
        assert_eq!(settled.cleanup.state, "removed");
        assert!(directory.join("production-0.acknowledged.json").is_file());
        assert!(!directory.join("production-0.json").exists());
        assert_eq!(git::ref_value(&paths, &repo, git::ACCEPTED)?, Some(base));
        acknowledge_preparation(&paths, &blocked.id.0)?;
        assert_eq!(store.job(&blocked.id.0)?.revision, settled.revision);
        Ok(())
    }

    #[test]
    fn production_and_deployment_scope_preserve_explicit_intent() -> Result<()> {
        let (_temporary, paths, repo, base) = fixture()?;
        let worker = worker(&paths)?;
        let mut job = job(&paths, &base, "scope-fixture")?;
        job.base = Some(base.clone());
        job.candidate = Some(base.clone());
        git::ensure_worktree(&paths, &repo, &worker.worktree(&job), &base)?;
        job.validations.push(Validation {
            report: report(&base, &base, &["alpha", "telete"]),
            diagnostics: paths.job(&job.id.0).join("diagnostics"),
            stamp: 1,
            patch_content: None,
            fixed_candidate: None,
        });
        job.options.deploy = vec!["beta".into()];
        assert_eq!(
            worker.production_products(&job)?,
            vec!["alpha", "beta", "telete"]
        );
        assert_eq!(worker.deployment_products(&job)?, vec!["beta"]);
        job.options.no_deploy = true;
        assert_eq!(
            worker.production_products(&job)?,
            vec!["alpha", "beta", "telete"]
        );
        assert!(worker.deployment_products(&job)?.is_empty());
        job.options.no_deploy = false;
        job.options.deploy.clear();
        assert_eq!(worker.deployment_products(&job)?, vec!["alpha"]);
        job.validations
            .last_mut()
            .context("validation")?
            .report
            .products = vec!["decisions".into()];
        assert_eq!(worker.production_products(&job)?, vec!["krisis"]);
        assert_eq!(worker.deployment_products(&job)?, vec!["krisis"]);
        Ok(())
    }

    #[test]
    fn accepted_git_patch_refunds_only_recorded_private_candidate() -> Result<()> {
        let (_temporary, paths, repo, base) = fixture()?;
        let worker = worker(&paths)?;
        let mut job = job(&paths, &base, "refund-fixture")?;
        job.base = Some(base.clone());
        job.candidate = Some(base.clone());
        job.phase = Phase::Applying;
        store::private_directory(&paths.job(&job.id.0))?;
        git::ensure_worktree(&paths, &repo, &worker.worktree(&job), &base)?;
        git::advance(
            &paths,
            &repo,
            &git::private_ref(&job.id.0, "candidate")?,
            None,
            &base,
        )?;
        let raw = "diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -1,99 +1,99 @@\n-base\n+patched";
        job.attempts.push(repair(&job, &paths, 1, raw)?);
        let mut job = worker.store.insert(&job)?;
        assert_eq!(job.charged_attempts(), 1);
        worker.applying(&mut job)?;
        assert_eq!(job.charged_attempts(), 0);
        assert_eq!(job.phase, Phase::Checking);
        assert_eq!(job.budget()["invocations"], 1);
        assert_eq!(job.budget()["refunded"], 1);
        assert_eq!(
            fs::read_to_string(worker.worktree(&job).join("file.txt"))?,
            "patched\n"
        );
        assert_eq!(
            git::ref_value(&paths, &repo, git::ACCEPTED)?,
            Some(base.clone())
        );
        assert_eq!(git::commit(&paths, &repo, "HEAD")?, base);
        job.attempts
            .push(repair(&job, &paths, 2, "this is not a Git patch")?);
        job.phase = Phase::Applying;
        worker.store.save(&mut job)?;
        worker.applying(&mut job)?;
        assert_eq!(job.charged_attempts(), 1);
        assert_eq!(job.phase, Phase::RepairPrepare);
        assert!(job.attempts[1].rejection.is_some());
        assert_eq!(
            job.attempts[1].request.id.as_str(),
            "telete-refund-fixture-repair-2"
        );
        Ok(())
    }

    #[tokio::test]
    async fn missing_validation_completion_blocks_and_cannot_be_replayed() -> Result<()> {
        let (_temporary, paths, _repo, base) = fixture()?;
        let worker = worker(&paths)?;
        let mut job = job(&paths, &base, "lost-validator")?;
        job.base = Some(base.clone());
        job.candidate = Some(base.clone());
        job.phase = Phase::Checking;
        let directory = worker.directory(&job)?;
        job.operation = Some(Operation {
            phase: Phase::Checking,
            name: "validation-0".into(),
            candidate: base.clone(),
            result: directory.join("validation-0.json"),
            started: 1,
        });
        worker.store.insert(&job)?;
        drop(worker);
        let result = worker_once(&paths).await?;
        assert_eq!(result["phase"], "blocked");
        assert_eq!(result["unresolved"], true);
        assert!(recover(&paths, "lost-validator").await.is_err());
        assert_eq!(status(&paths, Some("lost-validator"))?["phase"], "blocked");
        Ok(())
    }

    #[test]
    fn cleanup_removes_only_settled_private_worktree_and_registration() -> Result<()> {
        let (_temporary, paths, repo, base) = fixture()?;
        let worker = worker(&paths)?;
        let mut job = job(&paths, &base, "cleanup-fixture")?;
        job.base = Some(base.clone());
        job.candidate = Some(base.clone());
        job.phase = Phase::Checking;
        let mut job = worker.store.insert(&job)?;
        store::private_directory(&paths.job(&job.id.0))?;
        git::ensure_worktree(&paths, &repo, &worker.worktree(&job), &base)?;
        fs::write(worker.worktree(&job).join("private-untracked"), "private")?;
        let registration = git::registration(&paths, &repo, &worker.worktree(&job))?
            .context("linked registration")?;
        worker.finish(&mut job, Phase::Failed, "fixture failure".into(), true)?;
        assert!(worker.worktree(&job).exists());
        assert!(registration.exists());
        job.unresolved = false;
        job.phase = Phase::Cancelled;
        job.outcome = Some(Phase::Cancelled);
        worker.store.save(&mut job)?;
        worker.cleanup(&mut job)?;
        assert_eq!(job.cleanup.state, "removed");
        assert!(!worker.worktree(&job).exists());
        assert!(!registration.exists());
        assert_eq!(git::commit(&paths, &repo, "HEAD")?, base);
        Ok(())
    }

    #[test]
    fn maintenance_requires_paused_and_settled_queue() -> Result<()> {
        let (_temporary, paths, _repo, base) = fixture()?;
        assert!(require_quiescent(&paths).is_ok());
        let store = Store::open(&paths.root, false)?;
        store.set("paused", &false)?;
        assert!(require_quiescent(&paths).is_err());
        store.set("paused", &true)?;
        let queued = job(&paths, &base, "maintenance-fixture")?;
        store.insert(&queued)?;
        assert!(require_quiescent(&paths).is_err());
        drop(store);
        cancel(&paths, &queued.id.0)?;
        assert!(require_quiescent(&paths).is_ok());
        Ok(())
    }

    #[test]
    fn maintenance_drain_keeps_non_model_operations_visible() -> Result<()> {
        let (_temporary, paths, _repo, base) = fixture()?;
        let store = Store::open(&paths.root, false)?;
        let mut active = job(&paths, &base, "drain-fixture")?;
        active.phase = Phase::Preparing;
        active.base = Some(base.clone());
        active.candidate = Some(base);
        active.model_unresolved = false;
        store.insert(&active)?;
        drop(store);
        let held = maintenance_hold(&paths, "fixture-owner")?;
        assert_eq!(held["held"], true);
        assert_eq!(held["drained"], false);
        assert_eq!(held["active_job"], "drain-fixture");
        Ok(())
    }

    #[tokio::test]
    async fn recovery_reads_completed_preparation_without_repeating_it() -> Result<()> {
        let (_temporary, paths, _repo, base) = fixture()?;
        let worker = worker(&paths)?;
        let mut job = job(&paths, &base, "prepared-fixture")?;
        job.base = Some(base.clone());
        job.candidate = Some(base.clone());
        job.phase = Phase::Blocked;
        job.stopped_phase = Some(Phase::Preparing);
        job.unresolved = true;
        job.outcome = Some(Phase::Failed);
        job.validations.push(Validation {
            report: report(&base, &base, &[]),
            diagnostics: paths.job(&job.id.0).join("diagnostics"),
            stamp: 1,
            patch_content: None,
            fixed_candidate: None,
        });
        let directory = worker.directory(&job)?;
        let receipt = directory.join("production-0.json");
        job.operation = Some(Operation {
            phase: Phase::Preparing,
            name: "production-0".into(),
            candidate: base.clone(),
            result: receipt.clone(),
            started: 1,
        });
        let prepared = Preparation {
            schema: 1,
            source: base.clone(),
            products: Vec::new(),
            signing_policy: job.policy.signing_policy.clone(),
            candidates: BTreeMap::new(),
            release_check: true,
        };
        store::atomic_json(&directory.join("production-0/result.json"), &prepared)?;
        worker.store.insert(&job)?;
        drop(worker);
        let recovered = recover(&paths, &job.id.0).await?;
        assert_eq!(recovered["phase"], "accepting");
        assert_eq!(recovered["unresolved"], false);
        assert_eq!(recovered["preparation_generation"], 1);
        let retained: PreparationOutcome = serde_json::from_slice(&fs::read(receipt)?)?;
        assert_eq!(
            retained.prepared.context("recovered preparation")?,
            prepared
        );
        assert!(retained.compilation_failure.is_none());
        Ok(())
    }
}
