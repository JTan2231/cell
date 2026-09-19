#![allow(clippy::unwrap_used, clippy::expect_used)]

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../deployment/tests/simple_fixture.rs"
));

fn specification() -> cell_install::simple::Spec {
    bazaar::installation::specification()
}

fn installer() -> &'static str {
    env!("CARGO_BIN_EXE_bazaar-install")
}

fn source_root() -> std::path::PathBuf {
    std::fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..")).unwrap()
}

#[test]
fn modified_installed_bytes_fail_verification_and_recovery() {
    let fixture = Fixture::new();
    let installed = fixture.success("install", &[]);
    let release = fixture
        .root()
        .join("releases")
        .join(installed["data"]["release_id"].as_str().unwrap());
    let binary = release.join("bin/bazaar");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(&binary, "#!/bin/sh\n# changed\nexit 0\n").unwrap();
    let output = Command::new(installer())
        .arg("verify-release")
        .arg(&release)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        !fixture
            .run("recover", &["--release", release.to_str().unwrap()])
            .status
            .success()
    );
}

#[test]
fn program_upgrade_and_recovery_preserve_string_history() {
    let fixture = Fixture::new();
    let database = bazaar::database_path(&fixture.home);
    let saved = bazaar::api::Writer::initialize(&database)
        .unwrap()
        .update("step", "private content")
        .unwrap();
    let first = fixture.success("install", &[]);
    let release = fixture
        .root()
        .join("releases")
        .join(first["data"]["release_id"].as_str().unwrap());
    fixture.payload("upgrade", false);
    fixture.success("install", &[]);
    fixture.success("recover", &["--release", release.to_str().unwrap()]);
    assert_eq!(
        bazaar::api::Reader::open(&database)
            .unwrap()
            .get("step", None)
            .unwrap(),
        saved
    );
    assert_eq!(
        bazaar::api::Reader::open(database)
            .unwrap()
            .history("step")
            .unwrap(),
        vec![1]
    );
}
