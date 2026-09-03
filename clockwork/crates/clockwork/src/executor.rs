use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};
use std::time::Duration;

use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt as _;
use tokio::process::{Child, Command};
use tokio::signal::unix::{Signal, SignalKind, signal};

use crate::error::{Context as _, Error, Result};
use crate::launchd;
use crate::lock::KeyLock;
use crate::manifest;
use crate::model::{ActivationRecord, ActivationState, LaunchImage, Manifest, Trigger};
use crate::paths::{Layout, current_uid};
use crate::store::Store;

const TERMINATION_GRACE: Duration = Duration::from_secs(5);

#[allow(clippy::too_many_lines)]
pub(crate) async fn run(
    store: &mut Store,
    layout: &Layout,
    key: &str,
    trigger: Trigger,
) -> Result<ActivationRecord> {
    manifest::validate_key(key)?;
    let _transition_gate = KeyLock::acquire_activation_gate(layout, key)?;
    launchd::require_no_pending_transition(layout, key)?;
    let selected = store.selected_definition(key)?;
    let Some(_activation_lock) = KeyLock::try_acquire_activation(layout, key)? else {
        return overlap(store, key, &selected.digest, trigger);
    };

    store.recover_stale(Some(key))?;
    if store.has_running_activation(key)? {
        return overlap(store, key, &selected.digest, trigger);
    }

    let admitted = store.begin_activation(key, &selected.digest, trigger)?;
    if let Err(error) = manifest::validate(&selected.manifest, layout) {
        let detail = error.to_string();
        let _ = store.finish_activation(
            &admitted.id,
            ActivationState::StartFailed,
            None,
            None,
            Some(&detail),
        );
        return Err(error);
    }

    let mut signals = match BrokerSignals::new() {
        Ok(signals) => signals,
        Err(error) => {
            record_start_failure(store, &admitted.id, &error);
            return Err(error);
        }
    };
    let mut gate = match build_gate_command(layout, key, &admitted.id) {
        Ok(gate) => gate,
        Err(error) => {
            record_start_failure(store, &admitted.id, &error);
            return Err(error);
        }
    };
    let mut child = match gate.command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let error = Error::new(
                "activation_start_failed",
                format!("spawn registered launch image: {error}"),
            );
            record_start_failure(store, &admitted.id, &error);
            return Err(error);
        }
    };
    let Some(mut gate_stdin) = child.stdin.take() else {
        let error = Error::new(
            "activation_start_failed",
            "spawned execution gate did not expose its handshake pipe",
        );
        if child.kill().await.is_ok() {
            record_start_failure(store, &admitted.id, &error);
        }
        return Err(error);
    };
    let Some(child_pid) = child.id() else {
        let error = Error::new(
            "activation_start_failed",
            "spawned child did not expose a process id",
        );
        if child.kill().await.is_ok() {
            record_start_failure(store, &admitted.id, &error);
        }
        return Err(error);
    };
    if let Err(error) = store.mark_started(&admitted.id, child_pid) {
        return match recover_terminal_child(&mut child, child_pid).await {
            Ok(_) => {
                record_start_failure(store, &admitted.id, &error);
                Err(error)
            }
            Err(cleanup) => Err(Error::new(
                "activation_supervision_lost",
                format!("{error}; direct child termination was not proved: {cleanup}"),
            )),
        };
    }
    if let Err(write_error) = gate_stdin.write_all(b"G").await {
        let error = Error::new(
            "activation_start_failed",
            format!("release registered execution gate: {write_error}"),
        );
        return match recover_terminal_child(&mut child, child_pid).await {
            Ok(_) => {
                record_start_failure(store, &admitted.id, &error);
                Err(error)
            }
            Err(cleanup) => Err(Error::new(
                "activation_supervision_lost",
                format!("{error}; execution gate termination was not proved: {cleanup}"),
            )),
        };
    }
    drop(gate_stdin);

    let outcome = wait_for_child(
        &mut child,
        child_pid,
        selected.manifest.timeout_seconds.map(Duration::from_secs),
        &mut signals,
    )
    .await;
    match outcome {
        Ok(WaitOutcome::Completed(status)) => {
            let gate_status = match read_gate_status(&mut gate.status) {
                Ok(gate_status) => gate_status,
                Err(error) => {
                    let detail = error.to_string();
                    finish_observed_exit(store, &admitted.id, status, Some(&detail))?;
                    return Err(error);
                }
            };
            match gate_status {
                GateStatus::ProductExec => {}
                GateStatus::StartFailed(detail) => {
                    let error = Error::new(
                        "activation_start_failed",
                        format!("registered execution gate failed before product exec: {detail}"),
                    );
                    record_start_failure(store, &admitted.id, &error);
                    return Err(error);
                }
            }
            finish_observed_exit(store, &admitted.id, status, None)
        }
        Ok(WaitOutcome::TimedOut(status)) => store.finish_activation(
            &admitted.id,
            ActivationState::TimedOut,
            status.code(),
            status.signal(),
            Some("registered timeout elapsed; process group received TERM then KILL if needed"),
        ),
        Err(error) => {
            let detail = error.to_string();
            match recover_terminal_child(&mut child, child_pid).await {
                Ok(status) => {
                    if let Some(signal) = status.signal() {
                        store.finish_activation(
                            &admitted.id,
                            ActivationState::Signaled,
                            None,
                            Some(signal),
                            Some(&detail),
                        )?;
                    } else {
                        store.finish_activation(
                            &admitted.id,
                            ActivationState::Exited,
                            status.code(),
                            None,
                            Some(&detail),
                        )?;
                    }
                    Err(error)
                }
                Err(cleanup) => Err(Error::new(
                    "activation_supervision_lost",
                    format!("{error}; direct child termination was not proved: {cleanup}"),
                )),
            }
        }
    }
}

fn finish_observed_exit(
    store: &mut Store,
    activation_id: &str,
    status: std::process::ExitStatus,
    detail: Option<&str>,
) -> Result<ActivationRecord> {
    if let Some(signal) = status.signal() {
        store.finish_activation(
            activation_id,
            ActivationState::Signaled,
            None,
            Some(signal),
            detail,
        )
    } else {
        store.finish_activation(
            activation_id,
            ActivationState::Exited,
            status.code(),
            None,
            detail,
        )
    }
}

fn overlap(
    store: &mut Store,
    key: &str,
    digest: &str,
    trigger: Trigger,
) -> Result<ActivationRecord> {
    match trigger {
        Trigger::Launchd => store.record_skipped_overlap(key, digest, trigger),
        Trigger::Manual => {
            store.record_skipped_overlap(key, digest, trigger)?;
            Err(Error::new(
                "activation_busy",
                format!("an activation already owns {key}"),
            ))
        }
    }
}

fn record_start_failure(store: &mut Store, id: &str, error: &Error) {
    let detail = error.to_string();
    let _ = store.finish_activation(id, ActivationState::StartFailed, None, None, Some(&detail));
}

struct GateCommand {
    command: Command,
    status: NamedTempFile,
}

fn build_gate_command(layout: &Layout, key: &str, activation_id: &str) -> Result<GateCommand> {
    let executable = std::env::current_exe()
        .context(
            "clockwork_binary_unavailable",
            "locate Clockwork execution gate",
        )?
        .canonicalize()
        .context(
            "clockwork_binary_unavailable",
            "canonicalize Clockwork execution gate",
        )?;
    let mut status = NamedTempFile::new_in(layout.locks_root()).context(
        "activation_start_failed",
        "create private execution-gate status file",
    )?;
    status.write_all(b"PENDING\n").context(
        "activation_start_failed",
        "initialize execution-gate status",
    )?;
    status
        .flush()
        .context("activation_start_failed", "flush execution-gate status")?;
    let status_path = status.path().to_path_buf();
    let mut standard = StdCommand::new(executable);
    if let Some(state_root) = layout.state_root_override() {
        standard.arg("--state-root").arg(state_root);
    }
    standard
        .arg("__exec")
        .arg(key)
        .arg(activation_id)
        .arg(&status_path)
        .env_clear()
        .env("HOME", layout.home())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut command = Command::from(standard);
    command.kill_on_drop(true);
    Ok(GateCommand { command, status })
}

fn product_command(manifest: &Manifest, stdout: File, stderr: File) -> StdCommand {
    let mut standard = match &manifest.launch {
        LaunchImage::Direct { program, .. } => StdCommand::new(program),
        LaunchImage::Interpreted {
            interpreter,
            script,
            ..
        } => {
            let mut command = StdCommand::new(interpreter);
            command.arg(script);
            command
        }
    };
    standard
        .args(&manifest.arguments)
        .current_dir(&manifest.cwd)
        .env_clear()
        .envs(&manifest.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    standard
}

pub(crate) fn exec_registered(
    store: &Store,
    layout: &Layout,
    key: &str,
    activation_id: &str,
    status: &mut File,
) -> Result<()> {
    manifest::validate_key(key)?;
    let activation = store.activation(activation_id)?;
    if activation.key != key
        || activation.state != ActivationState::Running
        || activation.child_pid != Some(std::process::id())
    {
        return Err(Error::new(
            "activation_gate_invalid",
            "execution gate does not match the recorded running activation",
        ));
    }
    let definition = store.definition(&activation.definition_digest)?;
    if definition.key != key {
        return Err(Error::new(
            "activation_gate_invalid",
            "execution gate definition belongs to another binding",
        ));
    }
    manifest::validate(&definition.manifest, layout)?;
    let stdout = open_output(Path::new(&definition.manifest.output.stdout), "stdout")?;
    let stderr = open_output(Path::new(&definition.manifest.output.stderr), "stderr")?;
    require_distinct_outputs(&stdout, &stderr)?;
    clear_gate_status(status)?;
    let error = product_command(&definition.manifest, stdout, stderr).exec();
    Err(Error::new(
        "activation_exec_failed",
        format!("execute registered launch image: {error}"),
    ))
}

pub(crate) fn write_gate_failure(status: &mut File, error: &Error) -> Result<()> {
    status.set_len(0).context(
        "activation_start_failed",
        "reset execution-gate status file",
    )?;
    status.seek(SeekFrom::Start(0)).context(
        "activation_start_failed",
        "rewind execution-gate status file",
    )?;
    writeln!(status, "ERROR\t{}\t{}", error.code(), error.message())
        .context("activation_start_failed", "write execution-gate failure")?;
    status
        .flush()
        .context("activation_start_failed", "flush execution-gate failure")
}

fn clear_gate_status(status: &mut File) -> Result<()> {
    status.set_len(0).context(
        "activation_start_failed",
        "clear execution-gate status file",
    )?;
    status
        .flush()
        .context("activation_start_failed", "clear execution-gate status")
}

pub(crate) fn claim_gate_status(layout: &Layout, path: &Path) -> Result<File> {
    if path.parent() != Some(layout.locks_root().as_path()) {
        return Err(Error::new(
            "activation_gate_invalid",
            "execution-gate status file is outside Clockwork's private lock directory",
        ));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .context(
            "activation_start_failed",
            "open private execution-gate status file",
        )?;
    let metadata = file.metadata().context(
        "activation_start_failed",
        "inspect execution-gate status file",
    )?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != current_uid()?
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(Error::new(
            "activation_gate_invalid",
            "execution-gate status must be a private, current-user-owned, non-hard-linked regular file",
        ));
    }
    fs::remove_file(path).context(
        "activation_start_failed",
        "unlink claimed execution-gate status file",
    )?;
    File::open(layout.locks_root())
        .and_then(|directory| directory.sync_all())
        .context(
            "activation_start_failed",
            "sync execution-gate status-file removal",
        )?;
    Ok(file)
}

enum GateStatus {
    ProductExec,
    StartFailed(String),
}

fn read_gate_status(status: &mut NamedTempFile) -> Result<GateStatus> {
    const MAX_GATE_FAILURE_BYTES: u64 = 64 * 1024;
    status
        .as_file_mut()
        .seek(SeekFrom::Start(0))
        .context("activation_wait_failed", "rewind execution-gate status")?;
    let mut bytes = Vec::new();
    status
        .as_file_mut()
        .take(MAX_GATE_FAILURE_BYTES + 1)
        .read_to_end(&mut bytes)
        .context(
            "activation_wait_failed",
            "read execution-gate status channel",
        )?;
    if bytes.len() as u64 > MAX_GATE_FAILURE_BYTES {
        return Err(Error::new(
            "activation_wait_failed",
            "execution-gate failure exceeded the status-channel limit",
        ));
    }
    if bytes.is_empty() {
        return Ok(GateStatus::ProductExec);
    }
    let detail = String::from_utf8_lossy(&bytes).trim().to_owned();
    if detail == "PENDING" {
        Ok(GateStatus::StartFailed(
            "execution gate ended before it attempted product exec".to_owned(),
        ))
    } else if let Some(error) = detail.strip_prefix("ERROR\t") {
        Ok(GateStatus::StartFailed(error.to_owned()))
    } else {
        Err(Error::new(
            "activation_wait_failed",
            "execution-gate status file contained an invalid state",
        ))
    }
}

fn open_output(path: &Path, label: &str) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .context(
            "activation_output_unavailable",
            format!("open {label} {}", path.display()),
        )?;
    let metadata = file
        .metadata()
        .context("activation_output_unavailable", format!("inspect {label}"))?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.uid() != current_uid()? {
        return Err(Error::new(
            "activation_output_unavailable",
            format!(
                "{label} {} must be a current-user-owned, non-hard-linked regular file",
                path.display()
            ),
        ));
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .context(
            "activation_output_unavailable",
            format!("make {label} private"),
        )?;
    Ok(file)
}

fn require_distinct_outputs(stdout: &File, stderr: &File) -> Result<()> {
    let stdout_metadata = stdout.metadata().context(
        "activation_output_unavailable",
        "inspect opened stdout identity",
    )?;
    let stderr_metadata = stderr.metadata().context(
        "activation_output_unavailable",
        "inspect opened stderr identity",
    )?;
    if stdout_metadata.dev() == stderr_metadata.dev()
        && stdout_metadata.ino() == stderr_metadata.ino()
    {
        return Err(Error::new(
            "activation_output_unavailable",
            "stdout and stderr must resolve to distinct files",
        ));
    }
    Ok(())
}

enum WaitOutcome {
    Completed(std::process::ExitStatus),
    TimedOut(std::process::ExitStatus),
}

struct BrokerSignals {
    terminate: Signal,
    interrupt: Signal,
}

impl BrokerSignals {
    fn new() -> Result<Self> {
        Ok(Self {
            terminate: signal(SignalKind::terminate())
                .context("signal_unavailable", "listen for SIGTERM")?,
            interrupt: signal(SignalKind::interrupt())
                .context("signal_unavailable", "listen for SIGINT")?,
        })
    }
}

async fn wait_for_child(
    child: &mut Child,
    child_pid: u32,
    timeout: Option<Duration>,
    signals: &mut BrokerSignals,
) -> Result<WaitOutcome> {
    match timeout {
        Some(timeout) => {
            tokio::select! {
                status = child.wait() => status
                    .context("activation_wait_failed", "wait for registered child")
                    .map(WaitOutcome::Completed),
                _ = signals.terminate.recv() => forward_and_wait(child, child_pid, "TERM", false).await,
                _ = signals.interrupt.recv() => forward_and_wait(child, child_pid, "INT", false).await,
                () = tokio::time::sleep(timeout) => {
                    forward_and_wait(child, child_pid, "TERM", true).await
                }
            }
        }
        None => {
            tokio::select! {
                status = child.wait() => status
                    .context("activation_wait_failed", "wait for registered child")
                    .map(WaitOutcome::Completed),
                _ = signals.terminate.recv() => forward_and_wait(child, child_pid, "TERM", false).await,
                _ = signals.interrupt.recv() => forward_and_wait(child, child_pid, "INT", false).await,
            }
        }
    }
}

async fn forward_and_wait(
    child: &mut Child,
    child_pid: u32,
    signal_name: &str,
    timed_out: bool,
) -> Result<WaitOutcome> {
    if let Err(error) = signal_group(child_pid, signal_name) {
        if let Some(status) = child
            .try_wait()
            .context("activation_wait_failed", "check child after signal race")?
        {
            return Ok(WaitOutcome::Completed(status));
        }
        return Err(error);
    }
    let status = if let Ok(status) = tokio::time::timeout(TERMINATION_GRACE, child.wait()).await {
        status.context("activation_wait_failed", "wait after forwarded signal")?
    } else {
        if let Err(error) = signal_group(child_pid, "KILL") {
            if let Some(status) = child
                .try_wait()
                .context("activation_wait_failed", "check child after SIGKILL race")?
            {
                return if timed_out {
                    Ok(WaitOutcome::TimedOut(status))
                } else {
                    Ok(WaitOutcome::Completed(status))
                };
            }
            return Err(error);
        }
        child
            .wait()
            .await
            .context("activation_wait_failed", "wait after SIGKILL")?
    };
    if timed_out {
        Ok(WaitOutcome::TimedOut(status))
    } else {
        Ok(WaitOutcome::Completed(status))
    }
}

async fn recover_terminal_child(
    child: &mut Child,
    child_pid: u32,
) -> Result<std::process::ExitStatus> {
    if let Some(status) = child
        .try_wait()
        .context("activation_wait_failed", "inspect child during cleanup")?
    {
        return Ok(status);
    }
    let _ = signal_group(child_pid, "TERM");
    if let Ok(status) = tokio::time::timeout(TERMINATION_GRACE, child.wait()).await {
        return status.context("activation_wait_failed", "wait for child during cleanup");
    }
    if signal_group(child_pid, "KILL").is_err() {
        child.start_kill().context(
            "activation_signal_failed",
            "kill direct child during cleanup",
        )?;
    }
    tokio::time::timeout(TERMINATION_GRACE, child.wait())
        .await
        .map_err(|_| {
            Error::new(
                "activation_wait_failed",
                "direct child did not terminate after cleanup SIGKILL",
            )
        })?
        .context("activation_wait_failed", "wait after cleanup SIGKILL")
}

fn signal_group(child_pid: u32, signal_name: &str) -> Result<()> {
    let group = format!("-{child_pid}");
    let signal = format!("-{signal_name}");
    let output = StdCommand::new("/bin/kill")
        .args([signal.as_str(), group.as_str()])
        .output()
        .context(
            "activation_signal_failed",
            format!("send SIG{signal_name} to process group {child_pid}"),
        )?;
    if output.status.success() {
        return Ok(());
    }
    Err(Error::new(
        "activation_signal_failed",
        format!(
            "send SIG{signal_name} to process group {child_pid}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::fs::File;

    use tempfile::tempdir;

    use super::product_command;
    use crate::model::{Authority, LaunchImage, Manifest, Output, OverlapPolicy, Schedule};

    #[test]
    fn interpreted_image_places_script_before_literal_arguments() {
        let temporary = tempdir().expect("temporary directory");
        let manifest = Manifest {
            schema_version: 1,
            key: "annals/inbox".to_owned(),
            release_id: "a".repeat(64),
            release_root: temporary.path().display().to_string(),
            authority: Authority::CurrentUserBackground,
            overlap: OverlapPolicy::Skip,
            timeout_seconds: None,
            arguments: vec!["literal one".to_owned(), "$NOT_EXPANDED".to_owned()],
            cwd: temporary.path().display().to_string(),
            schedule: Schedule::Interval {
                seconds: 300,
                run_at_load: true,
            },
            launch: LaunchImage::Interpreted {
                interpreter: "/bin/sh".to_owned(),
                interpreter_sha256: "b".repeat(64),
                script: "/release/runner".to_owned(),
                script_sha256: "c".repeat(64),
            },
            environment: BTreeMap::from([("HOME".to_owned(), "/home/exact".to_owned())]),
            output: Output {
                stdout: "/product/stdout".to_owned(),
                stderr: "/product/stderr".to_owned(),
            },
        };

        let stdout = File::create(temporary.path().join("stdout")).expect("create test stdout");
        let stderr = File::create(temporary.path().join("stderr")).expect("create test stderr");
        let command = product_command(&manifest, stdout, stderr);
        assert_eq!(command.get_program(), OsStr::new("/bin/sh"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![
                OsStr::new("/release/runner"),
                OsStr::new("literal one"),
                OsStr::new("$NOT_EXPANDED"),
            ]
        );
        assert_eq!(command.get_current_dir(), Some(temporary.path()));
        assert_eq!(
            command
                .get_envs()
                .filter_map(|(name, value)| value.map(|value| (name, value)))
                .collect::<Vec<_>>(),
            vec![(OsStr::new("HOME"), OsStr::new("/home/exact"))]
        );
    }
}
