//! Finite Vizier workflow driver.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use fs2::FileExt as _;
use time::OffsetDateTime;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::git;
use crate::model::{
    AttemptState, AttemptView, CandidateView, Disposition, DocumentView, GateResult,
    MAX_GATE_OUTPUT_BYTES, MAX_INPUT_BUNDLE_BYTES, PacketState, PacketView, PathScope,
    RecoveryCause, RecoveryEnvelope, RecoveryFrontier, Role, RunState, RunView,
};
use crate::nucleus::{AgentRunner, AttemptSpec, neutral_workspace};
use crate::store::Store;

const MAX_PARALLEL_JOBS: usize = 8;
const DELEGATION_SUBJECT: &str = "delegation";
const PLAN_REVIEW_SUBJECT: &str = "plan-set";

#[derive(Clone, Debug)]
pub struct Workflow {
    store: Store,
    runner: AgentRunner,
    state_root: PathBuf,
    gate_shell: PathBuf,
}

impl Workflow {
    #[must_use]
    pub fn new(store: Store, runner: AgentRunner) -> Self {
        let state_root = store
            .path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("state");
        Self {
            store,
            runner,
            state_root,
            gate_shell: PathBuf::from("/bin/sh"),
        }
    }

    #[cfg(test)]
    fn with_gate_shell(store: Store, runner: AgentRunner, gate_shell: impl Into<PathBuf>) -> Self {
        let mut workflow = Self::new(store, runner);
        workflow.gate_shell = gate_shell.into();
        workflow
    }

    pub async fn drive(&self, run_id: &str) -> AppResult<RunView> {
        let _lock = self.acquire_driver_lock()?;
        let result = self.drive_locked(run_id).await;
        if let Err(_error) = &result
            && let Ok(run) = self.store.run(run_id)
            && !matches!(run.state, RunState::Succeeded | RunState::Cancelled)
        {
            let _ = self.terminalize_noncontinuable(&run, RecoveryCause::OperationalError);
        }
        result
    }

    pub async fn retry_attempt(&self, attempt_id: &str) -> AppResult<RunView> {
        let _lock = self.acquire_driver_lock()?;
        let old = self.store.attempt(attempt_id)?;
        if !old.state.is_terminal() || old.domain_document_id.is_some() {
            return Err(AppError::new(
                "attempt_not_retryable",
                "only a terminal attempt without a committed domain result may be retried",
            ));
        }
        let run = self.store.run(&old.run_id)?;
        validate_retry_target(&self.store, &run, &old)?;
        let old_workspace = Path::new(&old.workspace_path);
        if old_workspace.exists() {
            if old.role == Role::Assembler {
                let quarantine = self
                    .state_root
                    .join("quarantine")
                    .join(&run.id)
                    .join(&old.id);
                if let Some(parent) = quarantine.parent() {
                    private_directory(parent)?;
                }
                fs::rename(old_workspace, quarantine)?;
            } else {
                git::quarantine_worktree(
                    Path::new(&run.repository),
                    &self.state_root,
                    &run.id,
                    &old.id,
                    old_workspace,
                )?;
            }
        }
        let request: nucleus_core::JobRequestV1 = serde_json::from_slice(&old.request_bytes)?;
        let workspace_hint = format!("retry-{}", Uuid::now_v7());
        let workspace = if old.role == Role::Assembler {
            neutral_workspace(&self.state_root, &workspace_hint)?
        } else {
            let base = old.base_commit.as_deref().ok_or_else(|| {
                AppError::new("retry_basis_missing", "attempt has no exact Git base")
            })?;
            git::prepare_worktree(
                Path::new(&run.repository),
                &self.state_root,
                &run.id,
                &workspace_hint,
                base,
            )?
        };
        self.runner.prepare_attempt(
            &self.store,
            &AttemptSpec {
                run: &run,
                role: old.role,
                subject_id: &old.subject_id,
                round: old.round,
                targeted: old.targeted,
                prompt: &request.prompt,
                workspace: &workspace,
                base_commit: old.base_commit.as_deref(),
                allowed_scopes: &old.allowed_scopes,
                predecessor_attempt_id: Some(&old.id),
            },
        )?;
        self.store.set_run_state(
            &run.id,
            state_for_role(old.role),
            Some("explicit attempt retry admitted"),
        )?;
        self.drive_locked(&run.id).await
    }

    async fn drive_locked(&self, run_id: &str) -> AppResult<RunView> {
        let run = self.store.run(run_id)?;
        if run.state.is_terminal() {
            return Ok(run);
        }
        if run.cancel_requested {
            self.runner.cancel_run(&self.store, run_id).await?;
            return self.store.run(run_id);
        }
        if run.parent_run_id.is_some() && run.recovery_checkpoint_id.is_some() {
            return self.run_integrated_continuation(&run).await;
        }
        self.run_planners(&run).await?;
        self.run_assembler(&run).await?;
        self.run_plan_review(&run).await?;
        if self.store.run(run_id)?.state == RunState::NeedsAttention {
            return self.store.run(run_id);
        }
        self.run_packets(&run).await?;
        if self.store.run(run_id)?.state == RunState::NeedsAttention {
            return self.store.run(run_id);
        }
        self.run_integration(&run).await?;
        self.store.secure_files()?;
        self.store.run(run_id)
    }

    async fn run_planners(&self, run: &RunView) -> AppResult<()> {
        self.store
            .set_run_state(&run.id, RunState::Planning, None)?;
        let contracts = self.store.documents(&run.id, "contract_unit")?;
        let mut jobs = JoinSet::new();
        for contract in contracts {
            if self
                .store
                .document_for_subject(
                    &run.id,
                    "unit_plan",
                    contract.subject_id.as_deref().unwrap_or(""),
                    0,
                )?
                .is_some()
            {
                continue;
            }
            let workflow = self.clone();
            let run = run.clone();
            jobs.spawn(async move {
                let unit_id = contract.subject_id.clone().ok_or_else(|| {
                    AppError::new("contract_unit_invalid", "contract unit has no identity")
                })?;
                let prompt = workflow.unit_planner_prompt(&run, &unit_id)?;
                let attempt = workflow
                    .execute_attempt(
                        &run,
                        Role::Planner,
                        &unit_id,
                        0,
                        false,
                        &prompt,
                        Some(&run.source_commit),
                        &[],
                    )
                    .await?;
                require_domain_result(&attempt, "unit plan")?;
                Ok::<(), AppError>(())
            });
            if jobs.len() >= MAX_PARALLEL_JOBS {
                join_one(&mut jobs).await?;
            }
        }
        while !jobs.is_empty() {
            join_one(&mut jobs).await?;
        }
        Ok(())
    }

    async fn run_assembler(&self, run: &RunView) -> AppResult<()> {
        if !self.store.packets(&run.id)?.is_empty() {
            return Ok(());
        }
        self.store
            .set_run_state(&run.id, RunState::Assembling, None)?;
        let prompt = self.assembler_prompt(run)?;
        let attempt = self
            .execute_attempt(
                run,
                Role::Assembler,
                DELEGATION_SUBJECT,
                0,
                false,
                &prompt,
                None,
                &[],
            )
            .await?;
        require_domain_result(&attempt, "delegation plan")?;
        if self.store.packets(&run.id)?.is_empty() {
            return Err(AppError::new(
                "delegation_missing",
                "assembler completed without a valid packet manifest",
            ));
        }
        Ok(())
    }

    async fn run_plan_review(&self, run: &RunView) -> AppResult<()> {
        // Recovery begins at the latest durable assembled revision, never at a
        // historical round that the ledger has already superseded.
        let mut round = self.latest_delegation_revision(run)?;
        loop {
            if round > 0 {
                self.run_assembler_revision(run, round).await?;
            }
            self.store
                .set_run_state(&run.id, RunState::PlanReview, None)?;
            let current_review_is_complete = self
                .store
                .latest_attempt(&run.id, Role::PlanReviewer, PLAN_REVIEW_SUBJECT, round)?
                .is_some_and(|attempt| {
                    attempt.domain_document_id.is_some() && attempt.state.is_terminal()
                });
            // A retained current review needs no reconstructed prompt: execute_attempt
            // reuses its durable result. This permits interrupted recovery to begin at
            // a later assembled revision even when no historical review was retained.
            let prompt = if current_review_is_complete {
                String::new()
            } else {
                self.plan_review_prompt(run, round)?
            };
            let attempt = self
                .execute_attempt(
                    run,
                    Role::PlanReviewer,
                    PLAN_REVIEW_SUBJECT,
                    round,
                    round > 0,
                    &prompt,
                    Some(&run.source_commit),
                    &[],
                )
                .await?;
            match route_plan_review(
                require_review_result(&attempt)?,
                round,
                run.remediation_limit,
            ) {
                PlanReviewRoute::Accepted => return Ok(()),
                PlanReviewRoute::Revise { next_round } => round = next_round,
                PlanReviewRoute::NeedsAttention { detail } => {
                    self.store
                        .set_run_state(&run.id, RunState::NeedsAttention, Some(detail))?;
                    return Ok(());
                }
            }
        }
    }

    async fn run_assembler_revision(&self, run: &RunView, round: u32) -> AppResult<()> {
        let previous_round = round.checked_sub(1).ok_or_else(|| {
            AppError::new(
                "plan_revision_invalid",
                "an initial delegation plan cannot be handled as a revision",
            )
        })?;
        if self
            .plan_document_at_round(&run.id, "delegation_plan", round)?
            .is_some()
        {
            self.require_current_packet_revision(run, round)?;
            return Ok(());
        }
        let feedback = self
            .store
            .document_for_subject(&run.id, "plan_review", PLAN_REVIEW_SUBJECT, previous_round)?
            .ok_or_else(|| {
                AppError::new(
                    "plan_review_feedback_missing",
                    "targeted plan revision requires the exact prior plan-review Markdown",
                )
            })?;
        self.store
            .set_run_state(&run.id, RunState::Assembling, None)?;
        let prompt = self.assembler_revision_prompt(run, round, &feedback)?;
        let attempt = self
            .execute_attempt(
                run,
                Role::Assembler,
                DELEGATION_SUBJECT,
                round,
                true,
                &prompt,
                None,
                &[],
            )
            .await?;
        require_domain_result(&attempt, "delegation plan revision")?;
        self.require_current_packet_revision(run, round)
    }

    async fn run_packets(&self, run: &RunView) -> AppResult<()> {
        loop {
            let packets = self.store.packets(&run.id)?;
            if packets
                .iter()
                .all(|packet| packet.state == PacketState::Accepted)
            {
                return Ok(());
            }
            if packets
                .iter()
                .any(|packet| packet.state == PacketState::Blocked)
            {
                self.store.set_run_state(
                    &run.id,
                    RunState::NeedsAttention,
                    Some("one or more work packets require caller attention"),
                )?;
                return Ok(());
            }
            let accepted = packets
                .iter()
                .filter(|packet| packet.state == PacketState::Accepted)
                .map(|packet| packet.key.as_str())
                .collect::<BTreeSet<_>>();
            let mut ready = Vec::new();
            for packet in &packets {
                if packet.state != PacketState::Accepted
                    && packet
                        .depends_on
                        .iter()
                        .all(|dependency| accepted.contains(dependency.as_str()))
                    && ready.iter().all(|selected: &&PacketView| {
                        !git::scopes_overlap(&selected.path_scopes, &packet.path_scopes)
                    })
                {
                    ready.push(packet);
                }
                if ready.len() == MAX_PARALLEL_JOBS {
                    break;
                }
            }
            if ready.is_empty() {
                return Err(AppError::new(
                    "packet_graph_stalled",
                    "packet dependency graph has no ready work",
                ));
            }
            self.store
                .set_run_state(&run.id, RunState::Implementing, None)?;
            let mut jobs = JoinSet::new();
            for packet in ready {
                let workflow = self.clone();
                let run = run.clone();
                let packet = packet.clone();
                jobs.spawn(async move { workflow.process_packet(&run, &packet).await });
            }
            while !jobs.is_empty() {
                join_one(&mut jobs).await?;
            }
        }
    }

    async fn process_packet(&self, run: &RunView, original: &PacketView) -> AppResult<()> {
        let packet = self.store.packet(&run.id, &original.key)?;
        if packet.state == PacketState::Accepted {
            return Ok(());
        }
        let round = packet.remediation_round;
        let candidate = match self
            .store
            .latest_candidate(&run.id, &packet.key, "packet")?
        {
            Some(candidate) if candidate.round == round => candidate,
            _ => self.implement_packet(run, &packet).await?,
        };
        self.store
            .set_run_state(&run.id, RunState::PacketReview, None)?;
        self.store.set_packet_state(
            &run.id,
            &packet.key,
            PacketState::Reviewing,
            Some(&candidate.id),
            round,
        )?;
        let prompt = self.packet_review_prompt(run, &packet, &candidate, round > 0)?;
        let review = self
            .execute_attempt(
                run,
                Role::PacketReviewer,
                &packet.key,
                round,
                round > 0,
                &prompt,
                Some(&candidate.commit_oid),
                &[],
            )
            .await?;
        match require_review_result(&review)? {
            Disposition::Accepted => self.store.set_packet_state(
                &run.id,
                &packet.key,
                PacketState::Accepted,
                Some(&candidate.id),
                round,
            ),
            Disposition::ChangesRequested if round < run.remediation_limit => {
                self.store.set_packet_state(
                    &run.id,
                    &packet.key,
                    PacketState::Planned,
                    Some(&candidate.id),
                    round + 1,
                )
            }
            Disposition::ChangesRequested | Disposition::Blocked => self.store.set_packet_state(
                &run.id,
                &packet.key,
                PacketState::Blocked,
                Some(&candidate.id),
                round,
            ),
        }
    }

    async fn implement_packet(
        &self,
        run: &RunView,
        packet: &PacketView,
    ) -> AppResult<CandidateView> {
        let round = packet.remediation_round;
        let base = if round == 0 {
            let all = self.store.packets(&run.id)?;
            let candidates = packet
                .depends_on
                .iter()
                .map(|key| {
                    let dependency = all
                        .iter()
                        .find(|candidate| &candidate.key == key)
                        .ok_or_else(|| AppError::new("packet_dependency_missing", key.clone()))?;
                    let id = dependency.current_candidate_id.as_deref().ok_or_else(|| {
                        AppError::new("packet_dependency_unaccepted", key.clone())
                    })?;
                    Ok(self.store.candidate(id)?.commit_oid)
                })
                .collect::<AppResult<Vec<_>>>()?;
            git::compose_commits(
                Path::new(&run.repository),
                &run.id,
                &run.source_commit,
                &candidates,
                &format!("refs/vizier/runs/{}/packet-bases/{}/0", run.id, packet.key),
            )?
        } else {
            self.store
                .latest_candidate(&run.id, &packet.key, "packet")?
                .ok_or_else(|| {
                    AppError::new(
                        "remediation_basis_missing",
                        "prior packet candidate is missing",
                    )
                })?
                .commit_oid
        };
        self.store.set_packet_state(
            &run.id,
            &packet.key,
            PacketState::Implementing,
            packet.current_candidate_id.as_deref(),
            round,
        )?;
        let prompt = self.packet_implementation_prompt(run, packet, round > 0)?;
        let attempt = self
            .execute_attempt(
                run,
                Role::Implementor,
                &packet.key,
                round,
                round > 0,
                &prompt,
                Some(&base),
                &packet.path_scopes,
            )
            .await?;
        if attempt.disposition == Some(Disposition::Blocked) {
            self.store.set_packet_state(
                &run.id,
                &packet.key,
                PacketState::Blocked,
                packet.current_candidate_id.as_deref(),
                round,
            )?;
            return Err(AppError::new(
                "implementor_blocked",
                format!("packet {} implementor reported a blocker", packet.key),
            ));
        }
        self.freeze_writer_candidate(run, &attempt, "packet", &packet.key, round, &base)
    }

    async fn run_integration(&self, run: &RunView) -> AppResult<()> {
        let packets = self.store.packets(&run.id)?;
        let commits = packets
            .iter()
            .map(|packet| {
                let id = packet.current_candidate_id.as_deref().ok_or_else(|| {
                    AppError::new("accepted_candidate_missing", packet.key.clone())
                })?;
                Ok(self.store.candidate(id)?.commit_oid)
            })
            .collect::<AppResult<Vec<_>>>()?;
        let assembled = git::compose_commits(
            Path::new(&run.repository),
            &run.id,
            &run.source_commit,
            &commits,
            &format!("refs/vizier/runs/{}/assembled", run.id),
        )?;
        let scopes = union_scopes(&packets);
        for round in 0..=run.remediation_limit {
            self.store
                .set_run_state(&run.id, RunState::Integrating, None)?;
            let candidate = if let Some(candidate) =
                self.store
                    .candidate_at_round(&run.id, "integration", "integration", round)?
            {
                candidate
            } else {
                let base = if round == 0 {
                    assembled.clone()
                } else {
                    self.store
                        .latest_candidate(&run.id, "integration", "integration")?
                        .ok_or_else(|| {
                            AppError::new(
                                "integration_basis_missing",
                                "prior integrated candidate is missing",
                            )
                        })?
                        .commit_oid
                };
                let prompt = self.integration_prompt(run, &packets, round > 0, round)?;
                let attempt = self
                    .execute_attempt(
                        run,
                        Role::Integrator,
                        "integration",
                        round,
                        round > 0,
                        &prompt,
                        Some(&base),
                        &scopes,
                    )
                    .await?;
                if attempt.disposition == Some(Disposition::Blocked) {
                    self.terminalize_noncontinuable(run, RecoveryCause::Blocked)?;
                    return Ok(());
                }
                self.freeze_writer_candidate(
                    run,
                    &attempt,
                    "integration",
                    "integration",
                    round,
                    &base,
                )?
            };
            if self.run_gates(run, &candidate, round)? {
                if round < run.remediation_limit {
                    continue;
                }
                self.terminalize_gate_exhausted(run, &candidate)?;
                return Ok(());
            }
            if self.store.run(&run.id)?.state == RunState::NeedsAttention {
                return Ok(());
            }
            self.store
                .set_run_state(&run.id, RunState::FinalReview, None)?;
            let prompt = self.integrated_review_prompt(run, &candidate, round > 0)?;
            let review = self
                .execute_attempt(
                    run,
                    Role::IntegratedReviewer,
                    "integration",
                    round,
                    round > 0,
                    &prompt,
                    Some(&candidate.commit_oid),
                    &[],
                )
                .await?;
            match require_review_result(&review)? {
                Disposition::Accepted => {
                    let reference = git::publish_final_ref(
                        Path::new(&run.repository),
                        &run.id,
                        &candidate.commit_oid,
                    )?;
                    self.store.finish_run(&run.id, &candidate.id, &reference)?;
                    return Ok(());
                }
                Disposition::ChangesRequested if round < run.remediation_limit => {}
                Disposition::ChangesRequested => {
                    self.terminalize_integrated_review_exhausted(run, &candidate, &review)?;
                    return Ok(());
                }
                Disposition::Blocked => {
                    self.terminalize_noncontinuable(run, RecoveryCause::Blocked)?;
                    return Ok(());
                }
            }
        }
        unreachable!("bounded integration loop always returns")
    }

    /// Linked children deliberately enter only at the durable integrated
    /// frontier.  Their local round zero is their first (and counted)
    /// integrator correction.
    async fn run_integrated_continuation(&self, run: &RunView) -> AppResult<RunView> {
        let parent_id = run.parent_run_id.as_deref().ok_or_else(|| {
            AppError::new("continuation_parent_missing", "linked child has no parent")
        })?;
        let checkpoint = self.store.recovery_envelope(parent_id)?.ok_or_else(|| {
            AppError::new(
                "continuation_checkpoint_missing",
                "linked child has no checkpoint",
            )
        })?;
        if !matches!(
            checkpoint.cause,
            RecoveryCause::GateFailureExhausted | RecoveryCause::IntegratedReviewExhausted
        ) {
            return Err(AppError::new(
                "continuation_frontier_unsupported",
                "this continuation frontier is not executable",
            ));
        }
        let predecessor_id = checkpoint.candidate_id.as_deref().ok_or_else(|| {
            AppError::new(
                "continuation_evidence_missing",
                "checkpoint has no exact candidate",
            )
        })?;
        let predecessor = self.store.candidate(predecessor_id)?;
        for round in 0..run.remediation_limit {
            self.store
                .set_run_state(&run.id, RunState::Integrating, None)?;
            let prompt = self.integration_prompt(run, &[], true, round)?;
            let base = if round == 0 {
                predecessor.commit_oid.clone()
            } else {
                self.store
                    .latest_candidate(&run.id, "integration", "integration")?
                    .ok_or_else(|| {
                        AppError::new(
                            "integration_basis_missing",
                            "prior child candidate is missing",
                        )
                    })?
                    .commit_oid
            };
            let attempt = self
                .execute_attempt(
                    run,
                    Role::Integrator,
                    "integration",
                    round,
                    true,
                    &prompt,
                    Some(&base),
                    &checkpoint.permitted_scopes,
                )
                .await?;
            if attempt.disposition == Some(Disposition::Blocked) {
                self.terminalize_noncontinuable(run, RecoveryCause::Blocked)?;
                return self.store.run(&run.id);
            }
            let candidate = self.freeze_writer_candidate(
                run,
                &attempt,
                "integration",
                "integration",
                round,
                &base,
            )?;
            if self.run_gates(run, &candidate, round)? {
                if round + 1 == run.remediation_limit {
                    self.terminalize_gate_exhausted(run, &candidate)?;
                    return self.store.run(&run.id);
                }
                continue;
            }
            if self.store.run(&run.id)?.state == RunState::NeedsAttention {
                return self.store.run(&run.id);
            }
            self.store
                .set_run_state(&run.id, RunState::FinalReview, None)?;
            // Review mode is a property of the exact candidate lineage, not
            // of the frontier that happened to exhaust.  In particular, a
            // gate frontier may be a successor of a reviewed candidate.
            let targeted = self.integrated_review_in_candidate_lineage(&predecessor)?;
            let review = self
                .execute_attempt(
                    run,
                    Role::IntegratedReviewer,
                    "integration",
                    round,
                    targeted,
                    &self.integrated_review_prompt(run, &candidate, targeted)?,
                    Some(&candidate.commit_oid),
                    &[],
                )
                .await?;
            match require_review_result(&review)? {
                Disposition::Accepted => {
                    let reference = git::publish_final_ref(
                        Path::new(&run.repository),
                        &run.id,
                        &candidate.commit_oid,
                    )?;
                    self.store.finish_run(&run.id, &candidate.id, &reference)?;
                    return self.store.run(&run.id);
                }
                Disposition::ChangesRequested if round + 1 == run.remediation_limit => {
                    self.terminalize_integrated_review_exhausted(run, &candidate, &review)?;
                    return self.store.run(&run.id);
                }
                Disposition::ChangesRequested => {}
                Disposition::Blocked => {
                    self.terminalize_noncontinuable(run, RecoveryCause::Blocked)?;
                    return self.store.run(&run.id);
                }
            }
        }
        // The exclusive range is deliberately exhaustive; this is only
        // reachable for an invalid zero allowance, which admission rejects.
        self.terminalize_noncontinuable(run, RecoveryCause::MixedFrontier)?;
        self.store.run(&run.id)
    }

    fn terminalize_noncontinuable(&self, run: &RunView, cause: RecoveryCause) -> AppResult<()> {
        let envelope = RecoveryEnvelope {
            version: 1,
            run_id: run.id.clone(),
            checkpoint_id: format!("checkpoint-{}", Uuid::now_v7()),
            continuable: false,
            cause,
            frontier: None,
            responsible_role: None,
            subject_id: None,
            failed_packet_keys: Vec::new(),
            evidence_ids: Vec::new(),
            permitted_scopes: Vec::new(),
            invalidated_checks: Vec::new(),
            candidate_id: None,
            reviewed_candidate_id: None,
            predecessor_candidate_id: None,
            review_attempt_id: None,
            gate_result_ids: Vec::new(),
            canonical_basis_digest: run.input_bundle_sha256.clone(),
        };
        self.store.terminalize_needs_attention(&envelope)
    }

    fn terminalize_gate_exhausted(
        &self,
        run: &RunView,
        candidate: &CandidateView,
    ) -> AppResult<()> {
        let results = self
            .store
            .gates(&run.id)?
            .into_iter()
            .map(|gate| {
                self.store
                    .gate_result(&gate.id, &candidate.id)?
                    .ok_or_else(|| {
                        AppError::new("gate_evidence_missing", "configured gate result is missing")
                    })
            })
            .collect::<AppResult<Vec<_>>>()?;
        if results.iter().any(|result| result.output_truncated)
            || !results.iter().any(|result| result.exit_code != 0)
        {
            return self.terminalize_noncontinuable(run, RecoveryCause::AmbiguousEvidence);
        }
        let envelope = RecoveryEnvelope {
            version: 1,
            run_id: run.id.clone(),
            checkpoint_id: format!("checkpoint-{}", Uuid::now_v7()),
            continuable: true,
            cause: RecoveryCause::GateFailureExhausted,
            frontier: Some(RecoveryFrontier::IntegratedCandidate),
            responsible_role: Some(Role::Integrator),
            subject_id: Some("integration".to_owned()),
            failed_packet_keys: Vec::new(),
            evidence_ids: vec![candidate.handoff_document_id.clone()],
            permitted_scopes: Vec::new(),
            invalidated_checks: results
                .iter()
                .map(|result| result.gate_id.clone())
                .collect(),
            candidate_id: Some(candidate.id.clone()),
            reviewed_candidate_id: None,
            predecessor_candidate_id: candidate.predecessor_candidate_id.clone(),
            review_attempt_id: None,
            gate_result_ids: results.into_iter().map(|result| result.id).collect(),
            canonical_basis_digest: run.input_bundle_sha256.clone(),
        };
        if self.store.terminalize_needs_attention(&envelope).is_err() {
            self.terminalize_noncontinuable(run, RecoveryCause::UnsafeGit)
        } else {
            Ok(())
        }
    }

    fn terminalize_integrated_review_exhausted(
        &self,
        run: &RunView,
        candidate: &CandidateView,
        review: &AttemptView,
    ) -> AppResult<()> {
        let evidence = review.domain_document_id.clone().ok_or_else(|| {
            AppError::new(
                "review_evidence_missing",
                "integrated review has no document",
            )
        })?;
        self.store.terminalize_needs_attention(&RecoveryEnvelope {
            version: 1,
            run_id: run.id.clone(),
            checkpoint_id: format!("checkpoint-{}", Uuid::now_v7()),
            continuable: true,
            cause: RecoveryCause::IntegratedReviewExhausted,
            frontier: Some(RecoveryFrontier::IntegratedCandidate),
            responsible_role: Some(Role::Integrator),
            subject_id: Some("integration".to_owned()),
            failed_packet_keys: Vec::new(),
            evidence_ids: vec![evidence],
            permitted_scopes: Vec::new(),
            invalidated_checks: Vec::new(),
            candidate_id: Some(candidate.id.clone()),
            reviewed_candidate_id: Some(candidate.id.clone()),
            predecessor_candidate_id: candidate.predecessor_candidate_id.clone(),
            review_attempt_id: Some(review.id.clone()),
            gate_result_ids: Vec::new(),
            canonical_basis_digest: run.input_bundle_sha256.clone(),
        })
    }

    /// Returns whether an executed configured gate failed for this candidate.
    fn run_gates(&self, run: &RunView, candidate: &CandidateView, round: u32) -> AppResult<bool> {
        self.store.set_run_state(&run.id, RunState::Gates, None)?;
        let mut failed = false;
        for gate in self.store.gates(&run.id)? {
            if let Some(result) = self.store.gate_result(&gate.id, &candidate.id)? {
                if result.output_truncated {
                    self.terminalize_noncontinuable(run, RecoveryCause::AmbiguousEvidence)?;
                    return Ok(false);
                }
                if result.exit_code != 0 {
                    failed = true;
                }
                continue;
            }
            let hint = format!("gate-{}", Uuid::now_v7());
            let worktree = git::prepare_worktree(
                Path::new(&run.repository),
                &self.state_root,
                &run.id,
                &hint,
                &candidate.commit_oid,
            )?;
            let output = Command::new(&self.gate_shell)
                .args(["-lc", &gate.command])
                .current_dir(&worktree)
                .output()
                .map_err(|error| {
                    let cleanup = git::remove_worktree(Path::new(&run.repository), &worktree);
                    let detail = match cleanup {
                        Ok(()) => error.to_string(),
                        Err(cleanup) => {
                            format!("{error}; temporary gate worktree cleanup failed: {cleanup}")
                        }
                    };
                    AppError::new("gate_launch_failed", detail)
                })?;
            let mut raw = output.stdout;
            raw.extend_from_slice(&output.stderr);
            let signaled = output.status.code().is_none();
            let exit_code = output.status.code().unwrap_or(1);
            let (output, truncated) = bounded_output(&raw);
            let result = GateResult {
                id: format!("gate-result-{}", Uuid::now_v7()),
                gate_id: gate.id.clone(),
                candidate_id: candidate.id.clone(),
                round,
                exit_code,
                output,
                output_truncated: truncated,
                created_at: OffsetDateTime::now_utc().unix_timestamp(),
            };
            let clean = git::ensure_exact_clean(&worktree, &candidate.commit_oid);
            git::remove_worktree(Path::new(&run.repository), &worktree)?;
            clean?;
            self.store.record_gate_result(&result)?;
            if signaled || truncated {
                self.terminalize_noncontinuable(run, RecoveryCause::AmbiguousEvidence)?;
                return Ok(false);
            }
            if exit_code != 0 {
                failed = true;
            }
        }
        Ok(failed)
    }

    async fn execute_attempt(
        &self,
        run: &RunView,
        role: Role,
        subject_id: &str,
        round: u32,
        targeted: bool,
        prompt: &str,
        base_commit: Option<&str>,
        scopes: &[PathScope],
    ) -> AppResult<AttemptView> {
        let attempt = if let Some(attempt) = self
            .store
            .latest_attempt(&run.id, role, subject_id, round)?
        {
            if attempt.domain_document_id.is_some() && attempt.state.is_terminal() {
                return Ok(attempt);
            }
            if attempt.state.is_terminal() {
                return Err(AppError::new(
                    "attempt_retry_required",
                    format!(
                        "attempt {} ended without a domain result; use `vizier attempt retry {}`",
                        attempt.id, attempt.id
                    ),
                ));
            }
            attempt
        } else {
            self.prepare_attempt(
                run,
                role,
                subject_id,
                round,
                targeted,
                prompt,
                base_commit,
                scopes,
            )?
        };
        let result = self.runner.run_attempt(&self.store, &attempt.id).await?;
        if !role.is_writer() {
            let workspace = Path::new(&result.workspace_path);
            if role == Role::Assembler {
                if workspace.exists() {
                    fs::remove_dir_all(workspace)?;
                }
            } else {
                let expected = result.base_commit.as_deref().ok_or_else(|| {
                    AppError::new(
                        "attempt_basis_missing",
                        "read-only attempt has no exact candidate binding",
                    )
                })?;
                let clean = git::ensure_exact_clean(workspace, expected);
                git::remove_worktree(Path::new(&run.repository), workspace)?;
                clean?;
            }
        }
        Ok(result)
    }

    /// Persist the exact managed request before it is admitted to Nucleus.
    /// Keeping this construction on the workflow boundary lets recovery and
    /// tests inspect the same request that execution will admit.
    fn prepare_attempt(
        &self,
        run: &RunView,
        role: Role,
        subject_id: &str,
        round: u32,
        targeted: bool,
        prompt: &str,
        base_commit: Option<&str>,
        scopes: &[PathScope],
    ) -> AppResult<AttemptView> {
        let predecessor = plan_predecessor_round(role, subject_id, round)
            .map(|previous_round| self.require_round_attempt(run, role, subject_id, previous_round))
            .transpose()?;
        bounded_prompt(prompt)?;
        let hint = format!("workspace-{}", Uuid::now_v7());
        let workspace = if role == Role::Assembler {
            neutral_workspace(&self.state_root, &hint)?
        } else {
            let base = base_commit.ok_or_else(|| {
                AppError::new(
                    "attempt_basis_missing",
                    "repository role requires an exact Git candidate",
                )
            })?;
            git::prepare_worktree(
                Path::new(&run.repository),
                &self.state_root,
                &run.id,
                &hint,
                base,
            )?
        };
        self.runner.prepare_attempt(
            &self.store,
            &AttemptSpec {
                run,
                role,
                subject_id,
                round,
                targeted,
                prompt,
                workspace: &workspace,
                base_commit,
                allowed_scopes: scopes,
                predecessor_attempt_id: predecessor.as_ref().map(|attempt| attempt.id.as_str()),
            },
        )
    }

    fn require_round_attempt(
        &self,
        run: &RunView,
        role: Role,
        subject_id: &str,
        round: u32,
    ) -> AppResult<AttemptView> {
        let attempt = self
            .store
            .latest_attempt(&run.id, role, subject_id, round)?
            .ok_or_else(|| {
                AppError::new(
                    "plan_successor_lineage_missing",
                    format!(
                        "round {} {} attempt for subject {subject_id} is missing",
                        round,
                        role.as_str()
                    ),
                )
            })?;
        require_domain_result(&attempt, "plan successor predecessor")?;
        Ok(attempt)
    }

    fn plan_document_at_round(
        &self,
        run_id: &str,
        kind: &str,
        round: u32,
    ) -> AppResult<Option<DocumentView>> {
        Ok(self
            .store
            .documents(run_id, kind)?
            .into_iter()
            .find(|document| document.ordinal == round))
    }

    fn latest_delegation_revision(&self, run: &RunView) -> AppResult<u32> {
        Ok(self
            .store
            .documents(&run.id, "delegation_plan")?
            .into_iter()
            .map(|document| document.ordinal)
            .max()
            .unwrap_or(0))
    }

    fn plan_documents_at_round(
        &self,
        run_id: &str,
        kind: &str,
        round: u32,
    ) -> AppResult<Vec<DocumentView>> {
        Ok(self
            .store
            .documents(run_id, kind)?
            .into_iter()
            .filter(|document| document.ordinal == round)
            .collect())
    }

    fn require_current_packet_revision(&self, run: &RunView, round: u32) -> AppResult<()> {
        let packets = self.store.packets(&run.id)?;
        if packets.is_empty() {
            return Err(AppError::new(
                "delegation_missing",
                "assembler completed without a valid packet manifest",
            ));
        }
        for packet in packets {
            if self.store.document(&packet.plan_document_id)?.ordinal != round {
                return Err(AppError::new(
                    "delegation_revision_mismatch",
                    "current packet manifest does not match the assembled plan revision",
                ));
            }
        }
        Ok(())
    }

    fn freeze_writer_candidate(
        &self,
        run: &RunView,
        attempt: &AttemptView,
        kind: &str,
        subject: &str,
        round: u32,
        base: &str,
    ) -> AppResult<CandidateView> {
        if let Some(candidate) = self.store.candidate_for_attempt(&attempt.id)? {
            return Ok(candidate);
        }
        if attempt.state != AttemptState::Completed && attempt.domain_document_id.is_none() {
            return Err(AppError::new(
                "writer_not_complete",
                "writer has no committed handoff",
            ));
        }
        let handoff = attempt.domain_document_id.as_deref().ok_or_else(|| {
            AppError::new(
                "writer_handoff_missing",
                "writer completed without a committed handoff",
            )
        })?;
        let reference = if kind == "packet" {
            format!(
                "refs/vizier/runs/{}/packets/{subject}/round/{round}",
                run.id
            )
        } else {
            format!("refs/vizier/runs/{}/integration/round/{round}", run.id)
        };
        let commit = git::snapshot_worktree(
            Path::new(&run.repository),
            &self.state_root,
            Path::new(&attempt.workspace_path),
            base,
            &attempt.allowed_scopes,
            &reference,
            &format!("Vizier run {} {kind} {subject} round {round}", run.id),
        )?;
        let predecessor = if kind == "integration" {
            self.store
                .latest_candidate(&run.id, "integration", "integration")?
                .map(|candidate| candidate.id)
                .or_else(|| {
                    run.parent_run_id
                        .as_ref()
                        .and_then(|parent| self.store.recovery_envelope(parent).ok().flatten())
                        .and_then(|checkpoint| checkpoint.candidate_id)
                })
        } else {
            None
        };
        let candidate = self.store.record_candidate_with_predecessor(
            &run.id,
            subject,
            kind,
            round,
            base,
            &commit,
            &reference,
            handoff,
            &attempt.id,
            predecessor.as_deref(),
        )?;
        git::remove_worktree(
            Path::new(&run.repository),
            Path::new(&attempt.workspace_path),
        )?;
        Ok(candidate)
    }

    fn unit_planner_prompt(&self, run: &RunView, unit_id: &str) -> AppResult<String> {
        let mut prompt = format!(
            "# Vizier unit planning assignment\n\nRun: `{}`\nAssigned contract unit: `{unit_id}`\nExact source commit: `{}`\n\n",
            run.id, run.source_commit
        );
        self.append_base_bundle(run, &mut prompt, None)?;
        bounded_prompt(&prompt)?;
        Ok(prompt)
    }

    fn assembler_prompt(&self, run: &RunView) -> AppResult<String> {
        let mut prompt = format!(
            "# Vizier delegation assembly\n\nRun: `{}`\nExact source commit: `{}`\n\n",
            run.id, run.source_commit
        );
        self.append_base_bundle(run, &mut prompt, None)?;
        append_documents(&mut prompt, &self.store.documents(&run.id, "unit_plan")?);
        prompt.push_str("\nEvery packet must cover existing contract IDs, use safe repository-relative path scopes, and form an acyclic dependency graph. Overlapping scopes must be ordered by dependency.\n");
        bounded_prompt(&prompt)?;
        Ok(prompt)
    }

    fn assembler_revision_prompt(
        &self,
        run: &RunView,
        round: u32,
        feedback: &DocumentView,
    ) -> AppResult<String> {
        let previous_round = round.checked_sub(1).ok_or_else(|| {
            AppError::new(
                "plan_revision_invalid",
                "an initial delegation plan cannot be handled as a revision",
            )
        })?;
        let mut prompt = format!(
            "# Vizier targeted delegation-plan revision\n\nRun: `{}`\nRevision round: `{round}`\nExact source commit: `{}`\nRevise only the finding in the exact prior plan review and its directly affected packet seams. Preserve unaffected plan decisions.\n\n",
            run.id, run.source_commit
        );
        self.append_base_bundle(run, &mut prompt, None)?;
        append_documents(&mut prompt, &self.store.documents(&run.id, "unit_plan")?);
        let previous_delegation = self
            .plan_document_at_round(&run.id, "delegation_plan", previous_round)?
            .ok_or_else(|| {
                AppError::new(
                    "plan_revision_basis_missing",
                    "targeted plan revision requires the prior delegation plan",
                )
            })?;
        append_document(&mut prompt, &previous_delegation);
        append_documents(
            &mut prompt,
            &self.plan_documents_at_round(&run.id, "packet_plan", previous_round)?,
        );
        append_packet_manifest(&mut prompt, &self.store.packets(&run.id)?);
        prompt.push_str("\n## Exact prior plan-review feedback\n");
        append_document(&mut prompt, feedback);
        if let Some(scope) = self.store.review_scope(&feedback.id)? {
            append_review_scope(&mut prompt, &scope);
        }
        prompt.push_str("\nSubmit one complete successor delegation overview and packet manifest. Every packet must cover existing contract IDs, use safe repository-relative path scopes, and form an acyclic dependency graph. Overlapping scopes must be ordered by dependency.\n");
        bounded_prompt(&prompt)?;
        Ok(prompt)
    }

    fn plan_review_prompt(&self, run: &RunView, round: u32) -> AppResult<String> {
        let targeted = round > 0;
        let mut prompt = format!(
            "# Vizier {} assembled-plan review\n\nRun: `{}`\nReview subject ID: `{PLAN_REVIEW_SUBJECT}`\nReview subject: the current assembled delegation overview, current mechanical packet graph, and current packet-plan Markdown documents. Provisional unit-plan Markdown is not review material.\nReview round: `{round}`\n{}\n\n",
            if targeted { "targeted" } else { "one broad" },
            run.id,
            if targeted {
                "Recheck only the cited prior finding, the revised plan surface, and directly affected seams. Do not begin another broad audit."
            } else {
                "This is the only broad plan review."
            }
        );
        self.append_base_bundle(run, &mut prompt, None)?;
        // Unit plans are provisional assembler inputs, not plan-review material.
        let delegation = self
            .plan_document_at_round(&run.id, "delegation_plan", round)?
            .ok_or_else(|| {
                AppError::new(
                    "delegation_revision_missing",
                    format!("delegation plan revision {round} is missing"),
                )
            })?;
        append_document(&mut prompt, &delegation);
        append_documents(
            &mut prompt,
            &self.plan_documents_at_round(&run.id, "packet_plan", round)?,
        );
        append_packet_manifest(&mut prompt, &self.store.packets(&run.id)?);
        if targeted {
            let feedback = self
                .store
                .document_for_subject(&run.id, "plan_review", PLAN_REVIEW_SUBJECT, round - 1)?
                .ok_or_else(|| {
                    AppError::new(
                        "plan_review_feedback_missing",
                        "targeted plan recheck requires the exact prior plan-review Markdown",
                    )
                })?;
            prompt.push_str("\n## Exact prior plan-review feedback\n");
            append_document(&mut prompt, &feedback);
            if let Some(scope) = self.store.review_scope(&feedback.id)? {
                append_review_scope(&mut prompt, &scope);
            }
        }
        bounded_prompt(&prompt)?;
        Ok(prompt)
    }

    fn packet_implementation_prompt(
        &self,
        run: &RunView,
        packet: &PacketView,
        targeted: bool,
    ) -> AppResult<String> {
        let mut prompt = format!(
            "# Vizier packet implementation\n\nRun: `{}`\nPacket: `{}`\nMode: `{}`\n\n",
            run.id,
            packet.key,
            if targeted {
                "targeted_remediation"
            } else {
                "initial_implementation"
            }
        );
        self.append_base_bundle(run, &mut prompt, Some(&packet.contract_unit_ids))?;
        append_documents(
            &mut prompt,
            &self.store.documents(&run.id, "delegation_plan")?,
        );
        append_document(&mut prompt, &self.store.document(&packet.plan_document_id)?);
        if targeted
            && let Some(review) = self.store.document_for_subject(
                &run.id,
                "packet_review",
                &packet.key,
                packet.remediation_round - 1,
            )?
        {
            append_document(&mut prompt, &review);
            if let Some(scope) = self.store.review_scope(&review.id)? {
                append_review_scope(&mut prompt, &scope);
            }
        }
        append_candidate_context(&mut prompt, &self.store, run, packet)?;
        bounded_prompt(&prompt)?;
        Ok(prompt)
    }

    fn packet_review_prompt(
        &self,
        run: &RunView,
        packet: &PacketView,
        candidate: &CandidateView,
        targeted: bool,
    ) -> AppResult<String> {
        let mut prompt = format!(
            "# Vizier {} packet review\n\nRun: `{}`\nPacket: `{}`\nExact candidate commit: `{}`\n\n",
            if targeted { "targeted" } else { "one broad" },
            run.id,
            packet.key,
            candidate.commit_oid
        );
        self.append_base_bundle(run, &mut prompt, Some(&packet.contract_unit_ids))?;
        append_document(&mut prompt, &self.store.document(&packet.plan_document_id)?);
        append_document(
            &mut prompt,
            &self.store.document(&candidate.handoff_document_id)?,
        );
        if targeted
            && let Some(previous) = self.store.document_for_subject(
                &run.id,
                "packet_review",
                &packet.key,
                candidate.round.saturating_sub(1),
            )?
        {
            append_document(&mut prompt, &previous);
            if let Some(scope) = self.store.review_scope(&previous.id)? {
                append_review_scope(&mut prompt, &scope);
            }
        }
        bounded_prompt(&prompt)?;
        Ok(prompt)
    }

    fn integration_prompt(
        &self,
        run: &RunView,
        packets: &[PacketView],
        targeted: bool,
        round: u32,
    ) -> AppResult<String> {
        let mut prompt = format!(
            "# Vizier integration {}\n\nRun: `{}`\nMode: `{}`\nIntegrate accepted packet candidates and make only necessary seam changes inside the union of packet scopes.\n\n",
            if targeted { "remediation" } else { "pass" },
            run.id,
            if targeted {
                "targeted_remediation"
            } else {
                "initial_integration"
            }
        );
        self.append_base_bundle(run, &mut prompt, None)?;
        append_documents(
            &mut prompt,
            &self.store.documents(&run.id, "delegation_plan")?,
        );
        for packet in packets {
            append_document(&mut prompt, &self.store.document(&packet.plan_document_id)?);
            if let Some(id) = &packet.current_candidate_id {
                let candidate = self.store.candidate(id)?;
                append_document(
                    &mut prompt,
                    &self.store.document(&candidate.handoff_document_id)?,
                );
            }
        }
        if targeted {
            self.append_gate_remediation_evidence(run, round, &mut prompt)?;
            self.append_inherited_gate_remediation_evidence(run, round, &mut prompt)?;
            self.append_inherited_integrated_review_remediation_evidence(run, round, &mut prompt)?;
        }
        if targeted
            && let Some(previous) = self.store.document_for_subject(
                &run.id,
                "integrated_review",
                "integration",
                self.store
                    .latest_candidate(&run.id, "integration", "integration")?
                    .map_or(0, |candidate| candidate.round),
            )?
        {
            append_document(&mut prompt, &previous);
            if let Some(scope) = self.store.review_scope(&previous.id)? {
                append_review_scope(&mut prompt, &scope);
            }
        }
        bounded_prompt(&prompt)?;
        Ok(prompt)
    }

    /// Adds the exact parent review finding to the first child correction.
    /// The child deliberately does not copy private parent documents, so the
    /// checkpoint is the durable authority for this cross-run reference.
    fn append_inherited_integrated_review_remediation_evidence(
        &self,
        run: &RunView,
        round: u32,
        prompt: &mut String,
    ) -> AppResult<()> {
        if round != 0 {
            return Ok(());
        }
        let Some(parent_id) = run.parent_run_id.as_deref() else {
            return Ok(());
        };
        let Some(checkpoint) = self.store.recovery_envelope(parent_id)? else {
            return Ok(());
        };
        if checkpoint.cause != RecoveryCause::IntegratedReviewExhausted {
            return Ok(());
        }
        let attempt_id = checkpoint.review_attempt_id.as_deref().ok_or_else(|| {
            AppError::new(
                "continuation_evidence_missing",
                "review checkpoint lacks review attempt",
            )
        })?;
        let attempt = self.store.attempt(attempt_id)?;
        let document_id = attempt.domain_document_id.as_deref().ok_or_else(|| {
            AppError::new(
                "continuation_evidence_missing",
                "review checkpoint lacks review document",
            )
        })?;
        let document = self.store.document(document_id)?;
        prompt.push_str("\n## Exact inherited integrated-review feedback\n");
        append_document(prompt, &document);
        let scope = self.store.review_scope(document_id)?.ok_or_else(|| {
            AppError::new(
                "continuation_evidence_missing",
                "review checkpoint lacks persisted review scope",
            )
        })?;
        append_review_scope(prompt, &scope);
        Ok(())
    }

    fn append_gate_remediation_evidence(
        &self,
        run: &RunView,
        round: u32,
        prompt: &mut String,
    ) -> AppResult<()> {
        let Some(previous) = round.checked_sub(1) else {
            return Ok(());
        };
        let Some(predecessor) =
            self.store
                .candidate_at_round(&run.id, "integration", "integration", previous)?
        else {
            return Ok(());
        };
        for gate in self.store.gates(&run.id)? {
            let Some(result) = self.store.gate_result(&gate.id, &predecessor.id)? else {
                continue;
            };
            if result.exit_code == 0 {
                continue;
            }
            let _ = writeln!(
                prompt,
                "\n## Executed gate remediation evidence\n\n- predecessor candidate: `{}` (commit `{}`)\n- gate name: `{}`\n- command identity: `{}`\n- exit code: `{}`\n- output truncated: `{}`\n- round: `{}`\n\nBounded exact output:\n```text\n{}\n```",
                predecessor.id,
                predecessor.commit_oid,
                gate.name,
                gate.command,
                result.exit_code,
                result.output_truncated,
                result.round,
                result.output,
            );
        }
        Ok(())
    }

    fn append_inherited_gate_remediation_evidence(
        &self,
        run: &RunView,
        round: u32,
        prompt: &mut String,
    ) -> AppResult<()> {
        if round != 0 {
            return Ok(());
        }
        let Some(parent_id) = run.parent_run_id.as_deref() else {
            return Ok(());
        };
        let Some(checkpoint) = self.store.recovery_envelope(parent_id)? else {
            return Ok(());
        };
        if checkpoint.cause != RecoveryCause::GateFailureExhausted {
            return Ok(());
        }
        let candidate_id = checkpoint.candidate_id.as_deref().ok_or_else(|| {
            AppError::new(
                "continuation_evidence_missing",
                "gate checkpoint lacks candidate",
            )
        })?;
        let candidate = self.store.candidate(candidate_id)?;
        for gate in self.store.gates(parent_id)? {
            let Some(result) = self.store.gate_result(&gate.id, &candidate.id)? else {
                continue;
            };
            if checkpoint.gate_result_ids.contains(&result.id) && result.exit_code != 0 {
                let _ = writeln!(
                    prompt,
                    "\n## Inherited executed gate remediation evidence\n\n- predecessor candidate: `{}` (commit `{}`)\n- gate name: `{}`\n- command identity: `{}`\n- exit code: `{}`\n- output truncated: `{}`\n- round: `{}`\n\nBounded exact output:\n```text\n{}\n```",
                    candidate.id,
                    candidate.commit_oid,
                    gate.name,
                    gate.command,
                    result.exit_code,
                    result.output_truncated,
                    result.round,
                    result.output
                );
            }
        }
        Ok(())
    }

    fn integrated_review_prompt(
        &self,
        run: &RunView,
        candidate: &CandidateView,
        targeted: bool,
    ) -> AppResult<String> {
        let mut prompt = format!(
            "# Vizier {} integrated review\n\nRun: `{}`\nExact integrated candidate commit: `{}`\nThis review follows the configured mechanical gates.\n\n",
            if targeted { "targeted" } else { "one broad" },
            run.id,
            candidate.commit_oid
        );
        self.append_base_bundle(run, &mut prompt, None)?;
        for kind in [
            "delegation_plan",
            "packet_plan",
            "implementation_handoff",
            "packet_review",
            "integration_handoff",
        ] {
            append_documents(&mut prompt, &self.store.documents(&run.id, kind)?);
        }
        if targeted {
            let previous = if candidate.round > 0 {
                self.store.document_for_subject(
                    &run.id,
                    "integrated_review",
                    "integration",
                    candidate.round - 1,
                )?
            } else if let Some(parent_id) = run.parent_run_id.as_deref() {
                let checkpoint = self.store.recovery_envelope(parent_id)?;
                if checkpoint.as_ref().is_some_and(|checkpoint| {
                    checkpoint.cause == RecoveryCause::IntegratedReviewExhausted
                }) {
                    let attempt_id = checkpoint
                        .and_then(|checkpoint| checkpoint.review_attempt_id)
                        .ok_or_else(|| {
                            AppError::new(
                                "continuation_evidence_missing",
                                "review checkpoint lacks review attempt",
                            )
                        })?;
                    let attempt = self.store.attempt(&attempt_id)?;
                    match attempt.domain_document_id {
                        Some(id) => Some(self.store.document(&id)?),
                        None => None,
                    }
                } else {
                    self.integrated_review_document_in_candidate_lineage(candidate)?
                }
            } else {
                self.integrated_review_document_in_candidate_lineage(candidate)?
            };
            if let Some(previous) = previous {
                append_document(&mut prompt, &previous);
                if let Some(scope) = self.store.review_scope(&previous.id)? {
                    append_review_scope(&mut prompt, &scope);
                }
            }
        }
        bounded_prompt(&prompt)?;
        Ok(prompt)
    }

    fn integrated_review_in_candidate_lineage(&self, candidate: &CandidateView) -> AppResult<bool> {
        Ok(self
            .integrated_review_document_in_candidate_lineage(candidate)?
            .is_some())
    }

    /// Finds persisted review evidence by following the explicit candidate
    /// predecessor links, including links that cross into a parent run.
    fn integrated_review_document_in_candidate_lineage(
        &self,
        candidate: &CandidateView,
    ) -> AppResult<Option<DocumentView>> {
        let mut current = Some(candidate.clone());
        while let Some(value) = current {
            if let Some(review) = self.store.document_for_subject(
                &value.run_id,
                "integrated_review",
                "integration",
                value.round,
            )? {
                return Ok(Some(review));
            }
            current = value
                .predecessor_candidate_id
                .as_deref()
                .map(|id| self.store.candidate(id))
                .transpose()?;
        }
        Ok(None)
    }

    fn append_base_bundle(
        &self,
        run: &RunView,
        prompt: &mut String,
        only_contracts: Option<&[String]>,
    ) -> AppResult<()> {
        append_documents(prompt, &self.store.documents(&run.id, "brief")?);
        append_documents(prompt, &self.store.documents(&run.id, "terminology")?);
        let contracts = self.store.documents(&run.id, "contract_unit")?;
        for contract in contracts {
            if only_contracts.is_none_or(|ids| {
                contract
                    .subject_id
                    .as_ref()
                    .is_some_and(|id| ids.contains(id))
            }) {
                append_document(prompt, &contract);
            }
        }
        Ok(())
    }

    fn acquire_driver_lock(&self) -> AppResult<std::fs::File> {
        private_directory(&self.state_root)?;
        let path = self.state_root.join("driver.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options.open(path)?;
        file.try_lock_exclusive().map_err(|_| {
            AppError::new(
                "workflow_already_driven",
                "another Vizier process is already driving the active run",
            )
        })?;
        Ok(file)
    }
}

async fn join_one(jobs: &mut JoinSet<AppResult<()>>) -> AppResult<()> {
    jobs.join_next()
        .await
        .ok_or_else(|| AppError::new("workflow_join_failed", "missing concurrent task"))?
        .map_err(|error| AppError::new("workflow_task_failed", error.to_string()))??;
    Ok(())
}

fn require_domain_result(attempt: &AttemptView, label: &str) -> AppResult<()> {
    if attempt.domain_document_id.is_none() {
        Err(AppError::new(
            "domain_result_missing",
            format!("{label} was not durably submitted"),
        ))
    } else {
        Ok(())
    }
}

fn validate_retry_target(store: &Store, run: &RunView, attempt: &AttemptView) -> AppResult<()> {
    if run.cancel_requested
        || run.state.is_terminal()
        || run.final_candidate_id.is_some()
        || run.final_ref.is_some()
    {
        return Err(AppError::new(
            "run_not_retryable",
            "a cancelled or successfully completed run cannot be reopened by attempt retry",
        ));
    }
    if !store.is_current_resultless_leaf(attempt)? {
        return Err(AppError::new(
            "attempt_not_current_leaf",
            "attempt retry requires the current resultless leaf for its run, role, and subject",
        ));
    }
    Ok(())
}

fn require_review_result(attempt: &AttemptView) -> AppResult<Disposition> {
    require_domain_result(attempt, "review")?;
    attempt.disposition.ok_or_else(|| {
        AppError::new(
            "review_disposition_missing",
            "review has no routing disposition",
        )
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlanReviewRoute {
    Accepted,
    Revise { next_round: u32 },
    NeedsAttention { detail: &'static str },
}

fn route_plan_review(
    disposition: Disposition,
    round: u32,
    remediation_limit: u32,
) -> PlanReviewRoute {
    match disposition {
        Disposition::Accepted => PlanReviewRoute::Accepted,
        Disposition::ChangesRequested if round < remediation_limit => PlanReviewRoute::Revise {
            next_round: round + 1,
        },
        Disposition::ChangesRequested => PlanReviewRoute::NeedsAttention {
            detail: "assembled-plan changes remain after the bounded remediation allowance",
        },
        Disposition::Blocked => PlanReviewRoute::NeedsAttention {
            detail: "the assembled-plan review reported a blocker requiring caller attention",
        },
    }
}

fn plan_predecessor_round(role: Role, subject_id: &str, round: u32) -> Option<u32> {
    if round > 0
        && matches!(
            (role, subject_id),
            (Role::Assembler, DELEGATION_SUBJECT) | (Role::PlanReviewer, PLAN_REVIEW_SUBJECT)
        )
    {
        Some(round - 1)
    } else {
        None
    }
}

fn state_for_role(role: Role) -> RunState {
    match role {
        Role::Planner => RunState::Planning,
        Role::Assembler => RunState::Assembling,
        Role::PlanReviewer => RunState::PlanReview,
        Role::Implementor => RunState::Implementing,
        Role::PacketReviewer => RunState::PacketReview,
        Role::Integrator => RunState::Integrating,
        Role::IntegratedReviewer => RunState::FinalReview,
    }
}

fn union_scopes(packets: &[PacketView]) -> Vec<PathScope> {
    let mut map = BTreeMap::new();
    for packet in packets {
        for scope in &packet.path_scopes {
            map.entry((scope.path.clone(), scope.recursive))
                .or_insert_with(|| scope.clone());
        }
    }
    map.into_values().collect()
}

fn append_documents(prompt: &mut String, documents: &[DocumentView]) {
    for document in documents {
        append_document(prompt, document);
    }
}

fn append_document(prompt: &mut String, document: &DocumentView) {
    let _ = write!(
        prompt,
        "\n## Exact document `{}`\n\nKind: `{}`  \nSubject: `{}`  \nOrdinal: `{}`  \nSHA-256: `{}`  \nByte length: `{}`\n\n<vizier-exact-markdown>\n",
        document.id,
        document.kind,
        document.subject_id.as_deref().unwrap_or(""),
        document.ordinal,
        document.sha256,
        document.markdown.as_bytes().len()
    );
    prompt.push_str(document.markdown.as_str());
    prompt.push_str("\n</vizier-exact-markdown>\n");
}

fn append_review_scope(prompt: &mut String, scope: &crate::model::ReviewScopeView) {
    let _ = writeln!(
        prompt,
        "\n## Persisted mechanical review scope\n\n- review attempt: `{}`\n- review document: `{}`\n- affected packets: {:?}\n- contract units: {:?}",
        scope.review_attempt_id,
        scope.review_document_id,
        scope.affected_packet_keys,
        scope.contract_unit_ids
    );
}

fn append_packet_manifest(prompt: &mut String, packets: &[PacketView]) {
    prompt.push_str("\n## Mechanical packet manifest\n\n");
    for packet in packets {
        let _ = writeln!(
            prompt,
            "- `{}`: contracts={:?}; depends_on={:?}; path_scopes={:?}\n",
            packet.key, packet.contract_unit_ids, packet.depends_on, packet.path_scopes
        );
    }
}

fn append_candidate_context(
    prompt: &mut String,
    store: &Store,
    run: &RunView,
    packet: &PacketView,
) -> AppResult<()> {
    for dependency in &packet.depends_on {
        let value = store.packet(&run.id, dependency)?;
        if let Some(id) = value.current_candidate_id {
            let candidate = store.candidate(&id)?;
            let _ = writeln!(
                prompt,
                "\nDependency `{dependency}` accepted candidate: `{}`\n",
                candidate.commit_oid
            );
            append_document(prompt, &store.document(&candidate.handoff_document_id)?);
        }
    }
    Ok(())
}

fn bounded_prompt(prompt: &str) -> AppResult<()> {
    if prompt.len() > MAX_INPUT_BUNDLE_BYTES {
        Err(AppError::new(
            "agent_prompt_too_large",
            format!("assembled exact Markdown prompt exceeds {MAX_INPUT_BUNDLE_BYTES} bytes"),
        ))
    } else {
        Ok(())
    }
}

fn bounded_output(bytes: &[u8]) -> (String, bool) {
    let truncated = bytes.len() > MAX_GATE_OUTPUT_BYTES;
    let bytes = if truncated {
        &bytes[..MAX_GATE_OUTPUT_BYTES]
    } else {
        bytes
    };
    (String::from_utf8_lossy(bytes).into_owned(), truncated)
}

fn private_directory(path: &Path) -> AppResult<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DELEGATION_SUBJECT, PLAN_REVIEW_SUBJECT, PlanReviewRoute, Workflow, plan_predecessor_round,
        route_plan_review, validate_retry_target,
    };
    use crate::contracts::ManagedSubmission;
    use crate::error::{AppError, AppResult};
    use crate::model::{
        AttemptState, CandidateView, DelegationSubmission, Disposition, HandoffOutcome,
        HandoffSubmission, NewRun, OpaqueMarkdown, PacketSubmission, PathScope, ReviewSubmission,
        Role, RunState,
    };
    use crate::nucleus::AgentRunner;
    use crate::store::{NewAttempt, Store};
    use std::process::Command;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn markdown(value: &str) -> TestResult<OpaqueMarkdown> {
        Ok(OpaqueMarkdown::from_text(value)?)
    }

    fn require_error<T>(result: AppResult<T>, message: &str) -> TestResult<AppError> {
        match result {
            Err(error) => Ok(error),
            Ok(_) => Err(message.to_owned().into()),
        }
    }

    fn attempt<'a>(
        run: &'a crate::model::RunView,
        role: Role,
        subject_id: &'a str,
        round: u32,
        targeted: bool,
        job: &'a str,
    ) -> NewAttempt<'a> {
        NewAttempt {
            run_id: &run.id,
            role,
            subject_id,
            round,
            targeted,
            nucleus_job_id: job,
            request_bytes: b"{}",
            request_sha256: "digest",
            toolset_name: "test",
            workspace_path: "/tmp",
            base_commit: Some("source"),
            allowed_scopes: &[],
            predecessor_attempt_id: None,
        }
    }

    fn complete(store: &Store, id: &str) -> TestResult {
        store.set_attempt_runtime(id, AttemptState::Completed, None)?;
        Ok(())
    }

    fn assert_persisted_request_scope(
        attempt: &crate::model::AttemptView,
        scope: &crate::model::ReviewScopeView,
    ) -> TestResult {
        let request: nucleus_core::JobRequestV1 = serde_json::from_slice(&attempt.request_bytes)?;
        let actual = request
            .prompt
            .rsplit_once("\n## Persisted mechanical review scope\n\n")
            .ok_or("persisted request has no review scope")?
            .1
            .trim_end();
        let expected = format!(
            "- review attempt: `{}`\n- review document: `{}`\n- affected packets: {:?}\n- contract units: {:?}",
            scope.review_attempt_id,
            scope.review_document_id,
            scope.affected_packet_keys,
            scope.contract_unit_ids,
        );
        assert_eq!(actual, expected);
        Ok(())
    }

    fn test_repository() -> TestResult<(tempfile::TempDir, String)> {
        let directory = tempfile::tempdir()?;
        for arguments in [
            vec!["init", "-q"],
            vec!["config", "user.email", "vizier-test@example.invalid"],
            vec!["config", "user.name", "Vizier Test"],
        ] {
            let status = std::process::Command::new("git")
                .args(arguments)
                .current_dir(directory.path())
                .status()?;
            if !status.success() {
                return Err("unable to initialize test Git repository".into());
            }
        }
        std::fs::write(directory.path().join("README.md"), "test\n")?;
        if !std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(directory.path())
            .status()?
            .success()
            || !std::process::Command::new("git")
                .args(["commit", "-qm", "test source"])
                .current_dir(directory.path())
                .status()?
                .success()
        {
            return Err("unable to commit test Git source".into());
        }
        let output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(directory.path())
            .output()?;
        if !output.status.success() {
            return Err("unable to identify test Git source".into());
        }
        Ok((
            directory,
            String::from_utf8(output.stdout)?.trim().to_owned(),
        ))
    }

    fn delegation(round: u32) -> DelegationSubmission {
        DelegationSubmission {
            overview_markdown: format!("# Assembled revision {round}\n"),
            packets: vec![PacketSubmission {
                packet_key: "packet".to_owned(),
                contract_unit_ids: vec!["unit".to_owned()],
                depends_on: Vec::new(),
                path_scopes: vec![PathScope {
                    path: "src".to_owned(),
                    recursive: true,
                }],
                plan_markdown: format!("# Packet revision {round}\n"),
            }],
        }
    }

    fn integration_fixture(
        run_id: &str,
        gates: Vec<(String, String)>,
        remediation_limit: u32,
    ) -> TestResult<(
        tempfile::TempDir,
        Store,
        crate::model::RunView,
        CandidateView,
    )> {
        let (repository, source_commit) = test_repository()?;
        let store = Store::new(repository.path().join("vizier.db"));
        store.initialize()?;
        let run = store.create_run(&NewRun {
            id: run_id.to_owned(),
            request_key: None,
            repository: repository.path().to_string_lossy().into_owned(),
            source_commit: source_commit.clone(),
            brief: markdown("# Brief\n")?,
            terminology: markdown("# Terms\n")?,
            contracts: vec![("unit".to_owned(), markdown("# Contract\n")?)],
            gates,
            remediation_limit,
        })?;
        store.record_document(
            &run.id,
            "unit_plan",
            Some("unit"),
            0,
            &markdown("# Plan\n")?,
        )?;
        store.set_run_state(&run.id, RunState::Assembling, None)?;
        let assembler = store.create_attempt(&attempt(
            &run,
            Role::Assembler,
            DELEGATION_SUBJECT,
            0,
            false,
            "assembler",
        ))?;
        store.commit_managed_submission(
            &assembler.id,
            "assembler",
            "call",
            "args",
            "result",
            &ManagedSubmission::Delegation(delegation(0)),
        )?;
        complete(&store, &assembler.id)?;
        let packet_handoff = store.record_document(
            &run.id,
            "implementation_handoff",
            Some("packet"),
            0,
            &markdown("# Packet handoff\n")?,
        )?;
        let packet_writer = store.create_attempt(&attempt(
            &run,
            Role::Implementor,
            "packet",
            0,
            false,
            "packet-writer",
        ))?;
        let packet_candidate = store.record_candidate(
            &run.id,
            "packet",
            "packet",
            0,
            &source_commit,
            &source_commit,
            "HEAD",
            &packet_handoff.id,
            &packet_writer.id,
        )?;
        store.set_packet_state(
            &run.id,
            "packet",
            crate::model::PacketState::Accepted,
            Some(&packet_candidate.id),
            0,
        )?;
        let handoff = store.record_document(
            &run.id,
            "integration_handoff",
            Some("integration"),
            0,
            &markdown("# Integration handoff\n")?,
        )?;
        let writer = store.create_attempt(&attempt(
            &run,
            Role::Integrator,
            "integration",
            0,
            false,
            "integrator-zero",
        ))?;
        let status = Command::new("git")
            .args(["update-ref", "refs/test/integration/0", &source_commit])
            .current_dir(repository.path())
            .status()?;
        assert!(status.success());
        let candidate = store.record_candidate(
            &run.id,
            "integration",
            "integration",
            0,
            &source_commit,
            &source_commit,
            "refs/test/integration/0",
            &handoff.id,
            &writer.id,
        )?;
        Ok((repository, store, run, candidate))
    }

    #[test]
    fn executed_gate_failure_is_candidate_bound_remediation_evidence() -> TestResult {
        let (repository, source_commit) = test_repository()?;
        let store = Store::new(repository.path().join("vizier.db"));
        store.initialize()?;
        let run = store.create_run(&NewRun {
            id: "run-gate-remediation".to_owned(),
            request_key: None,
            repository: repository.path().to_string_lossy().into_owned(),
            source_commit: source_commit.clone(),
            brief: markdown("# Brief\n")?,
            terminology: markdown("# Terms\n")?,
            contracts: vec![("unit".to_owned(), markdown("# Contract\n")?)],
            gates: vec![
                ("compile".to_owned(), "printf first; exit 7".to_owned()),
                ("test".to_owned(), "printf second".to_owned()),
            ],
            remediation_limit: 1,
        })?;
        let handoff = store.record_document(
            &run.id,
            "integration_handoff",
            Some("integration"),
            0,
            &markdown("# Handoff\n")?,
        )?;
        let writer = store.create_attempt(&attempt(
            &run,
            Role::Integrator,
            "integration",
            0,
            false,
            "integrator-zero",
        ))?;
        let predecessor = store.record_candidate(
            &run.id,
            "integration",
            "integration",
            0,
            &source_commit,
            &source_commit,
            "refs/test/integration/0",
            &handoff.id,
            &writer.id,
        )?;
        let workflow = Workflow::new(store.clone(), AgentRunner::for_current_user());
        assert!(workflow.run_gates(&run, &predecessor, 0)?);
        let gates = store.gates(&run.id)?;
        let failure = store
            .gate_result(&gates[0].id, &predecessor.id)?
            .ok_or("failed gate result was not recorded")?;
        assert_eq!(failure.exit_code, 7);
        assert_eq!(failure.output, "first");
        assert!(store.gate_result(&gates[1].id, &predecessor.id)?.is_some());

        let prompt = workflow.integration_prompt(&run, &[], true, 1)?;
        for expected in [
            &format!("predecessor candidate: `{}`", predecessor.id),
            "gate name: `compile`",
            "command identity: `printf first; exit 7`",
            "exit code: `7`",
            "output truncated: `false`",
            "round: `0`",
            "first",
        ] {
            assert!(prompt.contains(expected), "prompt lacks {expected}");
        }

        let successor_handoff = store.record_document(
            &run.id,
            "integration_handoff",
            Some("integration"),
            1,
            &markdown("# Successor handoff\n")?,
        )?;
        let successor_writer = store.create_attempt(&attempt(
            &run,
            Role::Integrator,
            "integration",
            1,
            true,
            "integrator-one",
        ))?;
        let successor = store.record_candidate(
            &run.id,
            "integration",
            "integration",
            1,
            &source_commit,
            &source_commit,
            "refs/test/integration/1",
            &successor_handoff.id,
            &successor_writer.id,
        )?;
        assert!(workflow.run_gates(&run, &successor, 1)?);
        for gate in &gates {
            let result = store
                .gate_result(&gate.id, &successor.id)?
                .ok_or("successor did not rerun every configured gate")?;
            assert_eq!(result.candidate_id, successor.id);
        }
        let predecessor_result = store
            .gate_result(&gates[0].id, &predecessor.id)?
            .ok_or("predecessor gate result was not recorded")?;
        let successor_result = store
            .gate_result(&gates[0].id, &successor.id)?
            .ok_or("successor gate result was not recorded")?;
        assert_ne!(predecessor_result.id, successor_result.id);
        Ok(())
    }

    #[test]
    fn gate_continuation_review_mode_follows_persisted_candidate_ancestry() -> TestResult {
        let (repository, store, run, reviewed_candidate) = integration_fixture(
            "run-gate-ancestry",
            vec![("gate".to_owned(), "test -f fixed || exit 7".to_owned())],
            1,
        )?;
        let baseline = Workflow::new(store.clone(), AgentRunner::for_current_user());
        assert!(!baseline.integrated_review_in_candidate_lineage(&reviewed_candidate)?);
        assert!(
            baseline
                .integrated_review_prompt(&run, &reviewed_candidate, false)?
                .starts_with("# Vizier one broad integrated review")
        );
        let review = store.record_document(
            &run.id,
            "integrated_review",
            Some("integration"),
            reviewed_candidate.round,
            &markdown("# Earlier integrated review\n")?,
        )?;
        let parent_handoff = store.record_document(
            &run.id,
            "integration_handoff",
            Some("integration"),
            1,
            &markdown("# Gate-failing successor\n")?,
        )?;
        let parent_writer = store.create_attempt(&attempt(
            &run,
            Role::Integrator,
            "integration",
            1,
            true,
            "gate-successor-writer",
        ))?;
        let status = Command::new("git")
            .args([
                "update-ref",
                "refs/test/integration/gate-successor",
                &reviewed_candidate.commit_oid,
            ])
            .current_dir(repository.path())
            .status()?;
        assert!(status.success());
        let gate_candidate = store.record_candidate_with_predecessor(
            &run.id,
            "integration",
            "integration",
            1,
            &reviewed_candidate.commit_oid,
            &reviewed_candidate.commit_oid,
            "refs/test/integration/gate-successor",
            &parent_handoff.id,
            &parent_writer.id,
            Some(&reviewed_candidate.id),
        )?;
        let workflow = Workflow::new(
            store.clone(),
            AgentRunner::with_socket(repository.path().join("unavailable-nucleus.sock")),
        );
        assert!(workflow.run_gates(&run, &gate_candidate, 1)?);
        workflow.terminalize_gate_exhausted(&run, &gate_candidate)?;
        let child = store.admit_continuation(&run.id, "gate-ancestry-child", 1)?;

        // The linked child starts with its counted round-zero integrator.
        let _ = require_error(
            tokio::runtime::Runtime::new()?.block_on(workflow.run_integrated_continuation(&child)),
            "unavailable Nucleus must leave the first child integrator durable",
        )?;
        let child_writer = store
            .latest_attempt(&child.id, Role::Integrator, "integration", 0)?
            .ok_or("child did not begin at the integrator")?;

        std::fs::write(repository.path().join("fixed"), "fixed\n")?;
        let status = Command::new("git")
            .args(["add", "fixed"])
            .current_dir(repository.path())
            .status()?;
        assert!(status.success());
        let status = Command::new("git")
            .args(["commit", "-qm", "fix child gate"])
            .current_dir(repository.path())
            .status()?;
        assert!(status.success());
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repository.path())
            .output()?;
        let child_commit = String::from_utf8(output.stdout)?.trim().to_owned();
        store.commit_managed_submission(
            &child_writer.id,
            &child_writer.nucleus_job_id,
            "call",
            "args",
            "result",
            &ManagedSubmission::Handoff(HandoffSubmission {
                outcome: HandoffOutcome::Ready,
                markdown: "# Child handoff\n".to_owned(),
            }),
        )?;
        complete(&store, &child_writer.id)?;
        let child_handoff = store
            .attempt(&child_writer.id)?
            .domain_document_id
            .ok_or("child handoff was not recorded")?;
        store.record_candidate_with_predecessor(
            &child.id,
            "integration",
            "integration",
            0,
            &gate_candidate.commit_oid,
            &child_commit,
            "refs/test/integration/child",
            &child_handoff,
            &child_writer.id,
            Some(&gate_candidate.id),
        )?;

        // Its gates now pass, so the first reviewer dispatch proves that the
        // gate checkpoint followed the reviewed predecessor across the run
        // boundary rather than choosing a mode from the round number.
        let _ = require_error(
            tokio::runtime::Runtime::new()?.block_on(workflow.run_integrated_continuation(&child)),
            "unavailable Nucleus must leave the child review durable",
        )?;
        let child_review = store
            .latest_attempt(&child.id, Role::IntegratedReviewer, "integration", 0)?
            .ok_or("child did not dispatch its independent review")?;
        assert!(child_review.targeted);
        let request: nucleus_core::JobRequestV1 =
            serde_json::from_slice(&child_review.request_bytes)?;
        assert!(request.prompt.contains(review.markdown.as_str()));
        Ok(())
    }

    #[test]
    fn integrated_review_continuation_first_integrator_gets_exact_parent_feedback() -> TestResult {
        let (_repository, store, run, candidate) =
            integration_fixture("run-inherited-review-feedback", Vec::new(), 1)?;
        store.set_run_state(&run.id, RunState::FinalReview, None)?;
        let review = store.create_attempt(&attempt(
            &run,
            Role::IntegratedReviewer,
            "integration",
            0,
            false,
            "integrated-review",
        ))?;
        store.commit_managed_submission(
            &review.id,
            "integrated-review",
            "call",
            "args",
            "result",
            &ManagedSubmission::Review(ReviewSubmission {
                disposition: Disposition::ChangesRequested,
                affected_packet_keys: vec!["packet".to_owned()],
                contract_unit_ids: vec!["unit".to_owned()],
                markdown: "# Exact inherited finding\n".to_owned(),
            }),
        )?;
        complete(&store, &review.id)?;
        let review = store.attempt(&review.id)?;
        let workflow = Workflow::new(store.clone(), AgentRunner::for_current_user());
        workflow.terminalize_integrated_review_exhausted(&run, &candidate, &review)?;
        let child = store.admit_continuation(&run.id, "inherited-review-key", 1)?;

        let prompt = workflow.integration_prompt(&child, &[], true, 0)?;
        assert!(prompt.contains("# Exact inherited finding"));
        assert!(prompt.contains("## Persisted mechanical review scope"));
        assert!(prompt.contains("affected packets: [\"packet\"]"));
        Ok(())
    }

    #[test]
    fn gate_failure_prepares_successor_then_reruns_all_gates_before_independent_recheck()
    -> TestResult {
        let (repository, store, run, predecessor) = integration_fixture(
            "run-gate-workflow",
            vec![
                (
                    "compile".to_owned(),
                    "test -f fixed && printf repaired || { printf broken; exit 7; }".to_owned(),
                ),
                ("test".to_owned(), "printf second".to_owned()),
            ],
            1,
        )?;
        let workflow = Workflow::new(
            store.clone(),
            AgentRunner::with_socket(repository.path().join("unavailable-nucleus.sock")),
        );

        let error = require_error(
            tokio::runtime::Runtime::new()?.block_on(workflow.run_integration(&run)),
            "unavailable Nucleus must stop after preparing gate remediation",
        )?;
        assert_ne!(error.code(), "gate_launch_failed");
        assert_ne!(store.run(&run.id)?.state, RunState::NeedsAttention);
        let gates = store.gates(&run.id)?;
        let failure = store
            .gate_result(&gates[0].id, &predecessor.id)?
            .ok_or("failed predecessor gate was not recorded")?;
        assert_eq!(failure.exit_code, 7);
        assert_eq!(failure.output, "broken");
        assert!(store.gate_result(&gates[1].id, &predecessor.id)?.is_some());
        let remediation = store
            .latest_attempt(&run.id, Role::Integrator, "integration", 1)?
            .ok_or("gate failure did not prepare an integrator successor")?;
        assert!(remediation.targeted);
        assert_ne!(remediation.nucleus_job_id, "integrator-zero");
        let request: nucleus_core::JobRequestV1 =
            serde_json::from_slice(&remediation.request_bytes)?;
        for field in [
            &format!("predecessor candidate: `{}`", predecessor.id),
            "gate name: `compile`",
            "command identity: `test -f fixed && printf repaired || { printf broken; exit 7; }`",
            "exit code: `7`",
            "output truncated: `false`",
            "round: `0`",
            "broken",
        ] {
            assert!(
                request.prompt.contains(field),
                "remediation prompt lacks {field}"
            );
        }

        std::fs::write(repository.path().join("fixed"), "fixed\n")?;
        let status = Command::new("git")
            .args(["add", "fixed"])
            .current_dir(repository.path())
            .status()?;
        assert!(status.success());
        let status = Command::new("git")
            .args(["commit", "-qm", "fix gate"])
            .current_dir(repository.path())
            .status()?;
        assert!(status.success());
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repository.path())
            .output()?;
        let successor_commit = String::from_utf8(output.stdout)?.trim().to_owned();
        let handoff = store.record_document(
            &run.id,
            "integration_handoff",
            Some("integration"),
            1,
            &markdown("# Fixed integration handoff\n")?,
        )?;
        let successor = store.record_candidate(
            &run.id,
            "integration",
            "integration",
            1,
            &predecessor.commit_oid,
            &successor_commit,
            "refs/test/integration/1",
            &handoff.id,
            &remediation.id,
        )?;

        let _ = require_error(
            tokio::runtime::Runtime::new()?.block_on(workflow.run_integration(&run)),
            "unavailable Nucleus must stop after preparing independent recheck",
        )?;
        for gate in &gates {
            let result = store
                .gate_result(&gate.id, &successor.id)?
                .ok_or("successor did not rerun every declared gate")?;
            assert_eq!(result.candidate_id, successor.id);
            assert_eq!(result.round, 1);
            assert_eq!(result.exit_code, 0);
        }
        let recheck = store
            .latest_attempt(&run.id, Role::IntegratedReviewer, "integration", 1)?
            .ok_or("successor was not independently rechecked")?;
        assert!(recheck.targeted);
        assert_ne!(recheck.role, remediation.role);
        Ok(())
    }

    #[test]
    fn gate_failure_exhaustion_and_integrator_blocker_need_attention() -> TestResult {
        let (_repository, store, run, _candidate) = integration_fixture(
            "run-gate-exhaustion",
            vec![("gate".to_owned(), "exit 9".to_owned())],
            0,
        )?;
        let workflow = Workflow::new(store.clone(), AgentRunner::for_current_user());
        tokio::runtime::Runtime::new()?.block_on(workflow.run_integration(&run))?;
        assert_eq!(store.run(&run.id)?.state, RunState::NeedsAttention);

        let (repository, store, run, _candidate) = integration_fixture(
            "run-gate-blocked",
            vec![("gate".to_owned(), "exit 9".to_owned())],
            1,
        )?;
        let workflow = Workflow::new(
            store.clone(),
            AgentRunner::with_socket(repository.path().join("unavailable-nucleus.sock")),
        );
        let _ = require_error(
            tokio::runtime::Runtime::new()?.block_on(workflow.run_integration(&run)),
            "gate failure must prepare the remediation integrator",
        )?;
        let integrator = store
            .latest_attempt(&run.id, Role::Integrator, "integration", 1)?
            .ok_or("missing remediation integrator")?;
        store.commit_managed_submission(
            &integrator.id,
            &integrator.nucleus_job_id,
            "call",
            "args",
            "result",
            &ManagedSubmission::Handoff(HandoffSubmission {
                outcome: HandoffOutcome::Blocked,
                markdown: "# blocked\n".to_owned(),
            }),
        )?;
        complete(&store, &integrator.id)?;
        tokio::runtime::Runtime::new()?.block_on(workflow.run_integration(&run))?;
        assert_eq!(store.run(&run.id)?.state, RunState::NeedsAttention);
        Ok(())
    }

    #[test]
    fn gate_launch_failure_is_recoverable_without_candidate_diagnostics() -> TestResult {
        let (repository, store, run, candidate) = integration_fixture(
            "run-gate-launch-failure",
            vec![("gate".to_owned(), "printf never-runs".to_owned())],
            1,
        )?;
        let workflow = Workflow::with_gate_shell(
            store.clone(),
            AgentRunner::for_current_user(),
            repository.path().join("missing-shell"),
        );
        for _ in 0..2 {
            let error = require_error(
                tokio::runtime::Runtime::new()?.block_on(workflow.run_integration(&run)),
                "missing gate shell must be a recoverable launch failure",
            )?;
            assert_eq!(error.code(), "gate_launch_failed");
            assert_ne!(store.run(&run.id)?.state, RunState::NeedsAttention);
            assert!(
                store
                    .gate_result(&store.gates(&run.id)?[0].id, &candidate.id)?
                    .is_none()
            );
            assert!(
                store
                    .latest_attempt(&run.id, Role::Integrator, "integration", 1)?
                    .is_none()
            );
        }
        Ok(())
    }

    #[test]
    fn accepted_plan_review_proceeds_without_a_successor() {
        assert_eq!(
            route_plan_review(Disposition::Accepted, 0, 1),
            PlanReviewRoute::Accepted
        );
    }

    #[test]
    fn in_budget_plan_changes_route_to_a_targeted_same_subject_successor() {
        assert_eq!(
            route_plan_review(Disposition::ChangesRequested, 0, 1),
            PlanReviewRoute::Revise { next_round: 1 }
        );
        assert_eq!(
            plan_predecessor_round(Role::PlanReviewer, PLAN_REVIEW_SUBJECT, 1),
            Some(0)
        );
        assert_eq!(
            plan_predecessor_round(Role::Assembler, DELEGATION_SUBJECT, 1),
            Some(0)
        );
    }

    #[test]
    fn exhausted_plan_changes_require_caller_attention() {
        assert!(matches!(
            route_plan_review(Disposition::ChangesRequested, 1, 1),
            PlanReviewRoute::NeedsAttention { .. }
        ));
    }

    #[test]
    fn blocked_plan_review_requires_caller_attention_without_a_successor() {
        assert!(matches!(
            route_plan_review(Disposition::Blocked, 0, 1),
            PlanReviewRoute::NeedsAttention { .. }
        ));
    }

    #[test]
    fn interrupted_plan_remediation_resumes_latest_assembled_revision() -> TestResult {
        let (repository, source_commit) = test_repository()?;
        let store = Store::new(repository.path().join("vizier.db"));
        store.initialize()?;
        let run = store.create_run(&NewRun {
            id: "run-plan-subject".to_owned(),
            request_key: None,
            repository: repository.path().to_string_lossy().into_owned(),
            source_commit,
            brief: markdown("# Brief\n")?,
            terminology: markdown("# Terms\n")?,
            contracts: vec![("unit".to_owned(), markdown("# Contract\n")?)],
            gates: Vec::new(),
            remediation_limit: 1,
        })?;
        store.record_document(
            &run.id,
            "unit_plan",
            Some("unit"),
            0,
            &markdown("# Cast-shaped planner marker\n")?,
        )?;
        store.set_run_state(&run.id, RunState::Assembling, None)?;
        for round in 0..=1 {
            let assembler = store.create_attempt(&attempt(
                &run,
                Role::Assembler,
                DELEGATION_SUBJECT,
                round,
                round > 0,
                &format!("assembler-{round}"),
            ))?;
            store.commit_managed_submission(
                &assembler.id,
                &assembler.nucleus_job_id,
                &format!("call-{round}"),
                "args",
                "result",
                &ManagedSubmission::Delegation(delegation(round)),
            )?;
            complete(&store, &assembler.id)?;
        }
        // Recovery starts with the current durable revision and its completed
        // review. There deliberately is no historical round-zero reviewer.
        store.set_run_state(&run.id, RunState::PlanReview, None)?;
        let review_workspace = crate::git::prepare_worktree(
            repository.path(),
            repository.path(),
            &run.id,
            "review-one",
            &run.source_commit,
        )?;
        let review_workspace = review_workspace.to_string_lossy().into_owned();
        let review = store.create_attempt(&NewAttempt {
            workspace_path: &review_workspace,
            ..attempt(
                &run,
                Role::PlanReviewer,
                PLAN_REVIEW_SUBJECT,
                1,
                false,
                "review-1",
            )
        })?;
        store.commit_managed_submission(
            &review.id,
            &review.nucleus_job_id,
            "call-1",
            "args",
            "result",
            &ManagedSubmission::Review(ReviewSubmission {
                disposition: Disposition::Accepted,
                affected_packet_keys: vec!["packet".to_owned()],
                contract_unit_ids: vec!["unit".to_owned()],
                markdown: "# Revision one accepted\n".to_owned(),
            }),
        )?;
        complete(&store, &review.id)?;

        let workflow = Workflow::new(store.clone(), AgentRunner::for_current_user());
        tokio::runtime::Runtime::new()?.block_on(workflow.run_plan_review(&run))?;
        assert!(
            store
                .latest_attempt(&run.id, Role::PlanReviewer, PLAN_REVIEW_SUBJECT, 0)?
                .is_none()
        );
        let Some(revision_one) =
            store.latest_attempt(&run.id, Role::PlanReviewer, PLAN_REVIEW_SUBJECT, 1)?
        else {
            return Err("revision-one review attempt is missing".into());
        };
        assert_eq!(revision_one.id, review.id);
        assert_eq!(workflow.latest_delegation_revision(&run)?, 1);
        assert_eq!(
            store
                .document(&store.packet(&run.id, "packet")?.plan_document_id)?
                .ordinal,
            1
        );
        Ok(())
    }

    #[test]
    fn provisional_cast_prose_cannot_route_an_assembler_remediation() -> TestResult {
        let directory = tempfile::tempdir()?;
        let store = Store::new(directory.path().join("vizier.db"));
        store.initialize()?;
        let run = store.create_run(&NewRun {
            id: "run-provisional-cast".to_owned(),
            request_key: None,
            repository: "/tmp/repository".to_owned(),
            source_commit: "source".to_owned(),
            brief: markdown("# Brief\n")?,
            terminology: markdown("# Terms\n")?,
            contracts: vec![("unit".to_owned(), markdown("# Contract\n")?)],
            gates: Vec::new(),
            remediation_limit: 1,
        })?;
        store.record_document(
            &run.id,
            "unit_plan",
            Some("unit"),
            0,
            &markdown("# Cast-shaped planner marker\n")?,
        )?;
        store.set_run_state(&run.id, RunState::Assembling, None)?;
        let assembler = store.create_attempt(&attempt(
            &run,
            Role::Assembler,
            DELEGATION_SUBJECT,
            0,
            false,
            "assembler",
        ))?;
        store.commit_managed_submission(
            &assembler.id,
            "assembler",
            "call",
            "args",
            "result",
            &ManagedSubmission::Delegation(delegation(0)),
        )?;
        complete(&store, &assembler.id)?;
        store.set_run_state(&run.id, RunState::PlanReview, None)?;
        let review = store.create_attempt(&attempt(
            &run,
            Role::PlanReviewer,
            PLAN_REVIEW_SUBJECT,
            0,
            false,
            "review",
        ))?;
        store.commit_managed_submission(
            &review.id,
            "review",
            "call",
            "args",
            "result",
            &ManagedSubmission::Review(ReviewSubmission {
                disposition: Disposition::Accepted,
                affected_packet_keys: Vec::new(),
                contract_unit_ids: Vec::new(),
                markdown: "# Assembled subject accepted\n".to_owned(),
            }),
        )?;
        complete(&store, &review.id)?;
        let workflow = Workflow::new(store.clone(), AgentRunner::for_current_user());
        assert!(
            !workflow
                .plan_review_prompt(&run, 0)?
                .contains("Cast-shaped planner marker")
        );
        tokio::runtime::Runtime::new()?.block_on(workflow.run_plan_review(&run))?;
        assert!(
            store
                .latest_attempt(&run.id, Role::Assembler, DELEGATION_SUBJECT, 1)?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn packet_and_integrated_scope_reach_remediation_and_targeted_recheck() -> TestResult {
        let (repository, source_commit) = test_repository()?;
        let store = Store::new(repository.path().join("vizier.db"));
        store.initialize()?;
        let run = store.create_run(&NewRun {
            id: "run-scope-managed-paths".to_owned(),
            request_key: None,
            repository: repository.path().to_string_lossy().into_owned(),
            source_commit,
            brief: markdown("# Brief\n")?,
            terminology: markdown("# Terms\n")?,
            contracts: vec![("unit".to_owned(), markdown("# Contract\n")?)],
            gates: Vec::new(),
            remediation_limit: 1,
        })?;
        store.set_run_state(&run.id, RunState::Assembling, None)?;
        let assembler = store.create_attempt(&attempt(
            &run,
            Role::Assembler,
            DELEGATION_SUBJECT,
            0,
            false,
            "assembler",
        ))?;
        store.commit_managed_submission(
            &assembler.id,
            "assembler",
            "call",
            "args",
            "result",
            &ManagedSubmission::Delegation(delegation(0)),
        )?;
        complete(&store, &assembler.id)?;
        let workflow = Workflow::new(
            store.clone(),
            AgentRunner::with_socket(repository.path().join("unavailable-nucleus.sock")),
        );

        // A durable packet-review result routes remediation through run_packets.
        store.set_run_state(&run.id, RunState::PacketReview, None)?;
        store.set_packet_state(
            &run.id,
            "packet",
            crate::model::PacketState::Reviewing,
            None,
            0,
        )?;
        let packet_review = store.create_attempt(&attempt(
            &run,
            Role::PacketReviewer,
            "packet",
            0,
            false,
            "packet-review-0",
        ))?;
        store.commit_managed_submission(
            &packet_review.id,
            "packet-review-0",
            "call",
            "args",
            "result",
            &ManagedSubmission::Review(ReviewSubmission {
                disposition: Disposition::ChangesRequested,
                affected_packet_keys: vec!["packet".to_owned()],
                contract_unit_ids: vec!["unit".to_owned()],
                markdown: "# Exact packet scope\n".to_owned(),
            }),
        )?;
        complete(&store, &packet_review.id)?;
        let packet_handoff = store.record_document(
            &run.id,
            "implementation_handoff",
            Some("packet"),
            0,
            &markdown("# Packet handoff\n")?,
        )?;
        let packet_writer = store.create_attempt(&attempt(
            &run,
            Role::Implementor,
            "packet",
            0,
            false,
            "packet-writer-0",
        ))?;
        let packet_candidate = store.record_candidate(
            &run.id,
            "packet",
            "packet",
            0,
            &run.source_commit,
            &run.source_commit,
            "refs/test/packet/0",
            &packet_handoff.id,
            &packet_writer.id,
        )?;
        store.set_packet_state(
            &run.id,
            "packet",
            crate::model::PacketState::Planned,
            Some(&packet_candidate.id),
            1,
        )?;
        let _ = require_error(
            tokio::runtime::Runtime::new()?.block_on(workflow.run_packets(&run)),
            "unavailable Nucleus must stop after managed implementor preparation",
        )?;
        let packet_remediation = store
            .latest_attempt(&run.id, Role::Implementor, "packet", 1)?
            .ok_or("managed packet remediation was not prepared")?;
        let packet_handoff_1 = store.record_document(
            &run.id,
            "implementation_handoff",
            Some("packet"),
            1,
            &markdown("# Packet handoff 1\n")?,
        )?;
        let packet_candidate_1 = store.record_candidate(
            &run.id,
            "packet",
            "packet",
            1,
            &run.source_commit,
            &run.source_commit,
            "refs/test/packet/1",
            &packet_handoff_1.id,
            &packet_remediation.id,
        )?;
        store.set_packet_state(
            &run.id,
            "packet",
            crate::model::PacketState::Planned,
            Some(&packet_candidate_1.id),
            1,
        )?;
        let _ = require_error(
            tokio::runtime::Runtime::new()?.block_on(workflow.run_packets(&run)),
            "unavailable Nucleus must stop after managed packet recheck preparation",
        )?;
        let packet_recheck = store
            .latest_attempt(&run.id, Role::PacketReviewer, "packet", 1)?
            .ok_or("managed packet recheck was not prepared")?;

        // The integration path reuses its durable round-zero review, then
        // prepares round-one remediation and recheck through run_integration.
        store.set_packet_state(
            &run.id,
            "packet",
            crate::model::PacketState::Accepted,
            Some(&packet_candidate_1.id),
            1,
        )?;
        store.set_run_state(&run.id, RunState::FinalReview, None)?;
        let integration_workspace = crate::git::prepare_worktree(
            repository.path(),
            repository.path(),
            &run.id,
            "integration-review-zero",
            &run.source_commit,
        )?;
        let integration_workspace = integration_workspace.to_string_lossy().into_owned();
        let integration_review = store.create_attempt(&NewAttempt {
            workspace_path: &integration_workspace,
            ..attempt(
                &run,
                Role::IntegratedReviewer,
                "integration",
                0,
                false,
                "integration-review-0",
            )
        })?;
        store.commit_managed_submission(
            &integration_review.id,
            "integration-review-0",
            "call",
            "args",
            "result",
            &ManagedSubmission::Review(ReviewSubmission {
                disposition: Disposition::ChangesRequested,
                affected_packet_keys: vec!["packet".to_owned()],
                contract_unit_ids: vec!["unit".to_owned()],
                markdown: "# Exact integration scope\n".to_owned(),
            }),
        )?;
        complete(&store, &integration_review.id)?;
        let integration_handoff = store.record_document(
            &run.id,
            "integration_handoff",
            Some("integration"),
            0,
            &markdown("# Integration handoff\n")?,
        )?;
        let integration_writer = store.create_attempt(&attempt(
            &run,
            Role::Integrator,
            "integration",
            0,
            false,
            "integration-writer-0",
        ))?;
        let integration_candidate = store.record_candidate(
            &run.id,
            "integration",
            "integration",
            0,
            &run.source_commit,
            &run.source_commit,
            "refs/test/integration/0",
            &integration_handoff.id,
            &integration_writer.id,
        )?;
        let _ = require_error(
            tokio::runtime::Runtime::new()?.block_on(workflow.run_integration(&run)),
            "unavailable Nucleus must stop after managed integrator preparation",
        )?;
        let integration_remediation = store
            .latest_attempt(&run.id, Role::Integrator, "integration", 1)?
            .ok_or("managed integration remediation was not prepared")?;
        let integration_handoff_1 = store.record_document(
            &run.id,
            "integration_handoff",
            Some("integration"),
            1,
            &markdown("# Integration handoff 1\n")?,
        )?;
        let _integration_candidate_1 = store.record_candidate(
            &run.id,
            "integration",
            "integration",
            1,
            &run.source_commit,
            &run.source_commit,
            "refs/test/integration/1",
            &integration_handoff_1.id,
            &integration_remediation.id,
        )?;
        let _ = integration_candidate;
        let _ = require_error(
            tokio::runtime::Runtime::new()?.block_on(workflow.run_integration(&run)),
            "unavailable Nucleus must stop after managed integrated recheck preparation",
        )?;
        let integration_recheck = store
            .latest_attempt(&run.id, Role::IntegratedReviewer, "integration", 1)?
            .ok_or("managed integrated recheck was not prepared")?;

        let packet_review = store.attempt(&packet_review.id)?;
        let packet_scope = store
            .review_scope(
                packet_review
                    .domain_document_id
                    .as_deref()
                    .ok_or("packet review has no persisted document")?,
            )?
            .ok_or("packet review has no persisted scope")?;
        assert_eq!(packet_scope.affected_packet_keys, ["packet"]);
        assert_eq!(packet_scope.contract_unit_ids, ["unit"]);
        let integration_review = store.attempt(&integration_review.id)?;
        let integration_scope = store
            .review_scope(
                integration_review
                    .domain_document_id
                    .as_deref()
                    .ok_or("integrated review has no persisted document")?,
            )?
            .ok_or("integrated review has no persisted scope")?;
        assert_eq!(integration_scope.affected_packet_keys, ["packet"]);
        assert_eq!(integration_scope.contract_unit_ids, ["unit"]);
        for persisted in [&packet_remediation, &packet_recheck] {
            assert!(persisted.targeted);
            assert_persisted_request_scope(persisted, &packet_scope)?;
        }
        for persisted in [&integration_remediation, &integration_recheck] {
            assert!(persisted.targeted);
            assert_persisted_request_scope(persisted, &integration_scope)?;
        }
        Ok(())
    }

    #[test]
    fn obsolete_attempt_cannot_reopen_a_successful_successor_run() -> TestResult {
        let directory = tempfile::tempdir()?;
        let store = Store::new(directory.path().join("vizier.db"));
        store.initialize()?;
        let run = store.create_run(&NewRun {
            id: "run-retry".to_owned(),
            request_key: None,
            repository: "/tmp/repository".to_owned(),
            source_commit: "source".to_owned(),
            brief: markdown("# Brief\n")?,
            terminology: markdown("# Terminology\n")?,
            contracts: vec![("unit".to_owned(), markdown("# Contract\n")?)],
            gates: Vec::new(),
            remediation_limit: 1,
        })?;
        store.set_run_state(&run.id, RunState::Planning, None)?;
        let original = store.create_attempt(&NewAttempt {
            run_id: &run.id,
            role: Role::Planner,
            subject_id: "unit",
            round: 0,
            targeted: false,
            nucleus_job_id: "job-original",
            request_bytes: b"{}",
            request_sha256: "original-digest",
            toolset_name: "unit-plan",
            workspace_path: "/tmp/original",
            base_commit: Some("source"),
            allowed_scopes: &[],
            predecessor_attempt_id: None,
        })?;
        store.set_attempt_runtime(&original.id, AttemptState::Failed, Some("failed"))?;
        let original = store.attempt(&original.id)?;
        validate_retry_target(&store, &store.run(&run.id)?, &original)?;

        let successor = store.create_attempt(&NewAttempt {
            run_id: &run.id,
            role: Role::Planner,
            subject_id: "unit",
            round: 0,
            targeted: false,
            nucleus_job_id: "job-successor",
            request_bytes: b"{}",
            request_sha256: "successor-digest",
            toolset_name: "unit-plan",
            workspace_path: "/tmp/successor",
            base_commit: Some("source"),
            allowed_scopes: &[],
            predecessor_attempt_id: Some(&original.id),
        })?;
        let plan = store.record_document(
            &run.id,
            "unit_plan",
            Some("unit"),
            0,
            &markdown("# Successful plan\n")?,
        )?;
        store.bind_attempt_result(&successor.id, &plan.id, None)?;
        store.set_attempt_runtime(&successor.id, AttemptState::Completed, None)?;

        let obsolete = require_error(
            validate_retry_target(&store, &store.run(&run.id)?, &original),
            "obsolete original attempt unexpectedly remained retryable",
        )?;
        assert_eq!(obsolete.code(), "attempt_not_current_leaf");

        store.set_run_state(&run.id, RunState::Succeeded, None)?;
        let completed = require_error(
            validate_retry_target(&store, &store.run(&run.id)?, &original),
            "successful run unexpectedly reopened",
        )?;
        assert_eq!(completed.code(), "run_not_retryable");
        assert_eq!(store.attempts(&run.id)?.len(), 2);
        Ok(())
    }
}
