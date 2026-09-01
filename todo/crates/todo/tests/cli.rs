use std::fs;
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
    assert_eq!(
        value["data"]["concerns"][0]["source_path"],
        "/tmp/origin.md"
    );
    assert_eq!(value["data"]["direction"], "Need the actionable work");
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

#[test]
fn v2_command_tree_keeps_proposals_separate_from_authorization() -> TestResult {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("todo.db");

    for args in [
        ["concern", "add", "--help"].as_slice(),
        ["concern", "assess", "--help"].as_slice(),
        ["routing", "accept", "--help"].as_slice(),
        ["assess", "--help"].as_slice(),
        ["situation", "show", "--help"].as_slice(),
        ["design", "propose", "--help"].as_slice(),
        ["design", "correct", "--help"].as_slice(),
        ["design", "accept", "--help"].as_slice(),
        ["migrate", "--help"].as_slice(),
    ] {
        let output = run(&database, args)?;
        assert!(
            output.status.success(),
            "help failed for {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let new_help = run(&database, &["new", "--help"])?;
    let help = String::from_utf8(new_help.stdout)?;
    assert!(help.contains("pending routing proposal"));

    for args in [
        ["--json", "routing", "accept", "r1"].as_slice(),
        [
            "--json",
            "routing",
            "reject",
            "r1",
            "--source",
            "/tmp/decision.jsonl",
        ]
        .as_slice(),
        ["--json", "design", "accept", "d1"].as_slice(),
        ["--json", "migrate"].as_slice(),
    ] {
        let output = run(&database, args)?;
        assert_eq!(output.status.code(), Some(2), "accepted {args:?}");
        assert_eq!(stderr_json(&output)?["error"]["code"], "invalid_command");
    }
    Ok(())
}

#[test]
fn email_preview_is_offline_and_send_requires_the_resend_key() -> TestResult {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("todo.db");
    let config = directory.path().join("todo.toml");
    assert!(run(&database, &["init"])?.status.success());
    seed_todos(&database)?;
    seed_pending_concern(&database)?;
    fs::write(
        &config,
        concat!(
            "[email]\n",
            "from = \"Todo <todo@example.com>\"\n",
            "to = \"person@example.com\"\n",
        ),
    )?;
    let config = config.to_str().ok_or("config path is not UTF-8")?;

    let preview = run(
        &database,
        &["--config", config, "--json", "email", "preview"],
    )?;
    assert!(preview.status.success());
    let value = stdout_json(&preview)?;
    assert_eq!(value["data"]["from"], "Todo <todo@example.com>");
    assert_eq!(value["data"]["to"], "person@example.com");
    assert_eq!(value["data"]["todo_count"], 1);
    assert_eq!(value["data"]["pending_concern_count"], 1);
    assert_eq!(value["data"]["attention_count"], 2);
    assert_eq!(
        value["data"]["subject"],
        "Todo daily: 2 need attention · 1 open todo"
    );
    assert!(value["data"]["text"].as_str().is_some_and(|text| {
        text.contains("Research usage reporting")
            && text.contains("Reference: Todo t1")
            && text.contains("Captured concern")
            && text.contains("Reference: Concern c3")
            && !text.contains("private concern body")
    }));
    assert!(value["data"]["html"].as_str().is_some_and(|html| {
        html.contains("<strong>Research usage reporting</strong>")
            && !html.contains("<strong>t1</strong>")
            && !html.contains("Determine how usage")
            && !html.contains("private concern body")
    }));

    let send = run_without_resend_key(
        &database,
        &["--config", config, "--json", "email", "send", "--scheduled"],
    )?;
    assert_eq!(send.status.code(), Some(2));
    assert_eq!(
        stderr_json(&send)?["error"]["code"],
        "resend_api_key_not_configured"
    );
    Ok(())
}

fn run(database: &Path, args: &[&str]) -> Result<Output, std::io::Error> {
    Command::new(env!("CARGO_BIN_EXE_todo"))
        .arg("--database")
        .arg(database)
        .args(args)
        .output()
}

fn run_without_resend_key(database: &Path, args: &[&str]) -> Result<Output, std::io::Error> {
    Command::new(env!("CARGO_BIN_EXE_todo"))
        .arg("--database")
        .arg(database)
        .args(args)
        .env_remove("RESEND_API_KEY")
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
    let mut connection = Connection::open(database)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    let transaction = connection.transaction()?;
    for (id, title, summary, direction, source, status, completed_at) in [
        (
            1,
            "Research usage reporting",
            "Determine how usage should be reported.",
            "Need the actionable work",
            "/tmp/origin.md",
            "open",
            None,
        ),
        (
            2,
            "Completed example",
            "Already complete.",
            "Historical context",
            "/tmp/older.md",
            "done",
            Some("2026-08-25T12:00:00.000Z"),
        ),
    ] {
        transaction.execute(
            "INSERT INTO todos(id, status, completed_at) VALUES(?1, ?2, ?3)",
            params![id, status, completed_at],
        )?;
        transaction.execute(
            "INSERT INTO concerns(id, body, source_path, status, resolved_at)
             VALUES(?1, ?2, ?3, 'attached', strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            params![id, direction, source],
        )?;
        transaction.execute(
            "INSERT INTO todo_direction_revisions(
                 id, todo_id, revision, title, body, source_concern_id, provenance_kind
             ) VALUES(?1, ?1, 1, ?2, ?3, ?1, 'legacy_v1')",
            params![id, title, direction],
        )?;
        transaction.execute(
            "INSERT INTO todo_concerns(id, todo_id, concern_id)
             VALUES(?1, ?1, ?1)",
            [id],
        )?;
        transaction.execute(
            "INSERT INTO todo_designs(
                 id, todo_id, revision, draft_version, state, summary
             ) VALUES(?1, ?1, 1, 1, 'legacy_unreviewed', ?2)",
            params![id, summary],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn seed_pending_concern(database: &Path) -> TestResult {
    let connection = Connection::open(database)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.execute(
        "INSERT INTO concerns(id, body, source_path) VALUES(3, ?1, ?2)",
        params!["private concern body", "/tmp/private-concern.md"],
    )?;
    Ok(())
}
