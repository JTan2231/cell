#![cfg(target_os = "macos")]

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn executable(path: &Path, source: &str) -> Result {
    fs::write(path, source)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn answer_jobs(mut stream: UnixStream) -> io::Result<()> {
    // macOS accepts inherit the listener's nonblocking flag. A connection can
    // arrive before any HTTP bytes, and one read need not contain the header.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut request = Vec::new();
    let mut chunk = [0; 1024];
    while !request.windows(4).any(|value| value == b"\r\n\r\n") {
        let length = stream.read(&mut chunk)?;
        if length == 0 || request.len() + length > 8192 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete or oversized fixture request",
            ));
        }
        request.extend_from_slice(&chunk[..length]);
    }
    if !request.starts_with(b"GET /v1/jobs?") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected fixture request",
        ));
    }
    let body = r#"{"version":1,"jobs":[],"next":null}"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())
}

struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    launchctl: PathBuf,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
}

impl Fixture {
    fn new() -> Result<Self> {
        let root = tempfile::Builder::new()
            .prefix("todo-i-")
            .tempdir_in("/private/tmp")?;
        let home = root.path().join("home");
        fs::create_dir(&home)?;
        fs::create_dir_all(home.join(".local/bin"))?;
        executable(
            &home.join(".local/bin/nucleus"),
            "#!/bin/sh\n[ \"$1\" = --compact ] && [ \"$2\" = health ]; echo '{}'\n",
        )?;
        let launchctl = root.path().join("launchctl");
        executable(
            &launchctl,
            r#"#!/bin/sh
set -eu
case "$1" in
 print) if [ -f "$HOME/loaded" ]; then exit 0; else exit 113; fi ;;
 print-disabled)
  echo 'disabled services = {'
  if [ -f "$HOME/disabled" ]; then echo '"org.todo.daily-email" => true'; fi
  echo '}' ;;
 bootout)
  rm -f "$HOME/loaded"
  if [ -f "$HOME/change-plist" ]; then echo foreign >"$HOME/Library/LaunchAgents/org.todo.daily-email.plist"; fi ;;
 bootstrap)
  : >"$HOME/loaded"
  if [ -f "$HOME/fail-bootstrap" ]; then rm "$HOME/fail-bootstrap"; exit 1; fi ;;
 *) exit 64 ;;
esac
"#,
        )?;
        let socket = root.path().join("nucleus.sock");
        let listener = UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                if let Ok((stream, _)) = listener.accept() {
                    let _ = answer_jobs(stream);
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        Ok(Self {
            _root: root,
            home,
            launchctl,
            socket,
            stop,
        })
    }

    fn install(&self, fresh: bool) -> Result<Output> {
        let product = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_todo-install"));
        command
            .args(["install", "--binary"])
            .arg(env!("CARGO_BIN_EXE_todo"))
            .arg("--bundle")
            .arg(product.join("chancery"))
            .arg("--package")
            .arg(product.join("packaging/macos"))
            .arg("--home")
            .arg(&self.home)
            .arg("--launchctl")
            .arg(&self.launchctl)
            .env("NUCLEUS_SOCKET", &self.socket)
            .env_remove("TODO_CONFIG")
            .env_remove("TODO_DATABASE")
            .env_remove("CELL_DEPLOYMENT_RUN_ID");
        if fresh {
            command.args([
                "--email-to",
                "recipient@example.com",
                "--email-from",
                "sender@example.com",
            ]);
        }
        Ok(command.output()?)
    }

    fn state(&self) -> PathBuf {
        self.home.join("Library/Application Support/Todo")
    }
    fn current(&self) -> Result<PathBuf> {
        Ok(fs::read_link(self.state().join("install/current"))?)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[test]
fn runtime_fixture_waits_for_delayed_fragmented_request() -> Result {
    let fixture = Fixture::new()?;
    let mut client = UnixStream::connect(&fixture.socket)?;
    client.set_read_timeout(Some(Duration::from_secs(2)))?;
    std::thread::sleep(Duration::from_millis(25));
    client.write_all(b"GET /v1/jobs?requester_")?;
    std::thread::sleep(Duration::from_millis(25));
    client.write_all(b"program=todo HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
    let mut response = String::new();
    client.read_to_string(&mut response)?;
    assert!(response.ends_with(r#"{"version":1,"jobs":[],"next":null}"#));
    Ok(())
}

#[test]
fn first_install_uses_rust_frontend_and_never_runs_email() -> Result {
    let fixture = Fixture::new()?;
    let result = fixture.install(true)?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(fixture.home.join("loaded").exists());
    let result = Command::new(fixture.home.join(".local/bin/todo"))
        .args(["--json", "list", "--limit", "1"])
        .env("HOME", &fixture.home)
        .env_remove("TODO_CONFIG")
        .env_remove("TODO_DATABASE")
        .env_remove("TODO_STATE_DIR")
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let current = fixture.current()?;
    assert!(fixture.install(false)?.status.success());
    assert_eq!(fixture.current()?, current);
    assert!(!fixture.state().join("install/previous").exists());
    Ok(())
}

#[test]
fn update_preserves_unloaded_disabled_schedule_and_config_bytes() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install(true)?.status.success());
    fs::remove_file(fixture.home.join("loaded"))?;
    fs::write(fixture.home.join("disabled"), b"")?;
    let config = fs::read(fixture.state().join("config.toml"))?;
    let result = fixture.install(false)?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(!fixture.home.join("loaded").exists());
    assert!(fixture.home.join("disabled").exists());
    assert_eq!(fs::read(fixture.state().join("config.toml"))?, config);
    Ok(())
}

#[test]
fn failed_bootstrap_restores_prior_schedule_and_program() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install(true)?.status.success());
    let before = fixture.current()?;
    let config = fs::read(fixture.state().join("config.toml"))?;
    fs::write(fixture.home.join("fail-bootstrap"), b"")?;
    let result = fixture.install(false)?;
    assert!(!result.status.success());
    assert_eq!(fixture.current()?, before);
    assert!(fixture.home.join("loaded").exists());
    assert_eq!(fs::read(fixture.state().join("config.toml"))?, config);
    assert!(!fixture.state().join("install/.update-lock").exists());
    Ok(())
}

#[test]
fn loaded_disabled_service_is_preserved_without_cutover() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install(true)?.status.success());
    let before = fixture.current()?;
    fs::write(fixture.home.join("disabled"), b"")?;
    assert!(!fixture.install(false)?.status.success());
    assert_eq!(fixture.current()?, before);
    assert!(fixture.home.join("loaded").exists());
    assert!(fixture.home.join("disabled").exists());
    Ok(())
}

#[test]
fn independent_plist_change_retains_hold_and_never_bootstraps_foreign_bytes() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install(true)?.status.success());
    fs::write(fixture.home.join("change-plist"), b"")?;
    let result = fixture.install(false)?;
    assert!(!result.status.success());
    assert!(!fixture.home.join("loaded").exists());
    assert_eq!(
        fs::read(
            fixture
                .home
                .join("Library/LaunchAgents/org.todo.daily-email.plist")
        )?,
        b"foreign\n"
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("recovery is incomplete"));
    assert!(
        fs::read_dir(fixture.state().join("install"))?
            .filter_map(std::result::Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".transaction."))
    );
    Ok(())
}
