//! Durable holds live beside domain records in the canonical Platter database.
//! An advisory lock on the state directory serializes live commands without a lock file.
use crate::store::{DATABASE, Store};
use anyhow::{Context, Result, ensure};
use cell_maintenance::Gate as LegacyGate;
use fs2::FileExt as _;
use nucleus_client::NucleusClient;
use nucleus_core::{JobId, ListJobsQueryV1};
use rusqlite::{OptionalExtension as _, TransactionBehavior};
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

pub struct Gate {
    home: PathBuf,
}
pub struct Admission {
    _directory: File,
    _legacy: Option<cell_maintenance::Admission>,
}

#[must_use]
pub fn gate(home: &Path) -> Gate {
    Gate { home: home.into() }
}

impl Gate {
    fn root(&self) -> Result<PathBuf> {
        crate::default_state_dir(&self.home)
    }
    fn legacy(&self) -> Result<Option<LegacyGate>> {
        let path = self
            .home
            .join("Library/Application Support/Platter/deployment-maintenance");
        Ok(if path.try_exists()? {
            Some(LegacyGate::new(path))
        } else {
            None
        })
    }
    pub fn path(&self) -> Result<PathBuf> {
        Ok(self.root()?.join(DATABASE))
    }
    pub fn enter(&self) -> Result<Admission> {
        self.admit(None)
    }
    pub fn enter_for(&self, owner: &str) -> Result<Admission> {
        validate_owner(owner)?;
        self.admit(Some(owner))
    }
    fn admit(&self, owner: Option<&str>) -> Result<Admission> {
        let legacy_gate = self.legacy()?;
        let legacy_owners = legacy_gate
            .as_ref()
            .map(LegacyGate::status)
            .transpose()?
            .map(|status| status.holds)
            .unwrap_or_default();
        let legacy = legacy_gate
            .map(|gate| match owner {
                Some(owner) => gate.enter_for(owner),
                None => gate.enter(),
            })
            .transpose()?;
        let store = Store::control(&self.root()?)?;
        let file = File::open(store.root())?;
        file.try_lock_exclusive().map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                cell_maintenance::Error::Busy
            } else {
                cell_maintenance::Error::Io(error)
            }
        })?;
        let tx = rusqlite::Transaction::new_unchecked(
            &store.connection,
            TransactionBehavior::Immediate,
        )?;
        for owner in legacy_owners {
            tx.execute(
                "INSERT OR IGNORE INTO maintenance_holds VALUES(?1)",
                [owner],
            )?;
        }
        let holds = holds(&store.connection)?;
        ensure!(
            match owner {
                Some(owner) => holds == [owner],
                None => holds.is_empty(),
            },
            cell_maintenance::Error::Held
        );
        tx.commit()?;
        Ok(Admission {
            _directory: file,
            _legacy: legacy,
        })
    }
    pub fn hold(&self, owner: &str) -> Result<cell_maintenance::Status> {
        validate_owner(owner)?;
        let legacy = self.legacy()?;
        if let Some(gate) = &legacy {
            gate.hold(owner)?;
        }
        let store = Store::control(&self.root()?)?;
        store.connection.execute(
            "INSERT OR IGNORE INTO maintenance_holds VALUES(?1)",
            [owner],
        )?;
        if let Some(gate) = legacy {
            for owner in gate.status()?.holds {
                store.connection.execute(
                    "INSERT OR IGNORE INTO maintenance_holds VALUES(?1)",
                    [owner],
                )?;
            }
        }
        self.status()
    }
    pub fn release(&self, owner: &str) -> Result<cell_maintenance::Status> {
        validate_owner(owner)?;
        let root = self.root()?;
        if root.join(DATABASE).try_exists()? {
            let store = Store::control(&root)?;
            store
                .connection
                .execute("DELETE FROM maintenance_holds WHERE owner=?1", [owner])?;
        }
        if let Some(gate) = self.legacy()? {
            gate.release(owner)?;
        }
        let status = self.status()?;
        if status.holds.is_empty()
            && status.drained
            && root.join(DATABASE).try_exists()?
            && Store::control(&root)?.version()? == crate::store::SCHEMA_VERSION
        {
            self.retire_legacy_gate()?;
        }
        Ok(status)
    }
    pub fn status(&self) -> Result<cell_maintenance::Status> {
        let root = self.root()?;
        let mut owners = std::collections::BTreeSet::new();
        let mut drained = true;
        if let Some(gate) = self.legacy()? {
            let status = gate.status()?;
            owners.extend(status.holds);
            drained = status.drained;
        }
        let path = root.join(DATABASE);
        if path.try_exists()? {
            crate::store::regular_file(&path)?;
            let connection = rusqlite::Connection::open_with_flags(
                &path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let exists: Option<String>=connection.query_row("SELECT name FROM sqlite_master WHERE type='table' AND name='maintenance_holds'",[],|r|r.get(0)).optional()?;
            if exists.is_some() {
                owners.extend(holds(&connection)?);
            }
            let file = File::open(&root)?;
            match file.try_lock_exclusive() {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => drained = false,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(cell_maintenance::Status {
            contract_version: 1,
            holds: owners.into_iter().collect(),
            drained,
        })
    }
    fn retire_legacy_gate(&self) -> Result<()> {
        let Some(gate) = self.legacy()? else {
            return Ok(());
        };
        let control = File::open(gate.path().join("control.lock"))?;
        control.try_lock_exclusive()?;
        let activity = File::open(gate.path().join("activity.lock"))?;
        activity.try_lock_exclusive()?;
        ensure!(
            std::fs::read_dir(gate.path().join("holds"))?
                .next()
                .is_none(),
            "legacy holds remain"
        );
        for entry in std::fs::read_dir(gate.path())? {
            let entry = entry?;
            ensure!(
                matches!(
                    entry.file_name().to_str(),
                    Some("holds" | "control.lock" | "activity.lock")
                ),
                "unexpected legacy maintenance state; preserve it"
            );
        }
        std::fs::remove_dir(gate.path().join("holds"))?;
        std::fs::remove_file(gate.path().join("activity.lock"))?;
        std::fs::remove_file(gate.path().join("control.lock"))?;
        std::fs::remove_dir(gate.path())?;
        Ok(())
    }
}
fn holds(connection: &rusqlite::Connection) -> Result<Vec<String>> {
    let mut statement = connection.prepare("SELECT owner FROM maintenance_holds ORDER BY owner")?;
    Ok(statement
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}
fn validate_owner(owner: &str) -> Result<()> {
    ensure!(
        !owner.is_empty()
            && owner.len() <= 128
            && !owner.starts_with('.')
            && owner
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
        cell_maintenance::Error::InvalidOwner
    );
    Ok(())
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
