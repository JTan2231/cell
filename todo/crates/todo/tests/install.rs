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
        executable(
            &home.join(".local/bin/clockwork"),
            include_str!("fixtures/clockwork.py"),
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
        Ok(self.install_command(fresh)?.output()?)
    }

    fn install_command(&self, fresh: bool) -> Result<Command> {
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
        Ok(command)
    }

    fn binding(&self) -> Result<serde_json::Value> {
        Ok(serde_json::from_slice(&fs::read(
            self.home.join("clockwork-binding.json"),
        )?)?)
    }

    fn disable_clockwork(&self) -> Result {
        let result = Command::new(self.home.join(".local/bin/clockwork"))
            .args(["--json", "binding", "disable", "todo/daily-email"])
            .env("HOME", &self.home)
            .output()?;
        assert!(result.status.success());
        Ok(())
    }

    fn make_legacy(&self, loaded: bool) -> Result {
        self.disable_clockwork()?;
        fs::remove_file(self.home.join("clockwork-binding.json"))?;
        let current = self.state().join("install").join(self.current()?);
        let template = current.join("package/org.todo.daily-email.plist");
        let out = Command::new("/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(template)
            .output()?;
        assert!(out.status.success());
        let mut value: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        value["WorkingDirectory"] = serde_json::json!(self.state());
        value["EnvironmentVariables"]["HOME"] = serde_json::json!(self.home);
        value["ProgramArguments"][1] =
            serde_json::json!(self.state().join("install/current/bin/todo-daily-email"));
        value["StandardOutPath"] =
            serde_json::json!(self.home.join("Library/Logs/Todo/email.stdout.log"));
        value["StandardErrorPath"] =
            serde_json::json!(self.home.join("Library/Logs/Todo/email.stderr.log"));
        fs::write(
            self.home
                .join("Library/LaunchAgents/org.todo.daily-email.plist"),
            serde_json::to_vec(&value)?,
        )?;
        if loaded {
            fs::write(self.home.join("loaded"), b"")?;
        }
        Ok(())
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
    assert!(fixture.home.join("clockwork-loaded").exists());
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
    fixture.disable_clockwork()?;
    fs::write(fixture.home.join("disabled"), b"")?;
    let config = fs::read(fixture.state().join("config.toml"))?;
    let result = fixture.install(false)?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(!fixture.home.join("clockwork-loaded").exists());
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
    fs::write(fixture.home.join("fail-switch"), b"")?;
    let result = fixture.install(false)?;
    assert!(!result.status.success());
    assert_eq!(fixture.current()?, before);
    assert!(fixture.home.join("clockwork-loaded").exists());
    assert_eq!(fs::read(fixture.state().join("config.toml"))?, config);
    assert!(!fixture.state().join("install/.update-lock").exists());
    Ok(())
}

#[test]
fn legacy_disabled_override_does_not_replace_clockwork_intent() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install(true)?.status.success());
    let before = fixture.current()?;
    fs::write(fixture.home.join("disabled"), b"")?;
    assert!(fixture.install(false)?.status.success());
    assert_eq!(fixture.current()?, before);
    assert!(fixture.home.join("clockwork-loaded").exists());
    assert!(fixture.home.join("disabled").exists());
    Ok(())
}

#[test]
fn foreign_binding_is_rejected_before_installation() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install(true)?.status.success());
    let before = fixture.current()?;
    let definition = fixture.binding()?["definition_digest"]
        .as_str()
        .ok_or("missing digest")?
        .to_owned();
    let path = fixture
        .home
        .join("clockwork-definitions")
        .join(format!("{definition}.json"));
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    value["manifest"]["arguments"] = serde_json::json!(["foreign"]);
    fs::write(path, serde_json::to_vec(&value)?)?;
    assert!(!fixture.install(false)?.status.success());
    assert_eq!(fixture.current()?, before);
    assert!(fixture.home.join("clockwork-loaded").exists());
    Ok(())
}

#[test]
fn failure_halt_survives_reinstallation() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install(true)?.status.success());
    let mut binding = fixture.binding()?;
    binding["halted_incident"] = serde_json::json!("incident-fixture");
    fs::write(
        fixture.home.join("clockwork-binding.json"),
        serde_json::to_vec(&binding)?,
    )?;
    assert!(fixture.install(false)?.status.success());
    assert_eq!(fixture.binding()?["halted_incident"], "incident-fixture");
    assert!(fixture.home.join("clockwork-loaded").exists());
    Ok(())
}

#[test]
fn legacy_schedule_handoff_preserves_enabled_state() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install(true)?.status.success());
    fixture.make_legacy(true)?;
    let result = fixture.install(false)?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(!fixture.home.join("loaded").exists());
    assert!(
        !fixture
            .home
            .join("Library/LaunchAgents/org.todo.daily-email.plist")
            .exists()
    );
    assert!(fixture.home.join("clockwork-loaded").exists());
    assert_eq!(fixture.binding()?["failure_policy_active"], true);
    Ok(())
}

#[test]
fn failed_handoff_restores_legacy_without_dual_activation() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install(true)?.status.success());
    fixture.make_legacy(true)?;
    fs::write(fixture.home.join("fail-switch"), b"")?;
    assert!(!fixture.install(false)?.status.success());
    assert!(fixture.home.join("loaded").exists());
    assert!(!fixture.home.join("clockwork-loaded").exists());
    assert!(
        fixture
            .home
            .join("Library/LaunchAgents/org.todo.daily-email.plist")
            .exists()
    );
    Ok(())
}

#[test]
fn coordinated_install_and_release_keep_schedule_disabled() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install(true)?.status.success());
    let owner = "todo-coordinated-fixture";
    let database = fixture.state().join("todo.db");
    let invoke = |operation: &str| -> Result<Output> {
        Ok(Command::new(env!("CARGO_BIN_EXE_todo"))
            .arg("--database")
            .arg(&database)
            .args(["--json", "maintenance", operation, owner])
            .env("HOME", &fixture.home)
            .env("NUCLEUS_SOCKET", &fixture.socket)
            .output()?)
    };
    assert!(invoke("hold")?.status.success());
    let result = fixture
        .install_command(false)?
        .env("CELL_DEPLOYMENT_RUN_ID", owner)
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert_eq!(fixture.binding()?["enabled"], false);
    assert!(invoke("release")?.status.success());
    assert_eq!(fixture.binding()?["enabled"], false);
    assert!(!fixture.home.join("clockwork-loaded").exists());
    Ok(())
}
