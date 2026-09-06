#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
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
    daemon: PathBuf,
    codex: PathBuf,
    provider: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self> {
        let root = tempfile::tempdir()?;
        let home = root.path().canonicalize()?.join("Operator Home");
        fs::create_dir(&home)?;
        let binary = root.path().join("nucleus");
        executable(
            &binary,
            &format!(
                r#"#!/bin/sh
set -eu
case "${{1:-}}" in
 --version) echo 'nucleus {}'; exit 0 ;;
 --help) echo 'fake Nucleus'; exit 0 ;;
esac
[ "$1" = service ] && [ "$2" = install ] && [ "$3" = --daemon ]
[ ! -f "$HOME/fail-before" ]
mkdir -p "$HOME/.local/bin" "$HOME/.local/libexec"
cp "$0" "$HOME/.local/bin/nucleus"
cp "$4" "$HOME/.local/libexec/nucleusd"
[ ! -f "$HOME/fail-after" ]
"#,
                env!("CARGO_PKG_VERSION")
            ),
        )?;
        let daemon = root.path().join("nucleusd");
        executable(
            &daemon,
            &format!(
                "#!/bin/sh\ncase \"$1\" in --version) echo 'nucleusd {}';; --help) echo daemon;; *) exit 64;; esac\n",
                env!("CARGO_PKG_VERSION")
            ),
        )?;
        let codex = root.path().join("codex");
        executable(
            &codex,
            "#!/bin/sh\n[ \"$1\" = --version ]; echo 'codex fixture'\n",
        )?;
        let provider = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../chancery")
            .canonicalize()?;
        Ok(Self {
            _root: root,
            home,
            binary,
            daemon,
            codex,
            provider,
        })
    }

    fn install(&self) -> Result<Output> {
        Ok(Command::new(env!("CARGO_BIN_EXE_nucleus-install"))
            .args(["install", "--binary"])
            .arg(&self.binary)
            .arg("--daemon")
            .arg(&self.daemon)
            .arg("--codex")
            .arg(&self.codex)
            .arg("--bundle")
            .arg(&self.provider)
            .arg("--home")
            .arg(&self.home)
            .env_remove("CELL_DEPLOYMENT_RUN_ID")
            .output()?)
    }

    fn current(&self) -> Result<PathBuf> {
        Ok(fs::read_link(self.home.join(
            "Library/Application Support/Nucleus/install/current",
        ))?)
    }

    fn update(&self) -> Result {
        let mut bytes = fs::read(&self.binary)?;
        bytes.extend_from_slice(b"\n# updated CLI\n");
        fs::write(&self.binary, bytes)?;
        Ok(())
    }
}

#[test]
fn service_owns_public_copies_and_package_is_idempotent() -> Result {
    let fixture = Fixture::new()?;
    let result = fixture.install()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let current = fixture.current()?;
    assert!(!fixture.home.join(".local/bin/nucleus").is_symlink());
    assert_eq!(
        fs::read(fixture.home.join(".local/bin/nucleus"))?,
        fs::read(&fixture.binary)?
    );
    assert!(fixture.install()?.status.success());
    assert_eq!(fixture.current()?, current);
    Ok(())
}

#[test]
fn failed_service_before_program_change_restores_package() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install()?.status.success());
    let before = fixture.current()?;
    fixture.update()?;
    fs::write(fixture.home.join("fail-before"), b"")?;
    assert!(!fixture.install()?.status.success());
    assert_eq!(fixture.current()?, before);
    Ok(())
}

#[test]
fn failed_service_with_retained_candidate_keeps_matching_package() -> Result {
    let fixture = Fixture::new()?;
    assert!(fixture.install()?.status.success());
    let before = fixture.current()?;
    fixture.update()?;
    fs::write(fixture.home.join("fail-after"), b"")?;
    let result = fixture.install()?;
    assert!(!result.status.success());
    assert_ne!(fixture.current()?, before);
    assert_eq!(
        fs::read(fixture.home.join(".local/bin/nucleus"))?,
        fs::read(&fixture.binary)?
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("candidate programs retained"));
    Ok(())
}
