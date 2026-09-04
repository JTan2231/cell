use serde::Serialize;

use crate::adapters::{AccountConversationLocator, DecisionAccountSource};
use crate::domain::{AccountIntake, AccountRoutingOutcome, DecisionAccountEvent, ProjectStatus};
use crate::nucleus::NucleusReconciler;
use crate::store::Store;
use crate::worker::{AuthorityGate, ReconciliationRunner, WorkerLock, authority_gate};
use crate::{Error, Result};

const PAGE_LIMIT: u16 = 100;
const MAX_PAGES_PER_PROJECT: usize = 10;
const ACCOUNT_RECONCILIATION_PENDING: &str =
    "account_reconciliation_pending: awaiting a definitive Nucleus outcome";

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct AccountWorkerReport {
    pub already_running: bool,
    pub events_seen: u64,
    pub intake_added: u64,
    pub applied_revision: Option<u64>,
    pub error_event_id: Option<String>,
    pub blocked_project_id: Option<String>,
}

impl AccountWorkerReport {
    fn running() -> Self {
        Self {
            already_running: true,
            events_seen: 0,
            intake_added: 0,
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
            applied_revision: None,
            error_event_id: None,
            blocked_project_id: None,
        }
    }
}

pub trait AccountReconciliationRunner {
    fn reconcile_account(&self, store: &Store, intake: &AccountIntake) -> Result<u64>;
}

impl AccountReconciliationRunner for NucleusReconciler {
    fn reconcile_account(&self, store: &Store, intake: &AccountIntake) -> Result<u64> {
        NucleusReconciler::reconcile_account(self, store, intake)
    }
}

pub struct AccountWorker<'a, A, C> {
    store: &'a Store,
    annals: A,
    conversations: C,
    legacy_reconciler: Box<dyn ReconciliationRunner>,
    account_reconciler: Box<dyn AccountReconciliationRunner>,
}

impl<'a, A, C> AccountWorker<'a, A, C>
where
    A: DecisionAccountSource,
    C: AccountConversationLocator,
{
    #[must_use]
    pub fn new(store: &'a Store, annals: A, conversations: C) -> Self {
        Self {
            store,
            annals,
            conversations,
            legacy_reconciler: Box::new(NucleusReconciler::for_current_user()),
            account_reconciler: Box::new(NucleusReconciler::for_current_user()),
        }
    }

    #[cfg(test)]
    fn with_reconcilers(
        store: &'a Store,
        annals: A,
        conversations: C,
        legacy_reconciler: impl ReconciliationRunner + 'static,
        account_reconciler: impl AccountReconciliationRunner + 'static,
    ) -> Self {
        Self {
            store,
            annals,
            conversations,
            legacy_reconciler: Box::new(legacy_reconciler),
            account_reconciler: Box::new(account_reconciler),
        }
    }

    pub fn run_once(mut self) -> Result<AccountWorkerReport> {
        let Some(_lock) = WorkerLock::acquire(self.store.path())? else {
            return Ok(AccountWorkerReport::running());
        };
        let mut report = AccountWorkerReport::idle();
        if let Some(intake) = self.store.processing_intake()? {
            let project_id = intake.project_id.as_deref().ok_or_else(|| {
                Error::domain(
                    "intake_unassigned",
                    "processing legacy intake is unassigned",
                )
            })?;
            if self.store.project(project_id)?.status != ProjectStatus::Active {
                report.blocked_project_id = Some(project_id.to_owned());
                return Ok(report);
            }
            match self.legacy_reconciler.reconcile(self.store, &intake) {
                Ok(revision) => report.applied_revision = Some(revision),
                Err(error) => self.record_legacy_error(&intake.event_id, &error, &mut report)?,
            }
            return Ok(report);
        }
        if let Some(intake) = self.store.processing_account_intake()? {
            let project_id = intake.project_id.as_deref().ok_or_else(|| {
                Error::domain(
                    "intake_unassigned",
                    "processing account intake is unassigned",
                )
            })?;
            if self.store.project(project_id)?.status != ProjectStatus::Active {
                report.blocked_project_id = Some(project_id.to_owned());
                return Ok(report);
            }
            self.process_account(intake, &mut report)?;
            return Ok(report);
        }
        if let Some(intake) = self.store.next_pending_intake()? {
            match authority_gate(self.store, &intake)? {
                AuthorityGate::Reconcile => {
                    self.store.mark_processing(&intake.event_id)?;
                    match self.legacy_reconciler.reconcile(self.store, &intake) {
                        Ok(revision) => report.applied_revision = Some(revision),
                        Err(error) => {
                            self.record_legacy_error(&intake.event_id, &error, &mut report)?;
                        }
                    }
                }
                AuthorityGate::Ignore(reason) => {
                    self.store.mark_ignored(&intake.event_id, &reason)?;
                }
                AuthorityGate::AwaitReview(reason) => {
                    self.store.mark_awaiting_review(&intake.event_id, &reason)?;
                }
                AuthorityGate::Fail(reason) => {
                    self.store.mark_failed(&intake.event_id, &reason)?;
                    report.error_event_id = Some(intake.event_id);
                }
            }
            return Ok(report);
        }
        self.scan(&mut report)?;
        if let Some(intake) = self.store.next_pending_account_intake()? {
            self.process_account(intake, &mut report)?;
        }
        Ok(report)
    }

    fn record_legacy_error(
        &self,
        event_id: &str,
        error: &Error,
        report: &mut AccountWorkerReport,
    ) -> Result<()> {
        let correlation = self.store.correlation(event_id)?;
        let has_domain_commit = correlation
            .as_ref()
            .map(|correlation| {
                self.store
                    .pending_committed_revision(event_id, &correlation.job_id)
            })
            .transpose()?
            .flatten()
            .is_some();
        let terminal_release = correlation.as_ref().is_some_and(|correlation| {
            error.releases_processing_slot()
                && (error.code() != "nucleus_admission_rejected" || !correlation.admitted)
        });
        if !has_domain_commit && terminal_release {
            self.store.mark_failed(event_id, &error.to_string())?;
        } else {
            self.store.record_processing_error(
                event_id,
                &format!(
                    "{}: legacy reconciliation remains in progress pending a definitive Nucleus outcome",
                    error.code()
                ),
            )?;
        }
        report.error_event_id = Some(event_id.to_owned());
        Ok(())
    }

    fn process_account(
        &self,
        intake: AccountIntake,
        report: &mut AccountWorkerReport,
    ) -> Result<()> {
        if intake.status == crate::domain::IntakeStatus::Pending {
            self.store.mark_account_processing(&intake.event_id)?;
        }
        match self
            .account_reconciler
            .reconcile_account(self.store, &intake)
        {
            Ok(revision) => report.applied_revision = Some(revision),
            Err(error) => {
                let correlation = self.store.account_correlation(&intake.event_id)?;
                let has_domain_commit = correlation
                    .as_ref()
                    .map(|correlation| {
                        self.store.account_pending_committed_revision(
                            &intake.event_id,
                            &correlation.job_id,
                        )
                    })
                    .transpose()?
                    .flatten()
                    .is_some();
                let terminal_release = correlation.as_ref().is_some_and(|correlation| {
                    error.releases_processing_slot()
                        && (error.code() != "nucleus_admission_rejected" || !correlation.admitted)
                });
                if !has_domain_commit && terminal_release {
                    self.store.mark_account_failed(
                        &intake.event_id,
                        bounded_terminal_account_error(&error),
                    )?;
                } else {
                    self.store.record_account_processing_error(
                        &intake.event_id,
                        ACCOUNT_RECONCILIATION_PENDING,
                    )?;
                }
                report.error_event_id = Some(intake.event_id);
            }
        }
        Ok(())
    }

    fn scan(&mut self, report: &mut AccountWorkerReport) -> Result<()> {
        let (library_id, watermark) = self.annals.watermark().map_err(|_| {
            Error::domain(
                "annals_feed_unavailable",
                "Annals decision-feed watermark read did not complete",
            )
        })?;
        let projects = self
            .store
            .list_projects()?
            .into_iter()
            .filter(|project| project.status != ProjectStatus::Retired)
            .filter(|project| project.annals_scan_cursor.is_some())
            .collect::<Vec<_>>();
        for project in projects {
            if project.annals_library_id.as_deref() != Some(library_id.as_str()) {
                return Err(Error::domain(
                    "annals_library_mismatch",
                    "Annals feed library does not match the project cursor namespace",
                ));
            }
            let mut cursor = project.annals_scan_cursor.ok_or_else(|| {
                Error::domain(
                    "annals_project_inactive",
                    "project has no Annals scan cursor",
                )
            })?;
            for _ in 0..MAX_PAGES_PER_PROJECT {
                let page = self
                    .annals
                    .read_page(&cursor, &watermark, PAGE_LIMIT)
                    .map_err(|_| {
                        Error::domain(
                            "annals_feed_unavailable",
                            "Annals decision-feed page read did not complete",
                        )
                    })?;
                if page.library_id != library_id
                    || page.request_cursor != cursor
                    || page.watermark != watermark
                {
                    return Err(Error::domain(
                        "annals_page_mismatch",
                        "Annals page did not echo the exact library, cursor, and watermark",
                    ));
                }
                let event_count = page.events.len();
                for event in page.events {
                    if event.cursor == cursor {
                        return Err(Error::domain(
                            "annals_event_cursor_nonadvancing",
                            "Annals returned a decision-account event at the request cursor",
                        ));
                    }
                    self.observe_event(&project.id, &cursor, &event, report)?;
                    cursor.clone_from(&event.cursor);
                    report.events_seen += 1;
                }
                if event_count == 0 {
                    if page.next_cursor != cursor {
                        return Err(Error::domain(
                            "annals_empty_cursor_advanced",
                            "Annals advanced an empty decision-feed page",
                        ));
                    }
                    break;
                }
                if page.next_cursor != cursor {
                    return Err(Error::domain(
                        "annals_next_cursor_mismatch",
                        "Annals next cursor does not match the last account event",
                    ));
                }
                if event_count < usize::from(PAGE_LIMIT) {
                    break;
                }
            }
        }
        Ok(())
    }

    fn observe_event(
        &mut self,
        scanner_project_id: &str,
        from_cursor: &str,
        event: &DecisionAccountEvent,
        report: &mut AccountWorkerReport,
    ) -> Result<()> {
        let cwd = match self.conversations.exact_account_cwd(&event.authority) {
            Ok(Some(cwd)) => cwd,
            Ok(None) => {
                if self.store.record_account_observation(
                    scanner_project_id,
                    from_cursor,
                    event,
                    None,
                    AccountRoutingOutcome::CwdMissing,
                    true,
                )? {
                    report.intake_added += 1;
                }
                return Ok(());
            }
            Err(_) => {
                if self.store.record_account_observation(
                    scanner_project_id,
                    from_cursor,
                    event,
                    None,
                    AccountRoutingOutcome::CwdUnavailable,
                    true,
                )? {
                    report.intake_added += 1;
                }
                return Ok(());
            }
        };
        let owner = match self.store.deepest_project_for_path(&cwd) {
            Ok(owner) => owner,
            Err(error) if error.code() == "project_route_ambiguous" => {
                if self.store.record_account_observation(
                    scanner_project_id,
                    from_cursor,
                    event,
                    None,
                    AccountRoutingOutcome::ProjectAmbiguous,
                    true,
                )? {
                    report.intake_added += 1;
                }
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let retain = owner
            .as_ref()
            .is_some_and(|owner| owner.id == scanner_project_id);
        let project_id = owner.as_ref().map(|owner| owner.id.as_str());
        if self.store.record_account_observation(
            scanner_project_id,
            from_cursor,
            event,
            project_id,
            AccountRoutingOutcome::ProjectAssigned,
            retain,
        )? {
            report.intake_added += 1;
        }
        Ok(())
    }
}

fn bounded_terminal_account_error(error: &Error) -> &'static str {
    match error.code() {
        "nucleus_admission_rejected" => {
            "nucleus_admission_rejected: Nucleus rejected the account reconciliation request"
        }
        "nucleus_job_terminal_invalid" => {
            "nucleus_job_terminal_invalid: Nucleus completed without an accepted account reconciliation"
        }
        "nucleus_job_terminal_failed" => {
            "nucleus_job_terminal_failed: Nucleus account reconciliation ended unsuccessfully"
        }
        _ => {
            "account_reconciliation_failed: account reconciliation ended without a semantic commit"
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use crate::adapters::{AccountConversationLocator, DecisionAccountPage, DecisionAccountSource};
    use crate::domain::{
        AccountIntake, AccountRoutingOutcome, DecisionAccountAnchor, DecisionAccountEvent,
        DecisionAnchor, Intake,
    };
    use crate::store::{Correlation, Store};
    use crate::worker::ReconciliationRunner;

    use super::{AccountReconciliationRunner, AccountWorker};

    struct Feed {
        watermark: String,
        pages: VecDeque<DecisionAccountPage>,
    }

    impl DecisionAccountSource for Feed {
        fn watermark(&mut self) -> crate::Result<(String, String)> {
            Ok((
                "0123456789abcdef0123456789abcdef".to_owned(),
                self.watermark.clone(),
            ))
        }

        fn read_page(
            &mut self,
            _cursor: &str,
            _watermark: &str,
            _limit: u16,
        ) -> crate::Result<DecisionAccountPage> {
            self.pages
                .pop_front()
                .ok_or_else(|| crate::Error::domain("fixture_empty", "missing page"))
        }
    }

    struct Locator(PathBuf);

    impl AccountConversationLocator for Locator {
        fn exact_account_cwd(
            &mut self,
            _anchor: &DecisionAccountAnchor,
        ) -> crate::Result<Option<PathBuf>> {
            Ok(Some(self.0.clone()))
        }
    }

    struct AccountReconciler;

    impl AccountReconciliationRunner for AccountReconciler {
        fn reconcile_account(&self, _store: &Store, _intake: &AccountIntake) -> crate::Result<u64> {
            Ok(1)
        }
    }

    struct LeakyAccountReconciler;

    impl AccountReconciliationRunner for LeakyAccountReconciler {
        fn reconcile_account(&self, store: &Store, intake: &AccountIntake) -> crate::Result<u64> {
            store.put_account_correlation(&Correlation {
                event_id: intake.event_id.clone(),
                requester_id: "account-requester".to_owned(),
                job_id: "account-job".to_owned(),
                request_json: "{}".to_owned(),
                request_sha256: "digest".to_owned(),
                tool_after: 0,
                admitted: false,
            })?;
            Err(crate::Error::domain(
                "nucleus_job_terminal_failed",
                "PRIVATE model output at /private/project from thread-secret",
            ))
        }
    }

    struct LegacyReconciler;

    impl ReconciliationRunner for LegacyReconciler {
        fn reconcile(&self, _store: &Store, _intake: &Intake) -> crate::Result<u64> {
            Ok(1)
        }
    }

    fn account(cursor: &str) -> DecisionAccountEvent {
        DecisionAccountEvent {
            library_id: "0123456789abcdef0123456789abcdef".to_owned(),
            cursor: cursor.to_owned(),
            event_id: "event-1".to_owned(),
            account_id: "account-1".to_owned(),
            account_schema_version: 1,
            statement: "Use stable identities.".to_owned(),
            context: "The boundary needs a durable name.".to_owned(),
            action: "Implemented the name.".to_owned(),
            result: "The boundary is stable.".to_owned(),
            occurred_at: 1,
            occurred_at_precision: "second".to_owned(),
            authority: DecisionAccountAnchor {
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
                turn_id: "turn".to_owned(),
                item_id: "item".to_owned(),
                span_start: 0,
                span_end: 10,
            },
        }
    }

    fn fixture() -> (TempDir, PathBuf, Store) {
        let temporary = TempDir::new().expect("temporary directory");
        let root = temporary.path().join("project");
        fs::create_dir(&root).expect("root");
        let store = Store::open(temporary.path().join("semantics.db")).expect("database");
        store
            .register_project("cell", &root, "legacy-cursor")
            .expect("project");
        store
            .activate_account_feed("0123456789abcdef0123456789abcdef", "a0")
            .expect("feed activation");
        (temporary, root, store)
    }

    #[test]
    fn accepted_account_has_no_review_gate() {
        let (_temporary, root, store) = fixture();
        let root_text = root.to_string_lossy().into_owned();
        let event = account("a1");
        let worker = AccountWorker::with_reconcilers(
            &store,
            Feed {
                watermark: "a1".to_owned(),
                pages: VecDeque::from([DecisionAccountPage {
                    library_id: "0123456789abcdef0123456789abcdef".to_owned(),
                    request_cursor: "a0".to_owned(),
                    next_cursor: "a1".to_owned(),
                    watermark: "a1".to_owned(),
                    events: vec![event],
                }]),
            },
            Locator(root),
            LegacyReconciler,
            AccountReconciler,
        );
        let report = worker.run_once().expect("worker");
        assert_eq!(report.intake_added, 1);
        assert_eq!(report.applied_revision, Some(1));
        let intake = store.account_intake("event-1").expect("intake");
        assert_eq!(intake.attempts, 1);
        assert_eq!(intake.status, crate::domain::IntakeStatus::Processing);
        assert_eq!(
            intake.routing_outcome,
            AccountRoutingOutcome::ProjectAssigned
        );
        let exposed = serde_json::to_value(&intake).expect("serialized intake");
        assert!(exposed.get("cwd").is_none());
        assert!(!exposed.to_string().contains(&root_text));
        let connection = rusqlite::Connection::open(store.path()).expect("database inspection");
        let cwd_columns: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('account_intake_events')
                 WHERE name = 'cwd'",
                [],
                |row| row.get(0),
            )
            .expect("account columns");
        assert_eq!(cwd_columns, 0);
    }

    #[test]
    fn missing_cwd_is_retained_unassigned_and_cursor_advances_atomically() {
        struct Missing;
        impl AccountConversationLocator for Missing {
            fn exact_account_cwd(
                &mut self,
                _anchor: &DecisionAccountAnchor,
            ) -> crate::Result<Option<PathBuf>> {
                Ok(None)
            }
        }

        let (_temporary, _root, store) = fixture();
        let worker = AccountWorker::with_reconcilers(
            &store,
            Feed {
                watermark: "a1".to_owned(),
                pages: VecDeque::from([DecisionAccountPage {
                    library_id: "0123456789abcdef0123456789abcdef".to_owned(),
                    request_cursor: "a0".to_owned(),
                    next_cursor: "a1".to_owned(),
                    watermark: "a1".to_owned(),
                    events: vec![account("a1")],
                }]),
            },
            Missing,
            LegacyReconciler,
            AccountReconciler,
        );
        worker.run_once().expect("worker");
        let intake = store.account_intake("event-1").expect("intake");
        assert_eq!(intake.status, crate::domain::IntakeStatus::Unassigned);
        assert_eq!(intake.routing_outcome, AccountRoutingOutcome::CwdMissing);
        assert_eq!(
            intake.last_error.as_deref(),
            Some("routing_cwd_missing: account authority has no exact cwd")
        );
        assert_eq!(
            store
                .project("cell")
                .expect("project")
                .annals_scan_cursor
                .as_deref(),
            Some("a1")
        );
    }

    #[test]
    fn later_project_converges_transient_unassigned_routing_and_all_cursors_advance() {
        struct FailsThenFinds {
            calls: usize,
            owner: PathBuf,
        }
        impl AccountConversationLocator for FailsThenFinds {
            fn exact_account_cwd(
                &mut self,
                _anchor: &DecisionAccountAnchor,
            ) -> crate::Result<Option<PathBuf>> {
                self.calls += 1;
                if self.calls == 1 {
                    Err(crate::Error::domain(
                        "conversations_failed",
                        "transient fixture failure",
                    ))
                } else {
                    Ok(Some(self.owner.clone()))
                }
            }
        }

        let temporary = TempDir::new().expect("temporary directory");
        let alpha = temporary.path().join("alpha");
        let beta = temporary.path().join("beta");
        fs::create_dir(&alpha).expect("alpha root");
        fs::create_dir(&beta).expect("beta root");
        let store = Store::open(temporary.path().join("semantics.db")).expect("database");
        store
            .register_project("alpha", &alpha, "legacy-final")
            .expect("alpha project");
        store
            .register_project("beta", &beta, "legacy-final")
            .expect("beta project");
        store
            .activate_account_feed("0123456789abcdef0123456789abcdef", "a0")
            .expect("feed activation");
        let event = account("a1");
        let page = DecisionAccountPage {
            library_id: "0123456789abcdef0123456789abcdef".to_owned(),
            request_cursor: "a0".to_owned(),
            next_cursor: "a1".to_owned(),
            watermark: "a1".to_owned(),
            events: vec![event],
        };
        let worker = AccountWorker::with_reconcilers(
            &store,
            Feed {
                watermark: "a1".to_owned(),
                pages: VecDeque::from([page.clone(), page]),
            },
            FailsThenFinds {
                calls: 0,
                owner: alpha,
            },
            LegacyReconciler,
            AccountReconciler,
        );
        worker.run_once().expect("worker");

        let intake = store.account_intake("event-1").expect("converged intake");
        assert_eq!(intake.project_id.as_deref(), Some("alpha"));
        assert_eq!(
            intake.routing_outcome,
            AccountRoutingOutcome::ProjectAssigned
        );
        assert_eq!(intake.last_error, None);
        for project_id in ["alpha", "beta"] {
            assert_eq!(
                store
                    .project(project_id)
                    .expect("project")
                    .annals_scan_cursor
                    .as_deref(),
                Some("a1")
            );
        }
        let connection = rusqlite::Connection::open(store.path()).expect("database inspection");
        let assignments: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM account_intake_assignments
                 WHERE event_id = 'event-1' AND previous_project_id IS NULL
                   AND project_id = 'alpha'",
                [],
                |row| row.get(0),
            )
            .expect("assignment audit");
        assert_eq!(assignments, 1);
    }

    #[test]
    fn nucleus_terminal_detail_is_not_persisted_or_exposed() {
        let (_temporary, root, store) = fixture();
        let worker = AccountWorker::with_reconcilers(
            &store,
            Feed {
                watermark: "a1".to_owned(),
                pages: VecDeque::from([DecisionAccountPage {
                    library_id: "0123456789abcdef0123456789abcdef".to_owned(),
                    request_cursor: "a0".to_owned(),
                    next_cursor: "a1".to_owned(),
                    watermark: "a1".to_owned(),
                    events: vec![account("a1")],
                }]),
            },
            Locator(root),
            LegacyReconciler,
            LeakyAccountReconciler,
        );
        worker.run_once().expect("worker");
        let intake = store.account_intake("event-1").expect("intake");
        assert_eq!(intake.status, crate::domain::IntakeStatus::Failed);
        assert_eq!(
            intake.last_error.as_deref(),
            Some(
                "nucleus_job_terminal_failed: Nucleus account reconciliation ended unsuccessfully"
            )
        );
        let exposed = serde_json::to_string(&intake).expect("serialized intake");
        for private in ["PRIVATE", "/private/project", "thread-secret"] {
            assert!(!exposed.contains(private));
        }
    }

    #[allow(dead_code)]
    fn _legacy_anchor_type_stays_decodable(_anchor: DecisionAnchor) {}
}
