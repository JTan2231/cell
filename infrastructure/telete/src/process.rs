#[cfg(not(test))]
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::paths::{self, Paths};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CommandSpec {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<String>,
    pub(crate) cwd: PathBuf,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) timeout_seconds: Option<u64>,
    pub(crate) stdin: Option<String>,
    pub(crate) confined: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProcessResult {
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) timed_out: bool,
}
impl ProcessResult {
    pub(crate) fn success(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out
    }
}

const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

fn read_stream(stream: impl Read) -> Result<String> {
    let mut bytes = Vec::new();
    let mut stream = stream;
    let mut chunk = [0u8; 8192];
    let mut overflow = false;
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        let remaining = OUTPUT_LIMIT.saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..count.min(remaining)]);
        overflow |= count > remaining;
    }
    ensure!(!overflow, "process output exceeded 16 MiB");
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub(crate) fn run(paths: &Paths, spec: &CommandSpec) -> Result<ProcessResult> {
    ensure!(
        spec.timeout_seconds.is_none_or(|seconds| seconds > 0) && spec.cwd.is_absolute(),
        "invalid process timeout or working directory"
    );
    let mut command = if spec.confined {
        ensure!(
            PathBuf::from("/usr/bin/sandbox-exec").is_file(),
            "confined compiler execution requires macOS sandbox-exec"
        );
        let base = serde_json::to_string(&paths.workspace.display().to_string())?;
        let profile = format!(
            "(version 1) (allow default) (deny file-write*) (allow file-write* (subpath {base}) (literal \"/dev/null\") (literal \"/dev/tty\"))"
        );
        let mut cmd = Command::new("/usr/bin/sandbox-exec");
        cmd.args(["-p", &profile]).arg(&spec.program);
        cmd
    } else {
        Command::new(&spec.program)
    };
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("NEXTEST_") {
            command.env_remove(name);
        }
    }
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .envs(paths.environment())
        .envs(&spec.env)
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command
        .spawn()
        .with_context(|| format!("start {}", spec.program.display()))?;
    let out = child.stdout.take().context("stdout missing")?;
    let err = child.stderr.take().context("stderr missing")?;
    let stdout = thread::spawn(move || read_stream(out));
    let stderr = thread::spawn(move || read_stream(err));
    let input = if let Some(value) = spec.stdin.clone() {
        let mut writer = child.stdin.take().context("stdin missing")?;
        Some(thread::spawn(move || writer.write_all(value.as_bytes())))
    } else {
        None
    };
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if spec
            .timeout_seconds
            .is_some_and(|seconds| started.elapsed() >= Duration::from_secs(seconds))
        {
            timed_out = true;
            let pid = format!("-{}", child.id());
            let _ = Command::new("/bin/kill")
                .args(["-TERM", "--", &pid])
                .status();
            thread::sleep(Duration::from_millis(150));
            let _ = Command::new("/bin/kill")
                .args(["-KILL", "--", &pid])
                .status();
            let _ = child.kill();
            break child.wait()?;
        }
        thread::sleep(Duration::from_millis(25));
    };
    // Close descendants' pipes as well: a direct process exit is not proof that
    // a surviving child stopped. Gate commands may not leave background work.
    let pid = format!("-{}", child.id());
    let _ = Command::new("/bin/kill")
        .args(["-KILL", "--", &pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if let Some(input) = input {
        let _ = input.join();
    }
    Ok(ProcessResult {
        exit_code: status.code(),
        stdout: stdout
            .join()
            .map_err(|_| anyhow::anyhow!("stdout reader panicked"))??,
        stderr: stderr
            .join()
            .map_err(|_| anyhow::anyhow!("stderr reader panicked"))??,
        timed_out,
    })
}

#[derive(Serialize, Deserialize)]
struct Completion {
    schema: u32,
    request: CommandSpec,
    result: ProcessResult,
}

pub(crate) fn observe(directory: &std::path::Path) -> Result<Option<ProcessResult>> {
    let result = directory.join("result.json");
    if !result.exists() {
        return Ok(None);
    }
    let request: CommandSpec =
        serde_json::from_slice(&std::fs::read(directory.join("request.json"))?)?;
    let completion: Completion = serde_json::from_slice(&std::fs::read(result)?)?;
    ensure!(
        completion.schema == 1
            && serde_json::to_value(request)? == serde_json::to_value(completion.request)?,
        "uncorrelated process completion"
    );
    Ok(Some(completion.result))
}

pub(crate) fn supervise(
    paths: &Paths,
    spec: &CommandSpec,
    directory: &std::path::Path,
) -> Result<ProcessResult> {
    ensure!(
        directory.starts_with(&paths.root),
        "process records must stay in Telete state"
    );
    paths::ensure_private(directory)?;
    let request_path = directory.join("request.json");
    let _admission = paths::lock(&directory.join("admission.lock"), true)?;
    if request_path.exists() {
        let saved: CommandSpec = serde_json::from_slice(&std::fs::read(&request_path)?)?;
        ensure!(
            serde_json::to_value(saved)? == serde_json::to_value(spec)?,
            "process identity reused with another request"
        );
    } else {
        paths::atomic_json(&request_path, spec)?;
    }
    if let Some(result) = observe(directory)? {
        return Ok(result);
    }
    ensure!(
        !directory.join("intent.json").exists(),
        "process has admitted intent without completion; do not repeat it"
    );
    paths::atomic_json(
        &directory.join("intent.json"),
        &serde_json::json!({"schema":1,"owner_pid":std::process::id()}),
    )?;
    #[cfg(test)]
    {
        // Unit tests use the test harness rather than an installed CLI process.
        let result = run(paths, spec)?;
        paths::atomic_json(
            &directory.join("result.json"),
            &Completion {
                schema: 1,
                request: spec.clone(),
                result: result.clone(),
            },
        )?;
        Ok(result)
    }
    #[cfg(not(test))]
    {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("supervisor.log"))?;
        log.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--state")
            .arg(&paths.root)
            .args(["internal-execute", "--directory"])
            .arg(directory)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .envs(paths.environment())
            .process_group(0)
            .spawn()?;
        paths::atomic_json(
            &directory.join("supervisor.json"),
            &serde_json::json!({"schema":1,"pid":child.id()}),
        )?;
        let status = child.wait()?;
        observe(directory)?
            .with_context(|| format!("supervisor stopped without terminal evidence: {status}"))
    }
}

pub(crate) fn execute_request(paths: &Paths, directory: &std::path::Path) -> Result<()> {
    ensure!(
        directory.starts_with(&paths.root),
        "foreign supervisor directory"
    );
    paths::ensure_private(directory)?;
    ensure!(
        directory.join("intent.json").is_file(),
        "supervisor request was not admitted"
    );
    let _owner = paths::lock(&directory.join("runner.lock"), false)?;
    ensure!(
        !directory.join("started.json").exists(),
        "supervisor execution cannot be repeated"
    );
    let request: CommandSpec =
        serde_json::from_slice(&std::fs::read(directory.join("request.json"))?)?;
    paths::atomic_json(
        &directory.join("started.json"),
        &serde_json::json!({"schema":1,"pid":std::process::id()}),
    )?;
    let result = run(paths, &request)?;
    paths::atomic_json(
        &directory.join("result.json"),
        &Completion {
            schema: 1,
            request,
            result,
        },
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Paths, CommandSpec) {
        let directory = tempfile::tempdir().unwrap();
        let paths = Paths::for_test(directory.path().join("work")).unwrap();
        let request = CommandSpec {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "printf output; printf diagnostic >&2; exit 7".into(),
            ],
            cwd: paths.root.clone(),
            env: BTreeMap::new(),
            timeout_seconds: Some(5),
            stdin: None,
            confined: false,
        };
        (directory, paths, request)
    }

    #[test]
    fn retained_completion_requires_the_same_frozen_command() {
        let (_temporary, paths, request) = fixture();
        let directory = paths.root.join("execution");
        let result = supervise(&paths, &request, &directory).unwrap();
        assert_eq!(result.exit_code, Some(7));
        assert_eq!(result.stdout, "output");
        assert_eq!(result.stderr, "diagnostic");
        assert!(!result.success());
        assert_eq!(
            supervise(&paths, &request, &directory).unwrap().exit_code,
            Some(7)
        );
        let mut changed = request.clone();
        changed.args.clear();
        assert!(supervise(&paths, &changed, &directory).is_err());
        paths::atomic_json(&directory.join("request.json"), &changed).unwrap();
        assert!(observe(&directory).is_err());
    }

    #[test]
    fn admitted_execution_writes_a_receipt_and_cannot_run_again() {
        let (_temporary, paths, request) = fixture();
        let directory = paths.root.join("execution");
        paths::ensure_private(&directory).unwrap();
        paths::atomic_json(&directory.join("request.json"), &request).unwrap();
        assert!(execute_request(&paths, &directory).is_err());
        paths::atomic_json(
            &directory.join("intent.json"),
            &serde_json::json!({"schema":1}),
        )
        .unwrap();
        execute_request(&paths, &directory).unwrap();
        assert_eq!(observe(&directory).unwrap().unwrap().exit_code, Some(7));
        assert!(execute_request(&paths, &directory).is_err());
    }

    #[test]
    fn optional_deadline_preserves_legacy_requests_and_bounded_commands() {
        let (_temporary, paths, mut request) = fixture();
        let legacy = serde_json::to_value(&request).unwrap();
        assert_eq!(legacy["timeout_seconds"], 5);
        assert_eq!(
            serde_json::from_value::<CommandSpec>(legacy)
                .unwrap()
                .timeout_seconds,
            Some(5)
        );
        request.args = vec!["-c".into(), "sleep 2; printf complete".into()];
        request.timeout_seconds = Some(1);
        assert!(run(&paths, &request).unwrap().timed_out);
        request.timeout_seconds = None;
        let result = run(&paths, &request).unwrap();
        assert!(result.success());
        assert_eq!(result.stdout, "complete");
        request.timeout_seconds = Some(0);
        assert!(run(&paths, &request).is_err());
    }
}
