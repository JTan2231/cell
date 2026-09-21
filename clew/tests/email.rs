use anyhow::{Context as _, Result};
use clew::{
    digest,
    store::{Record, Store},
};
use platter::api::Opportunity;
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt as _, path::Path, process::Command};

fn report(
    store: &mut Store,
    id: &str,
    reference: &str,
    status: Option<&str>,
    notes: Option<&str>,
    replaces: Option<&str>,
) -> Result<()> {
    store.record(&Record {
        id: id.into(),
        platter_job_ref: reference.into(),
        status: status.map(Into::into),
        notes: notes.map(Into::into),
        replaces: replaces.map(Into::into),
    })?;
    Ok(())
}

fn job(reference: &str, company: &str, title: &str) -> Opportunity {
    Opportunity {
        reference: reference.into(),
        cast_job_id: reference.into(),
        company: company.into(),
        title: title.into(),
        urls: vec![format!("https://example.com/{reference}")],
        packets: vec![],
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn snapshot_uses_current_status_and_active_notes_in_sequence_order() -> Result<()> {
    let temp = tempfile::tempdir()?;
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700))?;
    let mut store = Store::initialize(temp.path())?;
    report(
        &mut store,
        "a1",
        "alpha",
        Some("applied"),
        Some("superseded note"),
        None,
    )?;
    report(
        &mut store,
        "a2",
        "alpha",
        Some("interview"),
        Some("  exact note\nsecond line  "),
        Some("a1"),
    )?;
    report(&mut store, "a3", "alpha", None, Some("latest note"), None)?;
    report(
        &mut store,
        "a4",
        "alpha",
        None,
        Some("retracted note"),
        None,
    )?;
    store.retract("a5", "a4", Some("retraction explanation"))?;
    report(
        &mut store,
        "b1",
        "beta",
        Some("applied"),
        Some("old rejected-job note"),
        None,
    )?;
    report(&mut store, "b2", "beta", Some(" ReJeCtEd \n"), None, None)?;
    report(&mut store, "b3", "beta", None, Some("still excluded"), None)?;
    report(&mut store, "c1", "gamma", None, Some("notes only"), None)?;
    report(
        &mut store,
        "d1",
        "delta",
        Some("withdrawn"),
        Some("wrong job"),
        None,
    )?;
    report(
        &mut store,
        "d2",
        "epsilon",
        Some("withdrawn"),
        Some("correct job"),
        Some("d1"),
    )?;
    let jobs = vec![
        job("gamma", "Acme", "Z role"),
        job("alpha", "Acme", "A role"),
        job("epsilon", "Zoo", "Role"),
    ];
    let rendered = digest::render(&store.entries()?, Some(&jobs), "2026-09-20")?;
    assert_eq!(rendered.application_count, 3);
    assert!(rendered.context_available);
    assert_eq!(rendered.subject, "Clew — 2026-09-20");
    for text in [
        "  exact note\nsecond line  ",
        "latest note",
        "notes only",
        "correct job",
        "withdrawn",
        "No status recorded",
        "https://example.com/alpha",
    ] {
        assert!(rendered.body.contains(text), "missing {text}");
    }
    for text in [
        "superseded note",
        "retracted note",
        "retraction explanation",
        "old rejected-job note",
        "still excluded",
        "wrong job",
        "beta",
        "delta",
    ] {
        assert!(!rendered.body.contains(text), "unexpected {text}");
    }
    assert!(rendered.body.find("Acme — A role") < rendered.body.find("Acme — Z role"));
    assert!(rendered.body.find("exact note") < rendered.body.find("latest note"));
    let fallback = digest::render(&store.entries()?, None, "2026-09-20")?;
    assert_eq!(fallback.application_count, 3);
    assert!(!fallback.context_available);
    assert!(fallback.body.contains("alpha [job details unavailable]"));
    assert!(
        fallback
            .body
            .contains("All qualifying Clew records are included")
    );
    let partial = digest::render(&store.entries()?, Some(&jobs[..1]), "2026-09-20")?;
    assert_eq!(partial.application_count, 3);
    assert!(!partial.context_available);
    store.retract("b4", "b2", None)?;
    assert_eq!(
        digest::render(&store.entries()?, None, "date")?.application_count,
        4
    );
    Ok(())
}

fn executable(path: &Path, text: &str) -> Result<()> {
    fs::write(path, text)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn preview_and_uncertain_delivery_preserve_bytes_without_real_transport() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().canonicalize()?;
    let root = clew::state_dir(&home);
    let mut store = Store::initialize(&root)?;
    report(
        &mut store,
        "one",
        "acme",
        Some("applied"),
        Some(" Keep these words.\nAnd this line. "),
        None,
    )?;
    fs::create_dir_all(home.join(".local/bin"))?;
    let platter = home.join(".local/bin/platter");
    executable(
        &platter,
        &format!(
            "#!/bin/sh\n[ \"$*\" = 'opportunities list' ] || exit 2\n[ \"$CHANCERY_USAGE_INTERNAL\" = 1 ] || exit 3\nprintf '%s\\n' '{}'\n",
            json!({"schema_version":1,"items":[job("acme","Acme","Engineer")]})
        ),
    )?;
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_clew"))
            .env("HOME", &home)
            .env("CHANCERY_USAGE_DISABLED", "1")
            .arg("--json")
            .args(args)
            .output()
    };
    let ledger_before = fs::read(root.join("ledger.sqlite3"))?;
    let expected: Value = serde_json::from_slice(&invoke(&["email", "preview"])?.stdout)?;
    assert_eq!(expected["data"]["digest"]["application_count"], 1);
    assert!(!root.join("email.sqlite3").exists());
    assert!(!root.join("email.lock").exists());
    assert!(!root.join("deployment-maintenance").exists());
    assert_eq!(fs::read(root.join("ledger.sqlite3"))?, ledger_before);
    let transport = home.join(".local/bin/email");
    let capture =
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >>\"$HOME/send-calls\"\ncat >\"$HOME/payload\"\n";
    executable(&transport, &format!("{capture}exit 1\n"))?;
    let failed = invoke(&["email", "send", "--scheduled"])?;
    assert!(!failed.status.success());
    let failure: Value = serde_json::from_slice(&failed.stdout)?;
    let occurrence = failure["error"]["detail"]
        .as_str()
        .context("detail")?
        .split_whitespace()
        .nth(2)
        .context("occurrence")?;
    assert!(occurrence.starts_with("daily/"));
    let payload = fs::read(home.join("payload"))?;
    let first_call = fs::read_to_string(home.join("send-calls"))?;
    let payload_json: Value = serde_json::from_slice(&payload)?;
    assert_eq!(payload_json["body"], expected["data"]["digest"]["body"]);
    assert_eq!(payload_json["attachments"], json!([]));
    report(&mut store, "two", "acme", Some("rejected"), None, None)?;
    let current: Value = serde_json::from_slice(&invoke(&["email", "preview"])?.stdout)?;
    assert_eq!(
        current["data"]["digest"]["body"],
        "No applications to show.\n"
    );
    let retained: Value =
        serde_json::from_slice(&invoke(&["email", "preview", "--occurrence", occurrence])?.stdout)?;
    assert_eq!(retained["data"]["digest"], expected["data"]["digest"]);
    assert!(!invoke(&["email", "send", "--scheduled"])?.status.success());
    assert_eq!(fs::read_to_string(home.join("send-calls"))?, first_call);
    executable(
        &transport,
        &format!("{capture}printf 'Accepted fixture-receipt\\n'\n"),
    )?;
    let retry = invoke(&["email", "send", "--retry", occurrence])?;
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stdout)
    );
    assert_eq!(fs::read(home.join("payload"))?, payload);
    assert_eq!(
        fs::read_to_string(home.join("send-calls"))?,
        first_call.repeat(2)
    );
    assert!(invoke(&["email", "send", "--scheduled"])?.status.success());
    assert!(
        invoke(&["email", "send", "--retry", occurrence])?
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(home.join("send-calls"))?,
        first_call.repeat(2)
    );
    assert!(invoke(&["email", "send"])?.status.success());
    assert_eq!(
        fs::read_to_string(home.join("send-calls"))?.lines().count(),
        3
    );
    assert!(invoke(&["email", "send", "--scheduled"])?.status.success());
    assert_eq!(
        fs::read_to_string(home.join("send-calls"))?.lines().count(),
        3
    );
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("email.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)?;
    assert!(!invoke(&["email", "send"])?.status.success());
    assert_eq!(
        fs::read_to_string(home.join("send-calls"))?.lines().count(),
        3
    );
    drop(lock);
    clew::gate(&root).hold("deployment")?;
    let skipped: Value =
        serde_json::from_slice(&invoke(&["email", "send", "--scheduled"])?.stdout)?;
    assert_eq!(skipped["data"]["skipped"], "deployment_maintenance");
    assert!(!invoke(&["email", "send"])?.status.success());
    assert!(invoke(&["email", "preview"])?.status.success());
    clew::gate(&root).release("deployment")?;
    report(&mut store, "three", "acme", Some("applied"), None, None)?;
    fs::remove_file(&platter)?;
    let fallback: Value = serde_json::from_slice(&invoke(&["email", "preview"])?.stdout)?;
    assert_eq!(fallback["data"]["digest"]["application_count"], 1);
    assert_eq!(fallback["data"]["digest"]["context_available"], false);
    fs::remove_file(root.join("ledger.sqlite3"))?;
    assert!(!invoke(&["email", "preview"])?.status.success());
    assert!(!invoke(&["email", "send"])?.status.success());
    assert_eq!(
        fs::read_to_string(home.join("send-calls"))?.lines().count(),
        3
    );
    Ok(())
}

#[test]
fn rejection_only_and_large_digests_are_complete() -> Result<()> {
    let temp = tempfile::tempdir()?;
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700))?;
    let mut store = Store::initialize(temp.path())?;
    report(
        &mut store,
        "rejected",
        "hidden",
        Some("rejected"),
        Some("hidden note"),
        None,
    )?;
    assert_eq!(
        digest::render(&store.entries()?, None, "date")?.body,
        "No applications to show.\n"
    );
    for n in 0..105 {
        report(
            &mut store,
            &format!("id{n}"),
            &format!("ref{n}"),
            Some("applied"),
            Some(&format!("note {n}")),
            None,
        )?;
    }
    let digest = digest::render(&store.entries()?, None, "date")?;
    assert_eq!(digest.application_count, 105);
    for n in 0..105 {
        assert!(digest.body.contains(&format!("note {n}")));
    }
    Ok(())
}
