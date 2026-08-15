use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::SystemTime;

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
    fn new(max_items: usize, settle_seconds: u64) -> TestResult<Self> {
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
                    "max_items = {max_items}\n",
                    "max_elapsed_seconds = 300\n",
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
                max_items = max_items,
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
    let installation = Installation::new(2, 0)?;
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
fn run_honors_settling_and_max_items_and_moves_sources_unchanged() -> TestResult {
    let installation = Installation::new(2, 3_600)?;
    installation.init()?;
    let expected = BTreeMap::from([
        (
            "01-one.md".to_owned(),
            installation.incoming("01-one.md", b"Shared inbox claim.\nFirst source.\n", 0o640)?,
        ),
        (
            "02-two.txt".to_owned(),
            installation.incoming(
                "02-two.txt",
                b"Shared inbox claim.\nSecond source.\n",
                0o600,
            )?,
        ),
        (
            "job.json".to_owned(),
            installation.incoming("job.json", b"Shared inbox claim.\nThird source.\n", 0o644)?,
        ),
    ]);

    installation.json_ok(["inbox", "run"])?;
    assert!(!installation.counter.exists());
    assert_eq!(
        incoming_names(&installation.inbox)?,
        ["01-one.md", "02-two.txt", "job.json"]
    );
    assert!(archived_material(&installation.inbox, "done")?.is_empty());

    installation.json_ok(["inbox", "run", "--settle-seconds", "0", "--max-items", "2"])?;
    assert_eq!(incoming_names(&installation.inbox)?, ["job.json"]);
    let done = archived_material(&installation.inbox, "done")?;
    assert_eq!(
        done.keys().map(String::as_str).collect::<Vec<_>>(),
        ["01-one.md", "02-two.txt"]
    );
    for (name, actual) in &done {
        assert_unchanged(
            expected.get(name).ok_or("unexpected archived source")?,
            actual,
        );
    }

    let status = installation.json_ok(["inbox", "status"])?;
    assert_eq!(status_count(&status, "incoming"), Some(1), "{status}");
    assert_eq!(status_count(&status, "done"), Some(2), "{status}");

    installation.json_ok(["inbox", "run", "--settle-seconds", "0"])?;
    assert!(incoming_names(&installation.inbox)?.is_empty());
    let done = archived_material(&installation.inbox, "done")?;
    assert_eq!(done.len(), 3);
    for (name, actual) in &done {
        assert_unchanged(
            expected.get(name).ok_or("unexpected archived source")?,
            actual,
        );
    }
    assert!(archived_material(&installation.inbox, "processing")?.is_empty());
    assert!(archived_material(&installation.inbox, "failed")?.is_empty());

    let library_stats = installation.json_ok(["stats"])?;
    assert_eq!(library_stats["work_count"], 3);
    assert_eq!(library_stats["revision"], 3);
    Ok(())
}

#[test]
fn permanent_input_failures_are_archived_unchanged_and_do_not_stop_the_batch() -> TestResult {
    let installation = Installation::new(3, 0)?;
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
    Ok(())
}

#[test]
fn an_exclusive_spool_lock_prevents_overlapping_runs() -> TestResult {
    let installation = Installation::new(1, 0)?;
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
    let installation = Installation::new(2, 0)?;
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
    Ok(())
}

#[test]
fn startup_repairs_pre_receipt_claim_crashes() -> TestResult {
    let installation = Installation::new(2, 0)?;
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
    let installation = Installation::new(1, 0)?;
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
    assert_eq!(first["items"][0]["revision"], 1);
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
    assert_eq!(second["items"][0]["revision"], 2);
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
