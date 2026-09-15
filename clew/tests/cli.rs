use anyhow::Result;
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn exact_reference_admission_and_offline_retries_use_the_installed_reader() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let bin = temp.path().join(".local/bin");
    fs::create_dir_all(&bin)?;
    let provider = bin.join("platter");
    let opportunities = json!({"schema_version":1,"items":[
        {"reference":"ashby:acme:role","cast_job_id":"cast-one","company":"Acme","title":"Engineer","urls":["https://jobs.ashbyhq.com/acme/role"],"packets":[]},
        {"reference":"ashby:acme:other","cast_job_id":"cast-two","company":"Acme","title":"Other Engineer","urls":[],"packets":[]}
    ]});
    fs::write(
        &provider,
        format!(
            "#!/bin/sh\n[ \"$CHANCERY_USAGE_INTERNAL\" = 1 ] || exit 8\nprintf '%s\\n' '{opportunities}'\n"
        ),
    )?;
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700))?;
    let call = |args: &[&str]| -> Result<(bool, Value)> {
        let output = Command::new(env!("CARGO_BIN_EXE_clew"))
            .env("HOME", temp.path())
            .env("CHANCERY_USAGE_DISABLED", "1")
            .args(args)
            .output()?;
        Ok((
            output.status.success(),
            serde_json::from_slice(&output.stdout)?,
        ))
    };
    assert!(call(&["init"])?.0);
    let candidates = call(&["find", "Acme"])?;
    assert_eq!(
        candidates.1["data"]["candidates"].as_array().map(Vec::len),
        Some(2)
    );
    assert!(!call(&["record", "Acme", "--id", "one", "--status", "applied"])?.0);
    assert!(
        call(&["list"])?.1["data"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    let arguments = [
        "record",
        "ashby:acme:role",
        "--id",
        "one",
        "--status",
        "applied",
        "--notes",
        "Project fit",
    ];
    let first = call(&arguments)?;
    assert!(first.0);
    fs::remove_file(provider)?;
    assert_eq!(call(&arguments)?.1, first.1);
    assert!(
        call(&[
            "record",
            "ashby:acme:role",
            "--id",
            "two",
            "--notes",
            "Reply received"
        ])?
        .0
    );
    assert_eq!(call(&["list"])?.1["data"][0]["status"], "applied");
    assert!(
        !call(&[
            "record",
            "ashby:unknown:role",
            "--id",
            "three",
            "--status",
            "rejected"
        ])?
        .0
    );
    assert!(call(&["show", "ashby:acme:role"])?.0);
    Ok(())
}
