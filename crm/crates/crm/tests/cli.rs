#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::process::Command;

use crm::model::{RevisionProposal, Stage};
use crm::store::Store;
use tempfile::TempDir;

fn crm() -> Command {
    Command::new(env!("CARGO_BIN_EXE_crm"))
}

fn fixture() -> (TempDir, std::path::PathBuf) {
    let temporary = TempDir::new().expect("temporary directory");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private temporary directory");
    }
    let database = temporary.path().join("crm.db");
    let status = crm()
        .args([
            "--database",
            database.to_str().expect("database path"),
            "init",
        ])
        .status()
        .expect("run crm init");
    assert!(status.success());
    (temporary, database)
}

#[test]
fn cli_creates_and_reads_free_form_markdown() {
    let (temporary, database) = fixture();
    let input = temporary.path().join("case.md");
    std::fs::write(&input, "# Casey\n\nLoose notes are welcome.\n").expect("case input");
    let created = crm()
        .args([
            "--database",
            database.to_str().expect("database path"),
            "case",
            "new",
            "--title",
            "Casey",
            input.to_str().expect("input path"),
        ])
        .output()
        .expect("create case");
    assert!(created.status.success());
    let stdout = String::from_utf8(created.stdout).expect("UTF-8 output");
    let case_id = stdout
        .split_whitespace()
        .nth(1)
        .expect("case id")
        .to_owned();
    let shown = crm()
        .args([
            "--database",
            database.to_str().expect("database path"),
            "case",
            "show",
            &case_id,
        ])
        .output()
        .expect("show case");
    assert!(shown.status.success());
    let stdout = String::from_utf8(shown.stdout).expect("UTF-8 output");
    assert!(stdout.contains("Loose notes are welcome."));
}

#[test]
fn omitted_case_input_uses_the_suggested_unenforced_structure() {
    let (_temporary, database) = fixture();
    let created = crm()
        .arg("--database")
        .arg(&database)
        .args(["--json", "case", "new", "--title", "Taylor"])
        .output()
        .expect("create default case");
    assert!(created.status.success());
    let value: serde_json::Value =
        serde_json::from_slice(&created.stdout).expect("JSON case creation");
    assert!(value["data"]["case"].get("markdown").is_none());
    let shown = crm()
        .arg("--database")
        .arg(&database)
        .args([
            "--json",
            "case",
            "show",
            value["data"]["case"]["case_id"].as_str().unwrap(),
        ])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(
        value["data"]["case"]["markdown"],
        "# Taylor\n\n## Current picture\n\n## People\n\n## Chronicle\n\n## Open threads\n"
    );
}

#[test]
fn every_case_consumption_surfaces_advisory_without_blocking() {
    let (_temporary, database) = fixture();
    let store = Store::open(database.clone()).expect("store");
    let case = store
        .create_case("Jordan", "# Jordan\n", Stage::Research)
        .expect("case");
    let update = store
        .enqueue_delivery(&case.case_id, "signal", "Possible role", None)
        .expect("delivery");
    store.claim_next().expect("claim").expect("claimed update");
    store
        .commit_proposal(
            &update.id,
            &update.job_id,
            "call-1",
            &"d".repeat(64),
            &RevisionProposal {
                base_revision: 1,
                document_markdown: "# Jordan\n\nPossible role.\n".to_owned(),
                stage: Stage::Research,
                advisory: Some("The role has not been rechecked.".to_owned()),
                summary: "Added possible role".to_owned(),
            },
        )
        .expect("revision");

    for arguments in [
        vec!["case", "show", case.case_id.as_str()],
        vec!["case", "list"],
        vec!["case", "history", case.case_id.as_str()],
        vec!["search", "possible"],
        vec!["update", "show", update.id.as_str()],
        vec!["update", "list"],
    ] {
        let output = crm()
            .arg("--database")
            .arg(&database)
            .args(arguments)
            .output()
            .expect("consume case");
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
        assert!(stdout.contains("ATTENTION — STEWARD ADVISORY (NON-BLOCKING)"));
        assert!(stdout.contains("The role has not been rechecked."));
    }

    let json = crm()
        .arg("--database")
        .arg(&database)
        .args(["--json", "case", "show", &case.case_id])
        .output()
        .expect("JSON show");
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).expect("JSON output");
    assert_eq!(value["data"]["case"]["attention"], true);
    assert_eq!(
        value["data"]["case"]["advisory"],
        "The role has not been rechecked."
    );

    let later = store
        .enqueue_delivery(&case.case_id, "later", "More", None)
        .expect("advisory must not gate intake");
    assert_eq!(later.status.as_str(), "queued");
}

#[test]
fn failed_update_commands_keep_the_consumed_update_advisory() {
    let (_temporary, database) = fixture();
    let store = Store::open(database.clone()).expect("store");
    let case = store
        .create_case("Morgan", "# Morgan\n", Stage::Research)
        .expect("case");
    let advisory_update = store
        .enqueue_delivery(&case.case_id, "signal", "Possible role", None)
        .expect("delivery");
    store.claim_next().expect("claim").expect("claimed update");
    store
        .commit_proposal(
            &advisory_update.id,
            &advisory_update.job_id,
            "call-advisory",
            &"e".repeat(64),
            &RevisionProposal {
                base_revision: 1,
                document_markdown: "# Morgan\n\nPossible role.\n".to_owned(),
                stage: Stage::Research,
                advisory: Some("The role still needs review.".to_owned()),
                summary: "Added possible role".to_owned(),
            },
        )
        .expect("advisory revision");
    let update = store
        .enqueue_delivery(&case.case_id, "follow-up", "More", None)
        .expect("queued follow-up");
    assert_eq!(
        store
            .claim_next()
            .expect("claim")
            .expect("running update")
            .id,
        update.id
    );
    let _lease = store.acquire_worker_lease().expect("hold worker lease");

    for (mode, operation, expected_code) in [
        ("json", "wait", "update_wait_timeout"),
        ("human", "wait", "update_wait_timeout"),
        ("json", "resume", "worker_already_running"),
        ("human", "resume", "worker_already_running"),
        ("json", "retry", "update_retry_not_allowed"),
        ("human", "retry", "update_retry_not_allowed"),
    ] {
        let mut command = crm();
        command.arg("--database").arg(&database);
        if mode == "json" {
            command.arg("--json");
        }
        command.args(["update", operation, &update.id]);
        if operation == "wait" {
            command.args(["--timeout", "0"]);
        }
        let output = command.output().expect("run failing update command");
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).expect("UTF-8 diagnostics");
        if mode == "json" {
            let value: serde_json::Value = serde_json::from_str(&stderr).expect("JSON failure");
            assert_eq!(value["error"]["code"], expected_code);
            assert_eq!(value["context"]["type"], "update");
            assert_eq!(value["context"]["update"]["id"], update.id);
            assert_eq!(value["context"]["update"]["attention"], true);
            assert_eq!(
                value["context"]["update"]["advisory"],
                "The role still needs review."
            );
        } else {
            assert!(stderr.starts_with("ATTENTION — STEWARD ADVISORY (NON-BLOCKING)\n"));
            assert!(stderr.contains("The role still needs review."));
            assert!(stderr.contains(&format!("crm: {expected_code}:")));
        }
    }
}

#[test]
fn deployment_hold_blocks_intake_and_preserves_other_owners() {
    let (_temporary, database) = fixture();
    let invoke = |args: &[&str]| {
        crm()
            .arg("--database")
            .arg(&database)
            .arg("--json")
            .args(args)
            .output()
            .expect("CRM command")
    };
    let held = invoke(&["maintenance", "hold", "rollout-one"]);
    assert!(
        held.status.success(),
        "{}",
        String::from_utf8_lossy(&held.stderr)
    );
    let status: serde_json::Value = serde_json::from_slice(&held.stdout).unwrap();
    assert_eq!(status["data"]["maintenance"]["drained"], true);
    assert!(
        invoke(&["maintenance", "hold", "operator-two"])
            .status
            .success()
    );
    let blocked = invoke(&["case", "new", "--title", "Blocked intake"]);
    assert!(!blocked.status.success());
    let error: serde_json::Value = serde_json::from_slice(&blocked.stderr).unwrap();
    assert_eq!(error["error"]["code"], "deployment_maintenance");
    let released = invoke(&["maintenance", "release", "rollout-one"]);
    let status: serde_json::Value = serde_json::from_slice(&released.stdout).unwrap();
    assert_eq!(
        status["data"]["maintenance"]["holds"],
        serde_json::json!(["operator-two"])
    );
    assert!(invoke(&["case", "list"]).status.success());
    assert!(
        !invoke(&["case", "new", "--title", "Still blocked"])
            .status
            .success()
    );
    assert!(
        invoke(&["maintenance", "release", "operator-two"])
            .status
            .success()
    );
    assert!(
        invoke(&["case", "new", "--title", "Admitted"])
            .status
            .success()
    );
}

#[test]
fn maintenance_reports_durable_queued_and_applied_unsettled_work() {
    let (_temporary, database) = fixture();
    let store = Store::open(&database).unwrap();
    let case = store
        .create_case("Maintenance fixture", "Synthetic", Stage::Research)
        .unwrap();
    store
        .enqueue_delivery(&case.case_id, "Fixture", "Synthetic input", None)
        .unwrap();
    let output = crm()
        .arg("--database")
        .arg(&database)
        .args(["--json", "maintenance", "hold", "test-run"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["data"]["maintenance"]["drained"], false);
    assert_eq!(value["data"]["maintenance"]["unsettled_updates"], 1);
}

#[cfg(unix)]
#[test]
fn database_aliases_cannot_bypass_deployment_admission() {
    let (_temporary, database) = fixture();
    let aliases = tempfile::tempdir().unwrap();
    let held = crm()
        .arg("--database")
        .arg(&database)
        .args(["maintenance", "hold", "alias-test"])
        .output()
        .unwrap();
    assert!(held.status.success());
    let symbolic = aliases.path().join("symbolic.db");
    std::os::unix::fs::symlink(&database, &symbolic).unwrap();
    let hard = aliases.path().join("hard.db");
    for alias in [&symbolic, &hard] {
        if alias == &hard {
            std::fs::hard_link(&database, &hard).unwrap();
        }
        let output = crm()
            .arg("--database")
            .arg(alias)
            .args(["--json", "case", "new", "--title", "Blocked alias"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(value["error"]["code"], "deployment_maintenance");
    }
}

#[test]
fn selection_pages_bound_bodies_and_search_excerpts_point_to_matches() {
    let (_temporary, database) = fixture();
    let store = Store::open(&database).expect("open fixture");
    let body = "雪".repeat(4096);
    for index in 0..21 {
        store
            .create_profile_entry(&format!("Profile {index}"), &body)
            .expect("create profile");
    }
    let output = crm()
        .arg("--database")
        .arg(&database)
        .args(["--json", "profile", "list"])
        .output()
        .expect("list profiles");
    assert!(output.status.success());
    let page: serde_json::Value = serde_json::from_slice(&output.stdout).expect("decode page");
    assert_eq!(page["data"]["entries"].as_array().map(Vec::len), Some(20));
    assert_eq!(page["data"]["has_more"], true);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("body_md"));
    let output = crm()
        .arg("--database")
        .arg(&database)
        .args(["--json", "profile", "list", "--limit", "21"])
        .output()
        .expect("expand profiles");
    let page: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("decode expanded page");
    assert_eq!(page["data"]["entries"].as_array().map(Vec::len), Some(21));
    assert_eq!(page["data"]["has_more"], false);
    let markdown = format!("{body} distinctive match {}", "尾".repeat(4096));
    let case = store
        .create_case("Case", &markdown, Stage::Research)
        .expect("create case");
    let hits = store
        .search("distinctive match", 20)
        .expect("search near end");
    assert_eq!(hits.len(), 1);
    let hit = serde_json::to_value(&hits[0]).expect("encode hit");
    assert_eq!(hit["matched_field"], "markdown");
    assert_eq!(hit["excerpt"], true);
    let snippet = hit["snippet"].as_str().expect("excerpt");
    assert!(snippet.contains("distinctive match"));
    assert!(snippet.chars().count() <= 240);
    let output = crm()
        .arg("--database")
        .arg(&database)
        .args(["--json", "case", "show", &case.case_id])
        .output()
        .expect("read full case");
    let shown: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("decode full case");
    assert_eq!(shown["data"]["case"]["markdown"], markdown);
}
