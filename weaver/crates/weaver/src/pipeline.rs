use crate::error::{AppResult, WeaverError};
use crate::nucleus::{NucleusRunner, stage_job_id};
use crate::project::{Project, STAGES};
use crate::state::{CurrentRun, RunStatus, StateStore};
use crate::validator::{self, Verdict};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkerOutcome {
    Idle,
    Busy,
    Finished(Box<CurrentRun>),
}

pub(crate) async fn run_worker(store: &StateStore) -> AppResult<WorkerOutcome> {
    let Some(_run_lock) = store.try_acquire_run_lock()? else {
        return Ok(WorkerOutcome::Busy);
    };
    let Some(current) = store.claim_for_worker()? else {
        return Ok(WorkerOutcome::Idle);
    };
    let run_id = current.run_id.clone();

    match execute(store, current).await {
        Ok(outcome) => Ok(WorkerOutcome::Finished(Box::new(outcome))),
        Err(error) => {
            if error.is_retryable() {
                let detail = format!("will resume after a temporary failure: {error}");
                let _ = store.update(&run_id, |current| {
                    current.detail = Some(detail);
                    Ok(())
                });
                return Err(error);
            }
            let detail = error.to_string();
            let update = store.update(&run_id, |current| {
                let cancelled_between_steps =
                    current.cancel_requested && current.active_request.is_none();
                current.status = if cancelled_between_steps {
                    RunStatus::Cancelled
                } else {
                    RunStatus::Failed
                };
                current.active_job_id = None;
                current.active_request = None;
                current.detail = Some(if cancelled_between_steps {
                    "cancelled before the next workflow step".to_owned()
                } else {
                    detail.clone()
                });
                Ok(())
            });
            let updated = update.map_err(|state_error| {
                WeaverError::runtime(format!(
                    "{error}; additionally could not record workflow failure: {state_error}"
                ))
            })?;
            if updated.status == RunStatus::Cancelled {
                return Ok(WorkerOutcome::Finished(Box::new(updated)));
            }
            Err(error)
        }
    }
}

async fn execute(store: &StateStore, mut current: CurrentRun) -> AppResult<CurrentRun> {
    if current.cancel_requested && current.active_request.is_none() {
        return mark_cancelled(store, &current.run_id, "cancelled before execution");
    }
    let runner = NucleusRunner::for_current_user()?;
    if current.cancel_requested {
        return settle_active_cancellation(store, &runner, current).await;
    }
    runner.ensure_ready().await?;

    let project = Project::resolve(&current.repo_root, &current.narrative, true)?;
    current = prepare_project(store, &project, current)?;

    while current.next_stage < STAGES.len() {
        current = run_one_stage(store, &runner, &project, current).await?;
    }

    let verdict = validator::check(&project)?;
    store.update(&current.run_id, |state| {
        if state.cancel_requested {
            state.status = RunStatus::Cancelled;
            state.verdict = None;
            state.active_job_id = None;
            state.active_request = None;
            state.detail = Some("cancelled at the final workflow commit".to_owned());
            return Ok(());
        }
        state.status = match verdict {
            Verdict::Pass | Verdict::Revise => RunStatus::Succeeded,
            Verdict::Blocked => RunStatus::Blocked,
        };
        state.verdict = Some(verdict.as_str().to_owned());
        state.active_job_id = None;
        state.active_request = None;
        state.detail = Some(match verdict {
            Verdict::Pass | Verdict::Revise => format!(
                "build complete ({}) at {}",
                verdict.as_str(),
                project.output_relative(STAGES[4])
            ),
            Verdict::Blocked => format!(
                "editorial review is blocked; see {}",
                project.output_relative(STAGES[3])
            ),
        });
        Ok(())
    })
}

fn prepare_project(
    store: &StateStore,
    project: &Project,
    current: CurrentRun,
) -> AppResult<CurrentRun> {
    project.validate_existing_stage_tree()?;
    if current.outputs_prepared {
        return Ok(current);
    }
    project.prepare_outputs()?;
    store.update(&current.run_id, |state| {
        state.outputs_prepared = true;
        state.detail = Some("prepared current stage outputs".to_owned());
        Ok(())
    })
}

async fn run_one_stage(
    store: &StateStore,
    runner: &NucleusRunner,
    project: &Project,
    mut current: CurrentRun,
) -> AppResult<CurrentRun> {
    if (current.cancel_requested || store.cancellation_requested(&current.run_id)?)
        && current.active_request.is_none()
    {
        return mark_cancelled(store, &current.run_id, "cancelled before the next stage");
    }
    let stage = STAGES[current.next_stage];
    let request = if let Some(request) = current.active_request.clone() {
        request
    } else {
        let prompt = project.stage_prompt(stage)?;
        let parent = current
            .next_stage
            .checked_sub(1)
            .map(|index| stage_job_id(&current.run_id, STAGES[index]));
        let request = runner.stage_request(
            project,
            store.root(),
            &current.run_id,
            stage,
            &prompt,
            parent,
        )?;
        let persisted_request = request.clone();
        let request_id = request.id.to_string();
        current = store.update(&current.run_id, |state| {
            state.active_job_id = Some(request_id);
            state.active_request = Some(persisted_request);
            state.detail = Some(format!("stage {}/5: {}", stage.ordinal, stage.name));
            Ok(())
        })?;
        request
    };

    let state_read_failed = std::sync::Mutex::new(None::<String>);
    let cancellation_requested = || match store.cancellation_requested(&current.run_id) {
        Ok(requested) => requested,
        Err(error) => {
            if let Ok(mut slot) = state_read_failed.lock() {
                *slot = Some(error.to_string());
            }
            true
        }
    };
    let output_result = runner.run_stage(&request, &cancellation_requested).await;
    if let Some(message) = state_read_failed
        .lock()
        .map_err(|_| WeaverError::runtime("cancellation-state lock was poisoned"))?
        .take()
    {
        return Err(WeaverError::runtime(format!(
            "cannot monitor workflow cancellation: {message}"
        )));
    }
    let output = match output_result {
        Ok(output) => output,
        Err(error) => {
            if error.is_retryable() {
                return Err(error);
            }
            if store.cancellation_requested(&current.run_id)? {
                if cancellation_is_settled(runner, &request).await? {
                    return mark_cancelled(
                        store,
                        &current.run_id,
                        "cancelled during the current stage",
                    );
                }
                return Err(WeaverError::retryable(format!(
                    "Nucleus cancellation has not settled for {}: {error}",
                    request.id
                )));
            }
            return Err(error);
        }
    };
    if output.cancellation_requested || store.cancellation_requested(&current.run_id)? {
        return mark_cancelled(store, &current.run_id, "cancelled during the current stage");
    }

    project.write_stage_output(stage, output.final_message.as_bytes())?;
    store.update(&current.run_id, |state| {
        state.next_stage = stage.ordinal;
        state.active_job_id = None;
        state.active_request = None;
        state.detail = Some(format!(
            "completed stage {}/5: {}",
            stage.ordinal, stage.name
        ));
        Ok(())
    })
}

async fn settle_active_cancellation(
    store: &StateStore,
    runner: &NucleusRunner,
    current: CurrentRun,
) -> AppResult<CurrentRun> {
    let request = current.active_request.as_ref().ok_or_else(|| {
        WeaverError::runtime("cancelled workflow has no active request to settle")
    })?;
    let result = runner.run_stage(request, &|| true).await;
    match result {
        Ok(_) => mark_cancelled(
            store,
            &current.run_id,
            "cancelled after the active stage reached a terminal state",
        ),
        Err(error) if error.is_retryable() => Err(error),
        Err(error) => {
            if cancellation_is_settled(runner, request).await? {
                mark_cancelled(store, &current.run_id, "cancelled active Nucleus stage")
            } else {
                Err(WeaverError::retryable(format!(
                    "Nucleus cancellation has not settled for {}: {error}",
                    request.id
                )))
            }
        }
    }
}

async fn cancellation_is_settled(
    runner: &NucleusRunner,
    request: &nucleus_core::JobRequestV1,
) -> AppResult<bool> {
    let Some(job) = runner.inspect_job(&request.id).await? else {
        return Ok(true);
    };
    if job.summary.id != request.id || job.request != *request {
        return Err(WeaverError::runtime(format!(
            "Nucleus job {} does not match Weaver's persisted cancellation request",
            request.id
        )));
    }
    Ok(job.summary.state.is_terminal())
}

fn mark_cancelled(store: &StateStore, run_id: &str, detail: &str) -> AppResult<CurrentRun> {
    store.update(run_id, |current| {
        current.status = RunStatus::Cancelled;
        current.cancel_requested = true;
        current.active_job_id = None;
        current.active_request = None;
        current.detail = Some(detail.to_owned());
        Ok(())
    })
}
