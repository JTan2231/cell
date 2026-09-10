//! Isolated requester maintenance checks: child-only environments and a fake
//! Nucleus socket; no discovery, model execution, transport, or real user state.
#![allow(clippy::unwrap_used)]

use anyhow::{Context, Result, ensure};
use platter::{Config, maintenance, readiness, resume::ResumeTemplate, store::Store};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read as _, Write as _},
    os::unix::{fs::PermissionsExt as _, net::UnixListener},
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};

#[test]
fn successive_deployments_keep_distinct_repeatable_backups() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("state");
    let first = temporary.path().join("backups/first.sqlite");
    let second = temporary.path().join("backups/second.sqlite");
    platter::migration::migrate(&root, &first)?;
    let first_bytes = fs::read(&first)?;
    platter::migration::migrate(&root, &first)?;
    platter::migration::migrate(&root, &second)?;
    let second_bytes = fs::read(&second)?;
    platter::migration::migrate(&root, &second)?;
    assert_eq!(fs::read(first)?, first_bytes);
    assert_eq!(fs::read(second)?, second_bytes);
    Ok(())
}

fn cli(home: &Path, socket: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_platter"));
    command
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("NUCLEUS_SOCKET", socket);
    command
}

fn response(output: &Output) -> Result<Value> {
    ensure!(
        output.status.success(),
        "fixture child failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn mailbox(
    socket: &Path,
    exchanges: Vec<(String, Value)>,
) -> Result<thread::JoinHandle<Result<Vec<String>>>> {
    let listener = UnixListener::bind(socket)?;
    listener.set_nonblocking(true)?;
    Ok(thread::spawn(move || {
        let mut observed = Vec::new();
        for (route, response) in exchanges {
            let start = Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        ensure!(
                            start.elapsed() < Duration::from_secs(10),
                            "fixture socket timed out waiting for {route}"
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => return Err(error.into()),
                }
            };
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            stream.set_write_timeout(Some(Duration::from_secs(2)))?;
            let mut bytes = Vec::new();
            while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                let mut buffer = [0; 4096];
                let count = stream.read(&mut buffer)?;
                ensure!(
                    count > 0 && bytes.len() < 64 * 1024,
                    "incomplete fixture request"
                );
                bytes.extend_from_slice(&buffer[..count]);
            }
            let headers = String::from_utf8(bytes)?;
            let actual = headers.lines().next().context("request line missing")?;
            ensure!(
                actual.starts_with(&route),
                "unexpected fixture route: {actual}; wanted {route}"
            );
            observed.push(actual.to_owned());
            let body = serde_json::to_vec(&response)?;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )?;
            stream.write_all(&body)?;
        }
        Ok(observed)
    }))
}

fn list_route(program: &str) -> String {
    format!("GET /v1/jobs?requesterProgram={program}")
}

fn job(program: &str, id: &str, state: &str) -> Value {
    json!({
        "version":1,"id":id,"label":"Fixture stage",
        "requester":{"program":program,"id":"fixture-packet"},
        "state":state,"requestDigest":"fixture-digest",
        "createdAt":"2026-09-06T21:00:00Z","updatedAt":"2026-09-06T21:00:00Z"
    })
}

fn empty_page() -> Value {
    json!({"version":1,"jobs":[]})
}

fn frozen_state(root: &Path, home: &Path) -> Result<()> {
    let template = ResumeTemplate::from_source(
        "fixture-original.tex".into(),
        "Fixed heading\n{Jackson National Life}\n\\resumeItemListStart\n\\resumeItem{Supported work}\n\\resumeItemListEnd\nFixed footer\n".into(),
    )?;
    let original = root.join("original-resume.json");
    let settings = Config {
        daily_count: 3,
        delivery_hour: 9,
        delivery_minute: 0,
        timezone: "America/Chicago".into(),
        cast_executable: home.join(".local/bin/cast"),
        crm_executable: home.join(".local/bin/crm"),
        email_executable: home.join(".local/bin/email"),
        original_resume: original,
    };
    Store::open(root)?.initialize(&settings, &template)?;
    Ok(())
}

fn state_bytes(root: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut files = fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    files.sort();
    files
        .into_iter()
        .map(|path| Ok((path.clone(), fs::read(path)?)))
        .collect()
}

#[test]
fn global_hold_prevents_mutation_in_every_state_directory() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let home = fixture.path().join("home");
    let custom = home.join(".local/share/platter");
    maintenance::gate(&home).hold("deploy-one")?;
    for arguments in [
        vec!["init", "--resume", "/fixture/never-read.tex"],
        vec!["prepare", "never-discovered"],
        vec!["prepare-daily"],
        vec!["preview", "2026-09-06", "--ad-hoc", "test-one"],
        vec!["send", "2026-09-06", "--ad-hoc", "test-one"],
    ] {
        let output = cli(&home, &fixture.path().join("unavailable.sock"))
            .arg("--state-dir")
            .arg(&custom)
            .args(arguments)
            .output()?;
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("prevents new work"));
        assert!(Store::open_read_only(&custom)?.list()?.is_empty());
    }
    assert_eq!(maintenance::gate(&home).status()?.holds, ["deploy-one"]);
    Ok(())
}

#[test]
fn drain_cancels_only_orphaned_platter_namespaces_across_pages() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let home = fixture.path().join("home");
    let root = home.join(".local/share/platter");
    fs::create_dir_all(&root)?;
    fs::write(
        root.join("accepted-stage.json"),
        b"retained acceptance and receipt",
    )?;
    maintenance::gate(&home).hold("deploy-one")?;
    let before = state_bytes(&root)?;
    let socket = fixture.path().join("n.sock");
    let server = mailbox(
        &socket,
        vec![
            (
                list_route("platter"),
                json!({"version":1,"jobs":[job("platter","new-orphan","waiting_on_requester")],"next":"new-orphan"}),
            ),
            (
                list_route("platter"),
                json!({"version":1,"jobs":[job("platter","completed-job","completed")]}),
            ),
            (
                list_route("job-packets"),
                json!({"version":1,"jobs":[job("job-packets","old-orphan","running")]}),
            ),
            (
                "POST /v1/jobs/new-orphan/cancel ".into(),
                json!({"version":1,"jobId":"new-orphan","state":"cancelled","cancellationRequested":true}),
            ),
            (
                "POST /v1/jobs/old-orphan/cancel ".into(),
                json!({"version":1,"jobId":"old-orphan","state":"cancelled","cancellationRequested":true}),
            ),
            (list_route("platter"), empty_page()),
            (list_route("job-packets"), empty_page()),
        ],
    )?;
    let output = cli(&home, &socket)
        .env("CELL_DEPLOYMENT_RUN_ID", "deploy-one")
        .arg("--state-dir")
        .arg(&root)
        .args(["--json", "maintenance", "drain"])
        .output()?;
    let value = response(&output)?;
    let routes = server.join().unwrap()?;
    assert!(routes[1].contains("after=new-orphan"));
    assert_eq!(
        routes
            .iter()
            .filter(|route| route.starts_with("POST "))
            .count(),
        2
    );
    assert_eq!(value["data"]["drained"], true);
    assert_eq!(value["data"]["holds"], json!(["deploy-one"]));
    assert_eq!(state_bytes(&root)?, before);
    Ok(())
}

#[test]
fn active_admission_prevents_cancelling_a_running_stage() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let home = fixture.path().join("home");
    let root = home.join(".local/share/platter");
    let gate = maintenance::gate(&home);
    let _activity = gate.enter()?;
    gate.hold("deploy-one")?;
    let socket = fixture.path().join("n.sock");
    let server = mailbox(
        &socket,
        vec![
            (
                list_route("platter"),
                json!({"version":1,"jobs":[job("platter","active-stage","running")]}),
            ),
            (list_route("job-packets"), empty_page()),
        ],
    )?;
    let output = cli(&home, &socket)
        .env("CELL_DEPLOYMENT_RUN_ID", "deploy-one")
        .arg("--state-dir")
        .arg(&root)
        .args(["--json", "maintenance", "drain"])
        .output()?;
    assert_eq!(response(&output)?["data"]["drained"], false);
    assert!(
        server
            .join()
            .unwrap()?
            .iter()
            .all(|route| route.starts_with("GET "))
    );
    assert!(Store::open_read_only(&root)?.list()?.is_empty());
    Ok(())
}

#[test]
fn default_state_preserves_legacy_paths_and_rejects_ambiguity() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let home = fixture.path();
    let current = home.join(".local/share/platter");
    let legacy = home.join(".local/share/job-packets");
    assert_eq!(platter::default_state_dir(home)?, current);
    assert!(!current.exists());
    fs::create_dir_all(&legacy)?;
    fs::write(legacy.join("receipt.json"), b"immutable delivery identity")?;
    assert_eq!(platter::default_state_dir(home)?, legacy);
    fs::create_dir_all(&current)?;
    assert!(platter::default_state_dir(home).is_err());
    assert_eq!(
        fs::read(legacy.join("receipt.json"))?,
        b"immutable delivery identity"
    );
    Ok(())
}

#[test]
fn status_and_local_state_are_read_only_and_backup_preserves_schema() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let home = fixture.path().join("home");
    let root = home.join(".local/share/platter");
    assert!(!readiness::local_state(&root)?);
    assert!(!root.exists());
    frozen_state(&root, &home)?;
    let before = state_bytes(&root)?;
    assert!(readiness::local_state(&root)?);
    let output = cli(&home, &fixture.path().join("unused.sock"))
        .arg("--state-dir")
        .arg(&root)
        .arg("status")
        .output()?;
    assert!(output.status.success());
    assert_eq!(state_bytes(&root)?, before);
    let backup = fixture.path().join("backup/packets.sqlite3");
    Store::open_read_only(&root)?.backup(&backup)?;
    assert!(
        Store::open_read_only(backup.parent().unwrap())?
            .list()?
            .is_empty()
    );
    assert_eq!(fs::metadata(&backup)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(state_bytes(&root)?, before);
    let retained_backup = fs::read(&backup)?;
    // Repeating the same pre-install backup is safe; it must not replace it.
    Store::open_read_only(&root)?.backup(&backup)?;
    assert_eq!(fs::read(&backup)?, retained_backup);
    Store::open(&root)?.insert(&platter::store::PacketRecord {
        id: "new-packet".into(),
        opportunity: "fixture:changed-source".into(),
        job_id: "new-discovery".into(),
        company: "Fixture".into(),
        title: "Engineer".into(),
        status: "ready".into(),
        directory: root.join("new-packet").display().to_string(),
    })?;
    assert!(Store::open_read_only(&root)?.backup(&backup).is_err());
    assert_eq!(fs::read(&backup)?, retained_backup);
    let connection = rusqlite::Connection::open(root.join("packets.sqlite3"))?;
    connection.pragma_update(None, "user_version", platter::store::SCHEMA_VERSION + 1)?;
    drop(connection);
    let unsupported = state_bytes(&root)?;
    assert!(readiness::local_state(&root).is_err());
    assert_eq!(state_bytes(&root)?, unsupported);
    Ok(())
}

fn fake_prerequisites(home: &Path) -> Result<(PathBuf, PathBuf)> {
    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin)?;
    for name in ["cast", "crm", "email", "tectonic", "python3"] {
        let path = bin.join(name);
        let body = match name {
            "email" => "case \"$1\" in --version) echo 'email 0.5.2';; --help) echo '--payload-stdin';; *) exit 93;; esac".to_owned(),
            "python3" => "test \"$1\" = '-c' || exit 94; echo 'pypdf ready'".to_owned(),
            _ => format!("test \"$1\" = '--version' || exit 95; echo '{name} 0.1.0'"),
        };
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n"))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok((bin.join("tectonic"), bin.join("python3")))
}

#[test]
fn doctor_checks_fake_prerequisites_without_creating_private_state() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let home = fixture.path().join("home");
    let root = home.join(".local/share/platter");
    let (tectonic, python) = fake_prerequisites(&home)?;
    let socket = fixture.path().join("n.sock");
    let server = mailbox(
        &socket,
        vec![(
            "GET /v1/health ".into(),
            json!({
                "version":1,"status":"ok","daemonVersion":"0.5.0","acceptingJobs":true,
                "checkedAt":"2026-09-06T21:00:00Z","supportedProtocolVersions":[1],
                "harness":{"harness":"codex","harnessVersion":"proved","adapterVersion":"1"},
                "capabilities":["exact-model","reasoning-effort","workspace-none","builtin-local-execution","builtin-web-search","dynamic-client-tools"],
                "authentication":{"codexHome":"/fixture/nucleus","configured":true,"authenticated":true},
                "execution":{"maxActiveJobs":8,"activeJobs":0,"availableSlots":8}
            }),
        )],
    )?;
    let output = cli(&home, &socket)
        .env("PLATTER_TECTONIC", tectonic)
        .env("PLATTER_PYTHON", python)
        .arg("--state-dir")
        .arg(&root)
        .args(["--json", "doctor"])
        .output()?;
    let value = response(&output)?;
    assert_eq!(value["data"]["ready"], true);
    assert_eq!(value["data"]["local"]["initialized"], false);
    assert!(!root.exists());
    assert!(!maintenance::gate(&home).path()?.exists());
    assert_eq!(server.join().unwrap()?.len(), 1);
    Ok(())
}
