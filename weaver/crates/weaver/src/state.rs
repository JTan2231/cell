use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs2::FileExt as _;
use nucleus_core::JobRequestV1;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppResult, WeaverError};

const STATE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Blocked,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Blocked | Self::Failed | Self::Cancelled
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CurrentRun {
    pub(crate) version: u32,
    pub(crate) run_id: String,
    pub(crate) repo_root: PathBuf,
    pub(crate) narrative: String,
    pub(crate) status: RunStatus,
    pub(crate) next_stage: usize,
    pub(crate) outputs_prepared: bool,
    pub(crate) cancel_requested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) active_job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) active_request: Option<JobRequestV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) verdict: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
    pub(crate) created_unix_seconds: u64,
    pub(crate) updated_unix_seconds: u64,
}

impl CurrentRun {
    fn queued(repo_root: PathBuf, narrative: String) -> AppResult<Self> {
        let now = unix_seconds()?;
        Ok(Self {
            version: STATE_VERSION,
            run_id: Uuid::now_v7().to_string(),
            repo_root,
            narrative,
            status: RunStatus::Queued,
            next_stage: 0,
            outputs_prepared: false,
            cancel_requested: false,
            active_job_id: None,
            active_request: None,
            verdict: None,
            detail: Some("waiting for the Weaver worker".to_owned()),
            created_unix_seconds: now,
            updated_unix_seconds: now,
        })
    }

    fn validate(&self, workspace_root: &Path) -> AppResult<()> {
        if self.version != STATE_VERSION {
            return Err(WeaverError::runtime(format!(
                "unsupported Weaver state version {}",
                self.version
            )));
        }
        Uuid::parse_str(&self.run_id).map_err(|error| {
            WeaverError::runtime(format!("invalid current Weaver run ID: {error}"))
        })?;
        if !self.repo_root.is_absolute() || self.narrative.is_empty() || self.next_stage > 5 {
            return Err(WeaverError::runtime(
                "current Weaver state contains invalid project or stage data",
            ));
        }
        if self.status.is_terminal()
            && (self.active_job_id.is_some() || self.active_request.is_some())
        {
            return Err(WeaverError::runtime(
                "terminal Weaver state still retains an active Nucleus request",
            ));
        }
        match (&self.active_job_id, &self.active_request) {
            (Some(job_id), Some(request)) => {
                request.validate().map_err(|error| {
                    WeaverError::runtime(format!(
                        "current Weaver state contains an invalid active request: {error}"
                    ))
                })?;
                let stage = crate::project::STAGES.get(self.next_stage).ok_or_else(|| {
                    WeaverError::runtime(
                        "current Weaver state retains a request after the final stage",
                    )
                })?;
                let expected = crate::nucleus::stage_job_id(&self.run_id, *stage).to_string();
                if request.id.as_str() != job_id
                    || job_id != &expected
                    || request.requester.program != "weaver"
                    || request.requester.id != self.run_id
                    || request.invocation.cwd.as_path() != workspace_root
                {
                    return Err(WeaverError::runtime(
                        "current Weaver state has mismatched active request identity",
                    ));
                }
            }
            (None, None) => {}
            _ => {
                return Err(WeaverError::runtime(
                    "current Weaver state has incomplete active request data",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct StateStore {
    root: PathBuf,
}

pub(crate) struct RunLock {
    _file: File,
}

impl StateStore {
    pub(crate) fn open(root: PathBuf) -> AppResult<Self> {
        if !root.is_absolute() {
            return Err(WeaverError::usage(format!(
                "Weaver state directory must be absolute: {}",
                root.display()
            )));
        }
        match fs::symlink_metadata(&root) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(WeaverError::runtime(format!(
                        "Weaver state must be a non-symlink directory: {}",
                        root.display()
                    )));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                builder.recursive(true).mode(0o700);
                builder.create(&root).map_err(|create_error| {
                    WeaverError::runtime(format!(
                        "cannot create Weaver state directory {}: {create_error}",
                        root.display()
                    ))
                })?;
            }
            Err(error) => {
                return Err(WeaverError::runtime(format!(
                    "cannot inspect Weaver state directory {}: {error}",
                    root.display()
                )));
            }
        }
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).map_err(|error| {
            WeaverError::runtime(format!(
                "cannot secure Weaver state directory {}: {error}",
                root.display()
            ))
        })?;
        let store = Self { root };
        {
            let _control = store.acquire_control_lock()?;
            store.cleanup_state_temporaries_unlocked()?;
        }
        Ok(store)
    }

    pub(crate) fn default_root() -> AppResult<PathBuf> {
        let home = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| WeaverError::usage("HOME is not set; pass --state-dir"))?;
        if !home.is_absolute() {
            return Err(WeaverError::usage(
                "HOME must be absolute; pass --state-dir",
            ));
        }
        Ok(home.join("Library/Application Support/Weaver"))
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn activate_worker(&self) -> AppResult<()> {
        let executable = std::env::current_exe().map_err(|error| {
            WeaverError::runtime(format!("cannot locate the Weaver executable: {error}"))
        })?;
        let mut command = detached_worker_command(&executable, &self.root);
        let mut child = command.spawn().map_err(|error| {
            WeaverError::runtime(format!("cannot start the detached Weaver worker: {error}"))
        })?;
        let reaper = thread::Builder::new()
            .name("weaver-worker-reaper".to_owned())
            .spawn(move || drop(child.wait()))
            .map_err(|error| {
                WeaverError::runtime(format!(
                    "the Weaver worker started, but its reaper could not start: {error}"
                ))
            })?;
        drop(reaper);
        Ok(())
    }

    pub(crate) fn enqueue(&self, repo_root: PathBuf, narrative: String) -> AppResult<CurrentRun> {
        let _control = self.acquire_control_lock()?;
        if self.maintenance_requested_unlocked()? {
            return Err(WeaverError::runtime(
                "Weaver maintenance prevents submitting a workflow",
            ));
        }
        if let Some(current) = self.read_current_unlocked()?
            && !current.status.is_terminal()
        {
            return Err(WeaverError::runtime(format!(
                "Weaver run {} is already {}",
                current.run_id,
                current.status.as_str()
            )));
        }
        let current = CurrentRun::queued(repo_root, narrative)?;
        self.write_current_unlocked(&current)?;
        Ok(current)
    }

    pub(crate) fn read_current(&self, expected_run_id: Option<&str>) -> AppResult<CurrentRun> {
        let _control = self.acquire_control_lock()?;
        let current = self
            .read_current_unlocked()?
            .ok_or_else(|| WeaverError::runtime("Weaver has no current workflow"))?;
        require_run_id(&current, expected_run_id)?;
        Ok(current)
    }

    pub(crate) fn validate_operational_shape(&self) -> AppResult<()> {
        let _control = self.acquire_control_lock()?;
        self.maintenance_requested_unlocked()?;
        self.read_current_unlocked()?;
        for lock_name in [".control.lock", ".run.lock"] {
            let path = self.root.join(lock_name);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_file() => {}
                Ok(_) => {
                    return Err(WeaverError::runtime(format!(
                        "Weaver lock must be a non-symlink file: {}",
                        path.display()
                    )));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(WeaverError::runtime(format!(
                        "cannot inspect Weaver lock {}: {error}",
                        path.display()
                    )));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn update(
        &self,
        run_id: &str,
        mutate: impl FnOnce(&mut CurrentRun) -> AppResult<()>,
    ) -> AppResult<CurrentRun> {
        let _control = self.acquire_control_lock()?;
        let mut current = self
            .read_current_unlocked()?
            .ok_or_else(|| WeaverError::runtime("Weaver has no current workflow"))?;
        require_run_id(&current, Some(run_id))?;
        mutate(&mut current)?;
        current.updated_unix_seconds = unix_seconds()?;
        current.validate(&self.root)?;
        self.write_current_unlocked(&current)?;
        Ok(current)
    }

    pub(crate) fn request_cancel(&self, expected_run_id: Option<&str>) -> AppResult<CurrentRun> {
        let _control = self.acquire_control_lock()?;
        let mut current = self
            .read_current_unlocked()?
            .ok_or_else(|| WeaverError::runtime("Weaver has no current workflow"))?;
        require_run_id(&current, expected_run_id)?;
        if current.status == RunStatus::Cancelled {
            return Ok(current);
        }
        if current.status.is_terminal() {
            return Err(WeaverError::runtime(format!(
                "Weaver run {} is already {}",
                current.run_id,
                current.status.as_str()
            )));
        }
        if current.cancel_requested {
            return Ok(current);
        }
        current.cancel_requested = true;
        current.detail = Some("cancellation requested".to_owned());
        current.updated_unix_seconds = unix_seconds()?;
        self.write_current_unlocked(&current)?;
        Ok(current)
    }

    pub(crate) fn cancellation_requested(&self, run_id: &str) -> AppResult<bool> {
        let current = self.read_current(Some(run_id))?;
        Ok(current.cancel_requested)
    }

    pub(crate) fn try_acquire_run_lock(&self) -> AppResult<Option<RunLock>> {
        let file = Self::open_lock_file(&self.root.join(".run.lock"))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(RunLock { _file: file })),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(WeaverError::runtime(format!(
                "cannot lock Weaver worker: {error}"
            ))),
        }
    }

    pub(crate) fn claim_for_worker(&self) -> AppResult<Option<CurrentRun>> {
        let _control = self.acquire_control_lock()?;
        let Some(mut current) = self.read_current_unlocked()? else {
            return Ok(None);
        };
        if current.status.is_terminal() {
            return Ok(None);
        }
        if self.maintenance_requested_unlocked()? {
            return Ok(None);
        }
        if current.cancel_requested && current.status == RunStatus::Queued {
            current.status = RunStatus::Cancelled;
            current.active_job_id = None;
            current.active_request = None;
            current.detail = Some("cancelled before execution".to_owned());
            current.updated_unix_seconds = unix_seconds()?;
            self.write_current_unlocked(&current)?;
            return Ok(None);
        }
        current.status = RunStatus::Running;
        current.detail = Some(if current.next_stage == 0 {
            "starting workflow".to_owned()
        } else {
            format!("resuming at stage {}/5", current.next_stage + 1)
        });
        current.updated_unix_seconds = unix_seconds()?;
        self.write_current_unlocked(&current)?;
        Ok(Some(current))
    }

    pub(crate) fn begin_maintenance(&self, wait: Duration) -> AppResult<()> {
        {
            let _control = self.acquire_control_lock()?;
            let marker = self.root.join(".maintenance");
            match fs::symlink_metadata(&marker) {
                Ok(metadata) if metadata.file_type().is_file() => {}
                Ok(_) => {
                    return Err(WeaverError::runtime(format!(
                        "Weaver maintenance marker must be a regular file: {}",
                        marker.display()
                    )));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(&marker)
                        .map_err(|create_error| {
                            WeaverError::runtime(format!(
                                "cannot create Weaver maintenance marker: {create_error}"
                            ))
                        })?;
                    file.sync_all().map_err(|sync_error| {
                        WeaverError::runtime(format!(
                            "cannot sync Weaver maintenance marker: {sync_error}"
                        ))
                    })?;
                    sync_directory(&self.root)?;
                }
                Err(error) => {
                    return Err(WeaverError::runtime(format!(
                        "cannot inspect Weaver maintenance marker: {error}"
                    )));
                }
            }
        }

        let started = Instant::now();
        loop {
            if let Some(lock) = self.try_acquire_run_lock()? {
                drop(lock);
                return Ok(());
            }
            if started.elapsed() >= wait {
                return Err(WeaverError::runtime(format!(
                    "timed out waiting {} seconds for the active Weaver run; maintenance remains enabled",
                    wait.as_secs()
                )));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    pub(crate) fn end_maintenance(&self) -> AppResult<()> {
        let _control = self.acquire_control_lock()?;
        let marker = self.root.join(".maintenance");
        match fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.file_type().is_file() => {
                fs::remove_file(&marker).map_err(|error| {
                    WeaverError::runtime(format!(
                        "cannot remove Weaver maintenance marker: {error}"
                    ))
                })?;
                sync_directory(&self.root)?;
                Ok(())
            }
            Ok(_) => Err(WeaverError::runtime(format!(
                "Weaver maintenance marker must be a regular file: {}",
                marker.display()
            ))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(WeaverError::runtime(format!(
                "cannot inspect Weaver maintenance marker: {error}"
            ))),
        }
    }

    fn maintenance_requested_unlocked(&self) -> AppResult<bool> {
        let marker = self.root.join(".maintenance");
        match fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.file_type().is_file() => Ok(true),
            Ok(_) => Err(WeaverError::runtime(format!(
                "Weaver maintenance marker must be a regular file: {}",
                marker.display()
            ))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(WeaverError::runtime(format!(
                "cannot inspect Weaver maintenance marker: {error}"
            ))),
        }
    }

    fn acquire_control_lock(&self) -> AppResult<File> {
        let file = Self::open_lock_file(&self.root.join(".control.lock"))?;
        file.lock_exclusive().map_err(|error| {
            WeaverError::runtime(format!("cannot lock Weaver control state: {error}"))
        })?;
        Ok(file)
    }

    fn open_lock_file(path: &Path) -> AppResult<File> {
        if let Ok(metadata) = fs::symlink_metadata(path)
            && (metadata.file_type().is_symlink() || !metadata.is_file())
        {
            return Err(WeaverError::runtime(format!(
                "Weaver lock must be a non-symlink file: {}",
                path.display()
            )));
        }
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map_err(|error| {
                WeaverError::runtime(format!(
                    "cannot open Weaver lock {}: {error}",
                    path.display()
                ))
            })
    }

    fn read_current_unlocked(&self) -> AppResult<Option<CurrentRun>> {
        let path = self.root.join("current.json");
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(WeaverError::runtime(format!(
                    "cannot inspect current Weaver state: {error}"
                )));
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
            return Err(WeaverError::runtime(format!(
                "current Weaver state must be a nonempty non-symlink file: {}",
                path.display()
            )));
        }
        let bytes = fs::read(&path).map_err(|error| {
            WeaverError::runtime(format!("cannot read current Weaver state: {error}"))
        })?;
        let current: CurrentRun = serde_json::from_slice(&bytes).map_err(|error| {
            WeaverError::runtime(format!("cannot decode current Weaver state: {error}"))
        })?;
        current.validate(&self.root)?;
        Ok(Some(current))
    }

    fn cleanup_state_temporaries_unlocked(&self) -> AppResult<()> {
        let entries = fs::read_dir(&self.root).map_err(|error| {
            WeaverError::runtime(format!("cannot scan Weaver state directory: {error}"))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                WeaverError::runtime(format!("cannot scan Weaver state entry: {error}"))
            })?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !owned_temporary_name(name, "current.json.") {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
                WeaverError::runtime(format!("cannot inspect stale Weaver state: {error}"))
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(WeaverError::runtime(format!(
                    "stale Weaver state path must be a non-symlink file: {}",
                    entry.path().display()
                )));
            }
            fs::remove_file(entry.path()).map_err(|error| {
                WeaverError::runtime(format!("cannot remove stale Weaver state: {error}"))
            })?;
        }
        Ok(())
    }

    fn write_current_unlocked(&self, current: &CurrentRun) -> AppResult<()> {
        current.validate(&self.root)?;
        let path = self.root.join("current.json");
        if let Ok(metadata) = fs::symlink_metadata(&path)
            && (metadata.file_type().is_symlink() || !metadata.is_file())
        {
            return Err(WeaverError::runtime(format!(
                "current Weaver state must be a non-symlink file: {}",
                path.display()
            )));
        }
        let temporary = self
            .root
            .join(format!("current.json.{}.tmp", Uuid::now_v7().simple()));
        let mut bytes = serde_json::to_vec_pretty(current).map_err(|error| {
            WeaverError::runtime(format!("cannot encode current Weaver state: {error}"))
        })?;
        bytes.push(b'\n');
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|error| {
                WeaverError::runtime(format!("cannot create current Weaver state: {error}"))
            })?;
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
            let _ = fs::remove_file(&temporary);
            return Err(WeaverError::runtime(format!(
                "cannot persist current Weaver state: {error}"
            )));
        }
        if let Err(error) = fs::rename(&temporary, &path) {
            let _ = fs::remove_file(&temporary);
            return Err(WeaverError::runtime(format!(
                "cannot install current Weaver state: {error}"
            )));
        }
        sync_directory(&self.root)?;
        Ok(())
    }
}

fn detached_worker_command(executable: &Path, state_root: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("--state-dir")
        .arg(state_root)
        .arg("worker")
        .arg("run")
        .current_dir(state_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    command
}

fn require_run_id(current: &CurrentRun, expected: Option<&str>) -> AppResult<()> {
    if let Some(expected) = expected
        && current.run_id != expected
    {
        return Err(WeaverError::usage(format!(
            "current Weaver run is {}, not {expected}",
            current.run_id
        )));
    }
    Ok(())
}

fn owned_temporary_name(name: &str, prefix: &str) -> bool {
    let Some(identifier) = name
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(".tmp"))
    else {
        return false;
    };
    identifier.len() == 32
        && identifier
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn unix_seconds() -> AppResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| {
            WeaverError::runtime(format!("system clock is before Unix epoch: {error}"))
        })
}

fn sync_directory(path: &Path) -> AppResult<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            WeaverError::runtime(format!("cannot sync directory {}: {error}", path.display()))
        })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use nucleus_core::{
        AbsolutePath, AgentInvocationV1, BuiltinToolsV1, JobRequestV1, ModelId, ReasoningEffort,
        Requester, TimeoutSeconds, WorkspaceAccess,
    };
    use tempfile::TempDir;

    use super::*;

    fn must<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("unexpected error: {error}"),
        }
    }

    fn stage_request(current: &CurrentRun, workspace_root: &Path) -> JobRequestV1 {
        let mut invocation = AgentInvocationV1::new(
            "codex",
            ModelId::new("gpt-5.6-sol"),
            AbsolutePath::new(workspace_root.to_path_buf()),
            WorkspaceAccess::ReadOnly,
            BuiltinToolsV1 {
                local_execution: false,
                web_search: false,
            },
            TimeoutSeconds::new(3_600),
        );
        invocation.reasoning_effort = Some(ReasoningEffort::Max);
        JobRequestV1::new(
            crate::nucleus::stage_job_id(&current.run_id, crate::project::STAGES[0]),
            "stage",
            Requester {
                program: "weaver".to_owned(),
                id: current.run_id.clone(),
            },
            "instructions",
            "prompt",
            invocation,
        )
    }

    #[test]
    fn one_current_run_replaces_only_a_terminal_run() {
        let temporary = must(TempDir::new());
        let store = must(StateStore::open(temporary.path().join("state")));
        let first = must(store.enqueue(PathBuf::from("/tmp/repo"), "one".to_owned()));
        assert!(
            store
                .enqueue(PathBuf::from("/tmp/repo"), "two".to_owned())
                .is_err()
        );
        must(store.update(&first.run_id, |current| {
            current.status = RunStatus::Succeeded;
            current.detail = None;
            Ok(())
        }));
        let second = must(store.enqueue(PathBuf::from("/tmp/repo"), "two".to_owned()));
        assert_ne!(first.run_id, second.run_id);
        assert_eq!(must(store.read_current(None)).narrative, "two");
    }

    #[test]
    fn active_request_uses_the_exact_private_state_root() {
        let temporary = must(TempDir::new());
        let store = must(StateStore::open(temporary.path().join("state")));
        let current = must(store.enqueue(PathBuf::from("/tmp/repo"), "one".to_owned()));
        let request = stage_request(&current, store.root());
        let request_id = request.id.to_string();
        must(store.update(&current.run_id, |state| {
            state.status = RunStatus::Running;
            state.active_job_id = Some(request_id);
            state.active_request = Some(request);
            Ok(())
        }));

        assert!(
            store
                .update(&current.run_id, |state| {
                    let request = state.active_request.as_mut().ok_or_else(|| {
                        WeaverError::runtime("test active request unexpectedly absent")
                    })?;
                    request.invocation.cwd = AbsolutePath::new(state.repo_root.clone());
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(
            must(store.read_current(None))
                .active_request
                .map(|request| request.invocation.cwd),
            Some(AbsolutePath::new(store.root().to_path_buf()))
        );
    }

    #[test]
    fn maintenance_blocks_enqueue_and_new_worker_claims() {
        let temporary = must(TempDir::new());
        let store = must(StateStore::open(temporary.path().join("state")));
        let current = must(store.enqueue(PathBuf::from("/tmp/repo"), "one".to_owned()));
        must(store.begin_maintenance(Duration::ZERO));
        let run_lock = must(store.try_acquire_run_lock());
        assert!(run_lock.is_some());
        assert!(must(store.claim_for_worker()).is_none());
        assert!(
            store
                .enqueue(PathBuf::from("/tmp/repo"), "two".to_owned())
                .is_err()
        );
        assert_eq!(must(store.read_current(None)).run_id, current.run_id);
        must(store.end_maintenance());
    }

    #[test]
    fn cancellation_intent_is_idempotent() {
        let temporary = must(TempDir::new());
        let store = must(StateStore::open(temporary.path().join("state")));
        let current = must(store.enqueue(PathBuf::from("/tmp/repo"), "one".to_owned()));

        let first = must(store.request_cancel(Some(&current.run_id)));
        let second = must(store.request_cancel(Some(&current.run_id)));
        assert!(first.cancel_requested);
        assert_eq!(first, second);

        must(store.update(&current.run_id, |state| {
            state.status = RunStatus::Cancelled;
            state.detail = Some("cancelled".to_owned());
            Ok(())
        }));
        assert_eq!(
            must(store.request_cancel(Some(&current.run_id))).status,
            RunStatus::Cancelled
        );
    }

    #[test]
    fn detached_worker_uses_the_invoking_binary_and_exact_state_root() {
        let executable = Path::new("/tmp/weaver-release/bin/weaver");
        let state_root = Path::new("/tmp/Weaver State");
        let command = detached_worker_command(executable, state_root);

        assert_eq!(command.get_program(), executable.as_os_str());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                OsStr::new("--state-dir"),
                state_root.as_os_str(),
                OsStr::new("worker"),
                OsStr::new("run")
            ]
        );
        assert_eq!(command.get_current_dir(), Some(state_root));
    }
}
