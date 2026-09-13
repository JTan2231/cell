include!("support/deployment_fixture.rs");
fn product() -> &'static str {
    "mentor"
}
fn application() -> &'static str {
    "MentorMail"
}
fn program() -> &'static str {
    env!("CARGO_BIN_EXE_mentor")
}
fn installer() -> &'static str {
    env!("CARGO_BIN_EXE_mentor-install")
}
fn source_root() -> TestResult<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("source root absent")?
        .canonicalize()?)
}
fn paused(root: &std::path::Path) -> TestResult<bool> {
    mentor::store::Store::open(root)
        .and_then(|store| store.config())
        .map(|config| config.paused)
        .map_err(|error| error.to_string().into())
}

#[test]
fn hourly_upgrade_preserves_existing_failure_halt() -> TestResult {
    let mut deployment = Deployment::new()?;
    deployment.inspect()?;
    for phase in ["hold", "drain", "apply", "configure", "release", "activate"] {
        deployment.run(phase)?;
    }

    let path = deployment.home.join("clockwork-fixture.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path)?)?;
    let digest = state["binding"]["definition_digest"]
        .as_str()
        .ok_or("definition digest absent")?
        .to_owned();
    let mut definition = state["definitions"][&digest].clone();
    definition["manifest"]["schedule"]["seconds"] = json!(60);
    let manifest: clockwork::api::Manifest =
        serde_json::from_value(definition["manifest"].clone())?;
    let legacy_digest = manifest.digest()?;
    definition["digest"] = json!(legacy_digest);
    state["definitions"][&legacy_digest] = definition;
    state["binding"]["definition_digest"] = json!(legacy_digest);
    state["binding"]["halted_incident"] = json!("retained-incident");
    fs::write(&path, serde_json::to_vec(&state)?)?;

    deployment.request["run_id"] = json!("hourly-upgrade");
    deployment.request["run_dir"] = json!(deployment.home.join("hourly-upgrade-run"));
    deployment.request["settings"] = Value::Null;
    deployment.inspect()?;
    for phase in ["hold", "drain", "apply", "configure", "release", "activate"] {
        deployment.run(phase)?;
    }

    let state: Value = serde_json::from_slice(&fs::read(path)?)?;
    let digest = state["binding"]["definition_digest"]
        .as_str()
        .ok_or("definition digest absent")?;
    let manifest = &state["definitions"][digest]["manifest"];
    assert_eq!(manifest["schedule"]["seconds"], 3600);
    assert_eq!(manifest["schedule"]["run_at_load"], false);
    assert_eq!(manifest["timeout_seconds"], 90);
    assert_eq!(state["binding"]["enabled"], true);
    assert_eq!(state["binding"]["halted_incident"], "retained-incident");
    Ok(())
}
