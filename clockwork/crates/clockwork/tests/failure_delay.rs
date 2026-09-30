use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use clockwork::api::{
    AbendPolicy, Authority, FailurePolicy, LaunchImage, Manifest, Output, OverlapPolicy, Schedule,
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const KEY: &str = "example/worker";

struct Fixture {
    _directory: tempfile::TempDir,
    state: PathBuf,
    digest: String,
}

impl Fixture {
    fn new(policy: AbendPolicy) -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let root = directory.path().canonicalize()?;
        let state = root.join("state");
        command(&state, &["definition", "list"])?;
        let release_id = "0".repeat(64);
        let release = root.join(&release_id);
        fs::create_dir(&release)?;
        let script = release.join("worker");
        executable(&script, "#!/bin/sh\nexit 0\n")?;
        executable(
            &state.join("iatreion"),
            "#!/bin/sh\n/bin/cat \"$(/usr/bin/dirname \"$0\")/report.json\"\n",
        )?;
        executable(
            &state.join("email"),
            "#!/bin/sh\nprintf 'accepted\\n' >> \"$0.sent\"\nexit 0\n",
        )?;
        let manifest = Manifest {
            schema_version: 2,
            key: KEY.into(),
            release_id,
            release_root: release.to_string_lossy().into_owned(),
            authority: Authority::CurrentUserBackground,
            overlap: OverlapPolicy::Skip,
            failure: FailurePolicy {
                on_abend: policy,
                email_cli: None,
            },
            timeout_seconds: None,
            arguments: vec![],
            cwd: root.to_string_lossy().into_owned(),
            schedule: Schedule::Interval {
                seconds: 60,
                run_at_load: false,
            },
            launch: LaunchImage::Interpreted {
                interpreter: "/bin/sh".into(),
                interpreter_sha256: hex::encode(Sha256::digest(fs::read("/bin/sh")?)),
                script: script.to_string_lossy().into_owned(),
                script_sha256: hex::encode(Sha256::digest(fs::read(&script)?)),
            },
            environment: BTreeMap::new(),
            output: Output {
                stdout: root.join("worker.out").to_string_lossy().into_owned(),
                stderr: root.join("worker.err").to_string_lossy().into_owned(),
            },
        };
        let digest = manifest.digest()?;
        let connection = Connection::open(state.join("clockwork.db"))?;
        connection.execute(
            "INSERT INTO definitions(digest,key,manifest_json,registered_at) VALUES (?1,?2,?3,1)",
            params![digest, KEY, serde_json::to_string(&manifest)?],
        )?;
        connection.execute(
            "INSERT INTO bindings(key,definition_digest,enabled,plist_sha256,updated_at) VALUES (?1,?2,1,?3,1)",
            params![KEY, digest, "1".repeat(64)],
        )?;
        drop(connection);
        command(
            &state,
            &[
                "notification",
                "policy",
                "--failure-threshold",
                "5",
                "--interval-seconds",
                "1",
                "--cell-root",
                root.to_str().ok_or("UTF-8 fixture root")?,
            ],
        )?;
        let fixture = Self {
            _directory: directory,
            state,
            digest,
        };
        fixture.report(false)?;
        Ok(fixture)
    }

    fn report(&self, healthy_running: bool) -> TestResult {
        let now = iatreion_api::now_unix_seconds();
        let report = json!({
            "schema_version": 1, "scope": "fixture", "root": "/fixture",
            "observed_at_start": now, "observed_at_end": now,
            "products": [{
                "id": "example", "name": "Example", "aliases": [],
                "probe_state": "observed", "complete": true,
                "units": [{"group": "operating", "source_product_id": "example", "observation": {
                    "id": KEY, "owning_product_id": "example", "clockwork_key": KEY,
                    "intent": "active", "admission": {"state": "open"},
                    "activity": if healthy_running { "running" } else { "idle" },
                    "readiness": {"state": if healthy_running { "ready" } else { "blocked" }, "scope": "fixture"}
                }}]
            }]
        });
        write_json(&self.state.join("report.json"), &report)
    }

    fn failed_operation(&self, activation: &str, occurrence: &str) -> TestResult {
        self.start_operation(activation)?;
        command(
            &self.state,
            &[
                "abend",
                activation,
                "--code",
                "model_failed",
                "--occurrence",
                occurrence,
            ],
        )?;
        self.finish_operation(activation)
    }

    fn start_operation(&self, activation: &str) -> TestResult {
        let connection = Connection::open(self.state.join("clockwork.db"))?;
        connection.execute(
            "INSERT INTO activations(id,key,definition_digest,trigger,state,admitted_at) VALUES (?1,?2,?3,'manual','running',?4)",
            params![activation, KEY, self.digest, iatreion_api::now_unix_seconds()],
        )?;
        Ok(())
    }

    fn finish_operation(&self, activation: &str) -> TestResult {
        Connection::open(self.state.join("clockwork.db"))?.execute(
            "UPDATE activations SET state='exited',finished_at=?2,exit_code=0 WHERE id=?1",
            params![activation, iatreion_api::now_unix_seconds()],
        )?;
        Ok(())
    }

    fn report_runtime(&self, activation: &str, kind: &str, running: bool) -> TestResult {
        self.report(running)?;
        let path = self.state.join("report.json");
        let mut report = read_json(&path)?;
        let unit = &mut report["products"][0]["units"][0]["observation"];
        unit["readiness"]["state"] = json!("ready");
        unit["evidence"] = json!({"latest_runtime_outcome": {
            "kind": kind, "occurred_at": iatreion_api::now_unix_seconds(), "reference": activation
        }});
        write_json(&path, &report)
    }

    fn check(&self) -> TestResult<Value> {
        self.report(false)?;
        command(&self.state, &["notification", "check"])
    }

    fn expire_check(&self) -> TestResult {
        let path = self.state.join("failure-checks.json");
        let mut state = read_json(&path)?;
        state["pending"][KEY]["check"]["last_checked_at"] =
            json!(iatreion_api::now_unix_seconds() - 2);
        write_json(&path, &state)
    }
}

fn executable(path: &Path, contents: &str) -> TestResult {
    fs::write(path, contents)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn write_json(path: &Path, value: &Value) -> TestResult {
    fs::write(path, serde_json::to_vec(value)?)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn read_json(path: &Path) -> TestResult<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn command(state: &Path, args: &[&str]) -> TestResult<Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_clockwork"))
        .arg("--state-root")
        .arg(state)
        .arg("--json")
        .args(args)
        .env("CHANCERY_USAGE_INTERNAL", "1")
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "fixture command {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let result: Value = serde_json::from_slice(&output.stdout)?;
    if args == ["status-snapshot"] {
        return Ok(result);
    }
    assert_eq!(result["ok"], true);
    Ok(result["data"].clone())
}

fn rejected_command(state: &Path, args: &[&str]) -> TestResult<Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_clockwork"))
        .arg("--state-root")
        .arg(state)
        .arg("--json")
        .args(args)
        .env("CHANCERY_USAGE_INTERNAL", "1")
        .output()?;
    assert!(!output.status.success());
    let result: Value = serde_json::from_slice(&output.stderr)?;
    assert_eq!(result["ok"], false);
    Ok(result["error"].clone())
}

#[test]
fn malformed_or_unsupported_failure_sidecar_refuses_activation_admission() -> TestResult {
    for bytes in [
        b"{malformed".as_slice(),
        br#"{"version":999,"cursor":0,"pending":{}}"#.as_slice(),
    ] {
        let fixture = Fixture::new(AbendPolicy::HaltUntilApproved)?;
        let history = command(&fixture.state, &["history", KEY])?;
        let path = fixture.state.join("failure-checks.json");
        fs::write(&path, bytes)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        let error = rejected_command(&fixture.state, &["run", KEY])?;
        assert_eq!(error["code"], "failure_checks_invalid");
        assert_eq!(
            command(&fixture.state, &["history", KEY])?["items"],
            history["items"]
        );
    }
    Ok(())
}

#[test]
fn failure_ledger_read_error_refuses_activation_admission() -> TestResult {
    let fixture = Fixture::new(AbendPolicy::HaltUntilApproved)?;
    let history = command(&fixture.state, &["history", KEY])?;
    // Keep the schema valid, but make one synthetic TEXT value unreadable as
    // UTF-8 so this fails during ledger observation rather than store opening.
    Connection::open(fixture.state.join("clockwork.db"))?.execute(
        "INSERT INTO abends(key,occurrence,code,recorded_at) VALUES (CAST(x'80' AS TEXT),'bad-row','failed',0)",
        [],
    )?;
    let error = rejected_command(&fixture.state, &["run", KEY])?;
    assert_eq!(error["code"], "failure_checks_observation_failed");
    assert_eq!(
        command(&fixture.state, &["history", KEY])?["items"],
        history["items"]
    );
    Ok(())
}

#[test]
fn five_due_checks_halt_once_and_do_not_add_another_alert_delay() -> TestResult {
    let fixture = Fixture::new(AbendPolicy::HaltUntilApproved)?;
    fixture.failed_operation("first", "job/first")?;
    let first = fixture.check()?;
    assert_eq!(first["failures"][KEY]["check"]["consecutive_failures"], 1);
    assert!(command(&fixture.state, &["binding", "show", KEY])?["halted_incident"].is_null());
    let snapshot = command(&fixture.state, &["status-snapshot"])?;
    assert_eq!(
        snapshot["scheduler_observations"][0]["failure_pending"],
        true
    );

    // The product abend remains evidence even though its direct child exited zero.
    assert_eq!(first["failures"][KEY]["event"]["code"], "model_failed");
    for count in 2..=5 {
        thread::sleep(Duration::from_secs(1));
        let checked = fixture.check()?;
        if count < 5 {
            assert_eq!(
                checked["failures"][KEY]["check"]["consecutive_failures"],
                count
            );
            assert!(
                command(&fixture.state, &["binding", "show", KEY])?["halted_incident"].is_null()
            );
        } else {
            assert!(checked["failures"][KEY].is_null());
        }
    }
    let binding = command(&fixture.state, &["binding", "show", KEY])?;
    let incident = binding["halted_incident"]
        .as_str()
        .ok_or("confirmed incident")?;
    let notification = command(&fixture.state, &["notification", "show", incident])?;
    assert_eq!(notification["health_check"]["consecutive_failures"], 5);
    assert!(notification["health_check"]["eligible_at"].is_number());
    command(&fixture.state, &["notification", "send"])?;
    command(&fixture.state, &["notification", "send"])?;
    assert_eq!(
        fs::read_to_string(fixture.state.join("email.sent"))?
            .lines()
            .count(),
        1
    );
    let incidents = command(&fixture.state, &["incident", "list", KEY])?;
    assert_eq!(
        incidents["items"].as_array().ok_or("incident list")?.len(),
        1
    );
    Ok(())
}

#[test]
fn pending_failure_permits_a_later_activation() -> TestResult {
    let fixture = Fixture::new(AbendPolicy::HaltUntilApproved)?;
    fixture.failed_operation("first", "job/first")?;
    fixture.check()?;
    // The external test volume deliberately does not satisfy production launch
    // artifact ancestry. A failed artifact check still proves that the broker
    // admitted this activation rather than refusing the pending failure gate.
    let output = Command::new(env!("CARGO_BIN_EXE_clockwork"))
        .arg("--state-root")
        .arg(&fixture.state)
        .args(["--json", "run", KEY])
        .env("CHANCERY_USAGE_INTERNAL", "1")
        .output()?;
    if !output.status.success() {
        let error: Value = serde_json::from_slice(&output.stderr)?;
        assert_ne!(error["error"]["code"], "binding_halted");
    }
    let history = command(&fixture.state, &["history", KEY])?;
    assert_eq!(
        history["items"]
            .as_array()
            .ok_or("activation history")?
            .len(),
        2
    );
    assert!(command(&fixture.state, &["binding", "show", KEY])?["halted_incident"].is_null());
    Ok(())
}

#[test]
fn healthy_or_disabled_service_clears_progress_without_replaying_old_failures() -> TestResult {
    let fixture = Fixture::new(AbendPolicy::HaltUntilApproved)?;
    fixture.failed_operation("first", "job/first")?;
    fixture.check()?;
    fixture.expire_check()?;
    fixture.report(true)?;
    let recovered = command(&fixture.state, &["notification", "check"])?;
    assert!(recovered["failures"][KEY].is_null());
    fixture.failed_operation("repeated", "job/first")?;
    assert!(fixture.check()?["failures"][KEY].is_null());
    fixture.failed_operation("second", "job/second")?;
    assert_eq!(
        fixture.check()?["failures"][KEY]["check"]["consecutive_failures"],
        1
    );
    fixture.expire_check()?;
    Connection::open(fixture.state.join("clockwork.db"))?.execute(
        "UPDATE bindings SET enabled=0,plist_sha256=NULL WHERE key=?1",
        [KEY],
    )?;
    assert!(fixture.check()?["failures"][KEY].is_null());
    assert!(command(&fixture.state, &["binding", "show", KEY])?["halted_incident"].is_null());
    Ok(())
}

#[test]
fn reported_abends_override_zero_exit_and_running_health_until_a_distinct_success() -> TestResult {
    let fixture = Fixture::new(AbendPolicy::HaltUntilApproved)?;
    fixture.failed_operation("first", "job/first")?;
    fixture.check()?;
    fixture.failed_operation("later", "job/later")?;
    fixture.expire_check()?;
    fixture.report_runtime("later", "succeeded", false)?;
    let zero_exit = command(&fixture.state, &["notification", "check"])?;
    assert_eq!(
        zero_exit["failures"][KEY]["check"]["consecutive_failures"],
        2
    );

    fixture.start_operation("working")?;
    command(
        &fixture.state,
        &[
            "abend",
            "working",
            "--code",
            "model_failed",
            "--occurrence",
            "job/working",
        ],
    )?;
    fixture.expire_check()?;
    // The joined runtime observation can refer to newer work. It must not
    // conceal the immutable abend from this currently running activation.
    fixture.report_runtime("newer-other", "succeeded", true)?;
    let running = command(&fixture.state, &["notification", "check"])?;
    assert_eq!(running["failures"][KEY]["check"]["consecutive_failures"], 3);

    fixture.finish_operation("working")?;
    fixture.start_operation("recovered")?;
    fixture.finish_operation("recovered")?;
    fixture.expire_check()?;
    fixture.report_runtime("recovered", "succeeded", false)?;
    let recovered = command(&fixture.state, &["notification", "check"])?;
    assert!(recovered["failures"][KEY].is_null());
    assert!(command(&fixture.state, &["binding", "show", KEY])?["halted_incident"].is_null());
    Ok(())
}

#[test]
fn broker_only_failure_requires_a_success_in_a_strictly_later_second() -> TestResult {
    let fixture = Fixture::new(AbendPolicy::HaltUntilApproved)?;
    let recorded_at = iatreion_api::now_unix_seconds();
    Connection::open(fixture.state.join("clockwork.db"))?.execute(
        "INSERT INTO abends(key,occurrence,code,recorded_at) VALUES (?1,?2,'start_failed',?3)",
        params![
            KEY,
            format!("broker/{}/fixture", fixture.digest),
            recorded_at
        ],
    )?;
    fixture.check()?;
    fixture.expire_check()?;
    fixture.report_runtime("success", "succeeded", false)?;
    let path = fixture.state.join("report.json");
    let mut report = read_json(&path)?;
    report["products"][0]["units"][0]["observation"]["evidence"]["latest_runtime_outcome"]["occurred_at"] =
        json!(recorded_at);
    write_json(&path, &report)?;
    let same_second = command(&fixture.state, &["notification", "check"])?;
    assert_eq!(
        same_second["failures"][KEY]["check"]["consecutive_failures"],
        2
    );
    fixture.expire_check()?;
    report["products"][0]["units"][0]["observation"]["evidence"]["latest_runtime_outcome"]["occurred_at"] =
        json!(recorded_at + 1);
    write_json(&path, &report)?;
    assert!(command(&fixture.state, &["notification", "check"])?["failures"][KEY].is_null());
    Ok(())
}

#[test]
fn continue_policy_stays_excluded_and_existing_explicit_halts_stay_closed() -> TestResult {
    let fixture = Fixture::new(AbendPolicy::ContinueNextActivation)?;
    fixture.failed_operation("first", "job/first")?;
    assert!(fixture.check()?["failures"][KEY].is_null());
    let halt = command(
        &fixture.state,
        &[
            "binding",
            "halt",
            KEY,
            "--code",
            "legacy_failure",
            "--occurrence",
            "legacy/one",
        ],
    )?;
    let incident = halt["id"].as_str().ok_or("explicit halt incident")?;
    fixture.check()?;
    assert_eq!(
        command(&fixture.state, &["binding", "show", KEY])?["halted_incident"],
        incident
    );
    assert!(!fixture.state.join("email.sent").exists());
    Ok(())
}

#[test]
fn confirmed_halt_recovers_eligibility_from_a_stale_pending_sidecar() -> TestResult {
    let fixture = Fixture::new(AbendPolicy::HaltUntilApproved)?;
    fixture.failed_operation("first", "job/first")?;
    for _ in 0..4 {
        fixture.check()?;
        fixture.expire_check()?;
    }
    let stale_pending = read_json(&fixture.state.join("failure-checks.json"))?;
    let stale_notifications = read_json(&fixture.state.join("notification-checks.json"))?;
    fixture.check()?;
    let binding = command(&fixture.state, &["binding", "show", KEY])?;
    let incident = binding["halted_incident"]
        .as_str()
        .ok_or("confirmed incident")?;
    // Simulate a crash after the SQLite halt commit, before either sidecar
    // publishes the confirming observation and removal of the pending episode.
    write_json(&fixture.state.join("failure-checks.json"), &stale_pending)?;
    write_json(
        &fixture.state.join("notification-checks.json"),
        &stale_notifications,
    )?;
    assert!(fixture.check()?["failures"][KEY].is_null());
    let view = command(&fixture.state, &["notification", "show", incident])?;
    assert!(view["health_check"]["eligible_at"].is_number());
    command(&fixture.state, &["notification", "send"])?;
    assert_eq!(
        fs::read_to_string(fixture.state.join("email.sent"))?
            .lines()
            .count(),
        1
    );
    Ok(())
}

#[test]
fn approval_before_sidecar_recovery_does_not_restore_the_alert_episode() -> TestResult {
    let fixture = Fixture::new(AbendPolicy::HaltUntilApproved)?;
    fixture.failed_operation("first", "job/first")?;
    for _ in 0..4 {
        fixture.check()?;
        fixture.expire_check()?;
    }
    let stale_pending = read_json(&fixture.state.join("failure-checks.json"))?;
    let stale_notifications = read_json(&fixture.state.join("notification-checks.json"))?;
    fixture.check()?;
    let binding = command(&fixture.state, &["binding", "show", KEY])?;
    let incident = binding["halted_incident"]
        .as_str()
        .ok_or("confirmed incident")?;
    command(&fixture.state, &["binding", "resume", KEY, incident])?;
    write_json(&fixture.state.join("failure-checks.json"), &stale_pending)?;
    write_json(
        &fixture.state.join("notification-checks.json"),
        &stale_notifications,
    )?;
    assert!(fixture.check()?["failures"][KEY].is_null());
    let view = command(&fixture.state, &["notification", "show", incident])?;
    assert!(view["health_check"]["eligible_at"].is_null());
    command(&fixture.state, &["notification", "send"])?;
    assert!(!fixture.state.join("email.sent").exists());
    Ok(())
}
