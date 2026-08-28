use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use rusqlite::{Connection, params};
use serde_json::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn init_and_json_errors_follow_the_cli_contract() -> TestResult {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("todo.db");

    let initialized = run(&database, &["--json", "init"])?;
    assert!(initialized.status.success());
    let value = stdout_json(&initialized)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["data"]["database"], database.display().to_string());

    let duplicate = run(&database, &["--json", "init"])?;
    assert_eq!(duplicate.status.code(), Some(4));
    let value = stderr_json(&duplicate)?;
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "database_exists");
    Ok(())
}

#[test]
fn list_search_show_notes_and_statuses_work_end_to_end() -> TestResult {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("todo.db");
    assert!(run(&database, &["init"])?.status.success());
    seed_todos(&database)?;

    let list = run(&database, &["--json", "list"])?;
    assert!(list.status.success());
    let value = stdout_json(&list)?;
    assert_eq!(value["data"]["todos"].as_array().map(Vec::len), Some(1));
    assert_eq!(value["data"]["todos"][0]["id"], "t1");

    let all = run(&database, &["--json", "list", "--all"])?;
    assert_eq!(
        stdout_json(&all)?["data"]["todos"].as_array().map(Vec::len),
        Some(2)
    );

    let note_one = run(&database, &["note", "add", "t1", "First observation"])?;
    assert!(note_one.status.success());
    let note_two = run_with_stdin(
        &database,
        &["note", "add", "t1", "-"],
        "Second observation from stdin",
    )?;
    assert!(note_two.status.success());

    let search = run(&database, &["--json", "search", "FROM STDIN", "--all"])?;
    assert_eq!(stdout_json(&search)?["data"]["todos"][0]["id"], "t1");

    let shown = run(&database, &["--json", "show", "t1"])?;
    let value = stdout_json(&shown)?;
    assert_eq!(value["data"]["source_path"], "/tmp/origin.md");
    assert_eq!(value["data"]["pointer"], "Need the actionable work");
    assert_eq!(
        value["data"]["working_notes"][0]["text"],
        "First observation"
    );
    assert_eq!(
        value["data"]["working_notes"][1]["text"],
        "Second observation from stdin"
    );

    let done = run(&database, &["--json", "done", "t1"])?;
    assert_eq!(stdout_json(&done)?["data"]["changed"], true);
    let done_again = run(&database, &["--json", "done", "t1"])?;
    assert_eq!(stdout_json(&done_again)?["data"]["changed"], false);
    assert_eq!(
        stdout_json(&run(&database, &["--json", "list"])?)?["data"]["todos"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );

    let reopened = run(&database, &["--json", "reopen", "t1"])?;
    assert_eq!(stdout_json(&reopened)?["data"]["todo"]["status"], "open");
    Ok(())
}

#[test]
fn missing_todos_and_invalid_limits_have_stable_errors() -> TestResult {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("todo.db");
    assert!(run(&database, &["init"])?.status.success());

    let missing = run(&database, &["--json", "show", "t99"])?;
    assert_eq!(missing.status.code(), Some(3));
    assert_eq!(stderr_json(&missing)?["error"]["code"], "todo_not_found");

    let invalid = run(&database, &["--json", "list", "--limit", "0"])?;
    assert_eq!(invalid.status.code(), Some(2));
    assert_eq!(stderr_json(&invalid)?["error"]["code"], "invalid_limit");
    Ok(())
}

fn run(database: &Path, args: &[&str]) -> Result<Output, std::io::Error> {
    Command::new(env!("CARGO_BIN_EXE_todo"))
        .arg("--database")
        .arg(database)
        .args(args)
        .output()
}

fn run_with_stdin(database: &Path, args: &[&str], input: &str) -> Result<Output, std::io::Error> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_todo"))
        .arg("--database")
        .arg(database)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = &mut child.stdin {
        stdin.write_all(input.as_bytes())?;
    }
    child.wait_with_output()
}

fn stdout_json(output: &Output) -> Result<Value, serde_json::Error> {
    serde_json::from_slice(&output.stdout)
}

fn stderr_json(output: &Output) -> Result<Value, serde_json::Error> {
    serde_json::from_slice(&output.stderr)
}

fn seed_todos(database: &Path) -> TestResult {
    let connection = Connection::open(database)?;
    connection.execute(
        "INSERT INTO todos(title, note, pointer, source_path)
         VALUES(?1, ?2, ?3, ?4)",
        params![
            "Research usage reporting",
            "Determine how usage should be reported.",
            "Need the actionable work",
            "/tmp/origin.md"
        ],
    )?;
    connection.execute(
        "INSERT INTO todos(
             title, note, pointer, source_path, status, completed_at
         ) VALUES(?1, ?2, ?3, ?4, 'done', ?5)",
        params![
            "Completed example",
            "Already complete.",
            "Historical context",
            "/tmp/older.md",
            "2026-08-25T12:00:00.000Z"
        ],
    )?;
    Ok(())
}
