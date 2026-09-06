//! A retained synthetic classifier canary. It never delivers an account to a live library.
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use cell_maintenance::{Admission, Gate};
use nucleus_client::NucleusClient;
use nucleus_core::{JobId, JobState};
use serde_json::{Value, json};

use crate::classifier::Runner;
use crate::error::{AppError, AppResult, Context as _};
use crate::model::{AuthorityVerdict, MessageRole, Precision, SourceMessage, ThreadTranscript};
use crate::store::Store;

const LIBRARY: &str = "0123456789abcdef0123456789abcdef";
const AUTHORITY: &str = "I decide that deployment tests must use isolated state.";

pub(crate) fn run(
    directory: &Path,
    run_id: &str,
    socket: &Path,
    annals: &Path,
) -> AppResult<Value> {
    let (root, _admission) = enter(directory, run_id, socket, annals)?;
    let identity = crate::account::sha256(&format!("{}\n{run_id}", root.display()));
    let session = format!("synthetic-canary-{identity}");
    let job_id = format!("krisis-observe-deployment-{identity}");
    let requester_id = format!("krisis-deployment-{identity}");
    let transcript = transcript(&session);
    let mut store = Store::open(&root.join("krisis.db"))?;
    let _processing = store.lock_observation_processing()?;
    store.activate_observer(0)?;
    let observation = store.ingest_observation(&session, "synthetic-turn")?;
    if observation.status != "complete" {
        // An inert target bound only inside the canary directory proves the
        // normal durable outbox transaction without admitting production work.
        let config = root.join("isolated-annals.toml");
        write_once(
            &config,
            format!(
                "library = {}\n[decision_feed]\nexpected_library_id = \"{LIBRARY}\"\n",
                serde_json::to_string(&root.join("uninitialized-annals.db")).context(
                    "deployment_canary_failed",
                    "unable to encode isolated target"
                )?
            )
            .as_bytes(),
        )?;
        store.bind_observation_annals_target(
            &observation.id,
            LIBRARY,
            config
                .to_str()
                .ok_or_else(|| failed("canary path is not UTF-8"))?,
        )?;
        store.bind_observation_source(
            &observation.id,
            &transcript.host_id,
            &transcript.thread_id,
            2,
            &crate::account::sha256(AUTHORITY),
            0,
            &transcript.messages,
        )?;
    }
    store.plan_observation_job(&observation.id, 0, 0, &job_id)?;
    let classification = match store.persisted_observation_classification(&job_id)? {
        Some(value) => value,
        None => Runner::with_socket(socket)
            .classify_observation(
                &mut store,
                &requester_id,
                &job_id,
                &transcript,
                0,
                3,
                "synthetic-turn",
                false,
            )?
            .as_observation(),
    };
    if classification.needs_context
        || classification.accounts.len() != 1
        || classification.authority_verdicts.len() != 1
        || classification.authority_verdicts[0].verdict != AuthorityVerdict::Decision
    {
        return Err(failed(
            "the synthetic authority did not produce one durable decision account",
        ));
    }
    store.complete_observation_if_needed(&observation.id, &classification)?;
    store.mark_job(&job_id, "complete", None)?;
    let status = store.observation_status()?;
    let pending = store
        .pending_account()?
        .ok_or_else(|| failed("the canary has no durable account outbox item"))?;
    if status.complete != 1
        || status.accounts_pending_annals != 1
        || status.accounts_accepted_by_annals != 0
        || pending.account_id != classification.accounts[0].id
        || pending.target_library_id != LIBRARY
        || Path::new(&pending.target_config_path) != root.join("isolated-annals.toml")
    {
        return Err(failed(
            "the isolated classification and outbox do not agree",
        ));
    }
    let expected_digest = store.observation_job_request_digest(&job_id)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context(
            "deployment_canary_failed",
            "unable to initialize canary verifier",
        )?;
    let output = runtime.block_on(async {
        let client = NucleusClient::new(socket)
            .context("deployment_canary_failed", "unable to connect to Nucleus")?;
        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                let job = client.get_job(&JobId::new(&job_id)).await.context(
                    "deployment_canary_failed",
                    "unable to inspect the same Nucleus canary",
                )?;
                if job.summary.requester.program != "krisis"
                    || job.summary.requester.id != requester_id
                    || job
                        .request
                        .digest()
                        .context("deployment_canary_failed", "invalid Nucleus request")?
                        != expected_digest
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
                    return Ok(json!({"job_id": job_id, "attempt_id": job.attempts[0].id,
                        "thread_id": output.thread_id, "turn_id": output.turn_id}));
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .map_err(|_| failed("the same canary remains unfinished; retain its state"))?
    })?;
    let proof = json!({"protocol_version": 1, "verified": true, "observation_id": observation.id,
        "account_id": pending.account_id, "source_sha256": pending.source_sha256,
        "accounts_pending_in_isolated_outbox": 1, "nucleus": output, "directory": root});
    write_once(
        &root.join("verified.json"),
        &serde_json::to_vec_pretty(&proof).context(
            "deployment_canary_failed",
            "unable to encode canary evidence",
        )?,
    )?;
    Ok(proof)
}

fn transcript(session: &str) -> ThreadTranscript {
    ThreadTranscript {
        host_id: "synthetic-host".to_owned(),
        thread_id: session.to_owned(),
        messages: vec![SourceMessage {
            host_id: "synthetic-host".to_owned(),
            thread_id: session.to_owned(),
            turn_id: "synthetic-turn".to_owned(),
            item_id: "synthetic-authority".to_owned(),
            role: MessageRole::User,
            text: AUTHORITY.to_owned(),
            occurred_at: 1,
            precision: Precision::Item,
        }],
    }
}

fn failed(message: &str) -> AppError {
    AppError::new("deployment_canary_failed", message)
}

fn enter(
    directory: &Path,
    run_id: &str,
    socket: &Path,
    annals: &Path,
) -> AppResult<(PathBuf, Admission)> {
    if !directory.is_absolute() || !socket.is_absolute() || !annals.is_absolute() {
        return Err(failed(
            "canary directory and dependency paths must be absolute",
        ));
    }
    let marker = directory.join("canary.json");
    let expected = serde_json::to_vec(&json!({"version": 1, "product": "krisis", "run_id": run_id,
        "nucleus_socket": socket, "annals_binary": annals}))
    .context(
        "deployment_canary_failed",
        "unable to encode canary identity",
    )?;
    if directory.exists() && !marker.exists() {
        return Err(failed(
            "an existing directory without this canary identity cannot be reused",
        ));
    }
    let gate = Gate::new(directory);
    let status = gate.status().context(
        "deployment_canary_failed",
        "unable to inspect canary admission",
    )?;
    if !status.holds.is_empty() && status.holds != [run_id] {
        return Err(failed("the canary belongs to another run"));
    }
    gate.hold(run_id).context(
        "deployment_canary_failed",
        "unable to retain canary admission",
    )?;
    let admission = gate.enter_for(run_id).context(
        "deployment_canary_failed",
        "the same canary is already running",
    )?;
    write_once(&marker, &expected)?;
    Ok((
        directory.canonicalize().context(
            "deployment_canary_failed",
            "unable to resolve canary directory",
        )?,
        admission,
    ))
}

fn write_once(path: &Path, bytes: &[u8]) -> AppResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file()
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o777 != 0o600
                || fs::read(path).context(
                    "deployment_canary_failed",
                    "unable to read retained evidence",
                )? != bytes
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
                .context(
                    "deployment_canary_failed",
                    "unable to create private evidence",
                )?;
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .context(
                    "deployment_canary_failed",
                    "unable to retain canary evidence",
                )?;
            if let Some(parent) = path.parent() {
                File::open(parent)
                    .and_then(|file| file.sync_all())
                    .context(
                        "deployment_canary_failed",
                        "unable to sync canary directory",
                    )?;
            }
        }
        Err(error) => return Err(failed(&error.to_string())),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canary_identity_is_private_stable_and_exclusive() -> AppResult<()> {
        let parent = tempfile::tempdir().context("test", "temporary directory")?;
        let directory = parent.path().join("canary");
        let (_, admission) = enter(
            &directory,
            "run-one",
            Path::new("/synthetic/nucleus.sock"),
            Path::new("/synthetic/annals"),
        )?;
        assert!(
            enter(
                &directory,
                "run-one",
                Path::new("/synthetic/nucleus.sock"),
                Path::new("/synthetic/annals")
            )
            .is_err()
        );
        drop(admission);
        let (_, _admission) = enter(
            &directory,
            "run-one",
            Path::new("/synthetic/nucleus.sock"),
            Path::new("/synthetic/annals"),
        )?;
        assert!(
            enter(
                &directory,
                "run-two",
                Path::new("/synthetic/nucleus.sock"),
                Path::new("/synthetic/annals")
            )
            .is_err()
        );
        assert!(
            enter(
                parent.path(),
                "run-one",
                Path::new("/synthetic/nucleus.sock"),
                Path::new("/synthetic/annals")
            )
            .is_err()
        );
        assert_eq!(transcript("synthetic").messages[0].role, MessageRole::User);
        Ok(())
    }
}
