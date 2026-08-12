use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::{AppError, AppResult};

const BUNDLED_AGENT: &[u8] = include_bytes!("../bundles/codex/agent.sh");
const BUNDLED_SCHEMA: &[u8] = include_bytes!("../bundles/codex/generated-tree.schema.json");
const DEFAULT_TIMEOUT: Duration = Duration::from_mins(30);
const DEFAULT_MAX_STDOUT_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_STDERR_TAIL_BYTES: usize = 64 * 1024;

/// The deliberately small process boundary around the bundled Codex agent.
#[derive(Debug, Clone)]
pub(crate) struct Runner {
    program: RunnerProgram,
    timeout: Duration,
    max_stdout_bytes: usize,
    stderr_tail_bytes: usize,
}

#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
enum RunnerProgram {
    Embedded,
    Injected(PathBuf),
}

impl Default for Runner {
    fn default() -> Self {
        Self {
            program: RunnerProgram::Embedded,
            timeout: DEFAULT_TIMEOUT,
            max_stdout_bytes: DEFAULT_MAX_STDOUT_BYTES,
            stderr_tail_bytes: DEFAULT_STDERR_TAIL_BYTES,
        }
    }
}

impl Runner {
    /// Construct an injectable runner. Production ingestion uses [`Runner::default`].
    #[must_use]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(
        program: impl Into<PathBuf>,
        timeout: Duration,
        max_stdout_bytes: usize,
        stderr_tail_bytes: usize,
    ) -> Self {
        Self {
            program: RunnerProgram::Injected(program.into()),
            timeout,
            max_stdout_bytes,
            stderr_tail_bytes,
        }
    }

    /// Send one complete prompt to the agent and return its plain final response.
    /// When requested, child diagnostics are forwarded with terminal controls escaped.
    ///
    /// # Errors
    ///
    /// Returns an error when the runner cannot be started or communicated with, exceeds a
    /// configured limit, exits unsuccessfully, or returns empty or non-UTF-8 output.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn run(&self, prompt: &str, forward_stderr: bool) -> AppResult<String> {
        if self.max_stdout_bytes == 0 {
            return Err(AppError::unexpected(
                "model_runner_config",
                "model runner stdout limit must be greater than zero",
            ));
        }

        let prepared_program = PreparedProgram::create(&self.program)?;
        let program = prepared_program.path();
        let work_dir = TemporaryDirectory::create("annals-model-work").map_err(|error| {
            AppError::unexpected(
                "model_runner_workdir",
                format!("could not create model runner work directory: {error}"),
            )
        })?;

        let mut command = Command::new(program);
        command
            .current_dir(work_dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn().map_err(|error| {
            AppError::unexpected(
                "model_runner_spawn",
                format!(
                    "could not start model runner {}: {error}",
                    program.display()
                ),
            )
        })?;
        let process_group_id = child.id();

        let pipes = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (Some(stdin), Some(stdout), Some(stderr)) = pipes else {
            terminate_process_group(&mut child, process_group_id);
            let _ = child.wait();
            return Err(AppError::unexpected(
                "model_runner_pipe",
                "model runner did not provide all configured standard I/O pipes",
            ));
        };
        let stdout_overflow = Arc::new(AtomicBool::new(false));

        let prompt = prompt.as_bytes().to_vec();
        let stdin_thread = thread::spawn(move || write_prompt(stdin, &prompt));

        let overflow = Arc::clone(&stdout_overflow);
        let stdout_limit = self.max_stdout_bytes;
        let stdout_thread = thread::spawn(move || read_stdout(stdout, stdout_limit, overflow));

        let stderr_limit = self.stderr_tail_bytes;
        let stderr_thread =
            thread::spawn(move || read_stderr(stderr, stderr_limit, forward_stderr));

        let deadline = Instant::now() + self.timeout;
        let mut timed_out = false;
        let mut killed_for_output = false;
        let status = loop {
            if stdout_overflow.load(Ordering::Relaxed) {
                killed_for_output = true;
                terminate_process_group(&mut child, process_group_id);
                break child.wait();
            }

            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if Instant::now() >= deadline => {
                    timed_out = true;
                    terminate_process_group(&mut child, process_group_id);
                    break child.wait();
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    terminate_process_group(&mut child, process_group_id);
                    let _ = child.wait();
                    break Err(error);
                }
            }
        };
        // A process that exits after starting a pipe-inheriting descendant must not make the
        // reader threads outlive this runner call.
        signal_process_group(process_group_id);

        let stdin_result = join_stdin_thread(stdin_thread)?;
        let stdout_result = join_io_thread(stdout_thread, "stdout")?;
        let stderr_tail = join_io_thread(stderr_thread, "stderr")?;

        if timed_out {
            return Err(AppError::unexpected(
                "model_runner_timeout",
                format!(
                    "model runner exceeded its {} second timeout{}",
                    self.timeout.as_secs_f64(),
                    diagnostic_suffix(&stderr_tail)
                ),
            ));
        }
        if killed_for_output || stdout_result.overflowed {
            return Err(AppError::unexpected(
                "model_runner_output_too_large",
                format!(
                    "model runner stdout exceeded the {} byte limit{}",
                    self.max_stdout_bytes,
                    diagnostic_suffix(&stderr_tail)
                ),
            ));
        }

        let status = status.map_err(|error| {
            AppError::unexpected(
                "model_runner_wait",
                format!("could not wait for model runner: {error}"),
            )
        })?;
        if !status.success() {
            return Err(AppError::unexpected(
                "model_runner_failed",
                format!(
                    "model runner exited with {status}{}",
                    diagnostic_suffix(&stderr_tail)
                ),
            ));
        }
        stdin_result.map_err(|error| {
            AppError::unexpected(
                "model_runner_stdin",
                format!("could not write the complete model prompt: {error}"),
            )
        })?;
        let stdout = stdout_result.bytes;
        if stdout.is_empty() || stdout.iter().all(u8::is_ascii_whitespace) {
            return Err(AppError::unexpected(
                "model_runner_empty_output",
                format!(
                    "model runner returned no output{}",
                    diagnostic_suffix(&stderr_tail)
                ),
            ));
        }

        String::from_utf8(stdout).map_err(|error| {
            AppError::unexpected(
                "model_runner_invalid_utf8",
                format!("model runner returned invalid UTF-8: {error}"),
            )
        })
    }
}

fn terminate_process_group(child: &mut Child, process_group_id: u32) {
    if !signal_process_group(process_group_id) {
        let _ = child.kill();
    }
}

fn signal_process_group(process_group_id: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{process_group_id}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[derive(Debug)]
struct PreparedProgram {
    path: PathBuf,
    _bundle: Option<TemporaryDirectory>,
}

impl PreparedProgram {
    fn create(program: &RunnerProgram) -> AppResult<Self> {
        match program {
            RunnerProgram::Embedded => Self::materialize_bundle(),
            RunnerProgram::Injected(path) => {
                let canonical = fs::canonicalize(path).map_err(|error| {
                    AppError::unexpected(
                        "model_runner_unavailable",
                        format!("could not resolve model runner {}: {error}", path.display()),
                    )
                })?;
                Ok(Self {
                    path: canonical,
                    _bundle: None,
                })
            }
        }
    }

    fn materialize_bundle() -> AppResult<Self> {
        let bundle = TemporaryDirectory::create("annals-codex-bundle").map_err(|error| {
            AppError::unexpected(
                "model_runner_bundle",
                format!("could not create embedded Codex bundle: {error}"),
            )
        })?;
        let path = bundle.path().join("agent.sh");
        let schema_path = bundle.path().join("generated-tree.schema.json");
        fs::write(&path, BUNDLED_AGENT)
            .and_then(|()| fs::set_permissions(&path, fs::Permissions::from_mode(0o700)))
            .and_then(|()| fs::write(schema_path, BUNDLED_SCHEMA))
            .map_err(|error| {
                AppError::unexpected(
                    "model_runner_bundle",
                    format!("could not materialize embedded Codex bundle: {error}"),
                )
            })?;
        let path = fs::canonicalize(path).map_err(|error| {
            AppError::unexpected(
                "model_runner_bundle",
                format!("could not resolve embedded Codex agent: {error}"),
            )
        })?;

        Ok(Self {
            path,
            _bundle: Some(bundle),
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug)]
struct StdoutRead {
    bytes: Vec<u8>,
    overflowed: bool,
}

fn write_prompt(mut stdin: impl Write, prompt: &[u8]) -> io::Result<()> {
    stdin.write_all(prompt)
}

fn read_stdout(
    mut stdout: impl Read,
    limit: usize,
    overflow: Arc<AtomicBool>,
) -> io::Result<StdoutRead> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0_u8; 8192];
    let mut overflowed = false;

    loop {
        let count = stdout.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(bytes.len());
        bytes.extend_from_slice(&buffer[..count.min(remaining)]);
        if count > remaining {
            overflowed = true;
            overflow.store(true, Ordering::Relaxed);
        }
    }

    drop(overflow);
    Ok(StdoutRead { bytes, overflowed })
}

fn read_stderr(mut stderr: impl Read, tail_limit: usize, forward: bool) -> io::Result<Vec<u8>> {
    let mut tail = Vec::with_capacity(tail_limit.min(64 * 1024));
    let mut buffer = [0_u8; 4096];
    let process_stderr = io::stderr();
    let mut forwarded = forward.then(|| process_stderr.lock());

    loop {
        let count = stderr.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let chunk = &buffer[..count];
        if let Some(output) = &mut forwarded {
            let _ = write_terminal_safe(output, chunk);
        }
        retain_tail(&mut tail, chunk, tail_limit);
    }
    if let Some(output) = &mut forwarded {
        let _ = output.flush();
    }
    Ok(tail)
}

fn write_terminal_safe(mut output: impl Write, chunk: &[u8]) -> io::Result<()> {
    for byte in chunk {
        match byte {
            b'\n' | 0x20..=0x7e | 0x80..=0xff => output.write_all(&[*byte])?,
            _ => write!(output, "\\x{byte:02x}")?,
        }
    }
    Ok(())
}

fn retain_tail(tail: &mut Vec<u8>, chunk: &[u8], limit: usize) {
    if limit == 0 {
        return;
    }
    if chunk.len() >= limit {
        tail.clear();
        tail.extend_from_slice(&chunk[chunk.len() - limit..]);
        return;
    }
    let overflow = tail.len().saturating_add(chunk.len()).saturating_sub(limit);
    if overflow > 0 {
        tail.drain(..overflow);
    }
    tail.extend_from_slice(chunk);
}

fn diagnostic_suffix(stderr_tail: &[u8]) -> String {
    let diagnostic = String::from_utf8_lossy(stderr_tail);
    let diagnostic = diagnostic.trim();
    if diagnostic.is_empty() {
        String::new()
    } else {
        let diagnostic = diagnostic
            .chars()
            .flat_map(char::escape_default)
            .collect::<String>();
        format!("; stderr: {diagnostic}")
    }
}

fn join_io_thread<T>(
    handle: thread::JoinHandle<io::Result<T>>,
    stream: &'static str,
) -> AppResult<T> {
    handle
        .join()
        .map_err(|_| {
            AppError::unexpected(
                "model_runner_thread",
                format!("model runner {stream} worker panicked"),
            )
        })?
        .map_err(|error| {
            AppError::unexpected(
                "model_runner_io",
                format!("model runner {stream} failed: {error}"),
            )
        })
}

fn join_stdin_thread(handle: thread::JoinHandle<io::Result<()>>) -> AppResult<io::Result<()>> {
    handle.join().map_err(|_| {
        AppError::unexpected("model_runner_thread", "model runner stdin worker panicked")
    })
}

#[derive(Debug)]
struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn create(prefix: &str) -> io::Result<Self> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let base = std::env::temp_dir();

        for _ in 0..100 {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("{prefix}-{}-{stamp}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique temporary directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fs;
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use super::{
        BUNDLED_AGENT, BUNDLED_SCHEMA, PreparedProgram, Runner, RunnerProgram, write_terminal_safe,
    };
    use crate::error::AppError;

    struct Script {
        dir: PathBuf,
        path: PathBuf,
    }

    impl Script {
        fn new(body: &str) -> io::Result<Self> {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let dir = loop {
                let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                let candidate = std::env::temp_dir().join(format!(
                    "annals-model-runner-test-{}-{id}",
                    std::process::id()
                ));
                match fs::create_dir(&candidate) {
                    Ok(()) => break candidate,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
            };
            let path = dir.join("agent.sh");
            fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n"))?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
            Ok(Self { dir, path })
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for Script {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_dir(&self.dir);
        }
    }

    fn runner(script: &Script) -> Runner {
        Runner::new(script.path(), Duration::from_secs(5), 1024, 1024)
    }

    fn expected_error(runner: &Runner) -> Result<AppError, Box<dyn Error>> {
        match runner.run("prompt", false) {
            Ok(output) => Err(format!("expected runner failure, got {output:?}").into()),
            Err(error) => Ok(error),
        }
    }

    #[test]
    fn embedded_bundle_is_executable_and_pins_codex() -> Result<(), Box<dyn Error>> {
        let prepared = PreparedProgram::create(&RunnerProgram::Embedded)?;
        let bundle_dir = prepared
            .path()
            .parent()
            .ok_or_else(|| io::Error::other("materialized agent has no parent"))?;
        let script_bytes = fs::read(prepared.path())?;
        let schema_bytes = fs::read(bundle_dir.join("generated-tree.schema.json"))?;
        let mode = fs::metadata(prepared.path())?.permissions().mode();
        assert_eq!(script_bytes, BUNDLED_AGENT);
        assert_eq!(schema_bytes, BUNDLED_SCHEMA);
        let _: serde_json::Value = serde_json::from_slice(BUNDLED_SCHEMA)?;
        assert_ne!(mode & 0o100, 0);

        let script = std::str::from_utf8(BUNDLED_AGENT)?;
        for required in [
            "codex exec",
            "--ephemeral",
            "--ignore-user-config",
            "--ignore-rules",
            "--disable shell_tool",
            "--disable unified_exec",
            "--skip-git-repo-check",
            "--sandbox read-only",
            "--color never",
            "--model gpt-5.6-terra",
            "model_reasoning_effort=\"medium\"",
            "--output-schema",
            "generated-tree.schema.json",
        ] {
            assert!(
                script.contains(required),
                "missing invocation pin: {required}"
            );
        }
        assert!(!script.contains("--json"));
        assert!(script.trim_end().ends_with('-'));
        assert!(matches!(Runner::default().program, RunnerProgram::Embedded));
        Ok(())
    }

    #[test]
    fn sends_exact_prompt_from_an_empty_working_directory() -> Result<(), Box<dyn Error>> {
        let script = Script::new(
            r#"
[ -z "$(ls -A)" ] || { printf 'working directory was not empty' >&2; exit 24; }
cat >actual
printf 'the exact prompt\nwith final newline\n' >expected
cmp actual expected || { printf 'prompt bytes differed' >&2; exit 23; }
printf '{"schema_version":1,"nodes":[]}'
"#,
        )?;

        let output = runner(&script).run("the exact prompt\nwith final newline\n", false)?;

        assert_eq!(output, r#"{"schema_version":1,"nodes":[]}"#);
        Ok(())
    }

    #[test]
    fn drains_and_forwards_stderr_without_polluting_stdout() -> Result<(), Box<dyn Error>> {
        let script = Script::new(
            r"
printf 'visible progress\n' >&2
printf '{}'
",
        )?;

        assert_eq!(runner(&script).run("prompt", true)?, "{}");
        Ok(())
    }

    #[test]
    fn reports_nonzero_exit_with_stderr_tail() -> Result<(), Box<dyn Error>> {
        let script = Script::new("printf 'useful failure' >&2\nexit 7")?;

        let error = expected_error(&runner(&script))?;

        assert_eq!(error.code(), "model_runner_failed");
        assert!(error.to_string().contains("useful failure"));
        Ok(())
    }

    #[test]
    fn escapes_control_characters_in_retained_diagnostics() -> Result<(), Box<dyn Error>> {
        let script = Script::new("printf '\\033[31mfailure' >&2\nexit 7")?;

        let error = expected_error(&runner(&script))?;
        let message = error.to_string();

        assert!(!message.contains('\u{1b}'));
        assert!(message.contains(r"\u{1b}[31mfailure"));
        Ok(())
    }

    #[test]
    fn escapes_terminal_controls_while_forwarding_progress() -> Result<(), Box<dyn Error>> {
        let mut output = Vec::new();

        write_terminal_safe(&mut output, b"progress\t\x1b[31mred\n")?;

        assert_eq!(output, b"progress\\x09\\x1b[31mred\n");
        Ok(())
    }

    #[test]
    fn rejects_invalid_utf8_output() -> Result<(), Box<dyn Error>> {
        let script = Script::new("printf '\\377'")?;

        let error = expected_error(&runner(&script))?;

        assert_eq!(error.code(), "model_runner_invalid_utf8");
        Ok(())
    }

    #[test]
    fn kills_a_timed_out_runner() -> Result<(), Box<dyn Error>> {
        let script = Script::new("sleep 5 &\nwait")?;
        let runner = Runner::new(script.path(), Duration::from_millis(30), 1024, 1024);
        let started = std::time::Instant::now();

        let error = expected_error(&runner)?;

        assert_eq!(error.code(), "model_runner_timeout");
        assert!(started.elapsed() < Duration::from_secs(1));
        Ok(())
    }

    #[test]
    fn closes_pipes_left_open_by_a_descendant_after_the_runner_exits() -> Result<(), Box<dyn Error>>
    {
        let script = Script::new("sleep 5 &\nprintf '{}'")?;
        let started = std::time::Instant::now();

        let output = runner(&script).run("prompt", false)?;

        assert_eq!(output, "{}");
        assert!(started.elapsed() < Duration::from_secs(1));
        Ok(())
    }

    #[test]
    fn kills_output_that_exceeds_the_limit() -> Result<(), Box<dyn Error>> {
        let script = Script::new("printf '%02048d' 0")?;
        let runner = Runner::new(script.path(), Duration::from_secs(1), 64, 1024);

        let error = expected_error(&runner)?;

        assert_eq!(error.code(), "model_runner_output_too_large");
        Ok(())
    }

    #[test]
    fn rejects_empty_output() -> Result<(), Box<dyn Error>> {
        let script = Script::new("printf '   '")?;

        let error = expected_error(&runner(&script))?;

        assert_eq!(error.code(), "model_runner_empty_output");
        Ok(())
    }
}
