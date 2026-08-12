use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

struct Library {
    directory: TempDir,
    path: PathBuf,
}

impl Library {
    fn initialized() -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let library = Self { directory, path };
        library.json_ok(["init"])?;
        Ok(library)
    }

    fn json_ok<const N: usize>(&self, arguments: [&str; N]) -> TestResult<Value> {
        successful_json(&command(&self.path).args(arguments).output()?)
    }

    fn create(&self, text: &str) -> TestResult<i64> {
        self.json_ok(["tree", "create", "--text", text])?["node_ids"][0]
            .as_i64()
            .ok_or_else(|| io::Error::other("tree create omitted ID").into())
    }
}

fn command(path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_annals"));
    command.arg("--library").arg(path).arg("--json");
    command
}

fn successful_json(output: &Output) -> TestResult<Value> {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = serde_json::from_slice::<Value>(&output.stdout)?;
    assert_eq!(envelope["ok"], true);
    Ok(envelope["data"].clone())
}

fn error_json(output: &Output, code: &str) -> TestResult<Value> {
    assert!(!output.status.success());
    let envelope = serde_json::from_slice::<Value>(&output.stderr)?;
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], code);
    Ok(envelope)
}

#[test]
fn backup_is_consistent_and_never_overwrites() -> TestResult {
    let library = Library::initialized()?;
    library.create("Backed up concept")?;
    let backup = library.directory.path().join("backup.db");
    successful_json(&command(&library.path).arg("backup").arg(&backup).output()?)?;
    assert_eq!(
        successful_json(&command(&backup).arg("stats").output()?)?["node_count"],
        1
    );
    let second = command(&library.path).arg("backup").arg(&backup).output()?;
    error_json(&second, "backup_exists")?;
    Ok(())
}

#[test]
fn validation_detects_canonical_and_index_corruption() -> TestResult {
    let library = Library::initialized()?;
    let node = library.create("Canonical string")?;
    let connection = Connection::open(&library.path)?;
    connection.execute(
        "UPDATE search_units SET text = 'corrupt' WHERE node_id = ?1",
        [node],
    )?;
    drop(connection);

    let output = command(&library.path).arg("validate").output()?;
    let error = error_json(&output, "validation_failed")?;
    assert!(
        error["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("index_unit_mismatch"))
    );
    library.json_ok(["reindex"])?;
    assert_eq!(library.json_ok(["validate"])?["valid"], true);
    Ok(())
}

#[test]
fn stale_index_blocks_mutation_until_reindex() -> TestResult {
    let library = Library::initialized()?;
    library.create("First")?;
    let connection = Connection::open(&library.path)?;
    connection.execute(
        "UPDATE index_metadata SET value = '1' WHERE key = 'indexer_version'",
        [],
    )?;
    drop(connection);
    let output = command(&library.path)
        .args(["tree", "create", "--text", "Blocked"])
        .output()?;
    error_json(&output, "reindex_required")?;
    library.json_ok(["reindex"])?;
    library.create("Allowed")?;
    Ok(())
}

#[test]
fn generated_provenance_foreign_keys_are_enforced_by_schema() -> TestResult {
    let library = Library::initialized()?;
    let connection = Connection::open(&library.path)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    let result = connection.execute(
        "INSERT INTO node_support(node_id, run_id, unit_id) VALUES(99, 99, 'u000000')",
        [],
    );
    assert!(result.is_err());
    Ok(())
}
