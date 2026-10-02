use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt as _;
use serde::Serialize;

use crate::adapters::{ConversationLocator, DecisionEventSource};
use crate::domain::{DecisionEvent, GroundingSource, ProjectStatus};
use crate::nucleus::NucleusReconciler;
use crate::store::Store;
use crate::{Error, Result};

const PAGE_LIMIT: u16 = 100;
const MAX_PAGES_PER_PROJECT: usize = 10;

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct WorkerReport {
    pub already_running: bool,
    pub events_seen: u64,
    pub intake_added: u64,
    pub intake_ignored: u64,
    pub intake_awaiting_review: u64,
    pub applied_revision: Option<u64>,
    pub error_event_id: Option<String>,
    pub blocked_project_id: Option<String>,
}

impl WorkerReport {
    fn running() -> Self {
        Self {
            already_running: true,
            events_seen: 0,
            intake_added: 0,
            intake_ignored: 0,
            intake_awaiting_review: 0,
            applied_revision: None,
            error_event_id: None,
            blocked_project_id: None,
        }
    }

    fn idle() -> Self {
        Self {
            already_running: false,
            events_seen: 0,
            intake_added: 0,
            intake_ignored: 0,
            intake_awaiting_review: 0,
            applied_revision: None,
            error_event_id: None,
            blocked_project_id: None,
        }
    }
}

pub struct Worker<'a, D, C> {
    store: &'a Store,
    decisions: D,
    conversations: C,
    reconciler: Box<dyn ReconciliationRunner>,
}

pub trait ReconciliationRunner {
    fn reconcile(&self, store: &Store, intake: &crate::domain::Intake) -> Result<u64>;
}

impl ReconciliationRunner for NucleusReconciler {
    fn reconcile(&self, store: &Store, intake: &crate::domain::Intake) -> Result<u64> {
        NucleusReconciler::reconcile(self, store, intake)
    }
}

impl<'a, D, C> Worker<'a, D, C>
where
    D: DecisionEventSource,
    C: ConversationLocator,
{
    #[must_use]
    pub fn new(store: &'a Store, decisions: D, conversations: C) -> Self {
        Self {
            store,
            decisions,
            conversations,
            reconciler: Box::new(NucleusReconciler::for_current_user()),
        }
    }

    pub fn run_once(mut self) -> Result<WorkerReport> {
        let Some(_lock) = WorkerLock::acquire(self.store.path())? else {
            return Ok(WorkerReport::running());
        };
        let mut report = WorkerReport::idle();
        if let Some(intake) = self.store.processing_intake()? {
            let project_id = intake.project_id.as_deref().ok_or_else(|| {
                Error::domain("intake_unassigned", "processing intake is unassigned")
            })?;
            if self.store.project(project_id)?.status != ProjectStatus::Active {
                report.blocked_project_id = Some(project_id.to_owned());
                return Ok(report);
            }
            self.process_intake(intake, &mut report)?;
            return Ok(report);
        }
        self.scan(&mut report)?;
        let Some(intake) = self.store.next_pending_intake()? else {
            return Ok(report);
        };
        self.process_intake(intake, &mut report)?;
        Ok(report)
    }

    fn process_intake(
        &self,
        intake: crate::domain::Intake,
        report: &mut WorkerReport,
    ) -> Result<()> {
        if intake.status == crate::domain::IntakeStatus::Pending {
            match authority_gate(self.store, &intake)? {
                AuthorityGate::Reconcile => {}
                AuthorityGate::Ignore(reason) => {
                    self.store.mark_ignored(&intake.event_id, &reason)?;
                    report.intake_ignored += 1;
                    return Ok(());
                }
                AuthorityGate::AwaitReview(reason) => {
                    self.store.mark_awaiting_review(&intake.event_id, &reason)?;
                    report.intake_awaiting_review += 1;
                    return Ok(());
                }
                AuthorityGate::Fail(reason) => {
                    self.store.mark_failed(&intake.event_id, &reason)?;
                    report.error_event_id = Some(intake.event_id);
                    return Ok(());
                }
            }
            self.store.mark_processing(&intake.event_id)?;
        }
        match self.reconciler.reconcile(self.store, &intake) {
            Ok(revision) => {
                report.applied_revision = Some(revision);
            }
            Err(error) => {
                let correlation = self.store.correlation(&intake.event_id)?;
                let has_domain_commit = correlation
                    .as_ref()
                    .map(|correlation| {
                        self.store
                            .pending_committed_revision(&intake.event_id, &correlation.job_id)
                    })
                    .transpose()?
                    .flatten()
                    .is_some();
                let terminal_release = correlation.as_ref().is_some_and(|correlation| {
                    error.releases_processing_slot()
                        && (error.code() != "nucleus_admission_rejected" || !correlation.admitted)
                });
                if !has_domain_commit && terminal_release {
                    self.store
                        .mark_failed(&intake.event_id, &error.to_string())?;
                } else {
                    self.store.record_processing_error(
                        &intake.event_id,
                        &format!(
                            "{}: reconciliation remains in progress pending a definitive Nucleus outcome",
                            error.code()
                        ),
                    )?;
                }
                report.error_event_id = Some(intake.event_id);
            }
        }
        Ok(())
    }

    fn scan(&mut self, report: &mut WorkerReport) -> Result<()> {
        let projects = self
            .store
            .list_projects()?
            .into_iter()
            .filter(|project| project.status != ProjectStatus::Retired)
            .collect::<Vec<_>>();
        for project in projects {
            let mut cursor = project.scan_cursor.clone();
            for _page_number in 0..MAX_PAGES_PER_PROJECT {
                let page = self.decisions.read_after(&cursor, PAGE_LIMIT)?;
                if page.after_cursor != cursor {
                    return Err(Error::domain(
                        "decisions_cursor_mismatch",
                        "Decisions response did not echo the requested cursor",
                    ));
                }
                let event_count = page.events.len();
                for event in page.events {
                    self.observe_event(&project.id, &event, report)?;
                    self.store
                        .advance_scan_cursor(&project.id, &cursor, &event.cursor)?;
                    cursor = event.cursor;
                    report.events_seen += 1;
                }
                if event_count == 0 {
                    if page.next_cursor != cursor {
                        return Err(Error::domain(
                            "decisions_empty_cursor_advanced",
                            "Decisions advanced an empty event page",
                        ));
                    }
                    break;
                }
                if page.next_cursor != cursor {
                    return Err(Error::domain(
                        "decisions_next_cursor_mismatch",
                        "Decisions next cursor does not match the last event cursor",
                    ));
                }
                if !page.has_more {
                    break;
                }
            }
        }
        Ok(())
    }

    fn observe_event(
        &mut self,
        scanner_project_id: &str,
        event: &DecisionEvent,
        report: &mut WorkerReport,
    ) -> Result<()> {
        let authorities = event
            .anchors
            .iter()
            .filter(|anchor| anchor.source_role == "authority")
            .collect::<Vec<_>>();
        if authorities.len() != 1 {
            let reason = format!(
                "Decisions event has {} authority sources; exactly one is required",
                authorities.len()
            );
            if self.store.insert_intake(event, None, None)? {
                report.intake_added += 1;
            }
            self.store.annotate_unassigned(&event.event_id, &reason)?;
            return Ok(());
        }
        if event.event_kind == "decision_reviewed"
            && let Some(project) = self
                .store
                .assigned_project_for_decision(&event.decision_id)?
        {
            if project.status == ProjectStatus::Retired || project.id != scanner_project_id {
                return Ok(());
            }
            if self.store.insert_intake(event, Some(&project.id), None)? {
                report.intake_added += 1;
            }
            return Ok(());
        }
        let cwd = match self.conversations.exact_cwd(authorities[0]) {
            Ok(Some(cwd)) => Some(cwd),
            Ok(None) => {
                if self.store.insert_intake(event, None, None)? {
                    report.intake_added += 1;
                }
                self.store.annotate_unassigned(
                    &event.event_id,
                    "Conversations exact thread metadata has no cwd",
                )?;
                return Ok(());
            }
            Err(error) => {
                if self.store.insert_intake(event, None, None)? {
                    report.intake_added += 1;
                }
                self.store.annotate_unassigned(
                    &event.event_id,
                    &format!("Conversations exact cwd resolution failed: {error}"),
                )?;
                return Ok(());
            }
        };
        let owner = cwd
            .as_deref()
            .map(|path| self.store.deepest_project_for_path(path))
            .transpose()?
            .flatten();
        if owner
            .as_ref()
            .is_some_and(|owner| owner.id != scanner_project_id)
        {
            return Ok(());
        }
        let Some(owner) = owner else {
            return Ok(());
        };
        let project_id = Some(owner.id.as_str());
        if self
            .store
            .insert_intake(event, project_id, cwd.as_deref())?
        {
            report.intake_added += 1;
        }
        Ok(())
    }
}

pub(crate) enum AuthorityGate {
    Reconcile,
    Ignore(String),
    AwaitReview(String),
    Fail(String),
}

pub(crate) fn authority_gate(
    store: &Store,
    intake: &crate::domain::Intake,
) -> Result<AuthorityGate> {
    if intake
        .decision
        .anchors
        .iter()
        .filter(|anchor| anchor.source_role == "authority")
        .count()
        != 1
    {
        return Ok(AuthorityGate::Fail(
            "exactly one Decisions authority source is required".to_owned(),
        ));
    }
    match intake.decision.confidence.as_str() {
        "low" => {
            return Ok(AuthorityGate::Fail(
                "low-confidence Decisions events are incompatible with the Semantics authority gate"
                    .to_owned(),
            ));
        }
        "high" | "medium" => {}
        confidence => {
            return Ok(AuthorityGate::Fail(format!(
                "unknown Decisions confidence {confidence:?}"
            )));
        }
    }
    if intake.decision.event_kind == "decision_admitted" {
        return if intake.decision.confidence == "high" {
            Ok(AuthorityGate::Reconcile)
        } else {
            Ok(AuthorityGate::AwaitReview(
                "medium-confidence decision awaits explicit Decisions confirmation".to_owned(),
            ))
        };
    }
    if intake.decision.event_kind != "decision_reviewed" {
        return Ok(AuthorityGate::Fail(format!(
            "unsupported Decisions lifecycle event kind {:?}",
            intake.decision.event_kind
        )));
    }
    if let Some(reason) = store.review_admission_block(intake)? {
        return Ok(AuthorityGate::Fail(reason));
    }
    match intake.decision.review_action.as_deref() {
        Some("confirm") => {
            if has_active_grounding(store, intake)? {
                Ok(AuthorityGate::Ignore(
                    "Decisions confirmation preserves the already-active semantic grounding"
                        .to_owned(),
                ))
            } else {
                Ok(AuthorityGate::Reconcile)
            }
        }
        Some("dismiss") => {
            if has_active_grounding(store, intake)? {
                Ok(AuthorityGate::Reconcile)
            } else {
                Ok(AuthorityGate::Ignore(format!(
                    "Decisions dismissal {} has no active semantic grounding",
                    intake
                        .decision
                        .review_id
                        .as_deref()
                        .unwrap_or("unknown-review")
                )))
            }
        }
        Some(action) => Ok(AuthorityGate::Fail(format!(
            "unknown Decisions review action {action:?}"
        ))),
        None => Ok(AuthorityGate::Fail(
            "Decisions review event has no review action".to_owned(),
        )),
    }
}

fn has_active_grounding(store: &Store, intake: &crate::domain::Intake) -> Result<bool> {
    let project_id = intake
        .project_id
        .as_deref()
        .ok_or_else(|| Error::domain("intake_unassigned", "review intake is unassigned"))?;
    let repository = store.repository(project_id, None)?;
    Ok(repository.concepts.values().any(|concept| {
        concept.grounds.iter().any(|grounding| {
            grounding.active
                && matches!(
                    &grounding.source,
                    GroundingSource::Decision { decision_id, .. }
                        if decision_id == &intake.decision.decision_id
                )
        })
    }))
}

pub(crate) struct WorkerLock {
    file: File,
}

impl WorkerLock {
    pub(crate) fn acquire(database: &Path) -> Result<Option<Self>> {
        let lock_path = lock_path(database);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| crate::error::io(&lock_path, source))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(source) => Err(crate::error::io(lock_path, source)),
        }
    }
}

impl Drop for WorkerLock {
    fn drop(&mut self) {
        let _result = self.file.unlock();
    }
}

fn lock_path(database: &Path) -> PathBuf {
    let mut value = database.as_os_str().to_os_string();
    value.push(".worker.lock");
    PathBuf::from(value)
}
