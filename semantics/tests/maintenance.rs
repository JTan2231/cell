use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use semantics::api::{CliError, Client};
use serde_json::{Value, json};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn command(database: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_semantics"));
    command
        .arg("--database")
        .arg(database)
        .arg("--json")
        .env_remove("CELL_DEPLOYMENT_RUN_ID")
        .env_remove("CLOCKWORK_ACTIVATION_ID");
    command
}

fn success(output: &Output) -> TestResult<Value> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn held(output: &Output) -> TestResult {
    assert!(!output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stderr)?;
    assert_eq!(envelope["error"]["code"], "deployment_maintenance");
    Ok(())
}

#[test]
fn maintenance_does_not_initialize_state_and_fences_public_clients() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("semantics.db");
    let gate_path = temporary.path().join("semantics.db.cell-maintenance");
    let status = success(
        &command(&database)
            .args(["maintenance", "status"])
            .output()?,
    )?;
    assert_eq!(status["holds"], json!([]));
    assert_eq!(status["drained"], true);
    assert!(!database.exists());
    assert!(!gate_path.exists());
    held(
        &command(&database)
            .args(["maintenance", "hold", "../invalid"])
            .output()?,
    )?;
    assert!(!gate_path.exists());
    success(
        &command(&database)
            .args(["maintenance", "hold", "run-a"])
            .output()?,
    )?;
    for arguments in [
        vec!["project", "list"],
        vec!["project", "pause", "fixture"],
        vec!["doctor"],
    ] {
        held(&command(&database).args(arguments).output()?)?;
    }
    let client = Client::new(env!("CARGO_BIN_EXE_semantics")).with_database(&database);
    assert!(
        matches!(client.pause_project("fixture"), Err(CliError::Rejected { code, .. })
        if code == "deployment_maintenance")
    );
    assert!(!database.exists());
    Ok(())
}

#[test]
fn scheduled_worker_skips_only_a_valid_deployment_hold_before_opening_state() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("semantics.db");
    let gate = cell_maintenance::Gate::new(temporary.path().join("semantics.db.cell-maintenance"));
    gate.hold("run-a")?;
    let skipped = success(
        &command(&database)
            .env("CLOCKWORK_ACTIVATION_ID", "fixture-activation")
            .args(["intake", "run"])
            .output()?,
    )?;
    assert_eq!(skipped["skipped"], "deployment_maintenance");
    assert!(!database.exists());
    assert!(
        !command(&database)
            .args(["intake", "run"])
            .output()?
            .status
            .success()
    );
    held(
        &command(&database)
            .env("CLOCKWORK_ACTIVATION_ID", "fixture-activation")
            .args(["project", "list"])
            .output()?,
    )?;
    fs::write(gate.path().join("holds/run-a"), "invalid-owner\n")?;
    assert!(
        !command(&database)
            .env("CLOCKWORK_ACTIVATION_ID", "fixture-activation")
            .args(["intake", "run"])
            .output()?
            .status
            .success()
    );
    assert!(!database.exists());
    Ok(())
}

#[test]
fn installation_requires_sole_owner_and_drained_activity() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("semantics.db");
    let gate = cell_maintenance::Gate::new(temporary.path().join("semantics.db.cell-maintenance"));
    let activity = gate.enter()?;
    let status = success(
        &command(&database)
            .args(["maintenance", "hold", "run-a"])
            .output()?,
    )?;
    assert_eq!(status["drained"], false);
    held(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-a")
            .args(["project", "list"])
            .output()?,
    )?;
    drop(activity);
    success(
        &command(&database)
            .args(["maintenance", "hold", "run-b"])
            .output()?,
    )?;
    held(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-a")
            .args(["project", "list"])
            .output()?,
    )?;
    let status = success(
        &command(&database)
            .args(["maintenance", "release", "run-a"])
            .output()?,
    )?;
    assert_eq!(status["holds"], json!(["run-b"]));
    held(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-a")
            .args(["project", "list"])
            .output()?,
    )?;
    held(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "../invalid")
            .args(["project", "list"])
            .output()?,
    )?;
    assert!(!database.exists());
    success(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-b")
            .args(["project", "list"])
            .output()?,
    )?;
    assert!(database.is_file());
    success(
        &command(&database)
            .args(["maintenance", "release", "run-b"])
            .output()?,
    )?;
    success(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-b")
            .args(["project", "list"])
            .output()?,
    )?;
    Ok(())
}

#[test]
fn controlled_mutation_and_release_preserve_project_pause() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("semantics.db");
    let root = temporary.path().join("project");
    fs::create_dir(&root)?;
    fs::write(root.join("AGENTS.md"), "Semantics-Project: fixture\n")?;
    success(
        &command(&database)
            .args(["maintenance", "hold", "run-a"])
            .output()?,
    )?;
    // Only the upstream watermark is substituted, using the existing isolated-test override.
    success(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-a")
            .args(["project", "register", "fixture"])
            .arg(&root)
            .args([
                "--annals-library-id",
                "0123456789abcdef0123456789abcdef",
                "--annals-activation-cursor",
                "opaque-test-watermark",
            ])
            .output()?,
    )?;
    success(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-a")
            .args(["project", "pause", "fixture"])
            .output()?,
    )?;
    success(
        &command(&database)
            .args(["maintenance", "release", "run-a"])
            .output()?,
    )?;
    let project = success(
        &command(&database)
            .args(["project", "show", "fixture"])
            .output()?,
    )?;
    assert_eq!(project["status"], "paused");
    Ok(())
}
