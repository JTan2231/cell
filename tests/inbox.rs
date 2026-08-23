use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, FileTimes, OpenOptions};
use std::io;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use tempfile::TempDir;

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

struct Installation {
    directory: TempDir,
    config: PathBuf,
    library: PathBuf,
    inbox: PathBuf,
    counter: PathBuf,
    codex: PathBuf,
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
        let directory = tempfile::tempdir()?;
        let config = directory.path().join("annals.toml");
        let library = directory.path().join("state/annals.db");
        let inbox = directory.path().join("spool");
        let counter = directory.path().join("fake-codex-counter");
        let codex = directory.path().join("fake-codex");
        fs::create_dir_all(library.parent().ok_or("library had no parent directory")?)?;
        write_fake_codex(&codex)?;
        fs::write(
            &config,
            format!(
                concat!(
                    "library = {}\n",
                    "\n",
                    "[inbox]\n",
                    "root = {}\n",
                    "settle_seconds = {settle_seconds}\n",
                    "\n",
                    "[liaison]\n",
                    "quality = \"medium\"\n",
                    "model = \"fake-model\"\n",
                    "codex = {}\n",
                ),
                toml_string(&library),
                toml_string(&inbox),
                toml_string(&codex),
                settle_seconds = settle_seconds,
            ),
        )?;
        Ok(Self {
            directory,
            config,
            library,
            inbox,
            counter,
            codex,
        })
    }

    fn command(&self) -> Command {
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
        command
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

fn failed_json(output: &Output, code: &str) -> TestResult<Value> {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    assert!(output.stdout.is_empty());
    let envelope = serde_json::from_slice::<Value>(&output.stderr)?;
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], code);
    Ok(envelope)
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
        2
    );
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
        1
    );

    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["maintenance"], true);
    fs::remove_file(installation.inbox.join(".maintenance"))?;

    let resumed = installation.json_ok(["inbox", "run"])?;
    assert_eq!(resumed["attempted"], 1);
    assert_eq!(resumed["remaining"], 0);
    assert_eq!(resumed["stopped_for_maintenance"], false);
    assert_eq!(archived_material(&installation.inbox, "done")?.len(), 2);
    assert!(archived_material(&installation.inbox, "processing")?.is_empty());
    Ok(())
}

#[test]
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
    assert_eq!(resumed["attempted"], 2);
    assert_eq!(resumed["applied"], 2);
    assert_eq!(resumed["settling"], 1);
    assert_eq!(fs::read_to_string(&installation.counter)?, "2\n");

    let upgraded_queue: Value =
        serde_json::from_slice(&fs::read(installation.inbox.join(".queue.json"))?)?;
    assert_eq!(upgraded_queue["version"], 3);
    assert_eq!(
        upgraded_queue["entries"]["c2V0dGxpbmcudHh0"]["sequence"],
        41
    );
    let upgraded_receipt: Value = serde_json::from_slice(&fs::read(
        installation
            .inbox
            .join("done/j00000000000000000007/job.json"),
    )?)?;
    assert_eq!(upgraded_receipt["version"], 2);
    assert_eq!(upgraded_receipt["first_seen_at"], "2026-08-20T19:00:00Z");
    assert_eq!(upgraded_receipt["claimed_at"], "2026-08-20T19:00:00Z");
    assert_eq!(
        upgraded_receipt["delivery_key"],
        "inbox:j00000000000000000007:2026-08-20T19:00:00Z"
    );
    assert!(upgraded_receipt["ingestion_id"].is_number());
    let repaired_receipt: Value = serde_json::from_slice(&fs::read(
        installation
            .inbox
            .join("done/j00000000000000000008/job.json"),
    )?)?;
    assert_eq!(repaired_receipt["version"], 2);

    let settled = installation.json_ok(["inbox", "run", "--settle-seconds", "0"])?;
    assert_eq!(settled["attempted"], 1);
    assert!(
        installation
            .inbox
            .join("done/j00000000000000000041")
            .is_dir()
    );
    assert_eq!(fs::read_to_string(&installation.counter)?, "3\n");

    let lately = installation.json_ok(["lately", "--channel", "inbox"])?;
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
    assert_eq!(recovered["attempted"], 1);
    assert_eq!(recovered["applied"], 1);
    assert_eq!(recovered["remaining"], 0);
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    let upgraded: Value = serde_json::from_slice(&fs::read(
        installation.inbox.join("done").join(id).join("job.json"),
    )?)?;
    assert_eq!(upgraded["version"], 2);
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

fn write_fake_codex(path: &Path) -> TestResult {
    fs::write(
        path,
        r#"#!/bin/sh
set -eu

if [ "${1:-}" = "debug" ]; then
  printf '%s\n' '{"models":[{"slug":"fake-model"}]}'
  exit 0
fi

IFS= read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{}}'
IFS= read -r ignored
IFS= read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"data":[],"nextCursor":null}}'
IFS= read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thread"}}}'
IFS= read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"turn":{"id":"turn"}}}'

counter=${ANNALS_FAKE_COUNTER:?}
number=0
if [ -f "$counter" ]; then
  number=$(sed -n '1p' "$counter")
fi
number=$((number + 1))
printf '%s\n' "$number" > "$counter"
if [ "$number" -eq 1 ] && [ -n "${ANNALS_FAKE_BLOCK_READY:-}" ]; then
  release=${ANNALS_FAKE_BLOCK_RELEASE:?}
  printf '%s\n' ready > "$ANNALS_FAKE_BLOCK_READY"
  while [ ! -f "$release" ]; do
    sleep 0.01
  done
fi
printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":20,\"method\":\"item/tool/call\",\"params\":{\"threadId\":\"thread\",\"turnId\":\"turn\",\"callId\":\"call\",\"namespace\":null,\"tool\":\"submit_reconciliation\",\"arguments\":{\"summary\":\"Integrate inbox source $number\",\"operations\":[{\"action\":\"create_concept\",\"ref\":\"inbox_item\",\"label\":\"Inbox concept $number\",\"parents\":[],\"evidence\":[{\"quote\":\"Shared inbox claim.\"}]}]}}}"
IFS= read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"thread","turn":{"id":"turn","status":"completed"}}}'
"#,
    )?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)?;
    Ok(())
}
