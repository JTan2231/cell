#![allow(clippy::unwrap_used, clippy::expect_used)] // Isolated fixture setup and assertions.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../deployment/tests/simple_fixture.rs"
));

fn specification() -> cell_install::simple::Spec {
    clockwork::installation::specification()
}
fn installer() -> &'static str {
    env!("CARGO_BIN_EXE_clockwork-install")
}
fn source_root() -> std::path::PathBuf {
    std::fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."))
        .unwrap()
}

#[test]
fn uninstall_keeps_recovery_commands_until_binding_transition_is_resolved() {
    let fixture = Fixture::new();
    fixture.success("install", &[]);
    let locks = fixture
        .home
        .join("Library/Application Support/Clockwork/locks");
    fs::create_dir_all(&locks).unwrap();
    let transition = locks.join("fixture.transition.json");
    fs::write(&transition, "{}").unwrap();
    assert!(!fixture.run("uninstall", &[]).status.success());
    fixture.success("inspect", &[]);
    fs::remove_file(&transition).unwrap();
    fs::remove_dir(&locks).unwrap();
    symlink("/foreign/locks", &locks).unwrap();
    assert!(!fixture.run("uninstall", &[]).status.success());
    fs::remove_file(&locks).unwrap();
    fixture.success("uninstall", &[]);
    assert!(!fixture.root().join("current").exists());
    assert!(fixture.root().join("releases").is_dir());
}
