use std::error::Error;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
        successful_json(&command(&library.path).arg("init").output()?)?;
        Ok(library)
    }

    fn input(&self, text: &str) -> TestResult<PathBuf> {
        let path = self.directory.path().join("input.txt");
        fs::write(&path, text)?;
        Ok(path)
    }

    fn fake_codex(&self, response: &str, exit_status: Option<i32>) -> TestResult<PathBuf> {
        let directory = self.directory.path().join(format!(
            "fake-codex-{}",
            self.directory.path().read_dir()?.count()
        ));
        fs::create_dir(&directory)?;
        let path = directory.join("codex");
        let script = exit_status.map_or_else(
            || {
                format!(
                    "#!/bin/sh\nset -eu\ncat >/dev/null\nprintf '%s' '{response}'\n"
                )
            },
            |status| {
                format!(
                    "#!/bin/sh\nset -eu\ncat >/dev/null\nprintf '%s' '{response}' >&2\nexit {status}\n"
                )
            },
        );
        fs::write(&path, script)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        Ok(path)
    }

    fn ingest(&self, input: &Path, fake_codex: &Path) -> TestResult<std::process::Output> {
        let existing_path = std::env::var_os("PATH").unwrap_or_default();
        let fake_directory = fake_codex.parent().ok_or("fake codex has no parent")?;
        let search_path = std::env::join_paths(
            std::iter::once(fake_directory.to_path_buf())
                .chain(std::env::split_paths(&existing_path)),
        )?;
        Ok(command(&self.path)
            .env("PATH", search_path)
            .arg("ingest")
            .arg(input)
            .output()?)
    }
}

fn command(path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_annals"));
    command.arg("--library").arg(path).arg("--json");
    command
}

fn successful_json(output: &std::process::Output) -> TestResult<Value> {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = serde_json::from_slice::<Value>(&output.stdout)?;
    assert_eq!(envelope["ok"], true);
    Ok(envelope["data"].clone())
}

fn error_code(output: &std::process::Output, code: &str) -> TestResult {
    assert!(!output.status.success());
    let envelope = serde_json::from_slice::<Value>(&output.stderr)?;
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], code);
    Ok(())
}

#[test]
fn stdin_rejects_non_utf8_before_model_invocation() -> TestResult {
    let library = Library::initialized()?;
    let mut child = command(&library.path)
        .arg("ingest")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let Some(mut stdin) = child.stdin.take() else {
        return Err("ingest stdin was not piped".into());
    };
    stdin.write_all(&[0xff])?;
    drop(stdin);
    error_code(&child.wait_with_output()?, "ingestion_not_utf8")
}

#[test]
fn invalid_limits_fail_before_model_invocation() -> TestResult {
    let library = Library::initialized()?;
    let input = library.input("evidence")?;
    let output = command(&library.path)
        .args([
            "ingest",
            input.to_str().ok_or("non-UTF-8 temp path")?,
            "--node-budget",
            "0",
        ])
        .output()?;
    error_code(&output, "invalid_ingestion")
}

#[test]
fn model_failure_does_not_open_a_write_transaction() -> TestResult {
    let library = Library::initialized()?;
    let input = library.input("evidence")?;
    let runner = library.fake_codex("model unavailable", Some(7))?;
    let output = library.ingest(&input, &runner)?;
    error_code(&output, "model_runner_failed")?;
    let connection = Connection::open(&library.path)?;
    assert_eq!(
        connection.query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get::<_, i64>(0))?,
        0
    );
    Ok(())
}

#[test]
fn structurally_invalid_model_output_rolls_back_everything() -> TestResult {
    let library = Library::initialized()?;
    let input = library.input("evidence")?;
    let runner = library.fake_codex(
        r#"{"schema_version":1,"nodes":[{"id":"n0","parent_id":null,"text":"Root","support_unit_ids":[]},{"id":"n1","parent_id":"n0","text":"Only child","support_unit_ids":["u000000"]}]}"#,
        None,
    )?;
    let output = library.ingest(&input, &runner)?;
    error_code(&output, "invalid_model_output")?;
    let connection = Connection::open(&library.path)?;
    for table in [
        "nodes",
        "raw_inputs",
        "generation_runs",
        "input_units",
        "node_support",
    ] {
        let count = connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get::<_, i64>(0)
        })?;
        assert_eq!(count, 0, "{table} was partially written");
    }
    Ok(())
}

#[test]
fn embedded_bundle_protocol_persists_and_deletes_one_complete_generation() -> TestResult {
    let library = Library::initialized()?;
    let input = library.input("alpha evidence")?;
    let codex = library.fake_codex(
        r#"{"schema_version":1,"nodes":[{"id":"n0","parent_id":null,"text":"Alpha","support_unit_ids":["u000000"]}]}"#,
        None,
    )?;
    let output = library.ingest(&input, &codex)?;
    let data = successful_json(&output)?;
    assert_eq!(data["root_node_id"], 1);
    assert_eq!(data["node_ids"].as_array().map(Vec::len), Some(1));

    let connection = Connection::open(&library.path)?;
    assert_eq!(
        connection.query_row("SELECT COUNT(*) FROM generation_runs", [], |row| row
            .get::<_, i64>(0))?,
        1
    );
    assert_eq!(
        connection.query_row("SELECT COUNT(*) FROM node_support", [], |row| row
            .get::<_, i64>(0))?,
        1
    );
    drop(connection);
    assert_eq!(
        successful_json(&command(&library.path).arg("validate").output()?)?["valid"],
        true
    );

    let edit = command(&library.path)
        .args(["node", "edit", "1", "--text", "Changed"])
        .output()?;
    error_code(&edit, "generated_tree_immutable")?;
    successful_json(
        &command(&library.path)
            .args(["tree", "delete", "1", "--yes"])
            .output()?,
    )?;
    let connection = Connection::open(&library.path)?;
    for table in [
        "nodes",
        "raw_inputs",
        "generation_runs",
        "input_units",
        "node_support",
    ] {
        let count = connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get::<_, i64>(0)
        })?;
        assert_eq!(count, 0, "{table} survived generated-tree deletion");
    }
    Ok(())
}
