mod common;
use anyhow::Result;
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

fn install_cast_fixture(bin: &Path) -> Result<PathBuf> {
    let provider = bin.join("cast");
    let names: Vec<_> = (0..25).map(|i| format!("cast-{i}")).collect();
    let specifications: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), "Acme", "Engineer"))
        .collect();
    let mut snapshot = common::snapshot(&specifications);
    snapshot["jobs"][0]["source_key"] = json!("ashby:acme:role");
    snapshot["jobs"][0]["url"] = json!("https://jobs.ashbyhq.com/acme/role");
    fs::write(
        &provider,
        format!(
            "#!/bin/sh\n[ \"$*\" = 'export --json' ] || exit 9\n[ \"$CHANCERY_USAGE_INTERNAL\" = 1 ] || exit 8\nprintf '%s\\n' '{snapshot}'\n"
        ),
    )?;
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700))?;
    Ok(provider)
}

#[test]
fn exact_reference_admission_and_offline_retries_use_the_installed_reader() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let bin = temp.path().join(".local/bin");
    fs::create_dir_all(&bin)?;
    let provider = install_cast_fixture(&bin)?;
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
        Some(25)
    );
    assert!(
        !call(&[
            "record",
            "--cast-job",
            "Acme",
            "--id",
            "one",
            "--status",
            "applied"
        ])?
        .0
    );
    assert!(
        call(&["list"])?.1["data"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    let arguments = [
        "record",
        "--cast-job",
        "cast-0",
        "--id",
        "one",
        "--status",
        "applied",
        "--notes",
        "Project fit",
    ];
    let first = call(&arguments)?;
    assert!(first.0);
    assert_eq!(first.1["data"]["cast_job_id"], "cast-0");
    let found = call(&[
        "find",
        "https://jobs.ashbyhq.com/acme/role/application?source=test",
    ])?;
    assert_eq!(
        found.1["data"]["candidates"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(found.1["data"]["candidates"][0]["tracked"], true);
    assert_eq!(
        call(&["find", "Project fit"])?.1["data"]["candidates"][0]["cast_job_id"],
        "cast-0"
    );
    fs::remove_file(provider)?;
    assert!(!call(&["find", "Acme"])?.0);
    assert_eq!(call(&arguments)?.1, first.1);
    assert!(
        call(&[
            "record",
            "--cast-job",
            "cast-0",
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
            "--cast-job",
            "cast-unknown",
            "--id",
            "three",
            "--status",
            "rejected"
        ])?
        .0
    );
    assert!(call(&["show", "cast-0"])?.0);
    Ok(())
}
