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
