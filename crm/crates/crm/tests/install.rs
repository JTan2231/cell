#![allow(clippy::unwrap_used, clippy::expect_used)] // Isolated fixture setup and assertions.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../deployment/tests/simple_fixture.rs"
));

fn specification() -> cell_install::simple::Spec {
    crm::installation::specification()
}
fn installer() -> &'static str {
    env!("CARGO_BIN_EXE_crm-install")
}
fn source_root() -> std::path::PathBuf {
    std::fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."))
        .unwrap()
}
