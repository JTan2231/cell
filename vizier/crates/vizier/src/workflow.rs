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
    MAX_GATE_OUTPUT_BYTES, MAX_INPUT_BUNDLE_BYTES, PacketState, PacketView, PathScope, Role,
    RunState, RunView,
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
        }
    }

    pub async fn drive(&self, run_id: &str) -> AppResult<RunView> {
        let _lock = self.acquire_driver_lock()?;
        let result = self.drive_locked(run_id).await;
        if let Err(error) = &result
            && let Ok(run) = self.store.run(run_id)
            && !matches!(run.state, RunState::Succeeded | RunState::Cancelled)
        {
            let _ = self.store.set_run_state(
                run_id,
                RunState::NeedsAttention,
                Some(&format!("{}: {}", error.code(), error.message())),
            );
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
        if run.state == RunState::Succeeded || run.state == RunState::Cancelled {
            return Ok(run);
        }
        if run.cancel_requested {
            self.runner.cancel_run(&self.store, run_id).await?;
            return self.store.run(run_id);
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
        let mut round = 0;
        loop {
            if round > 0 {
                self.run_assembler_revision(run, round).await?;
            }
            self.store
                .set_run_state(&run.id, RunState::PlanReview, None)?;
            let prompt = self.plan_review_prompt(run, round)?;
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
            let candidate =
                match self
                    .store
                    .latest_candidate(&run.id, "integration", "integration")?
                {
                    Some(candidate) if candidate.round == round => candidate,
                    _ => {
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
                        let prompt = self.integration_prompt(run, &packets, round > 0)?;
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
                            self.store.set_run_state(
                                &run.id,
                                RunState::NeedsAttention,
                                Some("integrator reported a blocker"),
                            )?;
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
                    }
                };
            self.run_gates(run, &candidate, round)?;
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
                Disposition::ChangesRequested | Disposition::Blocked => {
                    self.store.set_run_state(&run.id, RunState::NeedsAttention, Some("integrated candidate was not accepted within the bounded remediation policy"))?;
                    return Ok(());
                }
            }
        }
        unreachable!("bounded integration loop always returns")
    }

    fn run_gates(&self, run: &RunView, candidate: &CandidateView, round: u32) -> AppResult<()> {
        self.store.set_run_state(&run.id, RunState::Gates, None)?;
        for gate in self.store.gates(&run.id)? {
            if let Some(result) = self.store.gate_result(&gate.id, &candidate.id)? {
                if result.exit_code != 0 {
                    self.store.set_run_state(
                        &run.id,
                        RunState::NeedsAttention,
                        Some(&format!("gate {} failed", gate.name)),
                    )?;
                    return Ok(());
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
            let output = Command::new("/bin/sh")
                .args(["-lc", &gate.command])
                .current_dir(&worktree)
                .output();
            let (exit_code, raw) = match output {
                Ok(output) => {
                    let mut raw = output.stdout;
                    raw.extend_from_slice(&output.stderr);
                    (output.status.code().unwrap_or(1), raw)
                }
                Err(error) => (127, error.to_string().into_bytes()),
            };
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
            if exit_code != 0 {
                self.store.set_run_state(
                    &run.id,
                    RunState::NeedsAttention,
                    Some(&format!(
                        "gate {} failed with exit code {exit_code}",
                        gate.name
                    )),
                )?;
                return Ok(());
            }
        }
        Ok(())
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
        let predecessor = plan_predecessor_round(role, subject_id, round)
            .map(|previous_round| self.require_round_attempt(run, role, subject_id, previous_round))
            .transpose()?;
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
        let candidate = self.store.record_candidate(
            &run.id,
            subject,
            kind,
            round,
            base,
            &commit,
            &reference,
            handoff,
            &attempt.id,
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
        prompt.push_str("\nSubmit one complete successor delegation overview and packet manifest. Every packet must cover existing contract IDs, use safe repository-relative path scopes, and form an acyclic dependency graph. Overlapping scopes must be ordered by dependency.\n");
        bounded_prompt(&prompt)?;
        Ok(prompt)
    }

    fn plan_review_prompt(&self, run: &RunView, round: u32) -> AppResult<String> {
        let targeted = round > 0;
        let mut prompt = format!(
            "# Vizier {} assembled-plan review\n\nRun: `{}`\nReview subject: `{PLAN_REVIEW_SUBJECT}`\nReview round: `{round}`\n{}\n\n",
            if targeted { "targeted" } else { "one broad" },
            run.id,
            if targeted {
                "Recheck only the cited prior finding, the revised plan surface, and directly affected seams. Do not begin another broad audit."
            } else {
                "This is the only broad plan review."
            }
        );
        self.append_base_bundle(run, &mut prompt, None)?;
        append_documents(&mut prompt, &self.store.documents(&run.id, "unit_plan")?);
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
                candidate.round - 1,
            )?
        {
            append_document(&mut prompt, &previous);
        }
        bounded_prompt(&prompt)?;
        Ok(prompt)
    }

    fn integration_prompt(
        &self,
        run: &RunView,
        packets: &[PacketView],
        targeted: bool,
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
        }
        bounded_prompt(&prompt)?;
        Ok(prompt)
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
        if targeted
            && let Some(previous) = self.store.document_for_subject(
                &run.id,
                "integrated_review",
                "integration",
                candidate.round - 1,
            )?
        {
            append_document(&mut prompt, &previous);
        }
        bounded_prompt(&prompt)?;
        Ok(prompt)
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
        || matches!(run.state, RunState::Succeeded | RunState::Cancelled)
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
        DELEGATION_SUBJECT, PLAN_REVIEW_SUBJECT, PlanReviewRoute, plan_predecessor_round,
        route_plan_review, validate_retry_target,
    };
    use crate::error::{AppError, AppResult};
    use crate::model::{AttemptState, Disposition, NewRun, OpaqueMarkdown, Role, RunState};
    use crate::store::{NewAttempt, Store};

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
