#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(database: &Path, args: &[&str], input: Option<&[u8]>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_bazaar"));
    command
        .env("CHANCERY_USAGE_DISABLED", "1")
        .arg("--database")
        .arg(database)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    if let Some(input) = input {
        child.stdin.take().unwrap().write_all(input).unwrap();
    } else {
        drop(child.stdin.take());
    }
    child.wait_with_output().unwrap()
}

fn success(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["schema_version"], 1);
    assert_eq!(result["ok"], true);
    result["data"].clone()
}

#[test]
fn cli_reads_and_appends_exact_argument_file_and_stdin_content() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("private/bazaar.sqlite3");
    success(&run(&database, &["init"], None));
    let first = success(&run(
        &database,
        &["update", "step", "inline {{value}}"],
        None,
    ));
    assert_eq!(first["version"], 1);
    let text = "  Unicode 雪\r\n\0end\n";
    let second = success(&run(
        &database,
        &["update", "step", "--stdin"],
        Some(text.as_bytes()),
    ));
    assert_eq!(second["content"], text);
    let file = directory.path().join("content.txt");
    std::fs::write(&file, "file\n\n").unwrap();
    let third = success(&run(
        &database,
        &["update", "step", "--file", file.to_str().unwrap()],
        None,
    ));
    assert_eq!(third["content"], "file\n\n");
    assert_eq!(success(&run(&database, &["get", "step"], None)), third);
    assert_eq!(
        success(&run(&database, &["get", "step", "--version", "1"], None)),
        first
    );
    assert_eq!(
        success(&run(&database, &["history", "step"], None))["versions"],
        serde_json::json!([3, 2, 1])
    );
    assert_eq!(
        success(&run(&database, &["update", "empty", ""], None))["content"],
        ""
    );
    success(&run(&database, &["doctor"], None));
}

#[test]
fn invalid_input_does_not_create_or_append_state() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("private/bazaar.sqlite3");
    assert!(!run(&database, &["get", "step"], None).status.success());
    assert!(!database.exists());
    success(&run(&database, &["init"], None));
    for args in [
        vec!["update", "step"],
        vec!["update", "step", "text", "--stdin"],
        vec!["get", "step", "--version", "0"],
        vec!["history", "unknown"],
    ] {
        assert!(!run(&database, &args, None).status.success());
    }
    assert!(
        !run(&database, &["update", "step", "--stdin"], Some(&[0xff]))
            .status
            .success()
    );
    let first = success(&run(&database, &["update", "step", "valid"], None));
    assert_eq!(first["version"], 1);
}
