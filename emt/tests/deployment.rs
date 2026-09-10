include!("../../mentor/tests/support/deployment_fixture.rs");
fn product() -> &'static str {
    "emt"
}
fn application() -> &'static str {
    "EMT"
}
fn program() -> &'static str {
    env!("CARGO_BIN_EXE_emt")
}
fn installer() -> &'static str {
    env!("CARGO_BIN_EXE_emt-install")
}
fn source_root() -> TestResult<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("source root absent")?
        .canonicalize()?)
}
fn paused(root: &std::path::Path) -> TestResult<bool> {
    emt::store::Config::load(root)
        .map(|config| config.paused)
        .map_err(|error| error.to_string().into())
}
