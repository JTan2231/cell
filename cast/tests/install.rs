#![allow(clippy::unwrap_used, clippy::expect_used)] // Isolated fixture setup and assertions.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../deployment/tests/simple_fixture.rs"
));

fn specification() -> cell_install::simple::Spec {
    cast::installation::specification()
}
fn installer() -> &'static str {
    env!("CARGO_BIN_EXE_cast-install")
}
fn source_root() -> std::path::PathBuf {
    std::fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..")).unwrap()
}

impl Fixture {
    fn adapter_request(&self) -> Value {
        let candidate_dir = self.home.join("candidate");
        fs::create_dir_all(candidate_dir.join("bin")).unwrap();
        let mut binaries = serde_json::Map::new();
        for (name, original) in [
            ("cast", self.binary.as_path()),
            ("cast-install", Path::new(installer())),
        ] {
            let target = candidate_dir.join("bin").join(name);
            fs::copy(original, &target).unwrap();
            binaries.insert(
                name.into(),
                serde_json::json!({
                    "path":format!("bin/{name}"),
                    "sha256":cell_install::file_digest(&target).unwrap(),
                    "version":format!("{name} {}",env!("CARGO_PKG_VERSION"))
                }),
            );
        }
        let files = cell_install::provider_inventory(
            &source_root().join("cast/chancery"),
            &cell_install::InstallSpec {
                product: "cast",
                application: "Cast",
                commands: &["cast"],
                provider: "cast",
            },
        )
        .unwrap();
        let source_inputs: std::collections::BTreeMap<_, _> = files
            .into_iter()
            .map(|(path, entry)| (format!("cast/chancery/{path}"), entry.sha256))
            .collect();
        let mut candidate = serde_json::json!({
            "schema":1,"product":"cast","source_commit":"fixture","source_key":"fixture",
            "source_inputs":source_inputs,"binaries":binaries
        });
        let encoded = candidate_dir.join("candidate-content.json");
        fs::write(&encoded, format!("{candidate}\n")).unwrap();
        candidate["candidate_id"] = serde_json::json!(format!(
            "sha256:{}",
            cell_install::file_digest(&encoded).unwrap()
        ));
        serde_json::json!({
            "schema":1,"product":"cast","run_id":"cast-configuration-fixture",
            "run_dir":self.home.join("run"),"source_root":source_root(),
            "candidate_dir":candidate_dir,"candidate":candidate,"prior":null,
            "selected_products":["cast"],"recovery":null
        })
    }

    fn adapter(&self, operation: &str, request: &Value) -> Value {
        let mut child = Command::new(installer())
            .args(["adapter", operation])
            .env("HOME", &self.home)
            .env_remove("CAST_STATE_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(request).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{operation}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone()
    }
}

#[test]
fn configure_initializes_real_cast_and_applies_settings() {
    let fixture = Fixture::new();
    fs::copy(env!("CARGO_BIN_EXE_cast"), &fixture.binary).unwrap();
    let state = fixture.home.join("discovery");
    let config_file = fixture.home.join("configuration.json");
    let mut config = cast::models::Config::default();
    config.budgets.http_per_run = 17;
    fs::write(&config_file, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut request = fixture.adapter_request();
    request["settings"] = serde_json::json!({"state_dir":state,"config_file":config_file});
    request["prior"] = fixture.adapter("inspect", &request);
    fixture.success("install", &[]);

    fixture.adapter("configure", &request);
    let store = cast::store::Store::open(&state).unwrap();
    assert_eq!(store.config().unwrap().budgets.http_per_run, 17);
    assert!(store.snapshot().unwrap().jobs.is_empty());
    drop(store);

    request["recovery"] = serde_json::json!({"any_apply_started":true});
    assert_eq!(
        fixture.adapter("recover", &request)["installed"],
        "candidate"
    );
    assert_eq!(
        cast::store::Store::open(&state)
            .unwrap()
            .config()
            .unwrap()
            .budgets
            .http_per_run,
        17
    );
}
