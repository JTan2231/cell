use std::error::Error;
use std::fs;
use std::io;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

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

fn output_with_stdin(mut command: Command, input: &[u8]) -> io::Result<Output> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("child standard input was not piped"))?
        .write_all(input)?;
    child.wait_with_output()
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
    assert_eq!(document["data"]["value"]["schema_version"], 2);
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
    assert!(context_path.exists(), "borrowed context file was removed");
    Ok(())
}

#[test]
fn integration_open_accepts_exact_stdin_and_replays_by_request_key() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("private").join("pratica.db");
    assert!(init(&database)?.status.success());
    let context = "# Piped context\r\n\r\nUnicode: café 顧客  \r\n";

    let open = || {
        let mut command = pratica();
        command.args(["--database"]).arg(&database).args([
            "--json",
            "integration",
            "open",
            "--entrant",
            "stdin-caller",
            "--title",
            "Scratchless intake",
            "--context",
            "-",
            "--request-key",
            "tests:stdin-integration:1",
        ]);
        output_with_stdin(command, context.as_bytes())
    };
    let first = open()?;
    assert!(
        first.status.success(),
        "first stdin open stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_document = parse_json(&first.stdout)?;
    let first_id = required_string(&first_document, "/data/value/integration_id")?;

    let replay = open()?;
    assert!(
        replay.status.success(),
        "stdin replay stderr: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let replay_document = parse_json(&replay.stdout)?;
    assert_eq!(
        required_string(&replay_document, "/data/value/integration_id")?,
        first_id
    );

    let listed = pratica()
        .args(["--database"])
        .arg(&database)
        .args(["--json", "integration", "list"])
        .output()?;
    assert!(listed.status.success());
    let listed_document = parse_json(&listed.stdout)?;
    let items = listed_document["data"]["value"]
        .as_array()
        .ok_or_else(|| io::Error::other("integration list is not an array"))?;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["integration_id"], first_id);
    assert!(items[0].get("context_markdown").is_none());
    assert_eq!(items[0]["context_bytes"], context.len());

    let agreements = pratica()
        .args(["--database"])
        .arg(&database)
        .args(["--json", "agreement", "list"])
        .output()?;
    assert!(agreements.status.success());
    assert_eq!(parse_json(&agreements.stdout)?["data"]["value"], json!([]));

    let mut missing_key = pratica();
    missing_key.args(["--database"]).arg(&database).args([
        "--json",
        "integration",
        "open",
        "--entrant",
        "stdin-caller",
        "--title",
        "Missing key",
        "--context",
        "-",
    ]);
    let missing_key = output_with_stdin(missing_key, context.as_bytes())?;
    assert_eq!(missing_key.status.code(), Some(2));
    assert_eq!(
        parse_json(&missing_key.stderr)?["error"]["code"],
        "request_key_required"
    );
    Ok(())
}

#[test]
fn stdin_steward_manifest_uses_explicit_root_and_redacts_stored_sources() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("private").join("pratica.db");
    assert!(init(&database)?.status.success());
    let source_marker = "PRIVATE-SOURCE-BODY-DO-NOT-PRINT";
    let charter_marker = "PRIVATE-CHARTER-BODY-DO-NOT-PRINT";
    let source = temporary.path().join("contract.md");
    fs::write(&source, format!("{source_marker}\n"))?;
    let manifest_text = format!(
        r#"schema_version = 1
scope = "scratchless-intake"
version = 1
party = "scratchless-steward"
title = "Scratchless intake"
charter_markdown = "{charter_marker}"

[[sources]]
id = "contract"
kind = "contract"
path = "contract.md"
revision = "v1"
"#
    );

    let mut register = pratica();
    register
        .args(["--database"])
        .arg(&database)
        .args(["--json", "steward", "register", "-", "--source-root"])
        .arg(temporary.path());
    let registered = output_with_stdin(register, manifest_text.as_bytes())?;
    assert!(
        registered.status.success(),
        "stdin register stderr: {}",
        String::from_utf8_lossy(&registered.stderr)
    );
    let registered_text = String::from_utf8(registered.stdout)?;
    assert!(!registered_text.contains(source_marker));
    assert!(!registered_text.contains(charter_marker));
    assert!(!registered_text.contains(temporary.path().to_string_lossy().as_ref()));
    let registered_document: Value = serde_json::from_str(&registered_text)?;
    assert_eq!(
        registered_document["data"]["value"]["basis"]["source_count"],
        1
    );
    assert_eq!(
        registered_document["data"]["value"]["basis"]["sources"][0]["locator"],
        "contract.md"
    );

    for arguments in [
        vec!["--json", "steward", "list"],
        vec!["--json", "steward", "show", "scratchless-intake"],
    ] {
        let output = pratica()
            .args(["--database"])
            .arg(&database)
            .args(arguments)
            .output()?;
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout)?;
        assert!(!text.contains(source_marker));
        assert!(!text.contains(charter_marker));
        assert!(!text.contains(temporary.path().to_string_lossy().as_ref()));
    }
    assert_eq!(fs::read_to_string(&source)?, format!("{source_marker}\n"));

    let manifest_path = temporary.path().join("steward.toml");
    fs::write(&manifest_path, &manifest_text)?;
    let borrowed = pratica()
        .args(["--database"])
        .arg(&database)
        .args(["--json", "steward", "register"])
        .arg(&manifest_path)
        .output()?;
    assert!(borrowed.status.success());
    assert!(manifest_path.exists(), "borrowed manifest was removed");

    let mut missing_root = pratica();
    missing_root
        .args(["--database"])
        .arg(&database)
        .args(["--json", "steward", "register", "-"]);
    let missing_root = output_with_stdin(missing_root, manifest_text.as_bytes())?;
    assert_eq!(missing_root.status.code(), Some(2));
    assert_eq!(
        parse_json(&missing_root.stderr)?["error"]["code"],
        "source_root_required"
    );
    Ok(())
}
