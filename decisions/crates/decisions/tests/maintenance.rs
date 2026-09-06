use std::error::Error;
use std::path::Path;
use std::process::{Command, Output};

use decisions::api::{Client, ClientError};
use serde_json::{Value, json};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn command(database: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_krisis"));
    command
        .arg("--database")
        .arg(database)
        .arg("--json")
        .env_remove("CELL_DEPLOYMENT_RUN_ID")
        .env_remove("KRISIS_ANNALS_BINARY")
        .env_remove("KRISIS_ANNALS_CONFIG")
        .env_remove("KRISIS_ANNALS_LIBRARY_ID");
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

fn held(output: &Output) {
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("deployment_maintenance"));
}

#[test]
fn maintenance_does_not_initialize_state_and_fences_public_clients() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("krisis.db");
    let gate_path = temporary.path().join("krisis.db.cell-maintenance");
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
    );
    assert!(!gate_path.exists());
    success(
        &command(&database)
            .args(["maintenance", "hold", "run-a"])
            .output()?,
    )?;
    for arguments in [
        vec!["observe", "activate", "--at", "1"],
        vec!["observe", "status"],
        vec!["doctor"],
    ] {
        held(&command(&database).args(arguments).output()?);
    }
    let client = Client::new(env!("CARGO_BIN_EXE_krisis")).with_database(&database);
    assert!(
        matches!(client.activate(Some(1)), Err(ClientError::Failed(message))
        if message.contains("deployment_maintenance"))
    );
    assert!(!database.exists());
    Ok(())
}

#[test]
fn installation_requires_sole_owner_and_drain_without_resetting_baseline() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("krisis.db");
    let gate = cell_maintenance::Gate::new(temporary.path().join("krisis.db.cell-maintenance"));
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
            .args(["observe", "activate", "--at", "1"])
            .output()?,
    );
    drop(activity);
    success(
        &command(&database)
            .args(["maintenance", "hold", "run-b"])
            .output()?,
    )?;
    held(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-a")
            .args(["observe", "activate", "--at", "1"])
            .output()?,
    );
    let status = success(
        &command(&database)
            .args(["maintenance", "release", "run-a"])
            .output()?,
    )?;
    assert_eq!(status["holds"], json!(["run-b"]));
    held(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-a")
            .args(["observe", "activate", "--at", "1"])
            .output()?,
    );
    held(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "../invalid")
            .args(["observe", "activate", "--at", "1"])
            .output()?,
    );
    assert!(!database.exists());
    success(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-b")
            .args(["observe", "activate", "--at", "1"])
            .output()?,
    )?;
    assert!(database.is_file());
    success(
        &command(&database)
            .args(["maintenance", "release", "run-b"])
            .output()?,
    )?;
    let replay = success(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-b")
            .args(["observe", "activate", "--at", "2"])
            .output()?,
    )?;
    assert_eq!(replay["created"], false);
    assert_eq!(replay["observer_baseline_at"], 1);
    Ok(())
}
