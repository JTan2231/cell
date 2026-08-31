use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, FileTimes, OpenOptions};
use std::io;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use tempfile::TempDir;

use nucleus_daemon::{ServeConfig, serve};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const LEGACY_QUEUE_WITH_SETTLING_ENTRY: &[u8] = br#"{
  "version": 1,
  "next_sequence": 42,
  "entries": {
    "c2V0dGxpbmcudHh0": 41
  }
}"#;

const LEGACY_PROCESSING_RECEIPT: &[u8] = br#"{
  "version": 1,
  "id": "j00000000000000000007",
  "original_name": "legacy.txt",
  "original_name_base64": "bGVnYWN5LnR4dA",
  "state": "processing",
  "attempts": 1,
  "created_at": "2026-08-20T19:00:00Z",
  "started_at": "2026-08-20T19:01:00Z",
  "completed_at": null,
  "source_sha256": null,
  "work": null,
  "reconciliation_id": null,
  "model_run_token": null,
  "result_status": null,
  "result_revision": null,
  "last_error": {
    "code": "model_failed",
    "message": "predecessor retry"
  }
}"#;
const BLOCKING_MINIMUM_AVAILABLE_BYTES: u64 = 9_000_000_000_000_000_000;

struct Installation {
    directory: TempDir,
    config: PathBuf,
    library: PathBuf,
    inbox: PathBuf,
    counter: PathBuf,
    codex: PathBuf,
    controls: PathBuf,
}

struct InstallationCommand {
    inner: Command,
    controls: PathBuf,
}

#[derive(Debug)]
struct Material {
    bytes: Vec<u8>,
    inode: u64,
    mode: u32,
    modified: SystemTime,
}

impl Installation {
    fn new(settle_seconds: u64) -> TestResult<Self> {
        Self::new_with_minimum_available_bytes(settle_seconds, 0)
    }

    fn new_with_minimum_available_bytes(
        settle_seconds: u64,
        minimum_available_bytes: u64,
    ) -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let config = directory.path().join("annals.toml");
        let library = directory.path().join("state/annals.db");
        let inbox = directory.path().join("spool");
        let counter = directory.path().join("fake-codex-counter");
        let codex = directory.path().join("fake-codex");
        let socket = directory.path().join("nucleus.sock");
        let controls = directory.path().join("fake-controls");
        fs::create_dir_all(library.parent().ok_or("library had no parent directory")?)?;
        fs::create_dir(&controls)?;
        write_fake_codex(&codex, &counter, &controls)?;
        start_fake_nucleus(directory.path(), &socket, &codex)?;
        fs::write(
            &config,
            format!(
                concat!(
                    "library = {}\n",
                    "\n",
                    "[inbox]\n",
                    "root = {}\n",
                    "settle_seconds = {settle_seconds}\n",
                    "minimum_available_bytes = {minimum_available_bytes}\n",
                    "\n",
                    "[liaison]\n",
                    "quality = \"medium\"\n",
                    "model = \"fake-model\"\n",
                    "nucleus_socket = {}\n",
                ),
                toml_string(&library),
                toml_string(&inbox),
                toml_string(&socket),
                settle_seconds = settle_seconds,
                minimum_available_bytes = minimum_available_bytes,
            ),
        )?;
        Ok(Self {
            directory,
            config,
            library,
            inbox,
            counter,
            codex,
            controls,
        })
    }

    fn set_minimum_available_bytes(&self, value: u64) -> TestResult {
        let current = fs::read_to_string(&self.config)?;
        let line = current
            .lines()
            .find(|line| line.starts_with("minimum_available_bytes = "))
            .ok_or("config had no minimum_available_bytes setting")?;
        let updated = current.replacen(line, &format!("minimum_available_bytes = {value}"), 1);
        fs::write(&self.config, updated)?;
        Ok(())
    }

    fn command(&self) -> InstallationCommand {
        reset_controls(&self.controls);
        let mut command = Command::new(env!("CARGO_BIN_EXE_annals"));
        command
            .arg("--config")
            .arg(&self.config)
            .arg("--json")
            .env_remove("ANNALS_LIBRARY")
            .env("ANNALS_FAKE_COUNTER", &self.counter)
            .env(
                "CODEX_HOME",
                self.directory.path().join("source-codex-home"),
            )
            .current_dir(self.directory.path());
        InstallationCommand {
            inner: command,
            controls: self.controls.clone(),
        }
    }

    fn json_ok<I, S>(&self, arguments: I) -> TestResult<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        successful_json(&self.command().args(arguments).output()?)
    }

    fn init(&self) -> TestResult {
        self.json_ok(["init"])?;
        Ok(())
    }

    fn incoming(&self, name: &str, bytes: &[u8], mode: u32) -> TestResult<Material> {
        let incoming = self.inbox.join("incoming");
        fs::create_dir_all(&incoming)?;
        let path = incoming.join(name);
        fs::write(&path, bytes)?;
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_mode(mode);
        fs::set_permissions(&path, permissions)?;
        material(&path)
    }

    fn interrupt_output(
        &self,
        job_id: &str,
        disposition: &str,
        reason: Option<&str>,
    ) -> io::Result<Output> {
        let mut command = self.command();
        command
            .arg("inbox")
            .arg("interrupt")
            .arg(job_id)
            .args(["--as", disposition]);
        if let Some(reason) = reason {
            command.args(["--reason", reason]);
        }
        command.output()
    }
}

impl InstallationCommand {
    fn arg(&mut self, argument: impl AsRef<OsStr>) -> &mut Self {
        self.inner.arg(argument);
        self
    }

    fn args<I, S>(&mut self, arguments: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.inner.args(arguments);
        self
    }

    fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        let key = key.as_ref();
        let value = value.as_ref();
        self.inner.env(key, value);
        if let Some(name) = fake_control_name(key)
            && let Err(error) = fs::write(self.controls.join(name), value.as_encoded_bytes())
        {
            panic!("write fake Nucleus control {name}: {error}");
        }
        self
    }

    fn stdout(&mut self, configuration: Stdio) -> &mut Self {
        self.inner.stdout(configuration);
        self
    }

    fn stderr(&mut self, configuration: Stdio) -> &mut Self {
        self.inner.stderr(configuration);
        self
    }

    fn spawn(&mut self) -> io::Result<Child> {
        self.inner.spawn()
    }

    fn output(&mut self) -> io::Result<Output> {
        self.inner.output()
    }
}

fn fake_control_name(key: &OsStr) -> Option<&'static str> {
    match key.to_str()? {
        "ANNALS_FAKE_AUTH_FAIL" => Some("auth-fail"),
        "ANNALS_FAKE_FAIL_FIRST" => Some("fail-first"),
        "ANNALS_FAKE_BLOCK_READY" => Some("block-ready"),
        "ANNALS_FAKE_BLOCK_RELEASE" => Some("block-release"),
        "ANNALS_FAKE_AFTER_SUBMIT_READY" => Some("after-submit-ready"),
        "ANNALS_FAKE_AFTER_SUBMIT_RELEASE" => Some("after-submit-release"),
        _ => None,
    }
}

fn reset_controls(controls: &Path) {
    if let Ok(entries) = fs::read_dir(controls) {
        for entry in entries.flatten() {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn toml_string(path: &Path) -> String {
    format!(
        "\"{}\"",
        path.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

fn successful_json(output: &Output) -> TestResult<Value> {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let envelope = serde_json::from_slice::<Value>(&output.stdout)?;
    assert_eq!(envelope["ok"], true);
    Ok(envelope["data"].clone())
}

fn unsuccessful_json(output: &Output) -> TestResult<Value> {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    assert!(output.stdout.is_empty());
    let envelope = serde_json::from_slice::<Value>(&output.stderr)?;
    assert_eq!(envelope["ok"], false);
    Ok(envelope)
}

fn failed_json(output: &Output, code: &str) -> TestResult<Value> {
    let envelope = unsuccessful_json(output)?;
    assert_eq!(envelope["error"]["code"], code);
    Ok(envelope)
}

fn remove_retry_provenance(receipt: &mut Value) -> TestResult {
    let object = receipt
        .as_object_mut()
        .ok_or("job receipt was not an object")?;
    for field in [
        "retry_event_id",
        "retry_ordinal",
        "retry_of_job_id",
        "retry_of_ingestion_id",
        "retry_reconciliation_id",
    ] {
        object.remove(field);
    }
    Ok(())
}

fn conflict_json(output: &Output) -> TestResult<Value> {
    assert_eq!(
        output.status.code(),
        Some(4),
        "command did not report a conflict: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    unsuccessful_json(output)
}

fn material(path: &Path) -> TestResult<Material> {
    let metadata = fs::metadata(path)?;
    Ok(Material {
        bytes: fs::read(path)?,
        inode: metadata.ino(),
        mode: metadata.permissions().mode() & 0o777,
        modified: metadata.modified()?,
    })
}

fn set_modified(path: &Path, modified: SystemTime) -> TestResult {
    let file = OpenOptions::new().read(true).open(path)?;
    file.set_times(FileTimes::new().set_modified(modified))?;
    Ok(())
}

fn wait_for_file(path: &Path) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.is_file() {
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for {}", path.display()).into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn wait_for_child_output(mut child: Child, release_on_timeout: &Path) -> TestResult<Output> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }
        if Instant::now() >= deadline {
            let _ = fs::write(release_on_timeout, b"release after test timeout\n");
            let _ = child.kill();
            let _ = child.wait();
            return Err("timed out waiting for interrupted inbox worker".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn incoming_names(root: &Path) -> TestResult<Vec<String>> {
    let path = root.join("incoming");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut names = fs::read_dir(path)?
        .map(|entry| {
            let entry = entry?;
            Ok(entry.file_name().to_string_lossy().into_owned())
        })
        .collect::<io::Result<Vec<_>>>()?;
    names.sort();
    Ok(names)
}

fn archived_material(root: &Path, state: &str) -> TestResult<BTreeMap<String, Material>> {
    let path = root.join(state);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let mut archived = BTreeMap::new();
    for envelope in fs::read_dir(path)? {
        let envelope = envelope?;
        assert!(envelope.file_type()?.is_dir());
        let envelope = envelope.path();
        assert!(envelope.join("job.json").is_file());
        let material_directory = envelope.join("material");
        assert!(material_directory.is_dir());
        let mut material_paths = fs::read_dir(&material_directory)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()?;
        assert_eq!(
            material_paths.len(),
            1,
            "job envelope should contain exactly one source file: {}",
            envelope.display()
        );
        let material_path = material_paths.pop().ok_or("job envelope had no source")?;
        let name = material_path
            .file_name()
            .ok_or("archived source had no filename")?
            .to_string_lossy()
            .into_owned();
        assert!(archived.insert(name, material(&material_path)?).is_none());
    }
    Ok(archived)
}

fn only_archived_receipt(root: &Path, state: &str) -> TestResult<Value> {
    let mut envelopes = fs::read_dir(root.join(state))?.collect::<io::Result<Vec<_>>>()?;
    assert_eq!(envelopes.len(), 1, "expected one {state} inbox job");
    let envelope = envelopes.pop().ok_or("inbox archive was empty")?;
    Ok(serde_json::from_slice(&fs::read(
        envelope.path().join("job.json"),
    )?)?)
}

fn archived_receipt(root: &Path, state: &str, job_id: &str) -> TestResult<Value> {
    Ok(serde_json::from_slice(&fs::read(
        root.join(state).join(job_id).join("job.json"),
    )?)?)
}

fn fail_next_inbox_job(installation: &Installation) -> TestResult {
    fs::write(&installation.counter, b"0\n")?;
    let output = installation
        .command()
        .args(["inbox", "run"])
        .env("ANNALS_FAKE_FAIL_FIRST", "1")
        .output()?;
    failed_json(&output, "inbox_job_failed")?;
    Ok(())
}

fn assert_unchanged(expected: &Material, actual: &Material) {
    assert_eq!(actual.bytes, expected.bytes);
    assert_eq!(actual.inode, expected.inode);
    assert_eq!(actual.mode, expected.mode);
    assert_eq!(actual.modified, expected.modified);
}

fn return_archives_to_processing(root: &Path, state: &str) -> TestResult {
    let processing = root.join("processing");
    fs::create_dir_all(&processing)?;
    for envelope in fs::read_dir(root.join(state))? {
        let envelope = envelope?;
        assert!(envelope.file_type()?.is_dir());
        fs::rename(envelope.path(), processing.join(envelope.file_name()))?;
    }
    Ok(())
}

fn status_count(value: &Value, state: &str) -> Option<u64> {
    match value {
        Value::Object(object) => {
            for key in [state.to_owned(), format!("{state}_count")] {
                if let Some(candidate) = object.get(&key) {
                    if let Some(count) = candidate.as_u64() {
                        return Some(count);
                    }
                    if let Some(count) = candidate.get("count").and_then(Value::as_u64) {
                        return Some(count);
                    }
                }
            }
            object.values().find_map(|child| status_count(child, state))
        }
        Value::Array(array) => array.iter().find_map(|child| status_count(child, state)),
        _ => None,
    }
}

fn registered_job_id(summary: &Value, source_name: &str) -> TestResult<String> {
    summary["jobs"]
        .as_array()
        .and_then(|jobs| jobs.iter().find(|job| job["source_name"] == source_name))
        .and_then(|job| job["id"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("registered job for {source_name} was absent").into())
}

fn assert_work_summary(installation: &Installation, work: &str, expected: &str) -> TestResult {
    let shown = installation.json_ok(["change", "show", "--work", work])?;
    assert_eq!(shown["reconciliation"]["summary"], expected);
    Ok(())
}

#[test]
fn configuration_selects_paths_and_library_overrides_follow_precedence() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    assert!(installation.library.is_file());

    let environment_library = installation.directory.path().join("environment.db");
    let output = installation
        .command()
        .env("ANNALS_LIBRARY", &environment_library)
        .arg("init")
        .output()?;
    successful_json(&output)?;
    assert!(environment_library.is_file());

    let explicit_library = installation.directory.path().join("explicit.db");
    let output = installation
        .command()
        .env("ANNALS_LIBRARY", &environment_library)
        .args([OsStr::new("--library"), explicit_library.as_os_str()])
        .arg("init")
        .output()?;
    successful_json(&output)?;
    assert!(explicit_library.is_file());

    fs::create_dir_all(installation.inbox.join("incoming"))?;
    fs::write(installation.inbox.join("incoming/queued.txt"), "queued")?;
    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status_count(&status, "incoming"), Some(1), "{status}");
    Ok(())
}

#[test]
fn run_honors_settling_and_drains_more_than_five_sources_unchanged() -> TestResult {
    let installation = Installation::new(3_600)?;
    installation.init()?;
    let mut expected = BTreeMap::new();
    for (name, bytes, mode) in [
        (
            "01-one.md",
            b"Shared inbox claim.\nFirst source.\n".as_slice(),
            0o640,
        ),
        (
            "02-two.txt",
            b"Shared inbox claim.\nSecond source.\n".as_slice(),
            0o600,
        ),
        (
            "03-three.md",
            b"Shared inbox claim.\nThird source.\n".as_slice(),
            0o644,
        ),
        (
            "04-four.txt",
            b"Shared inbox claim.\nFourth source.\n".as_slice(),
            0o640,
        ),
        (
            "05-five.md",
            b"Shared inbox claim.\nFifth source.\n".as_slice(),
            0o600,
        ),
        (
            "06-six.txt",
            b"Shared inbox claim.\nSixth source.\n".as_slice(),
            0o644,
        ),
        (
            "job.json",
            b"Shared inbox claim.\nSeventh source.\n".as_slice(),
            0o640,
        ),
    ] {
        expected.insert(name.to_owned(), installation.incoming(name, bytes, mode)?);
    }

    installation.json_ok(["inbox", "run"])?;
    assert!(!installation.counter.exists());
    assert_eq!(
        incoming_names(&installation.inbox)?,
        [
            "01-one.md",
            "02-two.txt",
            "03-three.md",
            "04-four.txt",
            "05-five.md",
            "06-six.txt",
            "job.json",
        ]
    );
    assert!(archived_material(&installation.inbox, "done")?.is_empty());

    let run = installation.json_ok(["inbox", "run", "--settle-seconds", "0"])?;
    assert_eq!(run["attempted"], 7);
    assert_eq!(run["applied"], 7);
    assert!(incoming_names(&installation.inbox)?.is_empty());
    let done = archived_material(&installation.inbox, "done")?;
    assert_eq!(done.len(), 7);
    for (name, actual) in &done {
        assert_unchanged(
            expected.get(name).ok_or("unexpected archived source")?,
            actual,
        );
    }
    assert!(archived_material(&installation.inbox, "processing")?.is_empty());
    assert!(archived_material(&installation.inbox, "failed")?.is_empty());
    assert_eq!(fs::read_to_string(&installation.counter)?, "7\n");

    let library_stats = installation.json_ok(["stats"])?;
    assert_eq!(library_stats["work_count"], 7);
    assert_eq!(library_stats["revision"], 7);
    Ok(())
}

#[test]
fn run_rescans_for_ready_arrivals_and_leaves_settling_arrivals_for_later() -> TestResult {
    let installation = Installation::new(60)?;
    installation.init()?;
    let old = SystemTime::now()
        .checked_sub(Duration::from_mins(2))
        .ok_or("could not construct an old modification time")?;

    installation.incoming(
        "01-first.md",
        b"Shared inbox claim.\nFirst source.\n",
        0o640,
    )?;
    set_modified(&installation.inbox.join("incoming/01-first.md"), old)?;

    installation.incoming(
        "02-already-queued.md",
        b"Shared inbox claim.\nAlready queued source.\n",
        0o600,
    )?;
    set_modified(
        &installation.inbox.join("incoming/02-already-queued.md"),
        old,
    )?;

    installation.incoming(
        "03-arrived.md",
        b"Shared inbox claim.\nReady arrival.\n",
        0o640,
    )?;
    let staged = installation.directory.path().join("03-arrived.md");
    fs::rename(installation.inbox.join("incoming/03-arrived.md"), &staged)?;
    set_modified(&staged, old)?;

    let ready = installation.directory.path().join("fake-codex-ready");
    let release = installation.directory.path().join("fake-codex-release");
    let mut command = installation.command();
    let child = command
        .args(["inbox", "run"])
        .env("ANNALS_FAKE_BLOCK_READY", &ready)
        .env("ANNALS_FAKE_BLOCK_RELEASE", &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Err(error) = wait_for_file(&ready) {
        fs::write(&release, b"release\n")?;
        let _ = child.wait_with_output();
        return Err(error);
    }
    assert!(incoming_names(&installation.inbox)?.is_empty());
    assert_eq!(
        archived_material(&installation.inbox, "processing")?.len(),
        1
    );
    assert_eq!(archived_material(&installation.inbox, "queued")?.len(), 1);
    fs::rename(&staged, installation.inbox.join("incoming/03-arrived.md"))?;
    installation.incoming(
        "04-settling.md",
        b"Shared inbox claim.\nStill settling.\n",
        0o644,
    )?;
    fs::write(&release, b"release\n")?;

    let first = successful_json(&child.wait_with_output()?)?;
    assert_eq!(first["attempted"], 3);
    assert_eq!(first["applied"], 3);
    assert_eq!(incoming_names(&installation.inbox)?, ["04-settling.md"]);
    assert_eq!(archived_material(&installation.inbox, "done")?.len(), 3);
    assert!(archived_material(&installation.inbox, "processing")?.is_empty());
    assert_eq!(fs::read_to_string(&installation.counter)?, "3\n");

    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status_count(&status, "incoming"), Some(1), "{status}");
    assert_eq!(status_count(&status, "ready"), Some(0), "{status}");
    assert_eq!(status_count(&status, "settling"), Some(1), "{status}");

    let second = installation.json_ok(["inbox", "run", "--settle-seconds", "0"])?;
    assert_eq!(second["attempted"], 1);
    assert_eq!(second["applied"], 1);
    assert!(incoming_names(&installation.inbox)?.is_empty());
    assert_eq!(archived_material(&installation.inbox, "done")?.len(), 4);
    assert_eq!(fs::read_to_string(&installation.counter)?, "4\n");

    let log = installation.json_ok(["log"])?;
    assert_eq!(log["head_revision"], 4);
    assert_eq!(log["commits"][0]["summary"], "Integrate inbox source 4");
    assert_eq!(log["commits"][1]["summary"], "Integrate inbox source 3");
    assert_eq!(log["commits"][2]["summary"], "Integrate inbox source 2");
    assert_eq!(log["commits"][3]["summary"], "Integrate inbox source 1");
    Ok(())
}

#[test]
fn maintenance_marker_stops_the_worker_between_jobs() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "01-first.md",
        b"Shared inbox claim.\nFirst source.\n",
        0o600,
    )?;
    installation.incoming(
        "02-second.md",
        b"Shared inbox claim.\nSecond source.\n",
        0o600,
    )?;

    let ready = installation.directory.path().join("fake-codex-ready");
    let release = installation.directory.path().join("fake-codex-release");
    let child = installation
        .command()
        .args(["inbox", "run"])
        .env("ANNALS_FAKE_BLOCK_READY", &ready)
        .env("ANNALS_FAKE_BLOCK_RELEASE", &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Err(error) = wait_for_file(&ready) {
        fs::write(&release, b"release\n")?;
        let _ = child.wait_with_output();
        return Err(error);
    }
    fs::write(installation.inbox.join(".maintenance"), b"update\n")?;
    fs::write(&release, b"release\n")?;

    let stopped = successful_json(&child.wait_with_output()?)?;
    assert_eq!(stopped["attempted"], 1);
    assert_eq!(stopped["remaining"], 1);
    assert_eq!(stopped["queue_drained"], false);
    assert_eq!(stopped["stopped_for_maintenance"], true);
    assert_eq!(archived_material(&installation.inbox, "done")?.len(), 1);
    assert_eq!(
        archived_material(&installation.inbox, "processing")?.len(),
        0
    );
    assert_eq!(archived_material(&installation.inbox, "queued")?.len(), 1);

    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["maintenance"], true);
    fs::remove_file(installation.inbox.join(".maintenance"))?;

    let resumed = installation.json_ok(["inbox", "run"])?;
    assert_eq!(resumed["attempted"], 1);
    assert_eq!(resumed["remaining"], 0);
    assert_eq!(resumed["stopped_for_maintenance"], false);
    assert_eq!(archived_material(&installation.inbox, "done")?.len(), 2);
    assert!(archived_material(&installation.inbox, "processing")?.is_empty());
    assert!(archived_material(&installation.inbox, "queued")?.is_empty());
    Ok(())
}

#[test]
fn register_pause_and_resume_control_the_durable_queue() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "01-first.md",
        b"Shared inbox claim.\nFirst source.\n",
        0o600,
    )?;
    installation.incoming(
        "02-second.md",
        b"Shared inbox claim.\nSecond source.\n",
        0o600,
    )?;

    let registered = installation.json_ok(["inbox", "register"])?;
    assert_eq!(registered["registered"], 2);
    assert_eq!(registered["queued"], 2);
    assert_eq!(registered["jobs"].as_array().map(Vec::len), Some(2));
    assert!(!installation.counter.exists());
    assert!(archived_material(&installation.inbox, "processing")?.is_empty());
    for envelope in fs::read_dir(installation.inbox.join("queued"))? {
        let envelope = envelope?;
        let receipt: Value = serde_json::from_slice(&fs::read(envelope.path().join("job.json"))?)?;
        assert_eq!(receipt["version"], 6);
        assert_eq!(receipt["priority"], "normal");
        assert_eq!(receipt["state"], "queued");
        assert_eq!(receipt["attempts"], 0);
        assert!(receipt["sequence"].is_number());
        assert!(receipt["ingestion_id"].is_null());
    }
    let lately = installation.json_ok(["lately", "--channel", "inbox", "--by", "first-seen"])?;
    assert_eq!(lately["delivery_count"], 0);

    let paused = installation.json_ok(["inbox", "pause"])?;
    assert_eq!(paused["paused"], true);
    assert_eq!(paused["changed"], true);
    let already_paused = installation.json_ok(["inbox", "pause"])?;
    assert_eq!(already_paused["changed"], false);
    installation.incoming(
        "03-while-paused.md",
        b"Shared inbox claim.\nRegistered while paused.\n",
        0o600,
    )?;

    let stopped = installation.json_ok(["inbox", "run"])?;
    assert_eq!(stopped["registered"], 1);
    assert_eq!(stopped["attempted"], 0);
    assert_eq!(stopped["stopped_for_pause"], true);
    assert_eq!(stopped["remaining"], 3);
    assert!(!installation.counter.exists());

    let resumed = installation.json_ok(["inbox", "resume"])?;
    assert_eq!(resumed["paused"], false);
    assert_eq!(resumed["changed"], true);
    assert!(!installation.counter.exists());
    let run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(run["attempted"], 3);
    assert_eq!(run["remaining"], 0);
    assert_eq!(fs::read_to_string(&installation.counter)?, "3\n");
    let already_resumed = installation.json_ok(["inbox", "resume"])?;
    assert_eq!(already_resumed["changed"], false);
    Ok(())
}

#[test]
fn low_storage_defers_dispatch_and_a_later_activation_resumes_automatically() -> TestResult {
    let installation =
        Installation::new_with_minimum_available_bytes(0, BLOCKING_MINIMUM_AVAILABLE_BYTES)?;
    installation.init()?;
    installation.incoming(
        "storage-gated.md",
        b"Shared inbox claim.\nThis source waits for storage headroom.\n",
        0o600,
    )?;

    let stopped = installation.json_ok(["inbox", "run"])?;
    assert_eq!(stopped["registered"], 1);
    assert_eq!(stopped["attempted"], 0);
    assert_eq!(stopped["remaining"], 1);
    assert_eq!(stopped["queue_drained"], false);
    assert_eq!(stopped["stopped_for_low_space"], true);
    assert_eq!(stopped["storage"]["enabled"], true);
    assert_eq!(stopped["storage"]["ready"], false);
    assert_eq!(
        stopped["storage"]["minimum_available_bytes"],
        BLOCKING_MINIMUM_AVAILABLE_BYTES
    );
    assert!(!installation.counter.exists());

    let job = "j00000000000000000001";
    let receipt = archived_receipt(&installation.inbox, "queued", job)?;
    assert_eq!(receipt["state"], "queued");
    assert_eq!(receipt["attempts"], 0);
    assert!(receipt["ingestion_id"].is_null());

    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["paused"], false);
    assert_eq!(status["storage"]["ready"], false);

    installation.set_minimum_available_bytes(0)?;
    let resumed = installation.json_ok(["inbox", "run"])?;
    assert_eq!(resumed["attempted"], 1);
    assert_eq!(resumed["remaining"], 0);
    assert_eq!(resumed["stopped_for_low_space"], false);
    assert_eq!(resumed["storage"]["enabled"], false);
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");
    assert!(archived_material(&installation.inbox, "queued")?.is_empty());
    assert_eq!(archived_material(&installation.inbox, "done")?.len(), 1);
    Ok(())
}

#[test]
fn enqueue_rejects_a_copy_that_would_cross_the_storage_reserve() -> TestResult {
    let installation =
        Installation::new_with_minimum_available_bytes(0, BLOCKING_MINIMUM_AVAILABLE_BYTES)?;
    installation.init()?;
    let input = installation.directory.path().join("explicit-source.md");
    fs::write(&input, "Shared inbox claim.\nExplicit source.\n")?;

    let output = installation
        .command()
        .args([
            OsStr::new("inbox"),
            OsStr::new("enqueue"),
            input.as_os_str(),
        ])
        .output()?;
    failed_json(&output, "insufficient_storage")?;

    assert_eq!(
        fs::read_to_string(&input)?,
        "Shared inbox claim.\nExplicit source.\n"
    );
    assert!(archived_material(&installation.inbox, "queued")?.is_empty());
    assert!(
        fs::read_dir(installation.directory.path())?
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".annals-enqueue-"))
    );
    Ok(())
}

#[test]
fn priority_enqueue_runs_before_the_existing_queue_in_argument_order() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "01-normal.md",
        b"Shared inbox claim.\nFirst normal source.\n",
        0o600,
    )?;
    installation.incoming(
        "02-normal.md",
        b"Shared inbox claim.\nSecond normal source.\n",
        0o600,
    )?;
    let registered = installation.json_ok(["inbox", "register"])?;
    assert_eq!(registered["registered"], 2);

    let priority_first = installation.directory.path().join("z-priority.md");
    let priority_second = installation.directory.path().join("a-priority.md");
    fs::write(
        &priority_first,
        b"Shared inbox claim.\nFirst priority source.\n",
    )?;
    fs::write(
        &priority_second,
        b"Shared inbox claim.\nSecond priority source.\n",
    )?;
    let expected_first = material(&priority_first)?;
    let expected_second = material(&priority_second)?;

    let output = installation
        .command()
        .args(["inbox", "enqueue", "--priority"])
        .arg(&priority_first)
        .arg(&priority_second)
        .output()?;
    let enqueued = successful_json(&output)?;
    assert_eq!(enqueued["registered"], 2);
    assert_eq!(enqueued["queued"], 4);
    assert_eq!(enqueued["priority_queued"], 2);
    assert_eq!(enqueued["jobs"][0]["source_name"], "z-priority.md");
    assert_eq!(enqueued["jobs"][0]["priority"], "priority");
    assert_eq!(enqueued["jobs"][1]["source_name"], "a-priority.md");
    assert_eq!(enqueued["jobs"][1]["priority"], "priority");
    assert_eq!(enqueued["next_job"]["source_name"], "z-priority.md");
    assert_eq!(enqueued["next_job"]["priority"], "priority");
    assert_unchanged(&expected_first, &material(&priority_first)?);
    assert_unchanged(&expected_second, &material(&priority_second)?);

    let mut priorities = BTreeMap::new();
    for envelope in fs::read_dir(installation.inbox.join("queued"))? {
        let envelope = envelope?;
        let receipt: Value = serde_json::from_slice(&fs::read(envelope.path().join("job.json"))?)?;
        assert_eq!(receipt["version"], 6);
        priorities.insert(
            receipt["original_name"]
                .as_str()
                .ok_or("queued job receipt had no source name")?
                .to_owned(),
            receipt["priority"]
                .as_str()
                .ok_or("queued job receipt had no priority")?
                .to_owned(),
        );
    }
    assert_eq!(priorities["01-normal.md"], "normal");
    assert_eq!(priorities["02-normal.md"], "normal");
    assert_eq!(priorities["z-priority.md"], "priority");
    assert_eq!(priorities["a-priority.md"], "priority");

    let run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(run["attempted"], 4);
    assert_work_summary(&installation, "z-priority", "Integrate inbox source 1")?;
    assert_work_summary(&installation, "a-priority", "Integrate inbox source 2")?;
    assert_work_summary(&installation, "01-normal", "Integrate inbox source 3")?;
    assert_work_summary(&installation, "02-normal", "Integrate inbox source 4")?;
    Ok(())
}

#[test]
fn queued_jobs_can_be_prioritized_and_deprioritized_idempotently() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    for (name, bytes) in [
        (
            "01-oldest.md",
            b"Shared inbox claim.\nOldest normal source.\n".as_slice(),
        ),
        (
            "02-priority.md",
            b"Shared inbox claim.\nFirst selected priority source.\n".as_slice(),
        ),
        (
            "03-priority.md",
            b"Shared inbox claim.\nSecond selected priority source.\n".as_slice(),
        ),
        (
            "04-normal.md",
            b"Shared inbox claim.\nLast normal source.\n".as_slice(),
        ),
    ] {
        installation.incoming(name, bytes, 0o600)?;
    }
    let registered = installation.json_ok(["inbox", "register"])?;
    let second = registered_job_id(&registered, "02-priority.md")?;
    let third = registered_job_id(&registered, "03-priority.md")?;
    let fourth = registered_job_id(&registered, "04-normal.md")?;

    let first_priority = installation
        .command()
        .args(["inbox", "prioritize"])
        .arg(&third)
        .arg(&second)
        .output()?;
    let first_priority = successful_json(&first_priority)?;
    assert_eq!(first_priority["requested"], 2);
    assert_eq!(first_priority["changed"], 2);
    assert_eq!(first_priority["priority_queued"], 2);
    assert_eq!(first_priority["next_job"]["source_name"], "02-priority.md");

    let repeated_priority = installation
        .command()
        .args(["inbox", "prioritize"])
        .arg(&third)
        .arg(&second)
        .output()?;
    assert_eq!(successful_json(&repeated_priority)?["changed"], 0);

    let fourth_priority = installation
        .command()
        .args(["inbox", "prioritize"])
        .arg(&fourth)
        .output()?;
    assert_eq!(successful_json(&fourth_priority)?["changed"], 1);
    let fourth_normal = installation
        .command()
        .args(["inbox", "deprioritize"])
        .arg(&fourth)
        .output()?;
    assert_eq!(successful_json(&fourth_normal)?["changed"], 1);
    let repeated_normal = installation
        .command()
        .args(["inbox", "deprioritize"])
        .arg(&fourth)
        .output()?;
    assert_eq!(successful_json(&repeated_normal)?["changed"], 0);

    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["queued"], 4);
    assert_eq!(status["priority_queued"], 2);
    assert_eq!(status["next_job"]["source_name"], "02-priority.md");
    assert_eq!(status["next_job"]["priority"], "priority");

    let run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(run["attempted"], 4);
    assert_work_summary(&installation, "02-priority", "Integrate inbox source 1")?;
    assert_work_summary(&installation, "03-priority", "Integrate inbox source 2")?;
    assert_work_summary(&installation, "01-oldest", "Integrate inbox source 3")?;
    assert_work_summary(&installation, "04-normal", "Integrate inbox source 4")?;
    Ok(())
}

#[test]
fn version_four_queued_jobs_upgrade_into_the_normal_lane() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "legacy-normal.md",
        b"Shared inbox claim.\nQueued before priority lanes existed.\n",
        0o600,
    )?;
    installation.json_ok(["inbox", "register"])?;

    let receipt_path = installation
        .inbox
        .join("queued/j00000000000000000001/job.json");
    let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    receipt["version"] = Value::from(4);
    remove_retry_provenance(&mut receipt)?;
    receipt
        .as_object_mut()
        .ok_or("job receipt was not an object")?
        .remove("priority");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;

    installation.json_ok(["inbox", "pause"])?;
    let run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(run["attempted"], 0);
    assert_eq!(run["stopped_for_pause"], true);

    let upgraded: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
    assert_eq!(upgraded["version"], 6);
    assert_eq!(upgraded["priority"], "normal");
    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["priority_queued"], 0);
    assert_eq!(status["next_job"]["priority"], "normal");
    Ok(())
}

#[test]
fn pause_during_processing_leaves_later_and_new_arrivals_queued() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "01-first.md",
        b"Shared inbox claim.\nFirst source.\n",
        0o600,
    )?;
    installation.incoming(
        "02-second.md",
        b"Shared inbox claim.\nSecond source.\n",
        0o600,
    )?;

    let ready = installation.directory.path().join("fake-codex-ready");
    let release = installation.directory.path().join("fake-codex-release");
    let child = installation
        .command()
        .args(["inbox", "run"])
        .env("ANNALS_FAKE_BLOCK_READY", &ready)
        .env("ANNALS_FAKE_BLOCK_RELEASE", &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Err(error) = wait_for_file(&ready) {
        fs::write(&release, b"release\n")?;
        let _ = child.wait_with_output();
        return Err(error);
    }

    installation.incoming(
        "03-third.md",
        b"Shared inbox claim.\nThird source.\n",
        0o600,
    )?;
    let registered = installation.json_ok(["inbox", "register"])?;
    assert_eq!(registered["registered"], 1);
    assert_eq!(registered["queued"], 2);
    let paused = installation.json_ok(["inbox", "pause"])?;
    assert_eq!(paused["locked"], true);
    fs::write(&release, b"release\n")?;

    let stopped = successful_json(&child.wait_with_output()?)?;
    assert_eq!(stopped["attempted"], 1);
    assert_eq!(stopped["stopped_for_pause"], true);
    assert_eq!(stopped["remaining"], 2);
    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["paused"], true);
    assert_eq!(status["locked"], false);
    assert_eq!(status["queued"], 2);
    assert_eq!(status["processing"], 0);
    assert_eq!(status["next_job"]["sequence"], 2);
    Ok(())
}

#[test]
fn authentication_preflight_failure_leaves_every_source_queued() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "01-first.md",
        b"Shared inbox claim.\nFirst source.\n",
        0o600,
    )?;
    installation.incoming(
        "02-second.md",
        b"Shared inbox claim.\nSecond source.\n",
        0o600,
    )?;

    for _ in 0..2 {
        let output = installation
            .command()
            .args(["inbox", "run"])
            .env("ANNALS_FAKE_AUTH_FAIL", "1")
            .output()?;
        let error = unsuccessful_json(&output)?;
        assert_eq!(error["error"]["code"], "model_auth_unavailable");

        let status = installation.json_ok(["inbox", "status"])?;
        assert_eq!(status["queued"], 2);
        assert_eq!(status["processing"], 0);
        assert_eq!(status["failed"], 0);
        assert!(!installation.counter.exists());
        for envelope in fs::read_dir(installation.inbox.join("queued"))? {
            let envelope = envelope?;
            let receipt: Value =
                serde_json::from_slice(&fs::read(envelope.path().join("job.json"))?)?;
            assert_eq!(receipt["state"], "queued");
            assert_eq!(receipt["attempts"], 0);
        }
        let library = installation.json_ok(["stats"])?;
        assert_eq!(library["work_count"], 0);
        assert_eq!(library["model_run_count"], 0);
    }

    let recovered = installation.json_ok(["inbox", "run"])?;
    assert_eq!(recovered["attempted"], 2);
    assert_eq!(recovered["applied"], 2);
    assert_eq!(fs::read_to_string(&installation.counter)?, "2\n");
    Ok(())
}

#[test]
fn first_model_failure_is_archived_once_and_stops_only_that_activation() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let failed_source = installation.incoming(
        "01-fails.md",
        b"Shared inbox claim.\nThe first model attempt fails.\n",
        0o640,
    )?;
    let later_source = installation.incoming(
        "02-later.md",
        b"Shared inbox claim.\nThe later source remains runnable.\n",
        0o600,
    )?;

    let first_output = installation
        .command()
        .args(["inbox", "run"])
        .env("ANNALS_FAKE_FAIL_FIRST", "1")
        .output()?;
    let error = unsuccessful_json(&first_output)?;
    assert!(error["error"]["code"].is_string());
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    let failed = archived_material(&installation.inbox, "failed")?;
    assert_eq!(failed.len(), 1);
    assert_unchanged(
        &failed_source,
        failed
            .get("01-fails.md")
            .ok_or("failed source was not archived")?,
    );
    let receipt = only_archived_receipt(&installation.inbox, "failed")?;
    assert_eq!(receipt["state"], "failed");
    assert_eq!(receipt["attempts"], 1);
    assert!(receipt["completed_at"].is_string());
    assert!(receipt["last_error"]["code"].is_string());

    let queued = archived_material(&installation.inbox, "queued")?;
    assert_eq!(queued.len(), 1);
    assert_unchanged(
        &later_source,
        queued
            .get("02-later.md")
            .ok_or("later source did not remain queued")?,
    );
    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["active_job"], Value::Null);
    assert_eq!(status["queued"], 1);
    assert_eq!(status["failed"], 1);
    assert_eq!(status["next_job"]["id"], "j00000000000000000002");

    let second = installation.json_ok(["inbox", "run"])?;
    assert_eq!(second["attempted"], 1);
    assert_eq!(second["applied"], 1);
    assert_eq!(second["failed"], 0);
    assert_eq!(second["remaining"], 0);
    assert_eq!(fs::read_to_string(&installation.counter)?, "2\n");
    assert_eq!(archived_material(&installation.inbox, "failed")?.len(), 1);
    assert_eq!(archived_material(&installation.inbox, "done")?.len(), 1);
    let receipt = only_archived_receipt(&installation.inbox, "failed")?;
    assert_eq!(receipt["attempts"], 1);
    Ok(())
}

#[test]
fn first_apply_failure_is_archived_without_a_later_apply_retry() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "stale.md",
        b"Shared inbox claim.\nThe pending reconciliation becomes stale.\n",
        0o600,
    )?;

    let ready = installation.directory.path().join("fake-submit-ready");
    let release = installation.directory.path().join("fake-submit-release");
    let child = installation
        .command()
        .args(["inbox", "run"])
        .env("ANNALS_FAKE_AFTER_SUBMIT_READY", &ready)
        .env("ANNALS_FAKE_AFTER_SUBMIT_RELEASE", &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Err(error) = wait_for_file(&ready) {
        fs::write(&release, b"release\n")?;
        let _ = child.wait_with_output();
        return Err(error);
    }

    let connection = rusqlite::Connection::open(&installation.library)?;
    assert_eq!(
        connection.execute(
            "INSERT INTO commits(revision, kind, actor, created_at) \
             VALUES(1, 'shake', 'test', '2026-08-24T00:00:00Z')",
            [],
        )?,
        1
    );
    drop(connection);
    fs::write(&release, b"release\n")?;

    let output = wait_for_child_output(child, &release)?;
    failed_json(&output, "inbox_job_failed")?;
    let receipt = only_archived_receipt(&installation.inbox, "failed")?;
    assert_eq!(receipt["attempts"], 1);
    assert_eq!(receipt["last_error"]["code"], "stale_change");
    assert!(archived_material(&installation.inbox, "processing")?.is_empty());

    let next = installation.json_ok(["inbox", "run"])?;
    assert_eq!(next["attempted"], 0);
    assert_eq!(next["applied"], 0);
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");
    Ok(())
}

#[test]
fn active_job_can_be_interrupted_as_skipped_and_the_worker_continues() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let skipped_source = installation.incoming(
        "01-skipped.md",
        b"Shared inbox claim.\nThe active source is skipped.\n",
        0o640,
    )?;
    let later_source = installation.incoming(
        "02-continues.md",
        b"Shared inbox claim.\nThe successor still runs.\n",
        0o600,
    )?;

    let ready = installation.directory.path().join("fake-codex-ready");
    let release = installation.directory.path().join("fake-codex-release");
    let child = installation
        .command()
        .args(["inbox", "run"])
        .env("ANNALS_FAKE_BLOCK_READY", &ready)
        .env("ANNALS_FAKE_BLOCK_RELEASE", &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Err(error) = wait_for_file(&ready) {
        fs::write(&release, b"release\n")?;
        let _ = child.wait_with_output();
        return Err(error);
    }

    let status = installation.json_ok(["inbox", "status"])?;
    let active_id = status["active_job"]["id"]
        .as_str()
        .ok_or("status omitted the active job identifier")?
        .to_owned();
    assert_eq!(active_id, "j00000000000000000001");
    assert_eq!(status["active_job"]["source_name"], "01-skipped.md");
    successful_json(&installation.interrupt_output(
        &active_id,
        "skipped",
        Some("not suitable for this processing run"),
    )?)?;

    let run = successful_json(&wait_for_child_output(child, &release)?)?;
    assert_eq!(run["attempted"], 2);
    assert_eq!(run["skipped"], 1);
    assert_eq!(run["failed"], 0);
    assert_eq!(run["applied"], 1);
    assert_eq!(run["remaining"], 0);
    assert!(!release.exists(), "the blocked liaison was not interrupted");
    assert_eq!(fs::read_to_string(&installation.counter)?, "2\n");

    let skipped = archived_material(&installation.inbox, "skipped")?;
    assert_eq!(skipped.len(), 1);
    assert_unchanged(
        &skipped_source,
        skipped
            .get("01-skipped.md")
            .ok_or("interrupted source was not archived as skipped")?,
    );
    let done = archived_material(&installation.inbox, "done")?;
    assert_eq!(done.len(), 1);
    assert_unchanged(
        &later_source,
        done.get("02-continues.md")
            .ok_or("successor source was not processed")?,
    );
    let receipt = only_archived_receipt(&installation.inbox, "skipped")?;
    assert_eq!(receipt["state"], "skipped");
    assert_eq!(receipt["attempts"], 1);
    assert!(receipt["completed_at"].is_string());
    assert_eq!(receipt["last_error"]["code"], "inbox_job_skipped");
    assert!(
        receipt
            .to_string()
            .contains("not suitable for this processing run"),
        "operator reason was not retained in the skipped receipt"
    );
    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["active_job"], Value::Null);
    assert_eq!(status["skipped"], 1);
    assert_eq!(status["processing"], 0);
    Ok(())
}

#[test]
fn active_job_can_be_interrupted_as_failed() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let failed_source = installation.incoming(
        "interrupted.md",
        b"Shared inbox claim.\nThe operator fails this active source.\n",
        0o600,
    )?;

    let ready = installation.directory.path().join("fake-codex-ready");
    let release = installation.directory.path().join("fake-codex-release");
    let child = installation
        .command()
        .args(["inbox", "run"])
        .env("ANNALS_FAKE_BLOCK_READY", &ready)
        .env("ANNALS_FAKE_BLOCK_RELEASE", &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Err(error) = wait_for_file(&ready) {
        fs::write(&release, b"release\n")?;
        let _ = child.wait_with_output();
        return Err(error);
    }

    let status = installation.json_ok(["inbox", "status"])?;
    let active_id = status["active_job"]["id"]
        .as_str()
        .ok_or("status omitted the active job identifier")?;
    successful_json(&installation.interrupt_output(
        active_id,
        "failed",
        Some("operator rejected this source"),
    )?)?;

    let run = successful_json(&wait_for_child_output(child, &release)?)?;
    assert_eq!(run["attempted"], 1);
    assert_eq!(run["failed"], 1);
    assert_eq!(run["skipped"], 0);
    assert_eq!(run["remaining"], 0);
    let failed = archived_material(&installation.inbox, "failed")?;
    assert_unchanged(
        &failed_source,
        failed
            .get("interrupted.md")
            .ok_or("interrupted source was not archived as failed")?,
    );
    let receipt = only_archived_receipt(&installation.inbox, "failed")?;
    assert_eq!(receipt["state"], "failed");
    assert_eq!(receipt["attempts"], 1);
    assert_eq!(receipt["last_error"]["code"], "inbox_job_interrupted");
    assert!(
        receipt
            .to_string()
            .contains("operator rejected this source")
    );
    Ok(())
}

#[test]
fn pause_then_interrupt_skips_the_active_job_and_leaves_its_successor_queued() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let skipped_source = installation.incoming(
        "01-skipped.md",
        b"Shared inbox claim.\nPause before skipping this source.\n",
        0o640,
    )?;
    let later_source = installation.incoming(
        "02-queued.md",
        b"Shared inbox claim.\nThis source waits behind the pause.\n",
        0o600,
    )?;

    let ready = installation.directory.path().join("fake-codex-ready");
    let release = installation.directory.path().join("fake-codex-release");
    let child = installation
        .command()
        .args(["inbox", "run"])
        .env("ANNALS_FAKE_BLOCK_READY", &ready)
        .env("ANNALS_FAKE_BLOCK_RELEASE", &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Err(error) = wait_for_file(&ready) {
        fs::write(&release, b"release\n")?;
        let _ = child.wait_with_output();
        return Err(error);
    }

    let status = installation.json_ok(["inbox", "status"])?;
    let active_id = status["active_job"]["id"]
        .as_str()
        .ok_or("status omitted the active job identifier")?
        .to_owned();
    let paused = installation.json_ok(["inbox", "pause"])?;
    assert_eq!(paused["paused"], true);
    successful_json(&installation.interrupt_output(&active_id, "skipped", None)?)?;

    let stopped = successful_json(&wait_for_child_output(child, &release)?)?;
    assert_eq!(stopped["attempted"], 1);
    assert_eq!(stopped["skipped"], 1);
    assert_eq!(stopped["stopped_for_pause"], true);
    assert_eq!(stopped["remaining"], 1);
    assert_eq!(stopped["queue_drained"], false);
    assert!(!release.exists(), "the blocked liaison was not interrupted");
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    let skipped = archived_material(&installation.inbox, "skipped")?;
    assert_unchanged(
        &skipped_source,
        skipped
            .get("01-skipped.md")
            .ok_or("active source was not archived as skipped")?,
    );
    let queued = archived_material(&installation.inbox, "queued")?;
    assert_eq!(queued.len(), 1);
    assert_unchanged(
        &later_source,
        queued
            .get("02-queued.md")
            .ok_or("successor did not remain queued")?,
    );
    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["paused"], true);
    assert_eq!(status["active_job"], Value::Null);
    assert_eq!(status["queued"], 1);
    assert_eq!(status["skipped"], 1);
    assert_eq!(status["next_job"]["id"], "j00000000000000000002");

    installation.json_ok(["inbox", "resume"])?;
    let resumed = installation.json_ok(["inbox", "run"])?;
    assert_eq!(resumed["attempted"], 1);
    assert_eq!(resumed["applied"], 1);
    assert_eq!(resumed["remaining"], 0);
    assert_eq!(fs::read_to_string(&installation.counter)?, "2\n");
    Ok(())
}

#[test]
fn interrupt_requires_the_current_job_id_and_rejects_wrong_or_stale_targets() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let expected = installation.incoming(
        "active.md",
        b"Shared inbox claim.\nThe target remains active until released.\n",
        0o600,
    )?;
    let missing_id = installation
        .command()
        .args(["inbox", "interrupt", "--as", "skipped"])
        .output()?;
    assert_eq!(missing_id.status.code(), Some(2));

    let ready = installation.directory.path().join("fake-codex-ready");
    let release = installation.directory.path().join("fake-codex-release");
    let mut child = installation
        .command()
        .args(["inbox", "run"])
        .env("ANNALS_FAKE_BLOCK_READY", &ready)
        .env("ANNALS_FAKE_BLOCK_RELEASE", &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Err(error) = wait_for_file(&ready) {
        fs::write(&release, b"release\n")?;
        let _ = child.wait_with_output();
        return Err(error);
    }

    let status = installation.json_ok(["inbox", "status"])?;
    let active_id = status["active_job"]["id"]
        .as_str()
        .ok_or("status omitted the active job identifier")?
        .to_owned();
    let wrong =
        installation.interrupt_output("j00000000000000000999", "skipped", Some("wrong target"))?;
    conflict_json(&wrong)?;
    assert!(
        child.try_wait()?.is_none(),
        "wrong target stopped the worker"
    );

    fs::write(&release, b"release\n")?;
    let run = successful_json(&child.wait_with_output()?)?;
    assert_eq!(run["applied"], 1);
    let done = archived_material(&installation.inbox, "done")?;
    assert_unchanged(
        &expected,
        done.get("active.md")
            .ok_or("released active source was not completed")?,
    );

    let stale = installation.interrupt_output(&active_id, "failed", Some("stale target"))?;
    conflict_json(&stale)?;
    assert!(archived_material(&installation.inbox, "skipped")?.is_empty());
    Ok(())
}

#[test]
fn dispatch_crash_recovery_preserves_the_processing_boundary_while_paused() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "dispatched.md",
        b"Shared inbox claim.\nDispatched source.\n",
        0o600,
    )?;
    installation.json_ok(["inbox", "register"])?;
    let id = "j00000000000000000001";
    fs::rename(
        installation.inbox.join("queued").join(id),
        installation.inbox.join("processing").join(id),
    )?;

    installation.json_ok(["inbox", "pause"])?;
    let run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(run["attempted"], 0);
    assert_eq!(run["stopped_for_pause"], true);
    let receipt: Value = serde_json::from_slice(&fs::read(
        installation
            .inbox
            .join("processing")
            .join(id)
            .join("job.json"),
    )?)?;
    assert_eq!(receipt["state"], "processing");
    assert_eq!(receipt["attempts"], 0);
    assert!(!installation.inbox.join("queued").join(id).exists());
    Ok(())
}

#[test]
fn paused_recovery_finishes_a_durable_interruption_before_the_next_job() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let skipped = installation.incoming(
        "01-skipped.md",
        b"Shared inbox claim.\nInterrupted before the worker can finish.\n",
        0o600,
    )?;
    installation.incoming(
        "02-queued.md",
        b"Shared inbox claim.\nThe queued successor remains paused.\n",
        0o600,
    )?;
    installation.json_ok(["inbox", "register"])?;

    let id = "j00000000000000000001";
    let queued = installation.inbox.join("queued").join(id);
    let mut receipt: Value = serde_json::from_slice(&fs::read(queued.join("job.json"))?)?;
    receipt["state"] = Value::from("processing");
    fs::write(
        queued.join("job.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    fs::rename(&queued, installation.inbox.join("processing").join(id))?;

    installation.json_ok(["inbox", "pause"])?;
    successful_json(&installation.interrupt_output(
        id,
        "skipped",
        Some("recover while paused"),
    )?)?;
    let run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(run["recovered"], 2);
    assert_eq!(run["attempted"], 1);
    assert_eq!(run["skipped"], 1);
    assert_eq!(run["stopped_for_pause"], true);
    assert_eq!(run["remaining"], 1);
    assert!(!installation.counter.exists());
    assert_unchanged(
        &skipped,
        archived_material(&installation.inbox, "skipped")?
            .get("01-skipped.md")
            .ok_or("interrupted source was not recovered as skipped")?,
    );
    assert_eq!(archived_material(&installation.inbox, "queued")?.len(), 1);
    Ok(())
}

#[test]
fn invalid_zero_attempt_progress_never_starts_another_liaison() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "invalid-progress.md",
        b"Shared inbox claim.\nContradictory processing progress.\n",
        0o600,
    )?;
    installation.json_ok(["inbox", "register"])?;

    let id = "j00000000000000000001";
    let queued = installation.inbox.join("queued").join(id);
    let mut receipt: Value = serde_json::from_slice(&fs::read(queued.join("job.json"))?)?;
    receipt["state"] = Value::from("processing");
    receipt["model_run_token"] = Value::from("old-model-run");
    fs::write(
        queued.join("job.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    fs::rename(&queued, installation.inbox.join("processing").join(id))?;

    let output = installation.command().args(["inbox", "run"]).output()?;
    failed_json(&output, "invalid_job_receipt")?;
    assert!(!installation.counter.exists());
    assert!(installation.inbox.join("processing").join(id).is_dir());
    Ok(())
}

#[test]
fn legacy_unstarted_processing_receipt_migrates_to_queued_while_paused() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming("queued.md", b"Shared inbox claim.\nQueued source.\n", 0o600)?;
    installation.json_ok(["inbox", "register"])?;
    let id = "j00000000000000000001";
    let queued = installation.inbox.join("queued").join(id);
    let mut receipt: Value = serde_json::from_slice(&fs::read(queued.join("job.json"))?)?;
    receipt["version"] = Value::from(2);
    receipt["state"] = Value::from("processing");
    remove_retry_provenance(&mut receipt)?;
    {
        let object = receipt.as_object_mut().ok_or("receipt is not an object")?;
        object.remove("sequence");
        object.remove("priority");
    }
    fs::write(
        queued.join("job.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    fs::rename(&queued, installation.inbox.join("processing").join(id))?;

    installation.json_ok(["inbox", "pause"])?;
    let run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(run["attempted"], 0);
    assert_eq!(run["stopped_for_pause"], true);
    assert!(installation.inbox.join("queued").join(id).is_dir());
    assert!(!installation.inbox.join("processing").join(id).exists());
    let migrated: Value = serde_json::from_slice(&fs::read(
        installation.inbox.join("queued").join(id).join("job.json"),
    )?)?;
    assert_eq!(migrated["version"], 6);
    assert_eq!(migrated["priority"], "normal");
    assert_eq!(migrated["state"], "queued");
    assert_eq!(migrated["sequence"], 1);
    let lately = installation.json_ok(["lately", "--channel", "inbox"])?;
    assert_eq!(lately["delivery_count"], 0);
    Ok(())
}

#[test]
fn legacy_processing_receipt_with_a_delivery_record_stays_processing() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "started.md",
        b"Shared inbox claim.\nStarted source.\n",
        0o600,
    )?;
    installation.json_ok(["inbox", "register"])?;
    let id = "j00000000000000000001";
    let queued = installation.inbox.join("queued").join(id);
    let mut receipt: Value = serde_json::from_slice(&fs::read(queued.join("job.json"))?)?;
    let delivery_key = receipt["delivery_key"]
        .as_str()
        .ok_or("queued receipt omitted its delivery key")?
        .to_owned();
    let first_seen_at = receipt["first_seen_at"]
        .as_str()
        .ok_or("queued receipt omitted first_seen_at")?
        .to_owned();
    receipt["version"] = Value::from(2);
    receipt["state"] = Value::from("processing");
    remove_retry_provenance(&mut receipt)?;
    {
        let object = receipt.as_object_mut().ok_or("receipt is not an object")?;
        object.remove("sequence");
        object.remove("priority");
    }
    fs::write(
        queued.join("job.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    fs::rename(&queued, installation.inbox.join("processing").join(id))?;
    let connection = rusqlite::Connection::open(&installation.library)?;
    connection.execute(
        "INSERT INTO ingestions(\
             delivery_key, source_name, channel, first_seen_at, status\
         ) VALUES(?1, 'started.md', 'inbox', ?2, 'processing')",
        rusqlite::params![delivery_key, first_seen_at],
    )?;
    drop(connection);

    installation.json_ok(["inbox", "pause"])?;
    let run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(run["attempted"], 0);
    assert_eq!(run["stopped_for_pause"], true);
    assert!(installation.inbox.join("processing").join(id).is_dir());
    assert!(!installation.inbox.join("queued").join(id).exists());
    let migrated: Value = serde_json::from_slice(&fs::read(
        installation
            .inbox
            .join("processing")
            .join(id)
            .join("job.json"),
    )?)?;
    assert_eq!(migrated["version"], 6);
    assert_eq!(migrated["priority"], "normal");
    assert_eq!(migrated["state"], "processing");
    assert_eq!(migrated["attempts"], 0);
    let lately = installation.json_ok(["lately", "--channel", "inbox", "--by", "first-seen"])?;
    assert_eq!(lately["delivery_count"], 1);
    assert_eq!(lately["processing_count"], 1);
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn maintenance_smoke_does_not_rewrite_predecessor_spool_state() -> TestResult {
    let installation = Installation::new(3_600)?;
    installation.init()?;
    installation.incoming(
        "settling.txt",
        b"Shared inbox claim.\nA settling predecessor source.\n",
        0o600,
    )?;
    fs::write(
        installation.inbox.join(".queue.json"),
        LEGACY_QUEUE_WITH_SETTLING_ENTRY,
    )?;

    let processing = installation.inbox.join("processing");
    let legacy = processing.join("j00000000000000000007");
    fs::create_dir_all(legacy.join("material"))?;
    let legacy_source = legacy.join("material/legacy.txt");
    fs::write(
        &legacy_source,
        b"Shared inbox claim.\nA predecessor processing source.\n",
    )?;
    fs::write(legacy.join("job.json"), LEGACY_PROCESSING_RECEIPT)?;

    let missing = processing.join("j00000000000000000008");
    fs::create_dir_all(missing.join("material"))?;
    fs::write(
        missing.join("material/missing-receipt.txt"),
        b"Shared inbox claim.\nA pre-receipt predecessor source.\n",
    )?;
    fs::write(installation.inbox.join(".maintenance"), b"update\n")?;

    let smoke = installation.json_ok(["inbox", "run"])?;
    assert_eq!(smoke["recovered"], 2);
    assert_eq!(smoke["attempted"], 0);
    assert_eq!(smoke["stopped_for_maintenance"], true);
    assert_eq!(
        fs::read(installation.inbox.join(".queue.json"))?,
        LEGACY_QUEUE_WITH_SETTLING_ENTRY
    );
    assert_eq!(
        fs::read(legacy.join("job.json"))?,
        LEGACY_PROCESSING_RECEIPT
    );
    assert!(!missing.join("job.json").exists());
    assert!(!installation.counter.exists());

    fs::remove_file(installation.inbox.join(".maintenance"))?;
    let resumed = installation.json_ok(["inbox", "run"])?;
    assert_eq!(resumed["recovered"], 2);
    assert_eq!(resumed["attempted"], 1);
    assert_eq!(resumed["applied"], 1);
    assert_eq!(resumed["failed"], 1);
    assert_eq!(resumed["settling"], 1);
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    let upgraded_queue: Value =
        serde_json::from_slice(&fs::read(installation.inbox.join(".queue.json"))?)?;
    assert_eq!(upgraded_queue["version"], 4);
    assert_eq!(
        upgraded_queue["entries"]["c2V0dGxpbmcudHh0"]["sequence"],
        41
    );
    let upgraded_receipt: Value = serde_json::from_slice(&fs::read(
        installation
            .inbox
            .join("failed/j00000000000000000007/job.json"),
    )?)?;
    assert_eq!(upgraded_receipt["version"], 6);
    assert_eq!(upgraded_receipt["priority"], "normal");
    assert_eq!(upgraded_receipt["first_seen_at"], "2026-08-20T19:00:00Z");
    assert_eq!(upgraded_receipt["claimed_at"], "2026-08-20T19:00:00Z");
    assert_eq!(
        upgraded_receipt["delivery_key"],
        "inbox:j00000000000000000007:2026-08-20T19:00:00Z"
    );
    assert!(upgraded_receipt["ingestion_id"].is_number());
    assert_eq!(
        upgraded_receipt["last_error"]["code"],
        "inbox_processing_interrupted"
    );
    let repaired_receipt: Value = serde_json::from_slice(&fs::read(
        installation
            .inbox
            .join("done/j00000000000000000008/job.json"),
    )?)?;
    assert_eq!(repaired_receipt["version"], 6);
    assert_eq!(repaired_receipt["priority"], "normal");

    let settled = installation.json_ok(["inbox", "run", "--settle-seconds", "0"])?;
    assert_eq!(settled["attempted"], 1);
    assert!(
        installation
            .inbox
            .join("done/j00000000000000000041")
            .is_dir()
    );
    assert_eq!(fs::read_to_string(&installation.counter)?, "2\n");

    let lately = installation.json_ok([
        "lately",
        "--channel",
        "inbox",
        "--by",
        "first-seen",
        "--since",
        "2026-01-01",
    ])?;
    assert_eq!(lately["delivery_count"], 3);
    assert_eq!(
        lately["deliveries"]
            .as_array()
            .ok_or("lately deliveries were not an array")?
            .iter()
            .filter(|delivery| delivery["source_name"] == "legacy.txt")
            .count(),
        1
    );
    Ok(())
}

#[test]
fn permanent_input_failures_are_archived_unchanged_and_do_not_stop_the_batch() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let invalid = installation.incoming("01-invalid.bin", b"\xff\xfe", 0o640)?;
    let empty = installation.incoming("02-empty.txt", b" \n\t", 0o600)?;
    let valid = installation.incoming(
        "03-valid.txt",
        b"Shared inbox claim.\nA valid source.\n",
        0o644,
    )?;

    installation.json_ok(["inbox", "run"])?;
    assert!(incoming_names(&installation.inbox)?.is_empty());
    assert!(archived_material(&installation.inbox, "processing")?.is_empty());

    let failed = archived_material(&installation.inbox, "failed")?;
    assert_eq!(
        failed.keys().map(String::as_str).collect::<Vec<_>>(),
        ["01-invalid.bin", "02-empty.txt"]
    );
    assert_unchanged(
        &invalid,
        failed
            .get("01-invalid.bin")
            .ok_or("missing invalid input")?,
    );
    assert_unchanged(
        &empty,
        failed.get("02-empty.txt").ok_or("missing empty input")?,
    );

    let done = archived_material(&installation.inbox, "done")?;
    assert_eq!(done.len(), 1);
    assert_unchanged(
        &valid,
        done.get("03-valid.txt").ok_or("missing valid input")?,
    );
    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status_count(&status, "incoming"), Some(0), "{status}");
    assert_eq!(status_count(&status, "processing"), Some(0), "{status}");
    assert_eq!(status_count(&status, "done"), Some(1), "{status}");
    assert_eq!(status_count(&status, "failed"), Some(2), "{status}");

    let library_stats = installation.json_ok(["stats"])?;
    assert_eq!(library_stats["work_count"], 1);
    assert_eq!(library_stats["revision"], 1);

    let lately = installation.json_ok(["lately", "--channel", "inbox", "--by", "completed"])?;
    assert_eq!(lately["channel"], "inbox");
    assert_eq!(lately["time_basis"], "completed");
    assert_eq!(lately["delivery_count"], 3);
    assert_eq!(lately["completed_count"], 1);
    assert_eq!(lately["failed_count"], 2);
    assert_eq!(lately["new_work_count"], 1);
    assert_eq!(lately["duplicate_count"], 0);
    assert_eq!(lately["missing_time_count"], 0);

    let deliveries = lately["deliveries"]
        .as_array()
        .ok_or("lately deliveries were not an array")?;
    let valid_delivery = deliveries
        .iter()
        .find(|delivery| delivery["source_name"] == "03-valid.txt")
        .ok_or("valid inbox delivery was absent from lately")?;
    assert_eq!(valid_delivery["channel"], "inbox");
    assert_eq!(valid_delivery["status"], "completed");
    assert_eq!(valid_delivery["retention"], "new");
    assert_eq!(valid_delivery["result"], "applied");
    assert_eq!(valid_delivery["work"], "03-valid");
    assert_eq!(valid_delivery["applied_revision"], 1);
    assert!(valid_delivery["ingested_at"].is_string());
    assert!(valid_delivery["completed_at"].is_string());
    assert!(valid_delivery["error"].is_null());

    let invalid_delivery = deliveries
        .iter()
        .find(|delivery| delivery["source_name"] == "01-invalid.bin")
        .ok_or("invalid inbox delivery was absent from lately")?;
    assert_eq!(invalid_delivery["status"], "failed");
    assert!(invalid_delivery["retention"].is_null());
    assert!(invalid_delivery["result"].is_null());
    assert!(invalid_delivery["work"].is_null());
    assert!(invalid_delivery["ingested_at"].is_null());
    assert_eq!(invalid_delivery["error"]["code"], "input_not_utf8");
    assert!(invalid_delivery["completed_at"].is_string());

    let empty_delivery = deliveries
        .iter()
        .find(|delivery| delivery["source_name"] == "02-empty.txt")
        .ok_or("empty inbox delivery was absent from lately")?;
    assert_eq!(empty_delivery["status"], "failed");
    assert_eq!(empty_delivery["error"]["code"], "empty_work");
    Ok(())
}

#[test]
fn whitespace_source_name_is_recorded_before_label_failure() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let expected =
        installation.incoming("   ", b"Shared inbox claim.\nWhitespace filename.\n", 0o640)?;

    let run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(run["attempted"], 1);
    assert_eq!(run["failed"], 1);
    let failed = archived_material(&installation.inbox, "failed")?;
    assert_unchanged(
        &expected,
        failed
            .get("   ")
            .ok_or("whitespace-named source was not archived")?,
    );

    let lately = installation.json_ok(["lately", "--channel", "inbox", "--by", "completed"])?;
    assert_eq!(lately["delivery_count"], 1);
    let delivery = lately["deliveries"]
        .as_array()
        .and_then(|deliveries| deliveries.first())
        .ok_or("whitespace-named source was absent from lately")?;
    assert_eq!(delivery["source_name"], "   ");
    assert_eq!(delivery["status"], "failed");
    assert_eq!(delivery["error"]["code"], "invalid_label");
    Ok(())
}

#[test]
fn retained_bytes_bypass_an_unusable_inbox_label() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let bytes = b"Shared inbox claim.\nRetained before the unusable filename arrives.\n";
    let retained_source = installation.directory.path().join("original.txt");
    fs::write(&retained_source, bytes)?;
    installation.json_ok([
        OsStr::new("work"),
        OsStr::new("add"),
        retained_source.as_os_str(),
    ])?;

    let expected = installation.incoming("   ", bytes, 0o640)?;
    let run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(run["attempted"], 1);
    assert_eq!(run["duplicates"], 1);
    assert_eq!(run["failed"], 0);
    assert!(!installation.counter.exists());

    let duplicates = archived_material(&installation.inbox, "duplicates")?;
    assert_unchanged(
        &expected,
        duplicates
            .get("   ")
            .ok_or("whitespace-named duplicate was not archived")?,
    );
    let receipt = only_archived_receipt(&installation.inbox, "duplicates")?;
    assert_eq!(receipt["work"], "original");
    assert_eq!(receipt["result_status"], "retained");
    Ok(())
}

#[test]
fn an_exclusive_spool_lock_prevents_overlapping_runs() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    fs::create_dir_all(&installation.inbox)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(installation.inbox.join(".run.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)?;

    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["locked"], true);
    let output = installation.command().args(["inbox", "run"]).output()?;
    failed_json(&output, "inbox_locked")?;

    fs2::FileExt::unlock(&lock)?;
    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["locked"], false);
    Ok(())
}

#[test]
fn terminal_processing_receipts_are_archived_without_reprocessing() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "01-valid.txt",
        b"Shared inbox claim.\nA recoverable valid source.\n",
        0o640,
    )?;
    installation.incoming("02-invalid.bin", b"\xff\xfe", 0o600)?;
    let first = installation.json_ok(["inbox", "run"])?;
    assert_eq!(first["attempted"], 2);
    assert_eq!(first["applied"], 1);
    assert_eq!(first["failed"], 1);
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    return_archives_to_processing(&installation.inbox, "done")?;
    return_archives_to_processing(&installation.inbox, "failed")?;
    assert_eq!(
        archived_material(&installation.inbox, "processing")?.len(),
        2
    );
    fs::remove_file(&installation.codex)?;

    let recovered = installation.json_ok(["inbox", "run"])?;
    assert_eq!(recovered["recovered"], 2);
    assert_eq!(recovered["attempted"], 0);
    assert_eq!(recovered["applied"], 1);
    assert_eq!(recovered["failed"], 1);
    assert_eq!(recovered["remaining"], 0);
    assert!(archived_material(&installation.inbox, "processing")?.is_empty());
    assert_eq!(archived_material(&installation.inbox, "done")?.len(), 1);
    assert_eq!(archived_material(&installation.inbox, "failed")?.len(), 1);
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    let library_stats = installation.json_ok(["stats"])?;
    assert_eq!(library_stats["work_count"], 1);
    assert_eq!(library_stats["revision"], 1);

    let lately = installation.json_ok(["lately", "--channel", "inbox", "--by", "completed"])?;
    assert_eq!(lately["delivery_count"], 2);
    let deliveries = lately["deliveries"]
        .as_array()
        .ok_or("lately deliveries were not an array")?;
    assert_eq!(
        deliveries
            .iter()
            .filter(|delivery| delivery["source_name"] == "01-valid.txt")
            .count(),
        1
    );
    assert_eq!(
        deliveries
            .iter()
            .filter(|delivery| delivery["source_name"] == "02-invalid.bin")
            .count(),
        1
    );
    Ok(())
}

#[test]
fn interrupt_rejects_a_durably_completed_processing_envelope() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "completed.txt",
        b"Shared inbox claim.\nA completed delivery wins an interruption race.\n",
        0o600,
    )?;
    let first = installation.json_ok(["inbox", "run"])?;
    assert_eq!(first["applied"], 1);

    let id = "j00000000000000000001";
    let done = installation.inbox.join("done").join(id);
    let mut receipt: Value = serde_json::from_slice(&fs::read(done.join("job.json"))?)?;
    receipt["state"] = Value::from("processing");
    receipt["completed_at"] = Value::Null;
    receipt["result_status"] = Value::Null;
    receipt["result_revision"] = Value::Null;
    fs::write(done.join("job.json"), serde_json::to_vec_pretty(&receipt)?)?;
    let processing = installation.inbox.join("processing").join(id);
    fs::rename(&done, &processing)?;

    let interrupted = installation.interrupt_output(id, "skipped", Some("too late"))?;
    failed_json(&interrupted, "inbox_interrupt_too_late")?;
    assert!(!processing.join("interrupt.json").exists());

    let recovered = installation.json_ok(["inbox", "run"])?;
    assert_eq!(recovered["recovered"], 1);
    assert_eq!(recovered["attempted"], 0);
    assert_eq!(recovered["applied"], 1);
    assert_eq!(recovered["skipped"], 0);
    assert!(installation.inbox.join("done").join(id).is_dir());
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");
    Ok(())
}

#[test]
fn terminal_duplicate_receipt_is_archived_without_another_attempt() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let bytes = b"Shared inbox claim.\nA retained duplicate source.\n";
    let retained_source = installation.directory.path().join("retained.txt");
    fs::write(&retained_source, bytes)?;
    installation.json_ok([
        OsStr::new("work"),
        OsStr::new("add"),
        retained_source.as_os_str(),
    ])?;

    installation.incoming("duplicate.txt", bytes, 0o640)?;
    let first = installation.json_ok(["inbox", "run"])?;
    assert_eq!(first["attempted"], 1);
    assert_eq!(first["duplicates"], 1);
    assert_eq!(first["applied"], 0);
    assert!(!installation.counter.exists());

    return_archives_to_processing(&installation.inbox, "duplicates")?;
    assert_eq!(
        archived_material(&installation.inbox, "processing")?.len(),
        1
    );
    fs::remove_file(&installation.codex)?;

    let recovered = installation.json_ok(["inbox", "run"])?;
    assert_eq!(recovered["recovered"], 1);
    assert_eq!(recovered["attempted"], 0);
    assert_eq!(recovered["duplicates"], 1);
    assert_eq!(recovered["applied"], 0);
    assert_eq!(recovered["recorded"], 0);
    assert_eq!(recovered["failed"], 0);
    assert_eq!(recovered["remaining"], 0);
    assert!(archived_material(&installation.inbox, "processing")?.is_empty());
    assert_eq!(
        archived_material(&installation.inbox, "duplicates")?.len(),
        1
    );
    assert!(!installation.counter.exists());

    let lately = installation.json_ok(["lately", "--channel", "inbox"])?;
    assert_eq!(lately["delivery_count"], 1);
    assert_eq!(lately["duplicate_count"], 1);
    assert_eq!(lately["deliveries"][0]["result"], "retained");
    Ok(())
}

#[test]
fn legacy_done_receipt_reuses_its_applied_reconciliation() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "legacy-done.txt",
        b"Shared inbox claim.\nA predecessor-completed source.\n",
        0o600,
    )?;
    let first = installation.json_ok(["inbox", "run"])?;
    assert_eq!(first["applied"], 1);
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    let id = "j00000000000000000001";
    let done = installation.inbox.join("done").join(id);
    let current: Value = serde_json::from_slice(&fs::read(done.join("job.json"))?)?;
    let reconciliation_id = current["reconciliation_id"]
        .as_i64()
        .ok_or("completed receipt omitted its reconciliation")?;
    let legacy = serde_json::json!({
        "version": 1,
        "id": id,
        "original_name": "legacy-done.txt",
        "original_name_base64": "bGVnYWN5LWRvbmUudHh0",
        "state": "done",
        "attempts": 1,
        "created_at": "2026-08-20T20:00:00Z",
        "started_at": current["started_at"].clone(),
        "completed_at": current["completed_at"].clone(),
        "source_sha256": current["source_sha256"].clone(),
        "work": current["work"].clone(),
        "reconciliation_id": reconciliation_id,
        "model_run_token": current["model_run_token"].clone(),
        "result_status": "applied",
        "result_revision": 1,
        "last_error": null,
    });

    let connection = rusqlite::Connection::open(&installation.library)?;
    assert_eq!(connection.execute("DELETE FROM ingestions", [])?, 1);
    drop(connection);
    let processing = installation.inbox.join("processing").join(id);
    fs::create_dir_all(processing.parent().ok_or("processing had no parent")?)?;
    fs::rename(&done, &processing)?;
    fs::write(
        processing.join("job.json"),
        serde_json::to_vec_pretty(&legacy)?,
    )?;
    fs::remove_file(&installation.codex)?;

    let recovered = installation.json_ok(["inbox", "run"])?;
    assert_eq!(recovered["recovered"], 1);
    assert_eq!(recovered["attempted"], 0);
    assert_eq!(recovered["applied"], 1);
    assert_eq!(recovered["remaining"], 0);
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    let upgraded: Value = serde_json::from_slice(&fs::read(
        installation.inbox.join("done").join(id).join("job.json"),
    )?)?;
    assert_eq!(upgraded["version"], 6);
    assert_eq!(upgraded["priority"], "normal");
    assert_eq!(upgraded["state"], "done");
    assert_eq!(upgraded["first_seen_at"], "2026-08-20T20:00:00Z");
    assert_eq!(upgraded["reconciliation_id"], reconciliation_id);
    assert!(upgraded["ingestion_id"].is_number());

    let stats = installation.json_ok(["stats"])?;
    assert_eq!(stats["revision"], 1);
    assert_eq!(stats["work_count"], 1);
    assert_eq!(stats["model_run_count"], 1);
    let lately = installation.json_ok(["lately", "--channel", "inbox"])?;
    assert_eq!(lately["delivery_count"], 1);
    assert_eq!(lately["completed_count"], 1);
    assert_eq!(lately["deliveries"][0]["result"], "applied");
    Ok(())
}

#[test]
fn first_seen_time_survives_settling_before_inbox_ingestion() -> TestResult {
    let installation = Installation::new(3_600)?;
    installation.init()?;
    installation.incoming(
        "settling.txt",
        b"Shared inbox claim.\nA source observed before it settles.\n",
        0o640,
    )?;

    let settling = installation.json_ok(["inbox", "run"])?;
    assert_eq!(settling["attempted"], 0);
    assert_eq!(settling["settling"], 1);

    let index: Value = serde_json::from_slice(&fs::read(installation.inbox.join(".queue.json"))?)?;
    let entries = index["entries"]
        .as_object()
        .ok_or("inbox queue entries were not an object")?;
    assert_eq!(entries.len(), 1);
    let first_seen_at = entries
        .values()
        .next()
        .and_then(|entry| entry["first_seen_at"].as_str())
        .ok_or("queue entry omitted first_seen_at")?
        .to_owned();

    let processed = installation.json_ok(["inbox", "run", "--settle-seconds", "0"])?;
    assert_eq!(processed["attempted"], 1);
    assert_eq!(processed["applied"], 1);

    let lately = installation.json_ok(["lately", "--channel", "inbox", "--by", "first-seen"])?;
    assert_eq!(lately["delivery_count"], 1);
    assert_eq!(lately["missing_time_count"], 0);
    let delivery = lately["deliveries"]
        .as_array()
        .and_then(|deliveries| deliveries.first())
        .ok_or("settled inbox delivery was absent from lately")?;
    assert_eq!(delivery["source_name"], "settling.txt");
    assert_eq!(delivery["first_seen_at"], first_seen_at);
    assert_eq!(delivery["status"], "completed");
    assert_eq!(delivery["result"], "applied");
    Ok(())
}

#[test]
fn startup_repairs_pre_receipt_claim_crashes() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let processing = installation.inbox.join("processing");
    let recoverable = processing.join("j00000000000000000001");
    let recoverable_material = recoverable.join("material");
    fs::create_dir_all(&recoverable_material)?;
    let source = recoverable_material.join("recovered.txt");
    fs::write(
        &source,
        b"Shared inbox claim.\nSource moved before its receipt was written.\n",
    )?;
    let mut permissions = fs::metadata(&source)?.permissions();
    permissions.set_mode(0o640);
    fs::set_permissions(&source, permissions)?;
    let expected = material(&source)?;
    assert!(!recoverable.join("job.json").exists());

    let incomplete = processing.join("j00000000000000000002");
    fs::create_dir_all(incomplete.join("material"))?;
    assert!(!incomplete.join("job.json").exists());

    let repaired = installation.json_ok(["inbox", "run"])?;
    assert_eq!(repaired["recovered"], 1);
    assert_eq!(repaired["attempted"], 1);
    assert_eq!(repaired["applied"], 1);
    assert_eq!(repaired["failed"], 0);
    assert_eq!(repaired["remaining"], 0);
    assert!(!incomplete.exists());
    assert!(archived_material(&installation.inbox, "processing")?.is_empty());
    let done = archived_material(&installation.inbox, "done")?;
    assert_eq!(done.len(), 1);
    assert_unchanged(
        &expected,
        done.get("recovered.txt")
            .ok_or("repaired source was not archived")?,
    );
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");
    Ok(())
}

#[test]
fn pre_retained_duplicate_is_archived_and_reported_without_examination() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let bytes = b"Shared inbox claim.\nAlready retained source.\n";
    let retained_source = installation.directory.path().join("original.txt");
    fs::write(&retained_source, bytes)?;
    let added = installation.json_ok([
        OsStr::new("work"),
        OsStr::new("add"),
        retained_source.as_os_str(),
    ])?;
    assert_eq!(added["work"], "original");
    assert_eq!(added["retention"], "new");

    let colliding_source = installation.directory.path().join("copy.txt");
    fs::write(
        &colliding_source,
        b"Different retained bytes under the incoming basename.\n",
    )?;
    let colliding = installation.json_ok([
        OsStr::new("work"),
        OsStr::new("add"),
        colliding_source.as_os_str(),
    ])?;
    assert_eq!(colliding["work"], "copy");
    assert_eq!(colliding["retention"], "new");

    let expected = installation.incoming("copy.txt", bytes, 0o640)?;
    let run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(run["attempted"], 1);
    assert_eq!(run["duplicates"], 1);
    assert_eq!(run["applied"], 0);
    assert_eq!(run["recorded"], 0);
    assert_eq!(run["failed"], 0);
    assert!(!installation.counter.exists());

    assert!(archived_material(&installation.inbox, "done")?.is_empty());
    let duplicates = archived_material(&installation.inbox, "duplicates")?;
    assert_eq!(duplicates.len(), 1);
    assert_unchanged(
        &expected,
        duplicates
            .get("copy.txt")
            .ok_or("duplicate source was not archived")?,
    );
    let receipt = only_archived_receipt(&installation.inbox, "duplicates")?;
    assert_eq!(receipt["state"], "done");
    assert_eq!(receipt["attempts"], 1);
    assert_eq!(receipt["work"], "original");
    assert_eq!(receipt["result_status"], "retained");
    assert!(receipt["result_revision"].is_null());
    assert!(receipt["reconciliation_id"].is_null());
    assert!(receipt["model_run_token"].is_null());

    let inbox_status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(inbox_status["done"], 0);
    assert_eq!(inbox_status["duplicates"], 1);
    assert_eq!(inbox_status["failed"], 0);

    let library_stats = installation.json_ok(["stats"])?;
    assert_eq!(library_stats["work_count"], 2);
    assert_eq!(library_stats["model_run_count"], 0);
    assert_eq!(library_stats["revision"], 0);

    let lately = installation.json_ok(["lately", "--channel", "inbox"])?;
    assert_eq!(lately["delivery_count"], 1);
    assert_eq!(lately["completed_count"], 1);
    assert_eq!(lately["new_work_count"], 0);
    assert_eq!(lately["duplicate_count"], 1);
    let delivery = lately["deliveries"]
        .as_array()
        .and_then(|deliveries| deliveries.first())
        .ok_or("duplicate inbox delivery was absent from lately")?;
    assert_eq!(delivery["source_name"], "copy.txt");
    assert_eq!(delivery["status"], "completed");
    assert_eq!(delivery["retention"], "duplicate");
    assert_eq!(delivery["result"], "retained");
    assert_eq!(delivery["work"], "original");
    assert!(delivery["applied_revision"].is_null());
    assert!(delivery["error"].is_null());
    Ok(())
}

#[test]
fn redropping_an_ingested_source_runs_one_examination() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let bytes = b"Shared inbox claim.\nThe same source is delivered twice.\n";
    let first_expected = installation.incoming("source.txt", bytes, 0o640)?;

    let first_run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(first_run["attempted"], 1);
    assert_eq!(first_run["applied"], 1);
    assert_eq!(first_run["duplicates"], 0);
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    let second_expected = installation.incoming("source.txt", bytes, 0o600)?;
    let second_run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(second_run["attempted"], 1);
    assert_eq!(second_run["applied"], 0);
    assert_eq!(second_run["duplicates"], 1);
    assert_eq!(second_run["recorded"], 0);
    assert_eq!(second_run["failed"], 0);
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    let done = archived_material(&installation.inbox, "done")?;
    assert_eq!(done.len(), 1);
    assert_unchanged(
        &first_expected,
        done.get("source.txt")
            .ok_or("original source was not archived as done")?,
    );
    let duplicates = archived_material(&installation.inbox, "duplicates")?;
    assert_eq!(duplicates.len(), 1);
    assert_unchanged(
        &second_expected,
        duplicates
            .get("source.txt")
            .ok_or("repeated source was not archived as a duplicate")?,
    );

    let inbox_status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(inbox_status["done"], 1);
    assert_eq!(inbox_status["duplicates"], 1);
    let library_stats = installation.json_ok(["stats"])?;
    assert_eq!(library_stats["work_count"], 1);
    assert_eq!(library_stats["model_run_count"], 1);
    assert_eq!(library_stats["revision"], 1);

    let lately = installation.json_ok(["lately", "--channel", "inbox"])?;
    assert_eq!(lately["delivery_count"], 2);
    assert_eq!(lately["new_work_count"], 1);
    assert_eq!(lately["duplicate_count"], 1);
    let deliveries = lately["deliveries"]
        .as_array()
        .ok_or("lately deliveries were not an array")?;
    let original = deliveries
        .iter()
        .find(|delivery| delivery["retention"] == "new")
        .ok_or("original inbox delivery was absent from lately")?;
    assert_eq!(original["source_name"], "source.txt");
    assert_eq!(original["retention"], "new");
    assert_eq!(original["result"], "applied");
    let duplicate = deliveries
        .iter()
        .find(|delivery| delivery["retention"] == "duplicate")
        .ok_or("duplicate inbox delivery was absent from lately")?;
    assert_eq!(duplicate["source_name"], "source.txt");
    assert_eq!(duplicate["retention"], "duplicate");
    assert_eq!(duplicate["result"], "retained");
    assert_eq!(duplicate["work"], "source");

    let log = installation.json_ok(["log"])?;
    assert_eq!(log["head_revision"], 1);
    assert_eq!(log["commits"].as_array().map(Vec::len), Some(1));
    assert_eq!(log["commits"][0]["summary"], "Integrate inbox source 1");
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn bounded_retry_event_reexamines_retained_failures_and_preserves_originals() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "01-first.md",
        b"Shared inbox claim.\nFirst failed source.\n",
        0o640,
    )?;
    installation.incoming(
        "02-second.md",
        b"Shared inbox claim.\nSecond failed source.\n",
        0o600,
    )?;
    fail_next_inbox_job(&installation)?;
    fail_next_inbox_job(&installation)?;

    let first_job = "j00000000000000000001";
    let second_job = "j00000000000000000002";
    let first_receipt_path = installation
        .inbox
        .join("failed")
        .join(first_job)
        .join("job.json");
    let second_receipt_path = installation
        .inbox
        .join("failed")
        .join(second_job)
        .join("job.json");
    for path in [&first_receipt_path, &second_receipt_path] {
        let mut receipt: Value = serde_json::from_slice(&fs::read(path)?)?;
        receipt["version"] = Value::from(5);
        remove_retry_provenance(&mut receipt)?;
        fs::write(path, serde_json::to_vec_pretty(&receipt)?)?;
    }
    let first_receipt_before = fs::read(&first_receipt_path)?;
    let second_receipt_before = fs::read(&second_receipt_path)?;
    let failed_before = archived_material(&installation.inbox, "failed")?;

    let preview = installation.json_ok([
        "inbox",
        "retry",
        "preview",
        "--from",
        first_job,
        "--through",
        second_job,
    ])?;
    assert_eq!(preview["items"].as_array().map(Vec::len), Some(2));
    assert_eq!(preview["items"][0]["original_job_id"], first_job);
    assert_eq!(preview["items"][1]["original_job_id"], second_job);
    assert!(preview["items"][0].get("original_work_id").is_none());
    assert!(preview["items"][0]["already_selected_by"].is_null());

    let unpaused = installation
        .command()
        .args([
            "inbox",
            "retry",
            "start",
            "--from",
            first_job,
            "--through",
            second_job,
        ])
        .output()?;
    failed_json(&unpaused, "inbox_retry_requires_pause")?;

    installation.json_ok(["inbox", "pause"])?;
    let completed = installation.json_ok([
        "inbox",
        "retry",
        "start",
        "--from",
        first_job,
        "--through",
        second_job,
        "--reason",
        "recover the bounded authentication outage",
    ])?;
    assert_eq!(completed["event"]["id"], 1);
    assert_eq!(completed["event"]["state"], "completed");
    assert_eq!(
        completed["event"]["reason"],
        "recover the bounded authentication outage"
    );
    assert_eq!(completed["summary"]["selected"], 2);
    assert_eq!(completed["summary"]["attempted"], 2);
    assert_eq!(completed["summary"]["applied"], 2);
    assert_eq!(completed["summary"]["remaining"], 0);
    assert_eq!(
        completed["items"][0]["child_job_id"],
        "j00000000000000000003"
    );
    assert_eq!(
        completed["items"][1]["child_job_id"],
        "j00000000000000000004"
    );
    assert_eq!(completed["items"][0]["outcome"], "applied");
    assert_eq!(completed["items"][1]["outcome"], "applied");
    assert!(completed["items"][0].get("original_work_id").is_none());
    assert_eq!(fs::read_to_string(&installation.counter)?, "3\n");

    assert_eq!(fs::read(&first_receipt_path)?, first_receipt_before);
    assert_eq!(fs::read(&second_receipt_path)?, second_receipt_before);
    let failed_after = archived_material(&installation.inbox, "failed")?;
    assert_eq!(failed_after.len(), 2);
    for (name, before) in failed_before {
        assert_unchanged(
            &before,
            failed_after
                .get(&name)
                .ok_or("original failed material disappeared")?,
        );
    }

    for (child_job, original_job) in [
        ("j00000000000000000003", first_job),
        ("j00000000000000000004", second_job),
    ] {
        let receipt = archived_receipt(&installation.inbox, "done", child_job)?;
        assert_eq!(receipt["version"], 6);
        assert_eq!(receipt["state"], "done");
        assert_eq!(receipt["attempts"], 1);
        assert_eq!(receipt["retry_event_id"], 1);
        assert_eq!(receipt["retry_of_job_id"], original_job);
        assert!(receipt["retry_of_ingestion_id"].is_number());
        assert_eq!(receipt["result_status"], "applied");
    }
    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["paused"], true);
    assert_eq!(status["failed"], 2);
    assert_eq!(status["done"], 2);
    assert_eq!(status["queued"], 0);

    let selected_again = installation
        .command()
        .args([
            "inbox",
            "retry",
            "start",
            "--from",
            first_job,
            "--through",
            second_job,
        ])
        .output()?;
    failed_json(&selected_again, "inbox_retry_original_already_selected")?;
    let resumed = installation.json_ok(["inbox", "resume"])?;
    assert_eq!(resumed["paused"], false);
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn halted_retry_continues_only_unattempted_children() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    for (name, detail) in [
        ("01-first.md", "First"),
        ("02-second.md", "Second"),
        ("03-third.md", "Third"),
    ] {
        installation.incoming(
            name,
            format!("Shared inbox claim.\n{detail} failed source.\n").as_bytes(),
            0o600,
        )?;
    }
    for _ in 0..3 {
        fail_next_inbox_job(&installation)?;
    }
    installation.json_ok(["inbox", "pause"])?;
    fs::write(&installation.counter, b"0\n")?;

    let first_job = "j00000000000000000001";
    let third_job = "j00000000000000000003";
    let output = installation
        .command()
        .args([
            "inbox",
            "retry",
            "start",
            "--from",
            first_job,
            "--through",
            third_job,
        ])
        .env("ANNALS_FAKE_FAIL_FIRST", "1")
        .output()?;
    failed_json(&output, "inbox_retry_event_halted")?;

    let halted = installation.json_ok(["inbox", "retry", "status", "1"])?;
    assert_eq!(halted["event"]["state"], "halted");
    assert_eq!(halted["event"]["last_halt"]["code"], "model_runner_failed");
    assert!(
        halted["event"]["last_halt"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("simulated model failure"))
    );
    assert_eq!(halted["summary"]["selected"], 3);
    assert_eq!(halted["summary"]["attempted"], 1);
    assert_eq!(halted["summary"]["failed"], 1);
    assert_eq!(halted["summary"]["remaining"], 2);
    assert_eq!(halted["items"][0]["outcome"], "failed");
    assert_eq!(halted["items"][1]["outcome"], "not_attempted");
    assert_eq!(halted["items"][2]["outcome"], "not_attempted");
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    let resume_while_open = installation.command().args(["inbox", "resume"]).output()?;
    failed_json(&resume_while_open, "inbox_retry_event_active")?;

    let completed = installation.json_ok(["inbox", "retry", "continue", "1"])?;
    assert_eq!(completed["event"]["state"], "completed");
    assert_eq!(completed["summary"]["attempted"], 3);
    assert_eq!(completed["summary"]["applied"], 2);
    assert_eq!(completed["summary"]["failed"], 1);
    assert_eq!(completed["summary"]["remaining"], 0);
    assert_eq!(completed["items"][0]["outcome"], "failed");
    assert_eq!(completed["items"][1]["outcome"], "applied");
    assert_eq!(completed["items"][2]["outcome"], "applied");
    assert_eq!(fs::read_to_string(&installation.counter)?, "3\n");

    let retry_failed_child = completed["items"][0]["child_job_id"]
        .as_str()
        .ok_or("failed retry child had no job ID")?;
    let preview = installation.json_ok([
        "inbox",
        "retry",
        "preview",
        "--from",
        retry_failed_child,
        "--through",
        retry_failed_child,
    ])?;
    assert_eq!(preview["items"].as_array().map(Vec::len), Some(1));
    assert!(preview["items"][0]["already_selected_by"].is_null());
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn retry_reexamines_a_pending_reconciliation_that_becomes_stale_in_the_event() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "01-first.md",
        b"Shared inbox claim.\nFirst source with a pending change.\n",
        0o600,
    )?;
    installation.incoming(
        "02-second.md",
        b"Shared inbox claim.\nSecond source with a pending change.\n",
        0o600,
    )?;
    fail_next_inbox_job(&installation)?;
    fail_next_inbox_job(&installation)?;

    installation.json_ok(["integrate", "--work", "01-first", "--reexamine"])?;
    installation.json_ok(["integrate", "--work", "02-second", "--reexamine"])?;
    let connection = rusqlite::Connection::open(&installation.library)?;
    let mut statement = connection.prepare(
        "SELECT work.label, reconciliation.id
         FROM reconciliations AS reconciliation
         JOIN reconciliation_requests AS request
           ON request.id = reconciliation.request_id
         JOIN works AS work ON work.id = request.work_id
         WHERE reconciliation.status = 'pending'
         ORDER BY reconciliation.id",
    )?;
    let pending = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    drop(connection);
    assert_eq!(pending.len(), 2);

    for (job_id, label) in [
        ("j00000000000000000001", "01-first"),
        ("j00000000000000000002", "02-second"),
    ] {
        let reconciliation_id = pending
            .iter()
            .find(|(work, _)| work == label)
            .map(|(_, id)| *id)
            .ok_or("pending reconciliation was absent")?;
        let path = installation
            .inbox
            .join("failed")
            .join(job_id)
            .join("job.json");
        let mut receipt: Value = serde_json::from_slice(&fs::read(&path)?)?;
        receipt["reconciliation_id"] = Value::from(reconciliation_id);
        fs::write(path, serde_json::to_vec_pretty(&receipt)?)?;
    }

    installation.json_ok(["inbox", "pause"])?;
    let completed = installation.json_ok([
        "inbox",
        "retry",
        "start",
        "--from",
        "j00000000000000000001",
        "--through",
        "j00000000000000000002",
    ])?;
    assert_eq!(completed["event"]["state"], "completed");
    assert_eq!(completed["summary"]["applied"], 2);
    assert_eq!(completed["summary"]["failed"], 0);
    assert_eq!(fs::read_to_string(&installation.counter)?, "4\n");

    let first_child = archived_receipt(&installation.inbox, "done", "j00000000000000000003")?;
    let second_child = archived_receipt(&installation.inbox, "done", "j00000000000000000004")?;
    assert_eq!(
        first_child["reconciliation_id"],
        first_child["retry_reconciliation_id"]
    );
    assert_ne!(
        second_child["reconciliation_id"],
        second_child["retry_reconciliation_id"]
    );
    assert_eq!(second_child["result_status"], "applied");
    Ok(())
}

#[test]
fn retry_does_not_adopt_an_unlinked_reconciliation_for_the_same_work() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "source.md",
        b"Shared inbox claim.\nA source with unrelated later work.\n",
        0o600,
    )?;
    fail_next_inbox_job(&installation)?;
    installation.json_ok(["integrate", "--work", "source", "--reexamine"])?;
    let connection = rusqlite::Connection::open(&installation.library)?;
    let unrelated_id = connection.query_row(
        "SELECT reconciliation.id
         FROM reconciliations AS reconciliation
         JOIN reconciliation_requests AS request
           ON request.id = reconciliation.request_id
         JOIN works AS work ON work.id = request.work_id
         WHERE work.label = 'source' AND reconciliation.status = 'pending'",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    drop(connection);

    installation.json_ok(["inbox", "pause"])?;
    let job = "j00000000000000000001";
    let completed =
        installation.json_ok(["inbox", "retry", "start", "--from", job, "--through", job])?;
    assert_eq!(completed["summary"]["applied"], 1);
    assert_eq!(fs::read_to_string(&installation.counter)?, "3\n");
    let child = archived_receipt(&installation.inbox, "done", "j00000000000000000002")?;
    assert!(child["retry_reconciliation_id"].is_null());
    assert_ne!(child["reconciliation_id"], unrelated_id);
    let connection = rusqlite::Connection::open(&installation.library)?;
    let unrelated_status = connection.query_row(
        "SELECT status FROM reconciliations WHERE id = ?1",
        [unrelated_id],
        |row| row.get::<_, String>(0),
    )?;
    assert_eq!(unrelated_status, "superseded");
    Ok(())
}

#[test]
fn retry_rejects_a_pre_retention_failure_without_creating_an_event() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming("invalid.md", &[0xff, 0xfe, 0xfd], 0o600)?;
    let run = installation.json_ok(["inbox", "run"])?;
    assert_eq!(run["failed"], 1);

    let job = "j00000000000000000001";
    let preview = installation
        .command()
        .args(["inbox", "retry", "preview", "--from", job, "--through", job])
        .output()?;
    failed_json(&preview, "inbox_retry_original_not_retained")?;

    installation.json_ok(["inbox", "pause"])?;
    let start = installation
        .command()
        .args(["inbox", "retry", "start", "--from", job, "--through", job])
        .output()?;
    failed_json(&start, "inbox_retry_original_not_retained")?;
    let events = installation.json_ok(["inbox", "retry", "status"])?;
    assert!(events["events"].as_array().is_some_and(Vec::is_empty));
    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["failed"], 1);
    assert_eq!(status["queued"], 0);
    Ok(())
}

#[test]
fn retry_auth_preflight_halts_before_any_child_attempt() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "auth.md",
        b"Shared inbox claim.\nOriginal failed source.\n",
        0o600,
    )?;
    fail_next_inbox_job(&installation)?;
    installation.json_ok(["inbox", "pause"])?;

    let job = "j00000000000000000001";
    let output = installation
        .command()
        .args(["inbox", "retry", "start", "--from", job, "--through", job])
        .env("ANNALS_FAKE_AUTH_FAIL", "1")
        .output()?;
    failed_json(&output, "model_auth_unavailable")?;

    let halted = installation.json_ok(["inbox", "retry", "status", "1"])?;
    assert_eq!(halted["event"]["state"], "halted");
    assert_eq!(halted["summary"]["attempted"], 0);
    assert_eq!(halted["summary"]["remaining"], 1);
    assert_eq!(halted["items"][0]["outcome"], "not_attempted");
    assert!(halted["items"][0]["child_delivery_id"].is_null());
    let child_job = halted["items"][0]["child_job_id"]
        .as_str()
        .ok_or("retry child had no job ID")?;
    let receipt = archived_receipt(&installation.inbox, "queued", child_job)?;
    assert_eq!(receipt["attempts"], 0);
    assert!(receipt["ingestion_id"].is_null());
    assert!(receipt["model_run_token"].is_null());
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    let completed = installation.json_ok(["inbox", "retry", "continue", "1"])?;
    assert_eq!(completed["event"]["state"], "completed");
    assert_eq!(completed["summary"]["applied"], 1);
    assert_eq!(completed["summary"]["remaining"], 0);
    assert_eq!(fs::read_to_string(&installation.counter)?, "2\n");
    Ok(())
}

#[test]
fn retry_low_storage_halts_before_any_child_attempt() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    installation.incoming(
        "storage.md",
        b"Shared inbox claim.\nOriginal failed source.\n",
        0o600,
    )?;
    fail_next_inbox_job(&installation)?;
    installation.json_ok(["inbox", "pause"])?;
    installation.set_minimum_available_bytes(BLOCKING_MINIMUM_AVAILABLE_BYTES)?;

    let job = "j00000000000000000001";
    let output = installation
        .command()
        .args(["inbox", "retry", "start", "--from", job, "--through", job])
        .output()?;
    failed_json(&output, "insufficient_storage")?;

    let halted = installation.json_ok(["inbox", "retry", "status", "1"])?;
    assert_eq!(halted["event"]["state"], "halted");
    assert_eq!(halted["event"]["last_halt"]["code"], "insufficient_storage");
    assert_eq!(halted["summary"]["attempted"], 0);
    assert_eq!(halted["summary"]["remaining"], 1);
    assert_eq!(halted["items"][0]["outcome"], "not_attempted");
    assert!(halted["items"][0]["child_delivery_id"].is_null());
    let child_job = halted["items"][0]["child_job_id"]
        .as_str()
        .ok_or("retry child had no job ID")?;
    let receipt = archived_receipt(&installation.inbox, "queued", child_job)?;
    assert_eq!(receipt["attempts"], 0);
    assert!(receipt["ingestion_id"].is_null());
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    installation.set_minimum_available_bytes(0)?;
    let completed = installation.json_ok(["inbox", "retry", "continue", "1"])?;
    assert_eq!(completed["event"]["state"], "completed");
    assert_eq!(completed["summary"]["applied"], 1);
    assert_eq!(completed["summary"]["remaining"], 0);
    assert_eq!(fs::read_to_string(&installation.counter)?, "2\n");
    Ok(())
}

const COMPATIBLE_PROTOCOL_SCHEMA: &str = r##"{
  "definitions": {
    "ClientRequest": {"oneOf": [
      {"properties":{"method":{"enum":["initialize"]},"params":{"$ref":"#/definitions/InitializeParams"}}},
      {"properties":{"method":{"enum":["thread/start"]},"params":{"$ref":"#/definitions/v2/ThreadStartParams"}}},
      {"properties":{"method":{"enum":["turn/start"]},"params":{"$ref":"#/definitions/v2/TurnStartParams"}}},
      {"properties":{"method":{"enum":["mcpServerStatus/list"]},"params":{"$ref":"#/definitions/v2/ListMcpServerStatusParams"}}}
    ]},
    "ServerRequest": {"oneOf": [
      {"properties":{"method":{"enum":["item/tool/call"]},"params":{"$ref":"#/definitions/DynamicToolCallParams"}}}
    ]},
    "ServerNotification": {"oneOf": [
      {"properties":{"method":{"enum":["item/completed"]},"params":{"$ref":"#/definitions/v2/ItemCompletedNotification"}}},
      {"properties":{"method":{"enum":["turn/completed"]},"params":{"$ref":"#/definitions/v2/TurnCompletedNotification"}}},
      {"properties":{"method":{"enum":["thread/tokenUsage/updated"]},"params":{"$ref":"#/definitions/v2/ThreadTokenUsageUpdatedNotification"}}}
    ]},
    "DynamicToolCallParams": {"required":["arguments","callId","threadId","tool","turnId"]},
    "v2": {
      "ThreadStartParams": {"properties":{
        "approvalPolicy":{},"baseInstructions":{},"cwd":{},"developerInstructions":{},
        "dynamicTools":{},"ephemeral":{},"environments":{},"experimentalRawEvents":{},
        "model":{},"sandbox":{}
      }},
      "AskForApproval": {"enum":["never"]},
      "SandboxMode": {"enum":["read-only","workspace-write"]},
      "TurnStartParams": {
        "required":["input","threadId"],
        "properties":{"effort":{},"environments":{},"input":{},"threadId":{}}
      },
      "DynamicToolSpec": {"oneOf":[{
        "required":["description","inputSchema","name","type"],
        "properties":{"type":{"enum":["function"]}}
      }]},
      "ThreadStartResponse": {"required":["thread"]},
      "TurnStartResponse": {"required":["turn"]},
      "Thread": {"required":["id"]},
      "Turn": {"required":["id","status"]},
      "TurnStatus": {"enum":["completed","failed"]},
      "ItemCompletedNotification": {"required":["item","threadId","turnId"]},
      "TurnCompletedNotification": {"required":["threadId","turn"]},
      "ThreadTokenUsageUpdatedNotification": {"required":["threadId","tokenUsage","turnId"]},
      "RawResponseCompletedNotification": {"required":["responseId","threadId","turnId"]}
    }
  }
}"##;

fn start_fake_nucleus(root: &Path, socket: &Path, codex: &Path) -> TestResult {
    let codex_home = root.join("nucleus-codex-home");
    fs::create_dir(&codex_home)?;
    fs::set_permissions(&codex_home, fs::Permissions::from_mode(0o700))?;
    let config = codex_home.join("config.toml");
    fs::write(&config, "cli_auth_credentials_store = \"file\"\n")?;
    fs::set_permissions(&config, fs::Permissions::from_mode(0o600))?;
    let auth = codex_home.join("auth.json");
    fs::write(&auth, "{\"OPENAI_API_KEY\":\"test-api-key\"}\n")?;
    fs::set_permissions(&auth, fs::Permissions::from_mode(0o600))?;
    let serve_config = ServeConfig {
        socket: socket.to_path_buf(),
        database: root.join("nucleus.db"),
        codex: codex.to_path_buf(),
        codex_home,
    };
    thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => panic!("start fake Nucleus runtime: {error}"),
        };
        if let Err(error) = runtime.block_on(serve(serve_config)) {
            panic!("serve fake Nucleus: {error}");
        }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        if Instant::now() >= deadline {
            return Err("fake Nucleus did not create its socket".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn write_fake_codex(path: &Path, counter: &Path, controls: &Path) -> TestResult {
    let script = r#"#!/bin/sh
set -eu

if [ "${1:-}" = "--version" ]; then
  printf '%s\n' 'codex-cli 0.146.0'
  exit 0
fi

if [ "${1:-}" = "debug" ] && [ "${2:-}" = "models" ]; then
  printf '%s\n' '{"models":[{"slug":"fake-model","supported_reasoning_levels":[{"effort":"medium"},{"effort":"max"}],"default_reasoning_level":"medium","shell_type":"disabled","supports_search_tool":false}]}'
  exit 0
fi

case " $* " in
  *" generate-json-schema "*)
    output=''
    take_output=0
    for argument in "$@"; do
      if [ "$take_output" -eq 1 ]; then
        output=$argument
        break
      fi
      if [ "$argument" = "--out" ]; then
        take_output=1
      fi
    done
    test -n "$output"
    mkdir -p "$output"
    printf '%s\n' '__PROTOCOL_SCHEMA__' > "$output/codex_app_server_protocol.schemas.json"
    exit 0
    ;;
esac

IFS= read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{}}'
IFS= read -r ignored
IFS= read -r request
case "$request" in
  *'account/rateLimits/read'*)
  if [ -f '__CONTROLS__/auth-fail' ]; then
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"refresh_token_reused: please log out and sign in again"}}'
  else
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"rateLimits":{}}}'
  fi
  exit 0
  ;;
esac

printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"data":[],"nextCursor":null}}'
IFS= read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thread"}}}'
IFS= read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"turn":{"id":"turn"}}}'

counter='__COUNTER__'
number=0
if [ -f "$counter" ]; then
  number=$(sed -n '1p' "$counter")
fi
number=$((number + 1))
printf '%s\n' "$number" > "$counter"
if [ "$number" -eq 1 ] && [ -f '__CONTROLS__/block-ready' ]; then
  ready=$(cat '__CONTROLS__/block-ready')
  release=$(cat '__CONTROLS__/block-release')
  printf '%s\n' ready > "$ready"
  while [ ! -f "$release" ]; do
    sleep 0.01
  done
fi
if [ "$number" -eq 1 ] && [ -f '__CONTROLS__/fail-first' ]; then
  printf '%s\n' 'simulated model failure' >&2
  exit 19
fi
printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":20,\"method\":\"item/tool/call\",\"params\":{\"threadId\":\"thread\",\"turnId\":\"turn\",\"callId\":\"call\",\"namespace\":null,\"tool\":\"submit_reconciliation\",\"arguments\":{\"summary\":\"Integrate inbox source $number\",\"operations\":[{\"action\":\"create_concept\",\"ref\":\"inbox_item\",\"label\":\"Inbox concept $number\",\"parents\":[],\"evidence\":[{\"quote\":\"Shared inbox claim.\"}]}]}}}"
IFS= read -r ignored
if [ "$number" -eq 1 ] && [ -f '__CONTROLS__/after-submit-ready' ]; then
  submit_ready=$(cat '__CONTROLS__/after-submit-ready')
  submit_release=$(cat '__CONTROLS__/after-submit-release')
  printf '%s\n' ready > "$submit_ready"
  while [ ! -f "$submit_release" ]; do
    sleep 0.01
  done
fi
printf '%s\n' '{"jsonrpc":"2.0","method":"item/completed","params":{"threadId":"thread","turnId":"turn","item":{"id":"message","type":"agentMessage","text":"fake completed"}}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"thread","turn":{"id":"turn","status":"completed"}}}'
"#
        .replace("__PROTOCOL_SCHEMA__", COMPATIBLE_PROTOCOL_SCHEMA)
        .replace("__COUNTER__", &counter.display().to_string())
        .replace("__CONTROLS__", &controls.display().to_string());
    fs::write(path, script)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)?;
    Ok(())
}
