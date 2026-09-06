use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use annals::api::{CliClient, ClientError, Request, WorkAddArgs, WorkCommand};
use serde_json::{Value, json};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn command(database: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_annals"));
    command
        .arg("--library")
        .arg(database)
        .arg("--json")
        .env_remove("ANNALS_CONFIG")
        .env_remove("ANNALS_LIBRARY")
        .env_remove("CELL_DEPLOYMENT_RUN_ID");
    command
}

fn success(output: &Output) -> TestResult<Value> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(envelope["ok"], true);
    Ok(envelope["data"].clone())
}

fn held(output: &Output) -> TestResult {
    assert!(!output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stderr)?;
    assert_eq!(envelope["error"]["code"], "deployment_maintenance");
    Ok(())
}

#[test]
fn maintenance_does_not_open_state_and_fences_public_mutation() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("annals.db");
    let gate_path = temporary.path().join("annals.db.cell-maintenance");
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
    held(&command(&database).arg("init").output()?)?;
    let source = temporary.path().join("source.txt");
    fs::write(&source, "A retained source.\n")?;
    let mut client = CliClient::new(env!("CARGO_BIN_EXE_annals").into());
    client.library = Some(database.clone());
    let result = client.call(&Request::Work(WorkCommand::Add(WorkAddArgs {
        input: source,
        name: Some("held source".to_owned()),
    })));
    assert!(
        matches!(result, Err(ClientError::Failed { error: Some(error), .. })
        if error.code == "deployment_maintenance")
    );
    assert!(!database.exists());
    Ok(())
}

#[test]
fn installation_requires_one_owner_and_drained_activity() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("annals.db");
    let gate = cell_maintenance::Gate::new(temporary.path().join("annals.db.cell-maintenance"));
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
            .arg("init")
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
            .arg("init")
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
            .arg("init")
            .output()?,
    )?;
    held(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "../invalid")
            .arg("init")
            .output()?,
    )?;
    assert!(!database.exists());
    success(
        &command(&database)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-b")
            .arg("init")
            .output()?,
    )?;
    assert!(database.is_file());
    success(&command(&database).arg("stats").output()?)?;
    let mut client = CliClient::new(env!("CARGO_BIN_EXE_annals").into());
    client.library = Some(database.clone());
    client.call(&Request::Stats)?;
    success(
        &command(&database)
            .args(["maintenance", "release", "run-b"])
            .output()?,
    )?;
    // A deployment ID without a hold follows ordinary admission.
    let other = temporary.path().join("other.db");
    success(
        &command(&other)
            .env("CELL_DEPLOYMENT_RUN_ID", "run-b")
            .arg("init")
            .output()?,
    )?;
    Ok(())
}

#[test]
fn deployment_hold_and_release_preserve_operator_pause() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("annals.db");
    let config = temporary.path().join("config.toml");
    let spool = temporary.path().join("spool");
    fs::write(
        &config,
        format!(
            "library = {}\n[inbox]\nroot = {}\nminimum_available_bytes = 0\n",
            serde_json::to_string(&database)?,
            serde_json::to_string(&spool)?
        ),
    )?;
    success(&command(&database).arg("init").output()?)?;
    success(
        &command(&database)
            .arg("--config")
            .arg(&config)
            .args(["inbox", "pause"])
            .output()?,
    )?;
    success(
        &command(&database)
            .args(["maintenance", "hold", "run-a"])
            .output()?,
    )?;
    held(
        &command(&database)
            .arg("--config")
            .arg(&config)
            .args(["inbox", "resume"])
            .output()?,
    )?;
    success(
        &command(&database)
            .args(["maintenance", "release", "run-a"])
            .output()?,
    )?;
    let status = success(
        &command(&database)
            .arg("--config")
            .arg(&config)
            .args(["inbox", "status"])
            .output()?,
    )?;
    assert_eq!(status["paused"], true);
    assert!(spool.join(".paused").is_file());
    Ok(())
}

#[test]
fn decisions_feed_remains_readable_during_deployment() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let database = temporary.path().join("decisions.db");
    let initialized = success(
        &command(&database)
            .args(["init", "--kind", "decisions"])
            .output()?,
    )?;
    let config = temporary.path().join("decisions.toml");
    fs::write(
        &config,
        format!(
            "library = {}\n[inbox]\nroot = {}\nminimum_available_bytes = 0\n[decision_feed]\nexpected_library_id = {}\n",
            serde_json::to_string(&database)?,
            serde_json::to_string(&temporary.path().join("spool"))?,
            initialized["library_id"],
        ),
    )?;
    success(
        &command(&database)
            .args(["maintenance", "hold", "run-a"])
            .output()?,
    )?;
    let watermark = success(
        &Command::new(env!("CARGO_BIN_EXE_annals"))
            .arg("--config")
            .arg(config)
            .args(["--json", "decision-feed", "watermark"])
            .env_remove("CELL_DEPLOYMENT_RUN_ID")
            .env_remove("ANNALS_LIBRARY")
            .output()?,
    )?;
    assert_eq!(watermark["library_id"], initialized["library_id"]);
    Ok(())
}
