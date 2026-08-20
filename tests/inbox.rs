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
fn fresh_jobs_do_not_adopt_unrelated_reconciliations_for_the_same_work() -> TestResult {
    let installation = Installation::new(0)?;
    installation.init()?;
    let bytes = b"Shared inbox claim.\nHuman pending proposal.\n";
    let retained_source = installation.directory.path().join("linked.txt");
    fs::write(&retained_source, bytes)?;
    let added = installation.json_ok([
        OsStr::new("work"),
        OsStr::new("add"),
        retained_source.as_os_str(),
    ])?;
    assert_eq!(added["work"], "linked");

    let request = installation.directory.path().join("human-change.json");
    fs::write(
        &request,
        r#"{
  "summary": "Unrelated human proposal",
  "operations": [{
    "action": "create_concept",
    "ref": "human_pending",
    "label": "Human pending concept",
    "parents": [],
    "evidence": [{"quote": "Human pending proposal."}]
  }]
}
"#,
    )?;
    let submitted = installation.json_ok([
        OsStr::new("change"),
        OsStr::new("submit"),
        request.as_os_str(),
        OsStr::new("--work"),
        OsStr::new("linked"),
        OsStr::new("--base"),
        OsStr::new("0"),
    ])?;
    assert_eq!(submitted["status"], "pending");

    installation.incoming("linked.txt", bytes, 0o640)?;
    let first = installation.json_ok(["inbox", "run"])?;
    assert_eq!(first["applied"], 1);
    assert_eq!(fs::read_to_string(&installation.counter)?, "1\n");

    let changes = installation.json_ok(["change", "list"])?;
    let changes = changes.as_array().ok_or("change list was not an array")?;
    let human = changes
        .iter()
        .find(|change| change["summary"] == "Unrelated human proposal")
        .ok_or("human reconciliation was not listed")?;
    assert_eq!(human["status"], "superseded");
    let first_model = changes
        .iter()
        .find(|change| change["summary"] == "Integrate inbox source 1")
        .ok_or("first inbox reconciliation was not listed")?;
    assert_eq!(first_model["status"], "applied");

    installation.incoming("linked.txt", bytes, 0o600)?;
    let second = installation.json_ok(["inbox", "run"])?;
    assert_eq!(second["applied"], 1);
    assert_eq!(fs::read_to_string(&installation.counter)?, "2\n");

    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status["done"], 2);
    let log = installation.json_ok(["log"])?;
    assert_eq!(log["head_revision"], 2);
    assert_eq!(log["commits"][0]["summary"], "Integrate inbox source 2");
    assert_eq!(log["commits"][1]["summary"], "Integrate inbox source 1");
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
