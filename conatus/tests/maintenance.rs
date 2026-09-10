use conatus::{Config, store::Store};
use serde_json::Value;
use std::process::Command;

#[test]
fn held_admission_blocks_mutation_and_waits_for_a_prior_runner() -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let mut store = Store::create(&root)?;
    store.configure(
        &Config {
            annals: root.join("unused-annals"),
            annals_state_dir: Some(root.join("annals")),
            library: "conatus".into(),
            library_id: "0123456789abcdef0123456789abcdef".into(),
            decisions_config: root.join("decisions.toml"),
            decisions_library_id: "fedcba9876543210fedcba9876543210".into(),
        },
        "cursor",
    )?;
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_conatus"))
            .arg("--state-dir")
            .arg(&root)
            .args(args)
            .env("HOME", &root)
            .output()
    };
    let runner = conatus::store::runner_lock(&root)?;
    let held = invoke(&["maintenance", "hold", "run-a"])?;
    assert!(held.status.success());
    let value: Value = serde_json::from_slice(&held.stdout)?;
    assert_eq!(value["data"]["maintenance"]["drained"], false);
    assert!(
        !invoke(&["want", "add", "Not admitted", "--source", "synthetic"])?
            .status
            .success()
    );
    assert!(invoke(&["config"])?.status.success());
    drop(runner);
    let value: Value = serde_json::from_slice(&invoke(&["maintenance", "drain"])?.stdout)?;
    assert_eq!(value["data"]["maintenance"]["drained"], true);
    invoke(&["maintenance", "release", "other-owner"])?;
    assert!(
        !invoke(&["want", "add", "Still held", "--source", "synthetic"])?
            .status
            .success()
    );
    assert!(
        invoke(&["maintenance", "release", "run-a"])?
            .status
            .success()
    );
    assert!(
        invoke(&["want", "add", "Admitted", "--source", "synthetic"])?
            .status
            .success()
    );
    assert_eq!(store.pending()?.len(), 1);
    Ok(())
}
