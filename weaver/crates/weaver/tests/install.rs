#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn executable(path: &Path, text: &str) -> Result {
    fs::write(path, text)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    binary: PathBuf,
    launchctl: PathBuf,
    provider: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self> {
        let root = tempfile::tempdir()?;
        let home = root.path().canonicalize()?.join("Operator Home");
        fs::create_dir(&home)?;
        let binary = root.path().join("weaver");
        executable(
            &binary,
            &format!(
                r#"#!/bin/sh
set -eu
case "${{1:-}}" in
 --version) echo 'weaver {}'; exit 0 ;;
 --help) echo 'fake Weaver'; exit 0 ;;
esac
state=${{WEAVER_STATE_DIR:?}}
mkdir -p "$state"
case "${{1:-}}" in
 doctor) [ ! -f "$state/fail-doctor" ] ;;
 maintenance)
  case "${{2:-}}" in
   begin) : >"$state/.maintenance" ;;
   end) [ ! -f "$state/fail-maintenance-end" ]; rm "$state/.maintenance" ;;
   *) exit 64 ;;
  esac ;;
 worker) [ -f "$state/.maintenance" ]; [ ! -f "$state/fail-worker" ] ;;
 *) exit 64 ;;
esac
"#,
                env!("CARGO_PKG_VERSION")
            ),
        )?;
        let launchctl = root.path().join("launchctl");
        executable(
            &launchctl,
            r#"#!/bin/sh
set -eu
case "$1" in
 print) if [ -f "$HOME/prototype.loaded" ]; then exit 0; else exit 113; fi ;;
 print-disabled) echo 'disabled services = {' ;;
 disable|enable) : ;;
 bootout)
  rm -f "$HOME/prototype.loaded"
  if [ -f "$HOME/change-prototype" ]; then echo foreign >"$HOME/Library/LaunchAgents/org.weaver.worker.plist"; exit 1; fi
  if [ -f "$HOME/fail-bootout" ]; then rm "$HOME/fail-bootout"; exit 1; fi ;;
 bootstrap) : >"$HOME/prototype.loaded" ;;
 *) exit 64 ;;
esac
"#,
        )?;
        let provider = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../chancery")
            .canonicalize()?;
        Ok(Self {
            _root: root,
            home,
            binary,
            launchctl,
            provider,
        })
    }

    fn install(&self) -> Result<Output> {
        Ok(Command::new(env!("CARGO_BIN_EXE_weaver-install"))
            .args(["install", "--binary"])
            .arg(&self.binary)
            .arg("--bundle")
            .arg(&self.provider)
            .arg("--home")
            .arg(&self.home)
            .arg("--launchctl")
            .arg(&self.launchctl)
            .args(["--wait-seconds", "0"])
            .env_remove("CELL_DEPLOYMENT_RUN_ID")
            .output()?)
    }

    fn state(&self) -> PathBuf {
        self.home.join("Library/Application Support/Weaver")
    }
}

#[test]
fn installs_exact_release_and_retains_operator_maintenance() -> Result {
    let fixture = Fixture::new()?;
    fs::create_dir_all(fixture.state())?;
    fs::write(fixture.state().join(".maintenance"), b"operator\n")?;
    let result = fixture.install()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(fixture.state().join(".maintenance").exists());
    let current = fs::read_link(fixture.state().join("install/current"))?;
    assert!(
        fixture
            .state()
            .join("install")
            .join(&current)
            .join("manifest.json")
            .is_file()
    );
    assert!(fixture.home.join(".local/bin/weaver-install").is_symlink());
    assert!(fixture.install()?.status.success());
    assert_eq!(
        fs::read_link(fixture.state().join("install/current"))?,
        current
    );
    assert!(!fixture.state().join("install/previous").exists());
    Ok(())
}

#[test]
fn failed_installed_smoke_restores_program_and_owned_maintenance() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install()?.status.success());
    let before = fs::read_link(fixture.state().join("install/current"))?;
    let mut bytes = fs::read(&fixture.binary)?;
    bytes.extend_from_slice(b"\n# second candidate\n");
    fs::write(&fixture.binary, bytes)?;
    fs::write(fixture.state().join("fail-worker"), b"")?;
    assert!(!fixture.install()?.status.success());
    assert_eq!(
        fs::read_link(fixture.state().join("install/current"))?,
        before
    );
    assert!(!fixture.state().join(".maintenance").exists());
    assert!(!fixture.state().join("install/.update-lock").exists());
    Ok(())
}

#[test]
fn foreign_provider_and_tampered_release_fail_without_cutover() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install()?.status.success());
    let before = fs::read_link(fixture.state().join("install/current"))?;
    let provider = fixture
        .home
        .join("Library/Application Support/Chancery/providers/weaver");
    let owned = fs::read_link(&provider)?;
    fs::remove_file(&provider)?;
    symlink("/foreign/provider", &provider)?;
    assert!(!fixture.install()?.status.success());
    assert_eq!(fs::read_link(&provider)?, Path::new("/foreign/provider"));
    fs::remove_file(&provider)?;
    symlink(owned, provider)?;
    fs::write(
        fixture
            .state()
            .join("install")
            .join(&before)
            .join("bin/weaver"),
        b"tampered",
    )?;
    assert!(!fixture.install()?.status.success());
    assert_eq!(
        fs::read_link(fixture.state().join("install/current"))?,
        before
    );
    Ok(())
}

#[test]
fn post_commit_maintenance_failure_keeps_new_program_selected() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install()?.status.success());
    let before = fs::read_link(fixture.state().join("install/current"))?;
    let mut bytes = fs::read(&fixture.binary)?;
    bytes.extend_from_slice(b"\n# second candidate\n");
    fs::write(&fixture.binary, bytes)?;
    fs::write(fixture.state().join("fail-maintenance-end"), b"")?;
    let result = fixture.install()?;
    assert!(!result.status.success());
    assert_ne!(
        fs::read_link(fixture.state().join("install/current"))?,
        before
    );
    assert!(fixture.state().join(".maintenance").exists());
    assert!(String::from_utf8_lossy(&result.stdout).contains("committed"));
    Ok(())
}

#[test]
fn prototype_bootout_failure_restores_exact_plist_and_loaded_state() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install()?.status.success());
    let before = fs::read_link(fixture.state().join("install/current"))?;
    let agent = fixture
        .home
        .join("Library/LaunchAgents/org.weaver.worker.plist");
    fs::create_dir_all(agent.parent().ok_or("missing agent parent")?)?;
    fs::write(&agent, b"captured prototype\n")?;
    fs::write(fixture.home.join("prototype.loaded"), b"")?;
    fs::write(fixture.home.join("fail-bootout"), b"")?;
    assert!(!fixture.install()?.status.success());
    assert_eq!(fs::read(&agent)?, b"captured prototype\n");
    assert!(fixture.home.join("prototype.loaded").exists());
    assert!(!fixture.state().join(".maintenance").exists());
    assert_eq!(
        fs::read_link(fixture.state().join("install/current"))?,
        before
    );
    Ok(())
}

#[test]
fn changed_prototype_is_never_bootstrapped_during_recovery() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install()?.status.success());
    let agent = fixture
        .home
        .join("Library/LaunchAgents/org.weaver.worker.plist");
    fs::create_dir_all(agent.parent().ok_or("missing agent parent")?)?;
    fs::write(&agent, b"captured prototype\n")?;
    fs::write(fixture.home.join("prototype.loaded"), b"")?;
    fs::write(fixture.home.join("change-prototype"), b"")?;
    let result = fixture.install()?;
    assert!(!result.status.success());
    assert_eq!(fs::read(&agent)?, b"foreign\n");
    assert!(!fixture.home.join("prototype.loaded").exists());
    assert!(fixture.state().join(".maintenance").exists());
    assert!(String::from_utf8_lossy(&result.stdout).contains("incomplete"));
    Ok(())
}
