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

    #[cfg(test)]
    fn with_reconciler(
        store: &'a Store,
        decisions: D,
        conversations: C,
        reconciler: impl ReconciliationRunner + 'static,
    ) -> Self {
        Self {
            store,
            decisions,
            conversations,
            reconciler: Box::new(reconciler),
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
                    "Decisions confirmation preserves already-active evidentiary force".to_owned(),
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use crate::adapters::{ConversationLocator, DecisionEventPage, DecisionEventSource};
    use crate::domain::{DecisionAnchor, DecisionEvent};
    use crate::store::{Correlation, Store};

    use super::{ReconciliationRunner, Worker};

    struct Events {
        pages: VecDeque<DecisionEventPage>,
    }

    impl DecisionEventSource for Events {
        fn watermark(&mut self) -> crate::Result<String> {
            Ok("unused".to_owned())
        }

        fn read_after(&mut self, _cursor: &str, _limit: u16) -> crate::Result<DecisionEventPage> {
            self.pages
                .pop_front()
                .ok_or_else(|| crate::Error::domain("fixture_empty", "missing event page"))
        }
    }

    struct Locator(PathBuf);

    impl ConversationLocator for Locator {
        fn exact_cwd(&mut self, _anchor: &DecisionAnchor) -> crate::Result<Option<PathBuf>> {
            Ok(Some(self.0.clone()))
        }
    }

    struct OfflineReconciler;

    impl ReconciliationRunner for OfflineReconciler {
        fn reconcile(&self, _store: &Store, _intake: &crate::domain::Intake) -> crate::Result<u64> {
            Ok(1)
        }
    }

    struct FailingLocator;

    impl ConversationLocator for FailingLocator {
        fn exact_cwd(&mut self, _anchor: &DecisionAnchor) -> crate::Result<Option<PathBuf>> {
            Err(crate::Error::domain(
                "fixture_conversations_unavailable",
                "Conversations must not be consulted",
            ))
        }
    }

    struct FailingReconciler(&'static str);

    impl ReconciliationRunner for FailingReconciler {
        fn reconcile(&self, _store: &Store, _intake: &crate::domain::Intake) -> crate::Result<u64> {
            Err(crate::Error::domain(self.0, "fixture reconciliation error"))
        }
    }

    fn event(
        event_id: &str,
        decision_id: &str,
        event_kind: &str,
        confidence: &str,
        review_action: Option<&str>,
    ) -> DecisionEvent {
        DecisionEvent {
            event_id: event_id.to_owned(),
            event_version: 1,
            cursor: format!("cursor-{event_id}"),
            event_kind: event_kind.to_owned(),
            occurred_at: 1,
            decision_id: decision_id.to_owned(),
            decided_at: 1,
            timestamp_precision: "second".to_owned(),
            statement: "Use stable identities.".to_owned(),
            disposition: "adopt".to_owned(),
            confidence: confidence.to_owned(),
            rationale: None,
            supersedes_decision_id: None,
            authority_start: 0,
            authority_end: 10,
            review_state: review_action
                .map_or("unreviewed", |_| "reviewed")
                .to_owned(),
            review_id: review_action.map(|_| format!("review-{event_id}")),
            review_action: review_action.map(str::to_owned),
            reviewed_at: review_action.map(|_| 2),
            review_source: review_action.map(|_| "operator".to_owned()),
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
        }
    }

    fn correlate(store: &Store, event_id: &str) {
        correlate_with_admission(store, event_id, true);
    }

    fn correlate_with_admission(store: &Store, event_id: &str, admitted: bool) {
        store
            .put_correlation(&Correlation {
                event_id: event_id.to_owned(),
                requester_id: format!("requester-{event_id}"),
                job_id: format!("job-{event_id}"),
                request_json: "{}".to_owned(),
                request_sha256: "digest".to_owned(),
                tool_after: 0,
                admitted,
            })
            .expect("correlation");
    }

    fn single_project() -> (TempDir, PathBuf, Store) {
        let temporary = TempDir::new().expect("temporary directory");
        let root = temporary.path().join("project");
        fs::create_dir(&root).expect("project root");
        let store = Store::open(temporary.path().join("semantics.db")).expect("database");
        store
            .register_project("cell", &root, "c0")
            .expect("project registration");
        (temporary, root, store)
    }

    #[test]
    fn deepest_registered_owner_receives_event_once() {
        let temporary = TempDir::new().expect("temporary directory");
        let parent = temporary.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).expect("project roots");
        let store = Store::open(temporary.path().join("semantics.db")).expect("database");
        store
            .register_project("parent", &parent, "p0")
            .expect("parent registration");
        store
            .register_project("child", &child, "c0")
            .expect("child registration");
        let event = DecisionEvent {
            event_id: "event-1".to_owned(),
            event_version: 1,
            cursor: "p1".to_owned(),
            event_kind: "decision_admitted".to_owned(),
            occurred_at: 1,
            decision_id: "decision-1".to_owned(),
            decided_at: 1,
            timestamp_precision: "second".to_owned(),
            statement: "Use stable identities.".to_owned(),
            disposition: "adopt".to_owned(),
            confidence: "high".to_owned(),
            rationale: None,
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
        let pages = VecDeque::from([
            DecisionEventPage {
                after_cursor: "c0".to_owned(),
                next_cursor: "p1".to_owned(),
                watermark_cursor: "p1".to_owned(),
                has_more: false,
                events: vec![event.clone()],
            },
            DecisionEventPage {
                after_cursor: "p0".to_owned(),
                next_cursor: "p1".to_owned(),
                watermark_cursor: "p1".to_owned(),
                has_more: false,
                events: vec![event],
            },
        ]);
        let worker =
            Worker::with_reconciler(&store, Events { pages }, Locator(child), OfflineReconciler);
        let report = worker.run_once().expect("worker report");
        assert_eq!(report.intake_added, 1);
        assert_eq!(
            store
                .intake("event-1")
                .expect("intake")
                .project_id
                .as_deref(),
            Some("child")
        );
    }

    #[test]
    fn processing_resume_does_not_touch_upstream_dependencies() {
        let (_temporary, root, store) = single_project();
        let decision = event("event-1", "decision-1", "decision_admitted", "high", None);
        store
            .insert_intake(&decision, Some("cell"), Some(&root))
            .expect("intake");
        store.mark_processing("event-1").expect("processing");
        correlate(&store, "event-1");
        let worker = Worker::with_reconciler(
            &store,
            Events {
                pages: VecDeque::new(),
            },
            FailingLocator,
            OfflineReconciler,
        );
        let report = worker.run_once().expect("resume report");
        assert_eq!(report.applied_revision, Some(1));
    }

    #[test]
    fn paused_processing_intake_blocks_every_new_job() {
        let (_temporary, root, store) = single_project();
        let decision = event("event-1", "decision-1", "decision_admitted", "high", None);
        store
            .insert_intake(&decision, Some("cell"), Some(&root))
            .expect("intake");
        store.mark_processing("event-1").expect("processing");
        store
            .set_project_status("cell", crate::domain::ProjectStatus::Paused)
            .expect("pause");
        let worker = Worker::with_reconciler(
            &store,
            Events {
                pages: VecDeque::new(),
            },
            FailingLocator,
            FailingReconciler("must_not_run"),
        );
        let report = worker.run_once().expect("blocked report");
        assert_eq!(report.blocked_project_id.as_deref(), Some("cell"));
        assert_eq!(
            store.intake("event-1").expect("intake").status,
            crate::domain::IntakeStatus::Processing
        );
    }

    #[test]
    fn admitted_nonterminal_error_keeps_global_processing_slot() {
        let (_temporary, root, store) = single_project();
        let decision = event("event-1", "decision-1", "decision_admitted", "high", None);
        store
            .insert_intake(&decision, Some("cell"), Some(&root))
            .expect("intake");
        store.mark_processing("event-1").expect("processing");
        correlate(&store, "event-1");
        let worker = Worker::with_reconciler(
            &store,
            Events {
                pages: VecDeque::new(),
            },
            FailingLocator,
            FailingReconciler("nucleus_admission_rejected"),
        );
        let report = worker.run_once().expect("worker report");
        assert_eq!(report.error_event_id.as_deref(), Some("event-1"));
        let intake = store.intake("event-1").expect("intake");
        assert_eq!(intake.status, crate::domain::IntakeStatus::Processing);
        assert!(
            intake
                .last_error
                .as_deref()
                .is_some_and(|detail| detail.starts_with("nucleus_admission_rejected:"))
        );
    }

    #[test]
    fn pre_correlation_outage_remains_resumable_and_visible() {
        let (_temporary, root, store) = single_project();
        let decision = event("event-1", "decision-1", "decision_admitted", "high", None);
        store
            .insert_intake(&decision, Some("cell"), Some(&root))
            .expect("intake");
        store.mark_processing("event-1").expect("processing");
        let worker = Worker::with_reconciler(
            &store,
            Events {
                pages: VecDeque::new(),
            },
            FailingLocator,
            FailingReconciler("nucleus_not_ready"),
        );
        worker.run_once().expect("worker report");
        let intake = store.intake("event-1").expect("intake");
        assert_eq!(intake.status, crate::domain::IntakeStatus::Processing);
        assert_eq!(
            intake.last_error.as_deref(),
            Some(
                "nucleus_not_ready: reconciliation remains in progress pending a definitive Nucleus outcome"
            )
        );
    }

    #[test]
    fn proven_rejection_before_admission_releases_processing_slot() {
        let (_temporary, root, store) = single_project();
        let decision = event("event-1", "decision-1", "decision_admitted", "high", None);
        store
            .insert_intake(&decision, Some("cell"), Some(&root))
            .expect("intake");
        store.mark_processing("event-1").expect("processing");
        correlate_with_admission(&store, "event-1", false);
        let worker = Worker::with_reconciler(
            &store,
            Events {
                pages: VecDeque::new(),
            },
            FailingLocator,
            FailingReconciler("nucleus_admission_rejected"),
        );
        worker.run_once().expect("worker report");
        assert_eq!(
            store.intake("event-1").expect("intake").status,
            crate::domain::IntakeStatus::Failed
        );
    }

    #[test]
    fn positively_terminal_job_failure_releases_processing_slot() {
        let (_temporary, root, store) = single_project();
        let decision = event("event-1", "decision-1", "decision_admitted", "high", None);
        store
            .insert_intake(&decision, Some("cell"), Some(&root))
            .expect("intake");
        store.mark_processing("event-1").expect("processing");
        correlate(&store, "event-1");
        let worker = Worker::with_reconciler(
            &store,
            Events {
                pages: VecDeque::new(),
            },
            FailingLocator,
            FailingReconciler("nucleus_job_terminal_failed"),
        );
        worker.run_once().expect("worker report");
        assert_eq!(
            store.intake("event-1").expect("intake").status,
            crate::domain::IntakeStatus::Failed
        );
    }

    #[test]
    fn review_cannot_leapfrog_unassigned_admission() {
        let (_temporary, root, store) = single_project();
        let admission = event(
            "admission-1",
            "decision-1",
            "decision_admitted",
            "medium",
            None,
        );
        store
            .insert_intake(&admission, None, None)
            .expect("unassigned admission");
        let review = event(
            "review-1",
            "decision-1",
            "decision_reviewed",
            "medium",
            Some("confirm"),
        );
        store
            .insert_intake(&review, Some("cell"), Some(&root))
            .expect("review");
        let worker = Worker::with_reconciler(
            &store,
            Events {
                pages: VecDeque::from([DecisionEventPage {
                    after_cursor: "c0".to_owned(),
                    next_cursor: "c0".to_owned(),
                    watermark_cursor: "c0".to_owned(),
                    has_more: false,
                    events: Vec::new(),
                }]),
            },
            FailingLocator,
            FailingReconciler("must_not_run"),
        );
        worker.run_once().expect("worker report");
        let review = store.intake("review-1").expect("review");
        assert_eq!(review.status, crate::domain::IntakeStatus::Failed);
        assert!(
            review
                .last_error
                .as_deref()
                .is_some_and(|reason| reason.contains("assigned to None"))
        );
    }

    #[test]
    fn exact_cwd_outside_registered_roots_is_not_retained() {
        let (temporary, _root, store) = single_project();
        let outside = temporary.path().join("outside");
        fs::create_dir(&outside).expect("outside cwd");
        let decision = event("event-1", "decision-1", "decision_admitted", "high", None);
        let worker = Worker::with_reconciler(
            &store,
            Events {
                pages: VecDeque::from([DecisionEventPage {
                    after_cursor: "c0".to_owned(),
                    next_cursor: "cursor-event-1".to_owned(),
                    watermark_cursor: "cursor-event-1".to_owned(),
                    has_more: false,
                    events: vec![decision],
                }]),
            },
            Locator(outside),
            FailingReconciler("must_not_run"),
        );
        let report = worker.run_once().expect("worker report");
        assert_eq!(report.events_seen, 1);
        assert!(store.list_intake(None).expect("intake list").is_empty());
        assert_eq!(
            store.project("cell").expect("project").scan_cursor,
            "cursor-event-1"
        );
    }

    #[test]
    fn review_keeps_decision_project_identity_without_old_conversation_metadata() {
        let (temporary, old_root, store) = single_project();
        let admission = event(
            "admission-1",
            "decision-1",
            "decision_admitted",
            "medium",
            None,
        );
        store
            .insert_intake(&admission, Some("cell"), Some(&old_root))
            .expect("admission");
        store
            .mark_awaiting_review("admission-1", "await confirmation")
            .expect("awaiting review");
        let moved = temporary.path().join("moved");
        fs::create_dir(&moved).expect("moved root");
        store.move_project("cell", &moved).expect("project move");
        let review = event(
            "review-1",
            "decision-1",
            "decision_reviewed",
            "medium",
            Some("confirm"),
        );
        let worker = Worker::with_reconciler(
            &store,
            Events {
                pages: VecDeque::from([DecisionEventPage {
                    after_cursor: "c0".to_owned(),
                    next_cursor: "cursor-review-1".to_owned(),
                    watermark_cursor: "cursor-review-1".to_owned(),
                    has_more: false,
                    events: vec![review],
                }]),
            },
            FailingLocator,
            OfflineReconciler,
        );
        worker.run_once().expect("worker report");
        assert_eq!(
            store
                .intake("review-1")
                .expect("review")
                .project_id
                .as_deref(),
            Some("cell")
        );
    }
}
