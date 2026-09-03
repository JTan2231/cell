use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::model::{StewardUpdate, UpdateStatus};
use crate::nucleus::NucleusSteward;
use crate::store::{Store, WorkerLease};
use crate::{Error, Result};

const CHILD_LEASE_WAIT: Duration = Duration::from_secs(2);
const CHILD_LEASE_POLL: Duration = Duration::from_millis(50);

pub fn activate(database: &Path) -> Result<()> {
    activate_command(database, &["_worker", "drain"])
}

pub fn activate_resume(database: &Path, update_id: &str) -> Result<()> {
    activate_command(database, &["_worker", "resume", update_id])
}

fn activate_command(database: &Path, arguments: &[&str]) -> Result<()> {
    let executable =
        std::env::current_exe().map_err(|source| crate::error::io("current executable", source))?;
    Command::new(executable)
        .arg("--database")
        .arg(database)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|source| crate::error::io(database, source))
}

pub struct Worker<'a> {
    store: &'a Store,
    steward: NucleusSteward,
}

impl<'a> Worker<'a> {
    pub const fn new(store: &'a Store) -> Self {
        Self {
            store,
            steward: NucleusSteward::for_current_user(),
        }
    }

    #[cfg(test)]
    pub fn with_steward(store: &'a Store, steward: NucleusSteward) -> Self {
        Self { store, steward }
    }

    pub fn drain(&self) -> Result<u64> {
        self.drain_with(|update| self.process(update))
    }

    fn drain_with<F>(&self, process: F) -> Result<u64>
    where
        F: FnMut(&StewardUpdate) -> Result<u64>,
    {
        let Some(mut lease) = self.acquire_drain_lease()? else {
            return Ok(0);
        };
        self.drain_owned_with(&mut lease, process)
    }

    fn drain_owned_with<F>(&self, lease: &mut WorkerLease, mut process: F) -> Result<u64>
    where
        F: FnMut(&StewardUpdate) -> Result<u64>,
    {
        let mut applied = 0_u64;
        let mut first_unsettled_error = None;
        for update in self.store.unsettled_updates()? {
            lease.refresh()?;
            self.record_process_result(
                &update,
                process(&update),
                &mut applied,
                &mut first_unsettled_error,
            )?;
        }
        while let Some(update) = lease.claim_next_or_release()? {
            self.record_process_result(
                &update,
                process(&update),
                &mut applied,
                &mut first_unsettled_error,
            )?;
        }
        if let Some(error) = first_unsettled_error {
            return Err(error);
        }
        Ok(applied)
    }

    fn record_process_result(
        &self,
        update: &StewardUpdate,
        result: Result<u64>,
        applied: &mut u64,
        first_unsettled_error: &mut Option<Error>,
    ) -> Result<()> {
        match result {
            Ok(_) => *applied = applied.saturating_add(1),
            Err(error) => {
                let settled = self.store.update(&update.id)?.is_settled();
                if !settled && first_unsettled_error.is_none() {
                    *first_unsettled_error = Some(error);
                }
            }
        }
        Ok(())
    }

    pub fn resume(&self, update_id: &str) -> Result<u64> {
        self.resume_with(
            update_id,
            |update| self.process(update),
            || activate(self.store.path()),
        )
    }

    fn resume_with<P, A>(&self, update_id: &str, mut process: P, activate_drain: A) -> Result<u64>
    where
        P: FnMut(&StewardUpdate) -> Result<u64>,
        A: FnOnce() -> Result<()>,
    {
        let mut lease = self.store.acquire_worker_lease()?;
        let selected_result = match self.store.claim_or_resume(update_id) {
            Ok(update) if update.is_settled() => update.applied_revision.ok_or_else(|| {
                Error::domain(
                    "applied_revision_missing",
                    format!("settled update {} has no revision", update.id),
                )
            }),
            Ok(update) => process(&update),
            Err(error) => Err(error),
        };
        let release_result = lease.release_and_has_eligible_queue();
        drop(lease);
        let handoff_diagnostic = match release_result {
            Ok(false) => None,
            Ok(true) => activate_drain()
                .err()
                .map(|error| format!("queued-work activation failed after resume: {error}")),
            Err(release_error) => {
                let activation_error = activate_drain().err();
                Some(match activation_error {
                    Some(activation_error) => format!(
                        "resume lease handoff failed: {release_error}; fallback activation failed: {activation_error}"
                    ),
                    None => format!("resume lease handoff failed: {release_error}"),
                })
            }
        };
        match (selected_result, handoff_diagnostic) {
            (Ok(revision), Some(diagnostic)) => {
                let _result = self.store.record_applied_diagnostic(update_id, &diagnostic);
                Ok(revision)
            }
            (Ok(revision), None) => Ok(revision),
            (Err(error), Some(diagnostic)) => Err(Error::domain(
                error.code(),
                format!("{error}; additionally, {diagnostic}"),
            )),
            (Err(error), None) => Err(error),
        }
    }

    fn acquire_drain_lease(&self) -> Result<Option<WorkerLease>> {
        self.acquire_drain_lease_for(CHILD_LEASE_WAIT)
    }

    fn acquire_drain_lease_for(&self, wait: Duration) -> Result<Option<WorkerLease>> {
        let started = Instant::now();
        loop {
            match self.store.acquire_worker_lease() {
                Ok(lease) => return Ok(Some(lease)),
                Err(error) if error.code() == "worker_already_running" => {
                    if started.elapsed() >= wait {
                        return Ok(None);
                    }
                    thread::sleep(CHILD_LEASE_POLL);
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn process(&self, update: &StewardUpdate) -> Result<u64> {
        match self.steward.run(self.store, update) {
            Ok(revision) => Ok(revision),
            Err(error) => self.handle_process_error(update, error),
        }
    }

    fn handle_process_error(&self, update: &StewardUpdate, error: Error) -> Result<u64> {
        let refreshed = self.store.update(&update.id)?;
        if refreshed.status == UpdateStatus::Applied {
            let revision = refreshed.applied_revision.ok_or_else(|| {
                Error::domain(
                    "applied_revision_missing",
                    format!("applied update {} has no revision", update.id),
                )
            })?;
            self.store
                .record_applied_diagnostic(&update.id, &error.to_string())?;
            return Ok(revision);
        }
        match error.code() {
            "nucleus_job_lost" => self.store.mark_lost(&update.id, &error.to_string())?,
            "nucleus_admission_rejected"
            | "nucleus_job_terminal_invalid"
            | "nucleus_job_terminal_failed" => {
                self.store.mark_failed(&update.id, &error.to_string())?;
            }
            _ => self
                .store
                .record_running_error(&update.id, &error.to_string())?,
        }
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::Worker;
    use crate::Error;
    use crate::model::{RevisionProposal, Stage, UpdateStatus};
    use crate::store::Store;

    #[test]
    fn one_library_has_only_one_active_worker() {
        let temporary = TempDir::new().expect("temporary directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
                .expect("private temporary directory");
        }
        let database = temporary.path().join("crm.db");
        Store::init(&database).expect("initialize");
        let store = Store::open(database.clone()).expect("store");
        let _lease = store.acquire_worker_lease().expect("worker lease");
        let error = store
            .acquire_worker_lease()
            .expect_err("second worker must not start");
        assert_eq!(error.code(), "worker_already_running");
    }

    #[test]
    fn child_drain_waits_for_a_finishing_owner() {
        let temporary = TempDir::new().expect("temporary directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
                .expect("private temporary directory");
        }
        let database = temporary.path().join("crm.db");
        Store::init(&database).expect("initialize");
        let store = Store::open(database).expect("store");
        let lease = store.acquire_worker_lease().expect("first worker lease");
        let release = thread::spawn(move || {
            thread::sleep(Duration::from_millis(75));
            drop(lease);
        });

        let handed_off = Worker::new(&store)
            .acquire_drain_lease()
            .expect("wait for lease")
            .expect("acquire after handoff");
        release.join().expect("lease owner exits");
        drop(handed_off);
    }

    #[test]
    fn long_resume_hands_queue_to_a_new_worker_after_contender_times_out() {
        let temporary = TempDir::new().expect("temporary directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
                .expect("private temporary directory");
        }
        let database = temporary.path().join("crm.db");
        Store::init(&database).expect("initialize");
        let store = Store::open(database.clone()).expect("store");
        let selected_case = store
            .create_case("Selected", "# Selected\n", Stage::Research)
            .expect("selected case");
        let selected = store
            .enqueue_delivery(&selected_case.case_id, "note", "Signal", None)
            .expect("selected update");
        store.claim_next().expect("claim").expect("selected claim");
        let queued_case = store
            .create_case("Queued", "# Queued\n", Stage::Research)
            .expect("queued case");
        let (resume_started_sender, resume_started_receiver) = mpsc::channel();
        let (finish_resume_sender, finish_resume_receiver) = mpsc::channel();
        let (activation_sender, activation_receiver) = mpsc::channel();
        let resume_database = database.clone();
        let selected_id = selected.id.clone();
        let resume = thread::spawn(move || {
            let resume_store = Store::open(resume_database).expect("resume store");
            let worker = Worker::new(&resume_store);
            let error = worker
                .resume_with(
                    &selected_id,
                    |update| {
                        resume_started_sender.send(()).expect("resume started");
                        finish_resume_receiver.recv().expect("finish resume");
                        resume_store.mark_failed(&update.id, "synthetic terminal failure")?;
                        Err(Error::domain(
                            "nucleus_job_terminal_failed",
                            "synthetic terminal failure",
                        ))
                    },
                    || {
                        activation_sender.send(()).expect("handoff activation");
                        Ok(())
                    },
                )
                .expect_err("selected terminal failure remains visible");
            error.code()
        });

        resume_started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("resume owns the lease");
        let queued = store
            .enqueue_delivery(&queued_case.case_id, "note", "More signal", None)
            .expect("queue during resume");

        let (finished_sender, finished_receiver) = mpsc::channel();
        let contender = thread::spawn(move || {
            let contender_store = Store::open(database).expect("contender store");
            let result = Worker::new(&contender_store).drain_with(|_| {
                panic!("bounded contender must not process the resume owner's queue")
            });
            finished_sender
                .send(result.map_err(|error| error.code()))
                .expect("report contender result");
        });

        let contender_result = finished_receiver.recv_timeout(Duration::from_secs(3));
        if contender_result.is_err() {
            finish_resume_sender.send(()).expect("resume cleanup");
            resume.join().expect("resume cleanup");
            contender.join().expect("contender cleanup");
            panic!("drain contender did not stop after its bounded lease wait");
        }
        assert_eq!(contender_result.expect("bounded contender result"), Ok(0));
        contender.join().expect("bounded contender exits");
        assert_eq!(
            store.update(&queued.id).expect("queued update").status,
            UpdateStatus::Queued
        );

        finish_resume_sender.send(()).expect("finish resume");
        assert_eq!(
            resume.join().expect("resume thread"),
            "nucleus_job_terminal_failed"
        );
        activation_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("resume requests one replacement drain");

        let applied = Worker::new(&store)
            .drain_with(|update| {
                store.mark_failed(&update.id, "synthetic terminal failure")?;
                Err(Error::domain(
                    "nucleus_job_terminal_failed",
                    "synthetic terminal failure",
                ))
            })
            .expect("replacement drain processes queued work");
        assert_eq!(applied, 0);
        assert_eq!(
            store.update(&queued.id).expect("processed update").status,
            UpdateStatus::Failed
        );
    }

    #[test]
    fn resume_early_outcomes_release_and_report_handoff_failures() {
        let temporary = TempDir::new().expect("temporary directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
                .expect("private temporary directory");
        }
        let database = temporary.path().join("crm.db");
        Store::init(&database).expect("initialize");
        let store = Store::open(database).expect("store");
        let selected_case = store
            .create_case("Settled", "# Settled\n", Stage::Research)
            .expect("settled case");
        let selected = store
            .enqueue_delivery(&selected_case.case_id, "note", "Signal", None)
            .expect("selected update");
        store.claim_next().expect("claim").expect("selected claim");
        store
            .commit_proposal(
                &selected.id,
                &selected.job_id,
                "call-settled",
                &"f".repeat(64),
                &RevisionProposal {
                    base_revision: 1,
                    document_markdown: "# Settled\n\nSignal\n".to_owned(),
                    stage: Stage::Research,
                    advisory: None,
                    summary: "Recorded signal".to_owned(),
                },
            )
            .expect("commit selected revision");
        store
            .mark_runtime_finished(&selected.id, "completed", None)
            .expect("settle selected update");
        let queued_case = store
            .create_case("Queued", "# Queued\n", Stage::Research)
            .expect("queued case");
        let queued = store
            .enqueue_delivery(&queued_case.case_id, "note", "More signal", None)
            .expect("queued update");

        let worker = Worker::new(&store);
        let revision = worker
            .resume_with(
                &selected.id,
                |_| panic!("settled update must not be processed again"),
                || Err(Error::domain("activation_test", "synthetic spawn failure")),
            )
            .expect("settled revision remains domain success");
        assert_eq!(revision, 2);
        assert!(
            store
                .update(&selected.id)
                .expect("settled update")
                .last_error
                .as_deref()
                .is_some_and(|detail| detail.contains("synthetic spawn failure"))
        );
        drop(
            store
                .acquire_worker_lease()
                .expect("settled early return released its lease"),
        );
        let claimed = store.claim_next().expect("claim queued").expect("queued");
        assert_eq!(claimed.id, queued.id);
        store
            .mark_failed(&claimed.id, "test cleanup")
            .expect("settle queued update");

        let another = store
            .enqueue_delivery(&queued_case.case_id, "another", "Later signal", None)
            .expect("another queued update");
        let error = worker
            .resume_with(
                "missing-update",
                |_| panic!("missing update must not be processed"),
                || Err(Error::domain("activation_test", "synthetic spawn failure")),
            )
            .expect_err("selection error remains primary");
        assert_eq!(error.code(), "update_not_found");
        assert!(error.to_string().contains("additionally"));
        drop(
            store
                .acquire_worker_lease()
                .expect("selection error released its lease"),
        );
        let claimed = store
            .claim_next()
            .expect("claim another")
            .expect("another queued update");
        assert_eq!(claimed.id, another.id);
    }

    #[test]
    fn drain_visits_each_unsettled_item_once_and_keeps_claiming_after_errors() {
        let temporary = TempDir::new().expect("temporary directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
                .expect("private temporary directory");
        }
        let database = temporary.path().join("crm.db");
        Store::init(&database).expect("initialize");
        let store = Store::open(database).expect("store");
        for name in ["One", "Two", "Three"] {
            let case = store
                .create_case(name, &format!("# {name}\n"), Stage::Research)
                .expect("case");
            store
                .enqueue_delivery(&case.case_id, "note", "Signal", None)
                .expect("queued update");
        }
        store.claim_next().expect("claim first").expect("first");
        store.claim_next().expect("claim second").expect("second");

        let mut visited = Vec::new();
        let error = Worker::new(&store)
            .drain_with(|update| {
                visited.push(update.id.clone());
                if visited.len() == 1 {
                    store.record_running_error(&update.id, "synthetic recoverable failure")?;
                    Err(Error::domain(
                        "nucleus_transport",
                        "synthetic recoverable failure",
                    ))
                } else {
                    store.mark_failed(&update.id, "synthetic terminal failure")?;
                    Err(Error::domain(
                        "nucleus_job_terminal_failed",
                        "synthetic terminal failure",
                    ))
                }
            })
            .expect_err("the first unresolved error remains visible");

        assert_eq!(error.code(), "nucleus_transport");
        assert_eq!(visited.len(), 3);
        assert_eq!(
            visited
                .iter()
                .filter(|id| id.as_str() == visited[0])
                .count(),
            1,
            "an unresolved item must not be tight-looped"
        );
        assert_eq!(
            store.update(&visited[0]).expect("first update").status,
            UpdateStatus::Running
        );
        for update_id in &visited[1..] {
            assert_eq!(
                store.update(update_id).expect("later update").status,
                UpdateStatus::Failed
            );
        }
    }

    #[test]
    fn process_error_after_commit_preserves_domain_success() {
        let temporary = TempDir::new().expect("temporary directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
                .expect("private temporary directory");
        }
        let database = temporary.path().join("crm.db");
        Store::init(&database).expect("initialize");
        let store = Store::open(database).expect("store");
        let case = store
            .create_case("Committed", "# Committed\n", Stage::Research)
            .expect("case");
        let queued = store
            .enqueue_delivery(&case.case_id, "note", "Signal", None)
            .expect("delivery");
        let running = store.claim_next().expect("claim").expect("update");
        let proposal = RevisionProposal {
            base_revision: 1,
            document_markdown: "# Committed\n\nSignal\n".to_owned(),
            stage: Stage::Research,
            advisory: None,
            summary: "Recorded signal".to_owned(),
        };
        store
            .commit_proposal(
                &running.id,
                &running.job_id,
                "call-committed",
                &"e".repeat(64),
                &proposal,
            )
            .expect("commit revision");
        let next = store
            .enqueue_delivery(&case.case_id, "next", "More signal", None)
            .expect("queue after committed revision");

        let worker = Worker::new(&store);
        let applied = worker
            .drain_with(|update| {
                if update.id == running.id {
                    worker.handle_process_error(
                        update,
                        Error::domain("nucleus_transport", "result acknowledgment was interrupted"),
                    )
                } else {
                    store.mark_failed(&update.id, "synthetic terminal failure")?;
                    Err(Error::domain(
                        "nucleus_job_terminal_failed",
                        "synthetic terminal failure",
                    ))
                }
            })
            .expect("committed revision and settled failure complete the drain");
        assert_eq!(applied, 1);
        let update = store.update(&queued.id).expect("update");
        assert_eq!(update.status, UpdateStatus::Applied);
        assert_eq!(update.applied_revision, Some(2));
        assert!(
            update
                .last_error
                .as_deref()
                .is_some_and(|detail| detail.contains("result acknowledgment"))
        );
        assert_eq!(
            store.update(&next.id).expect("next update").status,
            UpdateStatus::Failed
        );
    }
}
