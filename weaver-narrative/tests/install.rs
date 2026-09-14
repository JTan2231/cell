// Exercise the real Weaver installer in an isolated home; dependency reads are stubs.
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
        fs::write(
            bin.join("annals"),
            "#!/bin/sh\nprintf '%s\\n' '{\"ok\":true,\"data\":{\"contract_version\":2,\"library_id\":\"0123456789abcdef0123456789abcdef\",\"watermark\":\"start\"}}'\n",
        )?;
        fs::set_permissions(bin.join("annals"), fs::Permissions::from_mode(0o755))?;
        let annals_config = home.join("decisions.toml");
        fs::write(&annals_config, "# Read by the Annals stub\n")?;
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
        let provider = "weaver-narrative/chancery".to_owned();
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
        let request = json!({"schema":1,"product":product(),"run_id":"weaver-fixture","run_dir":home.join("run"),"source_root":source,"candidate_dir":candidate_dir,"candidate":candidate,"prior":null,"selected_products":[product()],"recovery":null,"settings":{"annals_config":annals_config}});
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
                    let line = String::from_utf8_lossy(&request);
                    let body = if line.starts_with("GET /v1/jobs?") {
                        r#"{"version":1,"jobs":[]}"#
                    } else if line.starts_with("GET /v1/maintenance ") {
                        r#"{"protocol_version":1,"holds":[],"drained":true,"nonterminal_jobs":0}"#
                    } else if line.starts_with("GET /v1/health ") {
                        r#"{"version":1,"status":"ok","daemonVersion":"fixture","acceptingJobs":true,"checkedAt":"2026-09-14T00:00:00Z","supportedProtocolVersions":[1],"harness":{"harness":"codex","harnessVersion":"fixture","adapterVersion":"fixture"},"harnessExecutable":"/fixture/codex","capabilities":["exact-model","reasoning-effort","workspace-none","builtin-local-execution","builtin-web-search","dynamic-client-tools"],"authentication":{"codexHome":"/fixture/codex-home","configured":true,"authenticated":true},"execution":{"maxActiveJobs":8,"activeJobs":0,"availableSlots":8}}"#
                    } else {
                        panic!("unexpected dependency operation: {line}");
                    };
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
}

fn product() -> &'static str {
    "weaver"
}
fn application() -> &'static str {
    "Weaver"
}
fn program() -> &'static str {
    env!("CARGO_BIN_EXE_weaver")
}
fn installer() -> &'static str {
    env!("CARGO_BIN_EXE_weaver-install")
}
fn source_root() -> TestResult<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("root missing")?
        .canonicalize()?)
}

#[test]
fn maintained_install_redeploy_and_recovery_preserve_documents() -> TestResult {
    let mut deployment = Deployment::new()?;
    deployment.inspect()?;
    for phase in [
        "hold",
        "drain",
        "apply",
        "configure",
        "verify",
        "release",
        "activate",
    ] {
        deployment.run(phase)?;
    }
    let root = deployment.root();
    let store = weaver::store::Store::open(&root, false)?;
    let request =
        weaver::agent::request("Keep this document", None, &root.join("agent-workspace"))?;
    store.create(&request)?;
    store.connection.execute(
        "UPDATE documents SET markdown='Retained prose' WHERE id=?1",
        [request.id.as_str()],
    )?;
    drop(store);
    deployment.request["run_id"] = json!("redeploy");
    deployment.request["run_dir"] = json!(deployment.home.join("redeploy"));
    deployment.request["settings"] = Value::Null;
    deployment.inspect()?;
    for phase in ["hold", "drain", "apply", "configure"] {
        deployment.run(phase)?;
    }
    deployment.request["recovery"] = json!({"any_apply_started":true});
    let recovered = deployment.run("recover")?;
    assert_eq!(recovered["safe_to_release"], true);
    deployment.run("release")?;
    let store = weaver::store::Store::open(&root, true)?;
    assert_eq!(
        store.document(request.id.as_str())?.markdown.as_deref(),
        Some("Retained prose")
    );
    assert!(weaver::gate(&root).status()?.holds.is_empty());
    Ok(())
}

#[test]
fn altered_candidate_and_foreign_selector_are_refused() -> TestResult {
    use std::os::unix::fs::symlink;
    let deployment = Deployment::new()?;
    symlink(
        "/foreign/program",
        deployment.home.join(".local/bin/weaver"),
    )?;
    assert!(!deployment.invoke("inspect")?.0);
    assert_eq!(
        fs::read_link(deployment.home.join(".local/bin/weaver"))?,
        PathBuf::from("/foreign/program")
    );
    fs::remove_file(deployment.home.join(".local/bin/weaver"))?;
    let binary = deployment.request["candidate_dir"]
        .as_str()
        .ok_or("candidate missing")?;
    fs::write(PathBuf::from(binary).join("bin/weaver"), "altered")?;
    assert!(!deployment.invoke("inspect")?.0);
    Ok(())
}
