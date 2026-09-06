//! Retained synthetic requester verification. No project or upstream domain state is changed.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use cell_maintenance::{Admission, Gate};
use nucleus_client::NucleusClient;
use nucleus_core::{JobId, JobState};
use serde_json::{Value, json};

use crate::domain::{
    AccountRoutingOutcome, DecisionAccountAnchor, DecisionAccountEvent, GroundingSource,
    IntakeStatus,
};
use crate::error::io;
use crate::nucleus::NucleusReconciler;
use crate::store::Store;
use crate::{Error, Result};

const PROJECT: &str = "deployment-canary";
const LIBRARY: &str = "0123456789abcdef0123456789abcdef";
const EVENT: &str = "synthetic-deployment-account-event";
const ACCOUNT: &str = "synthetic-deployment-account";

/// Run or inspect the same isolated reconciliation. A failed attempt never gets a successor.
pub fn run(directory: &Path, run_id: &str, socket: &Path) -> Result<Value> {
    let (root, _admission) = enter(directory, run_id, socket)?;
    let store = prepare(&root)?;
    let intake = store.account_intake(EVENT)?;
    if intake.status == IntakeStatus::Pending {
        store.mark_account_processing(EVENT)?;
    }
    let reconciler = NucleusReconciler::with_socket(socket);
    let revision = reconciler.reconcile_account(&store, &store.account_intake(EVENT)?)?;
    let repository = store.repository(PROJECT, None)?;
    let expected = GroundingSource::AnnalsDecisionAccount {
        library_id: LIBRARY.to_owned(),
        event_id: EVENT.to_owned(),
        account_id: ACCOUNT.to_owned(),
    };
    if revision != 1
        || repository.revision != 1
        || !repository.concepts.values().any(|concept| {
            concept.active
                && concept
                    .grounds
                    .iter()
                    .any(|ground| ground.active && ground.source == expected)
        })
    {
        return Err(failed(
            "the isolated account did not produce its exact grounded revision",
        ));
    }
    let correlation = store
        .account_correlation(EVENT)?
        .ok_or_else(|| failed("the canary has no durable Nucleus correlation"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| io(&root, error))?;
    let output = runtime.block_on(async {
        let client = NucleusClient::new(socket)?;
        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                let job = client.get_job(&JobId::new(&correlation.job_id)).await?;
                let expected_request: nucleus_core::JobRequestV1 =
                    serde_json::from_str(&correlation.request_json)?;
                if job.request != expected_request
                    || job.summary.request_digest
                        != expected_request
                            .request_digest()
                            .map_err(|_| failed("the retained canary request is invalid"))?
                {
                    return Err(failed("Nucleus returned a different canary request"));
                }
                if job.summary.state.is_terminal() {
                    if job.summary.state != JobState::Completed || job.attempts.len() != 1 {
                        return Err(failed(
                            "the canary did not finish one successful Nucleus attempt",
                        ));
                    }
                    let output = job.attempts[0]
                        .output
                        .as_ref()
                        .filter(|output| {
                            !output.thread_id.is_empty()
                                && !output.turn_id.is_empty()
                                && !output.final_message.trim().is_empty()
                        })
                        .ok_or_else(|| {
                            failed("the completed canary has no structured Nucleus result")
                        })?;
                    return Ok(
                        json!({"job_id": correlation.job_id, "attempt_id": job.attempts[0].id,
                        "thread_id": output.thread_id, "turn_id": output.turn_id}),
                    );
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .map_err(|_| failed("the same canary job remains unfinished; retain its state"))?
    })?;
    let proof = json!({"protocol_version": 1, "verified": true, "revision": revision,
        "project_id": PROJECT, "event_id": EVENT, "nucleus": output, "directory": root});
    write_once(
        &root.join("verified.json"),
        &serde_json::to_vec_pretty(&proof)?,
    )?;
    Ok(proof)
}

fn failed(message: &str) -> Error {
    Error::domain("deployment_canary_failed", message)
}

fn enter(directory: &Path, run_id: &str, socket: &Path) -> Result<(PathBuf, Admission)> {
    if !directory.is_absolute() || !socket.is_absolute() {
        return Err(failed(
            "canary directory and Nucleus socket must be absolute",
        ));
    }
    let marker = directory.join("canary.json");
    let expected = serde_json::to_vec(
        &json!({"version": 1, "product": "semantics", "run_id": run_id, "nucleus_socket": socket}),
    )?;
    if directory.exists() && !marker.exists() {
        return Err(failed(
            "an existing directory without this canary identity cannot be reused",
        ));
    }
    let gate = Gate::new(directory);
    let status = gate.status().map_err(|error| failed(&error.to_string()))?;
    if !status.holds.is_empty() && status.holds != [run_id] {
        return Err(failed("the canary directory belongs to another run"));
    }
    gate.hold(run_id)
        .map_err(|error| failed(&error.to_string()))?;
    let admission = gate
        .enter_for(run_id)
        .map_err(|error| failed(&error.to_string()))?;
    write_once(&marker, &expected)?;
    Ok((
        directory
            .canonicalize()
            .map_err(|error| io(directory, error))?,
        admission,
    ))
}

fn write_once(path: &Path, bytes: &[u8]) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file()
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o777 != 0o600
                || fs::read(path).map_err(|error| io(path, error))? != bytes
            {
                return Err(failed(
                    "retained canary evidence conflicts or is not private regular state",
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .map_err(|error| io(path, error))?;
            file.write_all(bytes).map_err(|error| io(path, error))?;
            file.sync_all().map_err(|error| io(path, error))?;
            if let Some(parent) = path.parent() {
                File::open(parent)
                    .and_then(|file| file.sync_all())
                    .map_err(|error| io(parent, error))?;
            }
        }
        Err(error) => return Err(io(path, error)),
    }
    Ok(())
}

fn prepare(root: &Path) -> Result<Store> {
    let project = root.join("project");
    fs::create_dir_all(&project).map_err(|error| io(&project, error))?;
    fs::set_permissions(&project, fs::Permissions::from_mode(0o700))
        .map_err(|error| io(&project, error))?;
    write_once(
        &project.join("AGENTS.md"),
        b"Semantics-Project: deployment-canary\n",
    )?;
    let store = Store::open(root.join("semantics.db"))?;
    match store.project(PROJECT) {
        Ok(existing)
            if Path::new(&existing.current_path) == project
                && existing.annals_library_id.as_deref() == Some(LIBRARY) => {}
        Ok(_) => return Err(failed("isolated project identity changed")),
        Err(error) if error.code() == "project_not_found" => store
            .register_project_with_account_feed(PROJECT, &project, LIBRARY, "synthetic-before")?,
        Err(error) => return Err(error),
    }
    if !store.has_account_intake(EVENT)? {
        store.record_account_observation(
            PROJECT,
            "synthetic-before",
            &synthetic_account(),
            Some(PROJECT),
            AccountRoutingOutcome::ProjectAssigned,
            true,
        )?;
    }
    if store.account_intake(EVENT)?.account != synthetic_account() {
        return Err(failed("the isolated source account changed"));
    }
    Ok(store)
}

fn synthetic_account() -> DecisionAccountEvent {
    DecisionAccountEvent {
        library_id: LIBRARY.to_owned(), cursor: "synthetic-after".to_owned(), event_id: EVENT.to_owned(),
        account_id: ACCOUNT.to_owned(), account_schema_version: 1,
        statement: "Use Deployment isolation to mean that deployment test state is stored separately from user project state.".to_owned(),
        context: "A synthetic deployment test needs one durable vocabulary term.".to_owned(),
        action: "Unknown.".to_owned(), result: "Unknown.".to_owned(), occurred_at: 1,
        occurred_at_precision: "second".to_owned(), authority: DecisionAccountAnchor {
            host_id: "synthetic-host".to_owned(), thread_id: "synthetic-thread".to_owned(),
            turn_id: "synthetic-turn".to_owned(), item_id: "synthetic-authority".to_owned(), span_start: 0, span_end: 1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_replays_one_isolated_account_without_advancing_revision() {
        let parent = tempfile::tempdir().expect("temporary directory");
        let directory = parent.path().join("canary");
        let (root, admission) =
            enter(&directory, "run-one", Path::new("/synthetic/nucleus.sock")).expect("admit");
        let store = prepare(&root).expect("prepare");
        assert_eq!(
            store
                .repository(PROJECT, None)
                .expect("repository")
                .revision,
            0
        );
        assert_eq!(store.list_account_intake(None).expect("intake").len(), 1);
        drop(store);
        drop(admission);
        let (root, _admission) =
            enter(&directory, "run-one", Path::new("/synthetic/nucleus.sock")).expect("replay");
        assert_eq!(
            prepare(&root)
                .expect("same source")
                .list_account_intake(None)
                .expect("intake")
                .len(),
            1
        );
        assert!(enter(&directory, "run-two", Path::new("/synthetic/nucleus.sock")).is_err());
    }

    #[test]
    fn canary_refuses_existing_unowned_directory_and_changed_evidence() {
        let parent = tempfile::tempdir().expect("temporary directory");
        assert!(enter(parent.path(), "run", Path::new("/synthetic/nucleus.sock")).is_err());
        let path = parent.path().join("evidence");
        write_once(&path, b"first").expect("evidence");
        assert!(write_once(&path, b"changed").is_err());
    }
}
