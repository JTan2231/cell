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

#[test]
fn daily_definition_pins_the_release_without_activating_or_sending() {
    let fixture = Fixture::new();
    let selected = fixture.success("install", &[]);
    let root = clew::state_dir(&fixture.home);
    clew::store::Store::initialize(&root).unwrap();
    let output = fixture.home.join("daily.toml");
    let args = [
        "--state-dir",
        root.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ];
    let result = fixture.success("schedule-definition", &args);
    assert_eq!(result["data"]["registered"], false);
    assert_eq!(result["data"]["activated"], false);
    let definition =
        clockwork::api::Manifest::from_toml(&fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(definition.release_id, selected["data"]["release_id"]);
    assert_eq!(definition.key, "clew/daily-email");
    assert_eq!(
        definition.schedule,
        clockwork::api::Schedule::LocalCalendar {
            hour: 9,
            minute: 0,
            run_at_load: false
        }
    );
    assert_eq!(
        definition.failure.on_abend,
        clockwork::api::AbendPolicy::HaltUntilApproved
    );
    assert_eq!(definition.timeout_seconds, Some(180));
    assert_eq!(
        definition.arguments,
        [
            "--state-dir",
            root.to_str().unwrap(),
            "email",
            "send",
            "--scheduled"
        ]
    );
    assert_eq!(
        definition.environment["HOME"],
        fixture.home.to_str().unwrap()
    );
    match definition.launch {
        clockwork::api::LaunchImage::Direct { program, sha256 } => {
            assert_eq!(
                sha256,
                cell_install::file_digest(Path::new(&program)).unwrap()
            );
            assert!(program.starts_with(&definition.release_root));
        }
        clockwork::api::LaunchImage::Interpreted { .. } => {
            panic!("expected a direct release image");
        }
    }
    assert!(!fixture.run("schedule-definition", &args).status.success());
    assert!(!root.join("email.sqlite3").exists());
    assert!(!fixture.home.join("Library/LaunchAgents").exists());
}
