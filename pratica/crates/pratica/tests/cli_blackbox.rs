use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn Error>>;

fn pratica() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pratica"))
}

fn init(database: &Path) -> io::Result<Output> {
    pratica()
        .args(["--database"])
        .arg(database)
        .args(["--json", "init"])
        .output()
}

fn parse_json(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    serde_json::from_slice(bytes)
}

fn required_string<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, io::Error> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("missing JSON string at {pointer}")))
}

#[test]
fn init_emits_success_envelope_and_creates_private_storage() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database_parent = temporary.path().join("new-private-parent");
    let database = database_parent.join("pratica.db");

    let output = init(&database)?;

    assert!(
        output.status.success(),
        "init stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let document = parse_json(&output.stdout)?;
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["ok"], true);
    assert_eq!(document["data"]["type"], "initialized");
    assert_eq!(document["data"]["value"]["schema_version"], 1);
    assert_eq!(
        document["data"]["value"]["path"],
        database.to_string_lossy().as_ref()
    );
    assert_eq!(fs::metadata(&database)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        fs::metadata(&database_parent)?.permissions().mode() & 0o777,
        0o700
    );
    Ok(())
}

#[test]
fn second_init_is_a_deterministic_json_error_without_success_output() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("private").join("pratica.db");
    let first = init(&database)?;
    assert!(
        first.status.success(),
        "first init stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let second = init(&database)?;

    assert_eq!(second.status.code(), Some(1));
    assert!(second.stdout.is_empty());
    assert_eq!(
        parse_json(&second.stderr)?,
        json!({
            "schema_version": 1,
            "ok": false,
            "error": {
                "code": "conflict",
                "message": format!("database already exists at {}", database.display()),
            }
        })
    );
    Ok(())
}

#[test]
fn relative_database_path_is_rejected_without_creating_state() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let relative_database = Path::new("relative-pratica.db");
    let output = pratica()
        .current_dir(temporary.path())
        .args(["--database"])
        .arg(relative_database)
        .args(["--json", "init"])
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        parse_json(&output.stderr)?,
        json!({
            "schema_version": 1,
            "ok": false,
            "error": {
                "code": "database_path_relative",
                "message": "Pratica database paths must be absolute",
            }
        })
    );
    assert!(!temporary.path().join(relative_database).exists());
    Ok(())
}

#[test]
fn integration_open_and_status_preserve_exact_context_markdown() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("private").join("pratica.db");
    let context_path = temporary.path().join("context.md");
    let context = "# CRM integration\r\n\r\nEntrant: café ☕\r\nBoundary: 顧客データ\r\n";
    fs::write(&context_path, context.as_bytes())?;
    let initialization = init(&database)?;
    assert!(
        initialization.status.success(),
        "init stderr: {}",
        String::from_utf8_lossy(&initialization.stderr)
    );

    let opened = pratica()
        .args(["--database"])
        .arg(&database)
        .args([
            "--json",
            "integration",
            "open",
            "--entrant",
            "crm-design",
            "--title",
            "CRM contract design",
            "--context",
        ])
        .arg(&context_path)
        .output()?;
    assert!(
        opened.status.success(),
        "open stderr: {}",
        String::from_utf8_lossy(&opened.stderr)
    );
    assert!(opened.stderr.is_empty());
    let opened_document = parse_json(&opened.stdout)?;
    assert_eq!(opened_document["schema_version"], 1);
    assert_eq!(opened_document["ok"], true);
    assert_eq!(opened_document["data"]["type"], "integration_opened");
    assert_eq!(
        required_string(&opened_document, "/data/value/context_markdown")?,
        context
    );
    let integration_id = required_string(&opened_document, "/data/value/integration_id")?;

    let status = pratica()
        .args(["--database"])
        .arg(&database)
        .args(["--json", "integration", "status", integration_id])
        .output()?;
    assert!(
        status.status.success(),
        "status stderr: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert!(status.stderr.is_empty());
    let status_document = parse_json(&status.stdout)?;
    assert_eq!(status_document["schema_version"], 1);
    assert_eq!(status_document["ok"], true);
    assert_eq!(status_document["data"]["type"], "integration_status");
    assert_eq!(
        required_string(&status_document, "/data/value/integration/context_markdown")?,
        context
    );
    assert_eq!(
        status_document["data"]["value"]["integration"]["context_sha256"],
        opened_document["data"]["value"]["context_sha256"]
    );
    assert_eq!(status_document["data"]["value"]["tracks"], json!([]));
    assert_eq!(status_document["data"]["value"]["ready"], false);
    Ok(())
}
