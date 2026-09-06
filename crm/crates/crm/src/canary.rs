use crate::model::Stage;
use crate::store::Store;
use crate::worker::Worker;
use crate::{Error, Result};
use nucleus_client::NucleusClient;
use nucleus_core::{AttemptState, AttemptTerminalReason, JobId, JobState, JobV1};
use serde_json::{Value, json};
use std::path::Path;

const CANARY_ID: &str = "crm-deployment-canary/1\n";

pub(crate) fn run(directory: &Path) -> Result<Value> {
    prepare_root(directory).map_err(|error| crate::error::io(directory, error))?;
    let database = directory.join("crm.db");
    let _admission = crate::maintenance::gate(&database)?
        .enter()
        .map_err(crate::maintenance::error)?;
    Store::init(&database)?;
    let store = Store::open(&database)?;
    let cases = store.list_cases(2)?;
    let case = if let Some(case) = cases.first() {
        if cases.len() != 1 || case.title != "Synthetic deployment canary" {
            return Err(Error::domain(
                "canary_foreign_state",
                "unexpected CRM canary case",
            ));
        }
        store.case_revision(&case.case_id, None)?
    } else {
        store.create_case("Synthetic deployment canary", "Synthetic fixture only. No actual employer, person, contact, or relationship is represented.", Stage::Research)?
    };
    let updates = store.list_updates(2)?;
    let update = if let Some(update) = updates.first() {
        if updates.len() != 1 || update.case_id != case.case_id {
            return Err(Error::domain(
                "canary_foreign_state",
                "unexpected CRM canary update",
            ));
        }
        update.clone()
    } else {
        store.enqueue_delivery(&case.case_id, "Synthetic evidence", "The fictional Example Organization has an unverified opening. This synthetic evidence is incomplete and should remain visibly advisory. No outreach has occurred.", None)?
    };
    if !update.is_settled() {
        Worker::new(&store).resume(&update.id)?;
    }
    let update = store.update(&update.id)?;
    let revision = store.case_revision(&case.case_id, None)?;
    let history = store.case_history(&case.case_id)?;
    if !update.is_settled()
        || !update.result_posted
        || revision.source_update_id.as_deref() != Some(update.id.as_str())
        || history.len() != 2
        || revision.revision != 2
        || !revision.attention
        || revision.advisory.is_none()
    {
        return Err(Error::domain(
            "canary_not_verified",
            "CRM revision, advisory, or runtime settlement was not proved",
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| crate::error::io(directory, error))?;
    let job = runtime.block_on(async {
        NucleusClient::for_current_user()?
            .get_job(&JobId::new(&update.job_id))
            .await
    })?;
    verify_runtime(&job, &update.job_id, &update.requester_id)?;
    Ok(
        json!({ "protocol_version": 1, "verified": true, "database": database, "case_id": case.case_id, "update_id": update.id, "job_id": update.job_id }),
    )
}

pub(crate) fn verify_runtime(job: &JobV1, job_id: &str, requester_id: &str) -> Result<()> {
    let toolset = job.request.invocation.toolset.as_ref();
    let attempt = job
        .summary
        .current_attempt_id
        .as_ref()
        .and_then(|id| job.attempts.iter().find(|attempt| attempt.id == *id));
    let completed = attempt.is_some_and(|attempt| {
        attempt.job_id == job.summary.id
            && attempt.state == AttemptState::Completed
            && attempt.terminal_reason == Some(AttemptTerminalReason::Completed)
            && attempt.output.as_ref().is_some_and(|output| {
                !output.thread_id.trim().is_empty()
                    && !output.turn_id.trim().is_empty()
                    && !output.final_message.trim().is_empty()
            })
    });
    if job.summary.id.as_str() != job_id
        || job.summary.state != JobState::Completed
        || !completed
        || job.summary.requester.program != "crm"
        || job.summary.requester.id != requester_id
        || toolset.is_none_or(|toolset| {
            toolset.provider != "crm" || toolset.name != "case-steward" || toolset.version != 1
        })
    {
        return Err(Error::domain(
            "canary_not_verified",
            "CRM canary requires a correlated completed Nucleus job with usable structured output",
        ));
    }
    Ok(())
}

fn fixture_file(root: &std::path::Path, relative: &str, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        let mut checked = root.to_path_buf();
        for component in parent
            .strip_prefix(root)
            .map_err(std::io::Error::other)?
            .components()
        {
            checked.push(component);
            match std::fs::create_dir(&checked) {
                Ok(()) => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists
                        && std::fs::symlink_metadata(&checked)?.file_type().is_dir() => {}
                Err(error) => return Err(error),
            }
        }
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(text.as_bytes())?;
            file.sync_all()
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = std::fs::symlink_metadata(&path)?;
            if !metadata.file_type().is_file() || std::fs::read_to_string(&path)? != text {
                return Err(std::io::Error::other(
                    "canary fixture is foreign or changed",
                ));
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn prepare_root(root: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    if !root.is_absolute() {
        return Err(std::io::Error::other("canary directory must be absolute"));
    }
    match std::fs::DirBuilder::new().mode(0o700).create(root) {
        Ok(()) => fixture_file(root, ".deployment-canary", CANARY_ID),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !std::fs::symlink_metadata(root)?.file_type().is_dir()
                || !std::fs::symlink_metadata(root.join(".deployment-canary"))?
                    .file_type()
                    .is_file()
                || std::fs::read_to_string(root.join(".deployment-canary"))? != CANARY_ID
            {
                return Err(std::io::Error::other(
                    "canary directory is not owned by this canary",
                ));
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}
