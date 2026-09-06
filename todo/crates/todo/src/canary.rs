use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::{db, reconciliation, reconciliation_store as store};
use nucleus_client::NucleusClient;
use nucleus_core::{AttemptState, AttemptTerminalReason, JobId, JobState, JobV1};
use serde_json::{Value, json};
use std::path::Path;

const CANARY_ID: &str = "todo-deployment-canary/1\n";

pub(crate) fn run(directory: &Path) -> AppResult<Value> {
    prepare_root(directory)?;
    fixture_file(
        directory,
        "source.md",
        "Synthetic deployment fixture: a fictional greenhouse needs a written watering checklist. No real task or person is represented.\n",
    )?;
    let database = directory.join("todo.db");
    let _admission = cell_maintenance::Gate::new(directory.join("deployment-maintenance"))
        .enter()
        .map_err(|error| AppError::conflict("deployment_maintenance", error.to_string()))?;
    if !database.exists() {
        db::init(&database)?;
    }
    let mut connection = db::open_write(&database)?;
    let concerns = store::list_concerns(&connection, true, 2)?;
    let concern = if let Some(concern) = concerns.first() {
        if concerns.len() != 1 {
            return Err(AppError::conflict(
                "canary_foreign_state",
                "unexpected Todo canary concerns",
            ));
        }
        concern.clone()
    } else {
        store::capture_concern(
            &mut connection,
            "Prepare a watering checklist for the synthetic greenhouse.",
            &directory.join("source.md"),
        )?
    };
    let routing = store::list_routing_for_concern(&connection, concern.id)?;
    if routing.is_empty() {
        let jobs: i64 =
            connection.query_row("SELECT count(*) FROM todo_agent_jobs", [], |row| row.get(0))?;
        if jobs != 0 {
            return Err(AppError::conflict(
                "canary_interrupted",
                "existing canary research has no routing result; retained evidence requires recovery, never an automatic replacement attempt",
            ));
        }
        reconciliation::route_concern(&database, &Config::default(), concern.id, None, None)?;
    }
    let routing = store::list_routing_for_concern(&connection, concern.id)?;
    if routing.len() != 1 || routing[0].decision != "pending" {
        return Err(AppError::conflict(
            "canary_not_verified",
            "one pending Todo routing result was not proved",
        ));
    }
    let (job_id, requester_id): (String, String) = connection.query_row(
        "SELECT nucleus_job_id, nucleus_requester_id FROM todo_agent_jobs",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let job = runtime
        .block_on(async {
            NucleusClient::for_current_user()?
                .get_job(&JobId::new(&job_id))
                .await
        })
        .map_err(|error| AppError::unexpected("canary_runtime_failed", error.to_string()))?;
    verify_runtime(&job, &job_id, &requester_id)?;
    Ok(
        json!({ "protocol_version": 1, "verified": true, "database": database, "concern_id": concern.id, "routing_id": routing[0].id, "job_id": job_id }),
    )
}

pub(crate) fn verify_runtime(job: &JobV1, job_id: &str, requester_id: &str) -> AppResult<()> {
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
    let toolset = job.request.invocation.toolset.as_ref();
    if job.summary.id.as_str() != job_id
        || job.summary.state != JobState::Completed
        || !completed
        || job.summary.requester.program != "todo"
        || job.summary.requester.id != requester_id
        || toolset.is_none_or(|toolset| {
            toolset.provider != "todo" || toolset.name != "concern-routing" || toolset.version != 1
        })
    {
        return Err(AppError::conflict(
            "canary_not_verified",
            "Todo canary requires a correlated completed Nucleus job with usable structured output",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canary_requires_clean_current_attempt_output() -> Result<(), Box<dyn std::error::Error>> {
        let request = json!({
            "version": 1, "id": "canary-job", "label": "Synthetic canary",
            "requester": {"program": "todo", "id": "canary-requester"},
            "instructions": "Synthetic fixture only", "prompt": "Record a proposal",
            "invocation": {
                "version": 1, "harness": "codex", "model": "fixture-model",
                "reasoningEffort": "low", "cwd": "/tmp", "workspaceAccess": "none",
                "builtinTools": {"localExecution": false, "webSearch": false},
                "timeoutSeconds": 60,
                "toolset": {"provider": "todo", "name": "concern-routing", "version": 1}
            }
        });
        let job: JobV1 = serde_json::from_value(json!({
            "version": 1, "request": request,
            "summary": {
                "version": 1, "id": "canary-job", "label": "Synthetic canary",
                "requester": {"program": "todo", "id": "canary-requester"},
                "state": "completed", "requestDigest": "sha256:fixture",
                "createdAt": "2026-09-01T00:00:00Z", "updatedAt": "2026-09-01T00:00:01Z",
                "completedAt": "2026-09-01T00:00:01Z", "currentAttemptId": "attempt-1"
            },
            "attempts": [{
                "version": 1, "id": "attempt-1", "jobId": "canary-job", "ordinal": 1,
                "harness": {"harness": "codex", "harnessVersion": "0.146.0", "adapterVersion": "fixture"},
                "state": "completed", "terminalReason": "completed",
                "createdAt": "2026-09-01T00:00:00Z", "completedAt": "2026-09-01T00:00:01Z",
                "output": {"threadId": "thread-1", "turnId": "turn-1", "finalMessage": "Proposal recorded."}
            }]
        }))?;
        verify_runtime(&job, "canary-job", "canary-requester")?;
        for invalid in [
            "missing-output",
            "blank-output",
            "failed",
            "wrong-attempt",
            "wrong-toolset",
        ] {
            let mut candidate = serde_json::to_value(&job)?;
            match invalid {
                "missing-output" => {
                    candidate["attempts"][0]
                        .as_object_mut()
                        .ok_or("attempt is not an object")?
                        .remove("output");
                }
                "blank-output" => candidate["attempts"][0]["output"]["finalMessage"] = json!(" \n"),
                "failed" => candidate["summary"]["state"] = json!("failed"),
                "wrong-attempt" => candidate["summary"]["currentAttemptId"] = json!("other"),
                _ => candidate["request"]["invocation"]["toolset"]["name"] = json!("other"),
            }
            let candidate = serde_json::from_value(candidate)?;
            let Err(error) = verify_runtime(&candidate, "canary-job", "canary-requester") else {
                return Err(format!("accepted {invalid}").into());
            };
            assert_eq!(error.code(), "canary_not_verified");
        }
        Ok(())
    }
}
