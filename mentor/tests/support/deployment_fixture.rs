// Shared isolated process fixture for the Mentor and EMT adapter boundaries.
use serde_json::{Value, json};
use std::fs;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Deployment {
    home: PathBuf,
    request: Value,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Deployment {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        let _ = fs::remove_file(&self.socket);
        let _ = fs::remove_dir_all(&self.home);
    }
}

impl Deployment {
    fn new() -> TestResult<Self> {
        let home = std::env::temp_dir()
            .canonicalize()?
            .join(format!("worker-deployment-{}", uuid::Uuid::now_v7()));
        fs::DirBuilder::new().mode(0o700).create(&home)?;
        let bin = home.join(".local/bin");
        fs::create_dir_all(&bin)?;
        let stub = include_str!("deployment_clockwork.py");
        fs::write(bin.join("clockwork"), stub)?;
        fs::set_permissions(bin.join("clockwork"), fs::Permissions::from_mode(0o755))?;
        fs::write(
            bin.join("email"),
            "#!/bin/sh\nprintf '%s\\n' '{\"domains\":[\"fixture.example\"]}'\n",
        )?;
        fs::set_permissions(bin.join("email"), fs::Permissions::from_mode(0o755))?;
        let candidate_dir = home.join("candidate");
        fs::create_dir_all(candidate_dir.join("bin"))?;
        let mut binaries = serde_json::Map::new();
        for (name, path) in [
            (product().to_owned(), program()),
            (format!("{}-install", product()), installer()),
        ] {
            let target = candidate_dir.join("bin").join(&name);
            fs::copy(path, &target)?;
            binaries.insert(name.clone(), json!({"path":format!("bin/{name}"),"sha256":cell_install::file_digest(&target)?,"version":format!("{name} {}",env!("CARGO_PKG_VERSION"))}));
        }
        let original_source = source_root()?;
        let source = home.join("source");
        let provider = format!("{}/chancery", product());
        let mut source_inputs = std::collections::BTreeMap::new();
        let mut pending = vec![original_source.join(&provider)];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let relative = path.strip_prefix(&original_source)?;
                    let target = source.join(relative);
                    fs::create_dir_all(target.parent().ok_or("source parent missing")?)?;
                    fs::copy(&path, &target)?;
                    source_inputs.insert(
                        relative.to_str().ok_or("source path encoding")?.to_owned(),
                        cell_install::file_digest(&target)?,
                    );
                }
            }
        }
        let mut candidate = json!({"schema":1,"product":product(),"source_commit":"fixture","source_key":"fixture","source_inputs":source_inputs,"binaries":binaries});
        let encoded = candidate_dir.join("candidate-content.json");
        fs::write(
            &encoded,
            format!("{}\n", serde_json::to_string(&candidate)?),
        )?;
        let hash = format!("sha256:{}", cell_install::file_digest(&encoded)?);
        candidate["candidate_id"] = json!(hash);
        let mut request = json!({"schema":1,"product":product(),"run_id":"worker-fixture","run_dir":home.join("run"),"source_root":source,"candidate_dir":candidate_dir,"candidate":candidate,"prior":null,"selected_products":[product()],"recovery":null,"settings":{"receiving_domain":"fixture.example"}});
        if product() == "emt" {
            request["settings"]["cell_root"] = json!(source_root()?);
        }
        let socket = PathBuf::from(format!("/tmp/worker-health-{}.sock", uuid::Uuid::now_v7()));
        let listener = std::os::unix::net::UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = Arc::clone(&stop);
        let server = std::thread::spawn(move || {
            while !server_stop.load(Ordering::Relaxed) {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut request = [0; 4096];
                    let _ = stream.read(&mut request);
                    let body = r#"{"version":1,"status":"ok","daemonVersion":"fixture","acceptingJobs":true,"checkedAt":"2026-09-09T00:00:00Z","supportedProtocolVersions":[1],"harness":{"harness":"codex","harnessVersion":"fixture","adapterVersion":"fixture"},"harnessExecutable":"/fixture/codex","capabilities":[],"authentication":{"codexHome":"/fixture/codex-home","configured":true,"authenticated":true}}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        });
        Ok(Self {
            home,
            request,
            socket,
            stop,
            server: Some(server),
        })
    }

    fn invoke(&self, operation: &str) -> TestResult<(bool, Value)> {
        let mut child = Command::new(installer())
            .args(["adapter", operation])
            .env("HOME", &self.home)
            .env("NUCLEUS_SOCKET", &self.socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or("stdin absent")?
            .write_all(&serde_json::to_vec(&self.request)?)?;
        let output = child.wait_with_output()?;
        let value = serde_json::from_slice(&output.stdout).map_err(|error| {
            format!(
                "{operation}: {error}; stdout={}; stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })?;
        Ok((output.status.success(), value))
    }
    fn run(&self, operation: &str) -> TestResult<Value> {
        let (success, value) = self.invoke(operation)?;
        assert!(success, "{operation}: {value}");
        Ok(value["data"].clone())
    }
    fn inspect(&mut self) -> TestResult {
        self.request["prior"] = self.run("inspect")?;
        Ok(())
    }
    fn root(&self) -> PathBuf {
        self.home
            .join("Library/Application Support")
            .join(application())
    }
    fn binding(&self) -> TestResult<Value> {
        let value: Value =
            serde_json::from_slice(&fs::read(self.home.join("clockwork-fixture.json"))?)?;
        Ok(value["binding"].clone())
    }
    fn paused(&self) -> TestResult<bool> {
        paused(&self.root())
    }
}

#[test]
fn receiving_discovery_uses_selected_email_candidate() -> TestResult {
    let mut deployment = Deployment::new()?;
    fs::write(
        deployment.home.join(".local/bin/email"),
        "#!/bin/sh\nexit 1\n",
    )?;
    let directory = deployment.home.join("email-candidate");
    fs::create_dir_all(directory.join("bin"))?;
    let binary = directory.join("bin/email");
    fs::write(
        &binary,
        "#!/bin/sh\nprintf '%s\\n' '{\"domains\":[\"candidate.example\"]}'\n",
    )?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
    let mut candidate = json!({"schema":1,"product":"email","source_commit":"fixture","source_key":"fixture","source_inputs":{},"binaries":{"email":{"path":"bin/email","sha256":cell_install::file_digest(&binary)?,"version":"email 0.6.0"}}});
    let encoded = directory.join("candidate-content.json");
    fs::write(
        &encoded,
        format!("{}\n", serde_json::to_string(&candidate)?),
    )?;
    candidate["candidate_id"] = json!(format!("sha256:{}", cell_install::file_digest(&encoded)?));
    deployment.request["settings"]
        .as_object_mut()
        .ok_or("settings absent")?
        .remove("receiving_domain");
    deployment.request["selected_products"] = json!(["email", product()]);
    deployment.request["dependency_candidates"] =
        json!({"email":{"candidate_dir":directory,"candidate":candidate}});
    deployment.inspect()?;
    assert_eq!(
        deployment.request["prior"]["lifecycle"]["config"]["receiving_domain"],
        "candidate.example"
    );
    Ok(())
}

#[test]
fn another_products_apply_does_not_initialize_absent_worker_on_recovery() -> TestResult {
    let mut deployment = Deployment::new()?;
    deployment.inspect()?;
    deployment.run("hold")?;
    deployment.run("drain")?;
    deployment.request["recovery"] = json!({"any_apply_started":true});
    let recovery = deployment.run("recover")?;
    assert_eq!(recovery["installed"], "prior");
    deployment.run("release")?;
    deployment.run("activate")?;
    assert!(!deployment.home.join(".local/bin").join(product()).exists());
    Ok(())
}

#[test]
fn interrupted_fresh_configuration_keeps_original_activation_intent() -> TestResult {
    let mut deployment = Deployment::new()?;
    deployment.inspect()?;
    assert_eq!(
        deployment.request["prior"]["lifecycle"]["initialized"],
        false
    );
    deployment.run("hold")?;
    deployment.run("drain")?;
    deployment.run("apply")?;
    // The product migration can commit before the adapter records its receipt.
    // The next Configure must safely rediscover that committed initialization.
    let backup = deployment
        .root()
        .join(format!("{}-pre-migration-worker-fixture.sqlite", product()));
    let output = Command::new(program())
        .args(["--json", "migrate", "--backup"])
        .arg(&backup)
        .env("HOME", &deployment.home)
        .env("CELL_DEPLOYMENT_RUN_ID", "worker-fixture")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    deployment.run("configure")?;
    assert!(
        deployment
            .home
            .join("run")
            .join(format!("{}-migration.json", product()))
            .is_file()
    );
    let digest = deployment.binding()?["definition_digest"].clone();
    assert_eq!(deployment.binding()?["enabled"], false);
    assert!(deployment.paused()?);
    // Lose the response after configuration. Recovery must use the captured
    // first-install intent rather than interpreting its generated pause anew.
    deployment.request["recovery"] =
        json!({"any_apply_started":true,"apply_started":true,"configured":false});
    deployment.run("recover")?;
    assert_eq!(deployment.binding()?["definition_digest"], digest);
    assert_eq!(deployment.binding()?["enabled"], false);
    deployment.run("release")?;
    fs::write(deployment.home.join("fail-switch"), "")?;
    assert!(!deployment.invoke("activate")?.0);
    deployment.run("hold")?;
    deployment.run("drain")?;
    deployment.run("recover")?;
    assert!(deployment.paused()?);
    deployment.run("release")?;
    deployment.run("activate")?;
    assert_eq!(deployment.binding()?["enabled"], true);
    assert_eq!(deployment.binding()?["definition_digest"], digest);
    assert!(!deployment.paused()?);
    Ok(())
}

#[test]
fn disabled_pause_and_failure_halt_survive_update_and_preapply_recovery() -> TestResult {
    let mut deployment = Deployment::new()?;
    deployment.request["settings"]["paused"] = json!(true);
    deployment.request["settings"]["enabled"] = json!(false);
    deployment.inspect()?;
    for phase in ["hold", "drain", "apply", "configure", "release", "activate"] {
        deployment.run(phase)?;
    }
    assert!(deployment.paused()?);
    assert_eq!(deployment.binding()?["enabled"], false);
    let path = deployment.home.join("clockwork-fixture.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&path)?)?;
    state["binding"]["halted_incident"] = json!("retained-incident");
    fs::write(&path, serde_json::to_vec(&state)?)?;
    deployment.request["settings"] = Value::Null;
    deployment.inspect()?;
    deployment.run("hold")?;
    deployment.request["recovery"] = json!({"any_apply_started":false});
    deployment.run("recover")?;
    deployment.run("release")?;
    deployment.run("activate")?;
    assert!(deployment.paused()?);
    assert_eq!(deployment.binding()?["enabled"], false);
    assert_eq!(
        deployment.binding()?["halted_incident"],
        "retained-incident"
    );
    Ok(())
}
