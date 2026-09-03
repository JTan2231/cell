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
