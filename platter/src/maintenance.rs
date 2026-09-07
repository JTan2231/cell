//! One user-owned fence covers every Platter state directory. Orphaned runtime
//! jobs can be cancelled after all local runners settle; no work is submitted.
use anyhow::{Context, Result, ensure};
use cell_maintenance::{Admission, Gate};
use fs2::FileExt as _;
use nucleus_client::NucleusClient;
use nucleus_core::{JobId, ListJobsQueryV1};
use serde::Serialize;
use std::{
    fs::File,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Serialize)]
pub struct Status {
    pub protocol_version: u32,
    pub holds: Vec<String>,
    pub drained: bool,
    pub nonterminal_jobs: usize,
    pub runner_active: bool,
}

#[must_use]
pub fn gate(home: &Path) -> Gate {
    Gate::new(home.join("Library/Application Support/Platter/deployment-maintenance"))
}

/// Also observe the prototype's runner lock, which predates admission fencing.
fn runner(root: &Path) -> Result<Option<File>> {
    let path = root.join("runner.lock");
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(metadata) => ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "runner lock must be a regular file"
        ),
    }
    let file = File::open(path)?;
    file.try_lock_exclusive()?;
    Ok(Some(file))
}

async fn unfinished(client: &NucleusClient) -> Result<Vec<JobId>> {
    let mut jobs = Vec::new();
    for program in ["platter", "job-packets"] {
        let mut query = ListJobsQueryV1 {
            requester_program: Some(program.into()),
            limit: Some(1000),
            ..Default::default()
        };
        let mut cursors = std::collections::BTreeSet::new();
        loop {
            let page = client.list_jobs(&query).await?;
            ensure!(page.version == 1, "unsupported Nucleus job list version");
            for job in page.jobs {
                ensure!(
                    job.requester.program == program,
                    "Nucleus returned a foreign requester"
                );
                if !job.state.is_terminal() {
                    jobs.push(job.id);
                }
            }
            let Some(next) = page.next else {
                break;
            };
            ensure!(
                cursors.insert(next.to_string()),
                "Nucleus repeated its job cursor"
            );
            query.after = Some(next);
        }
    }
    Ok(jobs)
}

async fn status_inner(home: &Path, root: &Path, client: &NucleusClient) -> Result<Status> {
    let status = gate(home).status()?;
    let runner_active = match runner(root) {
        Ok(_guard) => false,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock) =>
        {
            true
        }
        Err(error) => return Err(error),
    };
    let jobs = unfinished(client).await?;
    Ok(Status {
        protocol_version: 1,
        holds: status.holds,
        drained: status.drained && !runner_active && jobs.is_empty(),
        nonterminal_jobs: jobs.len(),
        runner_active,
    })
}

pub async fn status(home: &Path, root: &Path) -> Result<Status> {
    let client = NucleusClient::for_current_user()?;
    tokio::time::timeout(Duration::from_secs(30), status_inner(home, root, &client))
        .await
        .context("maintenance observation timed out")?
}

pub fn install_admission(home: &Path, root: &Path) -> Result<(Admission, Option<File>)> {
    let owner = std::env::var("CELL_DEPLOYMENT_RUN_ID")
        .context("installation requires CELL_DEPLOYMENT_RUN_ID")?;
    let admission = gate(home).enter_for(&owner)?;
    let runner = runner(root)?;
    Ok((admission, runner))
}

pub async fn drain(home: &Path, root: &Path) -> Result<Status> {
    let client = NucleusClient::for_current_user()?;
    let gate = gate(home);
    let owner =
        std::env::var("CELL_DEPLOYMENT_RUN_ID").context("drain requires CELL_DEPLOYMENT_RUN_ID")?;
    ensure!(
        gate.status()?.holds == [owner.clone()],
        "drain requires the sole run-owned hold"
    );
    let guards = match install_admission(home, root) {
        Ok(guards) => Some(guards),
        Err(error)
            if error
                .downcast_ref::<cell_maintenance::Error>()
                .is_some_and(|error| matches!(error, cell_maintenance::Error::Busy))
                || error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock) =>
        {
            None
        }
        Err(error) => return Err(error),
    };
    if guards.is_some() {
        tokio::time::timeout(Duration::from_secs(30), async {
            for id in unfinished(&client).await? {
                client.cancel_job(&id).await?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("orphaned Nucleus cancellation timed out; hold retained")??;
    }
    drop(guards);
    status(home, root).await
}

pub fn home() -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is required")?);
    ensure!(home.is_absolute(), "HOME must be absolute");
    Ok(home)
}
