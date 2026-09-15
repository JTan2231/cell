#![allow(clippy::unwrap_used, clippy::expect_used)]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../deployment/tests/simple_fixture.rs"
));

fn specification() -> cell_install::simple::Spec {
    clew::installation::specification()
}
fn installer() -> &'static str {
    env!("CARGO_BIN_EXE_clew-install")
}
fn source_root() -> std::path::PathBuf {
    std::fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..")).unwrap()
}
