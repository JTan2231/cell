use std::os::unix::fs::PermissionsExt as _;
use std::process::Command;

use anyhow::{Context as _, Result};
use conatus::{Config, store::Store};
use serde_json::{Value, json};

#[test]
fn preview_is_read_only_and_delivery_retains_exact_bytes_across_recovery() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    std::fs::create_dir_all(root.join(".local/bin"))?;
    let annals = root.join("annals");
    std::fs::write(
        &annals,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >>\"${0%/*}/annals-calls\"\nexit 1\n",
    )?;
    std::fs::set_permissions(&annals, std::fs::Permissions::from_mode(0o700))?;
    let mut store = Store::create(&root)?;
    store.configure(
        &Config {
            annals,
            annals_state_dir: None,
            library: "conatus".into(),
            library_id: "0123456789abcdef0123456789abcdef".into(),
            decisions_config: root.join("decisions.toml"),
            decisions_library_id: "fedcba9876543210fedcba9876543210".into(),
        },
        "cursor",
    )?;
    let captured =
        conatus::operations::capture_want(&root, " Keep these words.\nAnd this line. ", "source")?;
    let want_id = captured["record"]["id"].as_str().context("want ID")?;
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_conatus"))
            .arg("--state-dir")
            .arg(&root)
            .arg("--json")
            .args(args)
            .env("HOME", &root)
            .output()
    };
    let preview = invoke(&["email", "preview"])?;
    assert!(preview.status.success());
    let expected: Value = serde_json::from_slice(&preview.stdout)?;
    assert!(!root.join("email.lock").exists());
    assert!(!root.join("payload").exists());
    let transport = root.join(".local/bin/email");
    std::fs::write(
        &transport,
        "#!/bin/sh\ncat > \"$HOME/payload\"\nprintf 'call\\n' >>\"$HOME/send-calls\"\nexit 1\n",
    )?;
    std::fs::set_permissions(&transport, std::fs::Permissions::from_mode(0o700))?;
    let failed = invoke(&["email", "send", "--scheduled"])?;
    assert!(!failed.status.success());
    let stderr = String::from_utf8(failed.stderr)?;
    let error: Value = serde_json::from_str(stderr.lines().last().context("error line")?)?;
    let occurrence = error["error"]
        .as_str()
        .context("error")?
        .split_whitespace()
        .nth(2)
        .context("occurrence")?;
    assert!(occurrence.starts_with("daily/"));
    let payload: Value = serde_json::from_slice(&std::fs::read(root.join("payload"))?)?;
    assert_eq!(payload["body"], expected["data"]["digest"]["body"]);
    assert!(invoke(&["want", "archive", want_id])?.status.success());
    let calls_before = std::fs::read(root.join("annals-calls"))?;
    let archived_preview: Value = serde_json::from_slice(&invoke(&["email", "preview"])?.stdout)?;
    assert_eq!(
        archived_preview["data"]["digest"]["body"],
        "No active wants.\n"
    );
    assert_eq!(archived_preview["data"]["digest"]["want_count"], 0);
    assert_eq!(std::fs::read(root.join("annals-calls"))?, calls_before);
    conatus::operations::capture_want(&root, "A later want", "later source")?;
    assert!(!invoke(&["email", "send", "--scheduled"])?.status.success());
    assert_eq!(std::fs::read_to_string(root.join("send-calls"))?, "call\n");
    let retained: Value =
        serde_json::from_slice(&invoke(&["email", "preview", "--occurrence", occurrence])?.stdout)?;
    assert_eq!(retained["data"]["digest"]["body"], payload["body"]);
    std::fs::write(
        &transport,
        "#!/bin/sh\ncat > \"$HOME/payload\"\nprintf 'call\\n' >>\"$HOME/send-calls\"\nprintf 'Accepted fixture-receipt\\n'\n",
    )?;
    let retried = invoke(&["email", "send", "--retry", occurrence])?;
    assert!(
        retried.status.success(),
        "{}",
        String::from_utf8_lossy(&retried.stderr)
    );
    let payload_after: Value = serde_json::from_slice(&std::fs::read(root.join("payload"))?)?;
    assert_eq!(payload_after, payload);
    assert!(invoke(&["email", "send", "--scheduled"])?.status.success());
    assert_eq!(
        std::fs::read_to_string(root.join("send-calls"))?,
        "call\ncall\n"
    );
    let key = format!("email/{occurrence}");
    let saved: Value = serde_json::from_str(&store.setting(&key)?.context("saved mail")?)?;
    assert_eq!(saved["accepted_id"], "fixture-receipt");
    let current: Value = serde_json::from_slice(&invoke(&["email", "preview"])?.stdout)?;
    assert_eq!(current["data"]["digest"]["want_count"], 1);
    assert!(
        !current["data"]["digest"]["body"]
            .as_str()
            .context("body")?
            .contains("Keep these words")
    );
    assert!(invoke(&["want", "unarchive", want_id])?.status.success());
    let restored: Value = serde_json::from_slice(&invoke(&["email", "preview"])?.stdout)?;
    assert_eq!(restored["data"]["digest"]["want_count"], 2);
    assert!(
        restored["data"]["digest"]["body"]
            .as_str()
            .context("body")?
            .contains(" Keep these words.\nAnd this line. ")
    );
    conatus::gate(&root).hold("deployment")?;
    let held: Value = serde_json::from_slice(&invoke(&["email", "send", "--scheduled"])?.stdout)?;
    assert_eq!(
        held["data"],
        json!({"scheduled":true,"skipped":"deployment_maintenance"})
    );
    assert!(!invoke(&["email", "send"])?.status.success());
    assert!(invoke(&["email", "preview"])?.status.success());
    for call in std::fs::read_to_string(root.join("annals-calls"))?.lines() {
        assert!(
            call.ends_with(" show"),
            "email invoked a non-read operation: {call}"
        );
    }
    Ok(())
}
