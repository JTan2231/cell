#![allow(clippy::unwrap_used, clippy::expect_used)] // Isolated installer fixtures.

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    source: PathBuf,
    candidate_dir: PathBuf,
    candidate: Value,
    selected: bool,
}

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn inventory(root: &Path, directory: &Path, output: &mut BTreeMap<String, String>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            inventory(root, &path, output);
        } else {
            output.insert(
                path.strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned(),
                digest(&path),
            );
        }
    }
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let home = root.join("home");
        fs::create_dir(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        let source = root.join("source");
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("chancery"),
            &source.join("platter/chancery"),
        );
        fs::create_dir(root.join("run")).unwrap();
        let mut value = Self {
            _temporary: temporary,
            root,
            home,
            source,
            candidate_dir: PathBuf::new(),
            candidate: Value::Null,
            selected: true,
        };
        value.prepare("first", false);
        value
    }

    fn prepare(&mut self, revision: &str, fail_help: bool) {
        self.candidate_dir = self.root.join(format!("candidate-{revision}"));
        fs::create_dir_all(self.candidate_dir.join("bin")).unwrap();
        let installer = self.candidate_dir.join("bin/platter-install");
        fs::copy(env!("CARGO_BIN_EXE_platter-install"), installer).unwrap();
        let program = self.candidate_dir.join("bin/platter");
        fs::write(
            &program,
            format!(
                r#"#!/bin/sh
# Fixture revision: {revision}
case "${{1-}}" in
--version) printf '%s\n' 'platter {version}'; exit 0;;
--help) exit {help_status};;
--json) shift;;
*) exit 2;;
esac
printf '%s\n' '{revision} '"$*" >> "$HOME/operations"
case "${{1-}}" in
maintenance)
  case "${{2-}}" in
  hold) printf '%s' "$3" > "$HOME/hold";;
  release) if [ -f "$HOME/hold" ] && [ "$(/bin/cat "$HOME/hold")" = "$3" ]; then /bin/rm "$HOME/hold"; fi;;
  status|drain) ;;
  *) exit 2;;
  esac
  holds='[]'
  if [ -f "$HOME/hold" ]; then holds='["'"$(/bin/cat "$HOME/hold")"'"]'; fi
  drained=true
  if [ -f "$HOME/busy" ]; then drained=false; fi
  printf '{{"ok":true,"data":{{"protocol_version":1,"holds":%s,"drained":%s}}}}\n' "$holds" "$drained";;
migrate)
  [ ! -f "$HOME/incompatible" ] || exit 9
  [ ! -f "$HOME/doctor-failure" ] || exit 9
  [ -f "$HOME/hold" ] || exit 8
  [ "$(/bin/cat "$HOME/hold")" = "$CELL_DEPLOYMENT_RUN_ID" ] || exit 8
  [ ! -f "$HOME/busy" ] || exit 8
  [ "${{2-}}" = '--backup' ] || exit 7
  case "${{3-}}" in "$HOME/.local/share/platter/backups/migration-"*.sqlite|"$HOME/.local/share/job-packets/backups/migration-"*.sqlite) ;; *) exit 7;; esac
  printf '{{"ok":true,"data":{{"compatible":true}}}}\n';;
doctor)
  [ ! -f "$HOME/incompatible" ] || exit 9
  if [ "${{2-}}" != '--state-only' ]; then
    [ ! -f "$HOME/doctor-failure" ] || exit 9
  fi
  printf '{{"ok":true,"data":{{"compatible":true}}}}\n';;
*) exit 2;;
esac
"#,
                version = env!("CARGO_PKG_VERSION"),
                help_status = if fail_help { 7 } else { 0 },
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        self.seal();
    }

    fn seal(&mut self) {
        let mut sources = BTreeMap::new();
        inventory(
            &self.source,
            &self.source.join("platter/chancery"),
            &mut sources,
        );
        let binaries: BTreeMap<_, _> = ["platter", "platter-install"]
            .into_iter()
            .map(|name| {
                (
                    name,
                    json!({"path":format!("bin/{name}"),"sha256":digest(&self.candidate_dir.join("bin").join(name)),"version":format!("{name} {}",env!("CARGO_PKG_VERSION"))}),
                )
            })
            .collect();
        self.candidate = json!({
            "schema":1,"product":"platter","source_commit":"fixture-commit",
            "source_key":"fixture-source-key","source_inputs":sources,"binaries":binaries
        });
        let bytes = format!("{}\n", self.candidate);
        self.candidate["candidate_id"] =
            json!(format!("sha256:{:x}", Sha256::digest(bytes.as_bytes())));
    }

    #[allow(clippy::needless_pass_by_value)] // Fixture requests own inline JSON values.
    fn adapter(&self, operation: &str, prior: Option<&Value>, recovery: Value) -> Output {
        let request = json!({
            "schema":1,"product":"platter","run_id":"fixture-run",
            "run_dir":self.root.join("run"),"source_root":self.source,
            "candidate_dir":self.candidate_dir,"candidate":self.candidate,
            "prior":prior,"selected_products":if self.selected {vec!["platter"]} else {vec![]},"recovery":recovery
        });
        let mut child = Command::new(self.candidate_dir.join("bin/platter-install"))
            .args(["adapter", operation])
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn success(&self, operation: &str, prior: Option<&Value>, recovery: Value) -> Value {
        let output = self.adapter(operation, prior, recovery);
        assert!(
            output.status.success(),
            "{operation}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone()
    }

    fn begin(&self) -> Value {
        let prior = self.success("inspect", None, Value::Null);
        self.success("hold", Some(&prior), Value::Null);
        self.success("drain", Some(&prior), Value::Null);
        prior
    }

    fn install(&self) -> String {
        let prior = self.begin();
        let applied = self.success("apply", Some(&prior), Value::Null);
        self.success("verify", Some(&prior), Value::Null);
        self.success("release", Some(&prior), Value::Null);
        applied["release_id"].as_str().unwrap().to_owned()
    }

    fn install_root(&self) -> PathBuf {
        self.home
            .join("Library/Application Support/Platter/install")
    }
}

#[test]
fn fresh_repeat_upgrade_and_verified_recovery_preserve_state() {
    let mut fixture = Fixture::new();
    let state = fixture.home.join(".local/share/job-packets");
    fs::create_dir_all(&state).unwrap();
    fs::write(
        state.join("private-state"),
        b"accepted work and send receipts",
    )
    .unwrap();
    let before = fixture.success("inspect", None, Value::Null);
    assert!(before["installation"]["current"].is_null());
    assert!(!fixture.install_root().exists());
    let first = fixture.install();
    assert_eq!(fixture.install(), first);
    assert!(!fixture.install_root().join("previous").exists());
    fixture.prepare("second", false);
    let second = fixture.install();
    assert_ne!(first, second);
    assert_eq!(
        fs::read_link(fixture.install_root().join("previous")).unwrap(),
        PathBuf::from(format!("releases/{first}"))
    );
    let prior = fixture.begin();
    fixture.success("apply", Some(&prior), Value::Null);
    let recovered = fixture.success(
        "recover",
        Some(&prior),
        json!({"any_apply_started":true,"verified":false}),
    );
    assert_eq!(recovered["safe_to_release"], true);
    fixture.success("release", Some(&prior), Value::Null);
    assert_eq!(
        fs::read(state.join("private-state")).unwrap(),
        b"accepted work and send receipts"
    );
    assert!(fixture.home.join(".local/bin/platter").exists());
    assert!(fixture.home.join(".local/bin/platter-install").exists());
    assert!(
        fixture
            .home
            .join("Library/Application Support/Chancery/providers/platter")
            .exists()
    );
}

#[test]
fn publication_failure_restores_prior_and_keeps_hold_for_recovery() {
    let mut fixture = Fixture::new();
    let first = fixture.install();
    fixture.prepare("broken-help", true);
    let prior = fixture.begin();
    let output = fixture.adapter("apply", Some(&prior), Value::Null);
    assert!(!output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["data"]["error"]["disposition"], "restored");
    assert_eq!(
        fs::read_link(fixture.install_root().join("current")).unwrap(),
        PathBuf::from(format!("releases/{first}"))
    );
    assert_eq!(
        fs::read_to_string(fixture.home.join("hold")).unwrap(),
        "fixture-run"
    );
    let recovered = fixture.success(
        "recover",
        Some(&prior),
        json!({"any_apply_started":true,"verified":false}),
    );
    assert_eq!(recovered["installed"], "prior");
    fixture.success("release", Some(&prior), Value::Null);
}

#[test]
fn fresh_publication_failure_recovers_the_absent_installation() {
    let mut fixture = Fixture::new();
    fixture.prepare("broken-first-install", true);
    let prior = fixture.begin();
    assert!(
        !fixture
            .adapter("apply", Some(&prior), Value::Null)
            .status
            .success()
    );
    assert!(!fixture.install_root().join("current").exists());
    let recovered = fixture.success(
        "recover",
        Some(&prior),
        json!({"any_apply_started":true,"verified":false}),
    );
    assert_eq!(recovered["installed"], "prior");
    assert_eq!(recovered["safe_to_release"], true);
    fixture.success("release", Some(&prior), Value::Null);
    assert!(!fixture.home.join("hold").exists());
}

#[test]
fn direct_mutation_cannot_bypass_maintenance() {
    let fixture = Fixture::new();
    assert!(
        platter::installation::specification()
            .legacy(&fixture.root)
            .unwrap_err()
            .message
            .contains("no predecessor release format")
    );
    for operation in ["install", "recover"] {
        let output = Command::new(env!("CARGO_BIN_EXE_platter-install"))
            .arg(operation)
            .env_clear()
            .env("HOME", &fixture.home)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("deployment coordinator"));
    }
    assert!(!fixture.install_root().exists());
}

#[test]
fn apply_requires_drained_sole_hold_and_compatible_state() {
    let fixture = Fixture::new();
    let prior = fixture.success("inspect", None, Value::Null);
    assert!(
        !fixture
            .adapter("apply", Some(&prior), Value::Null)
            .status
            .success()
    );
    fixture.success("hold", Some(&prior), Value::Null);
    fs::write(fixture.home.join("busy"), b"existing work").unwrap();
    assert!(
        !fixture
            .adapter("apply", Some(&prior), Value::Null)
            .status
            .success()
    );
    fs::remove_file(fixture.home.join("busy")).unwrap();
    fs::write(fixture.home.join("incompatible"), b"unsupported schema").unwrap();
    assert!(
        !fixture
            .adapter("apply", Some(&prior), Value::Null)
            .status
            .success()
    );
    assert!(!fixture.install_root().join("current").exists());
    assert_eq!(
        fs::read_to_string(fixture.home.join("hold")).unwrap(),
        "fixture-run"
    );
    fs::remove_file(fixture.home.join("incompatible")).unwrap();
    fs::write(fixture.home.join("hold"), b"another-owner").unwrap();
    assert!(
        !fixture
            .adapter("apply", Some(&prior), Value::Null)
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(fixture.home.join("hold")).unwrap(),
        "another-owner"
    );
}

#[test]
fn sealed_candidate_provider_and_owned_selectors_are_required() {
    let fixture = Fixture::new();
    fs::write(fixture.candidate_dir.join("bin/platter"), b"changed bytes").unwrap();
    assert!(
        !fixture
            .adapter("inspect", None, Value::Null)
            .status
            .success()
    );
    assert!(!fixture.install_root().exists());

    let fixture = Fixture::new();
    fs::write(
        fixture.source.join("platter/chancery/extra"),
        b"unsealed provider",
    )
    .unwrap();
    assert!(
        !fixture
            .adapter("inspect", None, Value::Null)
            .status
            .success()
    );

    let fixture = Fixture::new();
    fs::create_dir_all(fixture.home.join(".local/bin")).unwrap();
    let public = fixture.home.join(".local/bin/platter");
    symlink("/foreign/platter", &public).unwrap();
    assert!(
        !fixture
            .adapter("inspect", None, Value::Null)
            .status
            .success()
    );
    assert_eq!(
        fs::read_link(public).unwrap(),
        Path::new("/foreign/platter")
    );
}

#[test]
fn failed_verification_and_release_tampering_keep_maintenance() {
    let fixture = Fixture::new();
    let prior = fixture.begin();
    let release = fixture.success("apply", Some(&prior), Value::Null);
    fs::write(
        fixture.home.join("doctor-failure"),
        b"dependency unavailable",
    )
    .unwrap();
    assert!(
        !fixture
            .adapter("verify", Some(&prior), Value::Null)
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(fixture.home.join("hold")).unwrap(),
        "fixture-run"
    );
    fs::remove_file(fixture.home.join("doctor-failure")).unwrap();
    let binary = fixture
        .install_root()
        .join("releases")
        .join(release["release_id"].as_str().unwrap())
        .join("bin/platter");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(binary, b"tampered installed payload").unwrap();
    assert!(
        !fixture
            .adapter("verify", Some(&prior), Value::Null)
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(fixture.home.join("hold")).unwrap(),
        "fixture-run"
    );
}

#[test]
fn affected_absent_platter_checks_state_without_requiring_runtime_dependencies() {
    let mut fixture = Fixture::new();
    fixture.selected = false;
    fs::write(
        fixture.home.join("doctor-failure"),
        b"Email lacks attachments",
    )
    .unwrap();
    let prior = fixture.begin();
    fixture.success("verify", Some(&prior), Value::Null);
    assert!(
        fs::read_to_string(fixture.home.join("operations"))
            .unwrap()
            .contains("doctor --state-only")
    );
    fixture.success("release", Some(&prior), Value::Null);
    assert!(!fixture.install_root().join("current").exists());
    assert!(!fixture.home.join("hold").exists());
}

#[test]
fn failed_prerequisites_recover_compatible_absent_and_installed_prior() {
    for installed in [false, true] {
        let mut fixture = Fixture::new();
        let expected = if installed {
            Some(fixture.install())
        } else {
            None
        };
        fixture.prepare("candidate-with-unavailable-dependency", false);
        let prior = fixture.begin();
        fs::write(
            fixture.home.join("doctor-failure"),
            b"Email lacks attachments",
        )
        .unwrap();
        assert!(
            !fixture
                .adapter("apply", Some(&prior), Value::Null)
                .status
                .success()
        );
        let recovered = fixture.success(
            "recover",
            Some(&prior),
            json!({"any_apply_started":true,"verified":false}),
        );
        assert_eq!(recovered["installed"], "prior");
        assert_eq!(recovered["safe_to_release"], true);
        assert_eq!(
            recovered["installation"]["current"]["release_id"],
            json!(expected)
        );
        fixture.success("release", Some(&prior), Value::Null);
        assert!(!fixture.home.join("hold").exists());
    }
}

#[test]
fn prior_recovery_still_requires_state_compatibility_and_selected_verify_full_readiness() {
    let fixture = Fixture::new();
    fixture.install();
    let prior = fixture.begin();
    fixture.success("apply", Some(&prior), Value::Null);
    fs::write(
        fixture.home.join("doctor-failure"),
        b"dependency unavailable",
    )
    .unwrap();
    // Even when exact bytes were already selected, selected verification must
    // prove readiness to use this release.
    assert!(
        !fixture
            .adapter("verify", Some(&prior), Value::Null)
            .status
            .success()
    );
    fs::write(fixture.home.join("incompatible"), b"unsupported state").unwrap();
    assert!(
        !fixture
            .adapter(
                "recover",
                Some(&prior),
                json!({"any_apply_started":true,"verified":false})
            )
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(fixture.home.join("hold")).unwrap(),
        "fixture-run"
    );
}
