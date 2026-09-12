use std::error::Error;
use std::process::{Command, Output};

use chancery_usage::{Filter, Store};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn cli_records_valid_dispatches_without_arguments_and_preserves_product_errors() -> TestResult {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("usage.sqlite3");
    let registry = directory.path().join("providers");
    std::fs::create_dir(&registry)?;
    let run = |arguments: &[&str], thread: Option<&str>| -> std::io::Result<Output> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_chancery"));
        command
            .env("CHANCERY_USAGE_DB", &database)
            .env_remove("CHANCERY_USAGE_DISABLED")
            .env_remove("CODEX_THREAD_ID")
            .env("CHANCERY_REGISTRY", &registry)
            .args(arguments);
        if let Some(thread) = thread {
            command.env("CODEX_THREAD_ID", thread);
        }
        command.output()
    };
    assert!(run(&["--register-usage"], None)?.status.success());
    assert!(run(&["--help"], Some("thread-a"))?.status.success());
    assert!(run(&["--version"], Some("thread-a"))?.status.success());
    assert!(!run(&["show"], Some("thread-a"))?.status.success());
    assert!(
        !run(&["show", "missing.capability"], Some("thread-a"))?
            .status
            .success()
    );
    assert!(run(&["list"], None)?.status.success());
    let store = Store::read(&database)?;
    let events = store.events(&Filter::default(), 0, 100)?.items;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].command_id, "show");
    assert_eq!(events[0].codex_thread_id.as_deref(), Some("thread-a"));
    assert_eq!(events[1].codex_thread_id, None);
    assert!(
        store
            .counts(&Filter::default())?
            .iter()
            .any(|c| c.command_id == "resolve" && c.invocations == 0)
    );
    let report = run(
        &["--json", "usage", "commands", "--thread", "thread-a"],
        None,
    )?;
    assert!(report.status.success());
    let value: serde_json::Value = serde_json::from_slice(&report.stdout)?;
    assert_eq!(value["data"]["scope"]["thread"], "thread-a");
    Ok(())
}

#[test]
fn missing_journal_does_not_break_discovery_or_create_state() -> TestResult {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("missing.sqlite3");
    let registry = directory.path().join("providers");
    std::fs::create_dir(&registry)?;
    let output = Command::new(env!("CARGO_BIN_EXE_chancery"))
        .env("CHANCERY_USAGE_DB", &database)
        .env_remove("CHANCERY_USAGE_DISABLED")
        .args(["--json", "list"])
        .arg("--registry")
        .arg(&registry)
        .output()?;
    assert!(output.status.success());
    assert!(!database.exists());
    assert!(String::from_utf8(output.stderr)?.contains("chancery usage:"));
    let _: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(())
}
