//! Synthetic executable-boundary tests; no real scheduler or product state.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)] // Isolated fixture setup is expected to succeed.

use sha2::{Digest as _, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

fn write(path: &Path, bytes: impl AsRef<[u8]>, mode: u32) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temporary.path()).unwrap();
        let home = root.join("Home");
        fs::create_dir(&home).unwrap();
        fs::create_dir(root.join("definitions")).unwrap();
        fs::create_dir(root.join("bindings")).unwrap();
        write(
            &root.join("krisis"),
            format!(
                r#"#!/bin/sh
set -eu
case " $* " in
  *' --version '*) printf '%s\n' 'krisis {}';;
  *' doctor '*)
    [ -z "${{KRISIS_TEST_SECRET:-}}" ] || exit 70
    if [ -f "$HOME/expected-owner" ]; then
      [ "${{CELL_DEPLOYMENT_RUN_ID:-}}" = "$(cat "$HOME/expected-owner")" ] || exit 72
    fi
    database="$HOME/Library/Application Support/Decisions/decisions.db"
    printf '%s\n' 'candidate database' >"$database"
    chmod 600 "$database"
    [ ! -f "$HOME/fail-doctor" ] || exit 71
    printf '%s\n' '{{"ok":true,"schema_version":6,"annals_library_id":"0123456789abcdef0123456789abcdef"}}';;
  *' observe activate '*) : >"$HOME/activated";;
esac
"#,
                env!("CARGO_PKG_VERSION")
            ),
            0o755,
        );
        for name in ["annals", "codex"] {
            write(&root.join(name), "#!/bin/sh\nexit 0\n", 0o755);
        }
        write(
            &root.join("launchctl"),
            "#!/bin/sh\n[ \"$1\" != print ]\n",
            0o755,
        );
        write(&root.join("config.toml"), "fixture\n", 0o600);
        write(&root.join("clockwork"), CLOCKWORK, 0o755);
        Self {
            _temporary: temporary,
            root,
            home,
        }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_krisis-install"));
        command
            .env("HOME", &self.home)
            .env("KRISIS_TEST_SECRET", "must not reach payload");
        command
    }
    fn install(&self, flags: &[&str]) -> Output {
        self.install_with_config(flags, &self.root.join("config.toml"))
    }
    fn install_with_config(&self, flags: &[&str], config: &Path) -> Output {
        self.command()
            .arg("install")
            .arg("--binary")
            .arg(self.root.join("krisis"))
            .arg("--source-root")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .arg("--home")
            .arg(&self.home)
            .arg("--clockwork")
            .arg(self.root.join("clockwork"))
            .arg("--codex")
            .arg(self.root.join("codex"))
            .arg("--annals")
            .arg(self.root.join("annals"))
            .arg("--annals-config")
            .arg(config)
            .args([
                "--annals-library-id",
                "0123456789abcdef0123456789abcdef",
                "--launchctl",
            ])
            .arg(self.root.join("launchctl"))
            .args(flags)
            .output()
            .unwrap()
    }
    fn installed(&self) -> PathBuf {
        self.home
            .join("Library/Application Support/Decisions/install")
    }
    fn state(&self) -> PathBuf {
        self.home.join("Library/Application Support/Decisions")
    }
    fn current(&self) -> PathBuf {
        fs::canonicalize(self.installed().join("current")).unwrap()
    }
    fn success(output: &Output) {
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fn failure(output: &Output) {
        assert!(
            !output.status.success(),
            "unexpected success: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

const CLOCKWORK: &str = r"#!/usr/bin/python3
import hashlib,json,os,pathlib,sys
root=pathlib.Path(__file__).resolve().parent
args=sys.argv[1:]
def out(value): print(json.dumps({'ok':True,'data':value},separators=(',',':')))
def fail(code):
 print(json.dumps({'ok':False,'error':{'code':code}}));sys.exit(1)
def binding(key): return root/'bindings'/key.replace('/','_')
if args[1:3]==['definition','register']:
 data={};section=data
 for raw in pathlib.Path(args[3]).read_text().splitlines():
  raw=raw.strip()
  if not raw or raw.startswith('#'): continue
  if raw.startswith('['):
   key=raw[1:-1];data[key]={};section=data[key]
  else:
   key,value=raw.split('=',1);section[key.strip()]=json.loads(value.strip())
 canonical={key:data[key] for key in ['schema_version','key','release_id','release_root','authority','overlap','arguments','cwd','schedule','launch','environment','output']}
 canonical['environment']=dict(sorted(canonical['environment'].items()))
 digest=hashlib.sha256(json.dumps(canonical,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()
 (root/'definitions'/digest).write_text(json.dumps(canonical))
 out({'digest':digest})
elif args[1:3]==['definition','show']:
 digest=args[3];source=root/'definitions'/digest
 if not source.exists(): fail('definition_not_found')
 manifest=json.loads(source.read_text());out({'digest':digest,'key':manifest['key'],'manifest':manifest})
elif args[1]=='binding':
 operation,key=args[2:4];path=binding(key)
 if operation=='show':
  if not path.exists(): fail('binding_not_found')
  out(json.loads(path.read_text()))
 elif operation in ['switch','disable']:
  prior=json.loads(path.read_text()) if path.exists() else {'definition_digest':None}
  digest=args[4] if operation=='switch' else (args[5] if len(args)>4 and args[4]=='--select' else prior['definition_digest'])
  value={'key':key,'enabled':operation=='switch','definition_digest':digest}
  path.write_text(json.dumps(value))
  replace=root/'replace-hook-on-disable'
  if operation=='disable' and replace.exists():
   replacement=json.loads(replace.read_text());replace.unlink()
   hook=pathlib.Path(replacement['path']);hook.unlink();os.symlink(replacement['target'],hook)
  failure=root/('fail-'+operation)
  if failure.exists(): failure.unlink();fail('injected_failure')
  out(value)
 else: fail('unknown_operation')
else: fail('unknown_operation')
";

#[test]
fn prepare_cutover_release_and_uninstall_preserve_owned_state() {
    let fixture = Fixture::new();
    Fixture::success(&fixture.install(&[]));
    assert!(!fixture.installed().join("current").exists());
    assert!(!fixture.state().join("decisions.db").exists());
    assert!(fixture.state().join(".clockwork-maintenance").exists());
    Fixture::success(&fixture.install(&["--final-cutover", "--keep-maintenance"]));
    assert!(fixture.home.join("activated").exists());
    let release = fixture.current();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(release.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["format"], "cell-install-v2");
    assert_eq!(
        fs::read(release.join("package/install")).unwrap(),
        fs::read(env!("CARGO_BIN_EXE_krisis-install")).unwrap()
    );
    for _ in 0..2 {
        Fixture::success(&fixture.install(&["--release-maintenance"]));
    }
    let before = fs::read(fixture.state().join("decisions.db")).unwrap();
    Fixture::success(
        &fixture
            .command()
            .arg("uninstall")
            .arg("--clockwork")
            .arg(fixture.root.join("clockwork"))
            .arg("--launchctl")
            .arg(fixture.root.join("launchctl"))
            .output()
            .unwrap(),
    );
    assert!(!fixture.home.join(".local/bin/krisis").exists());
    assert!(!fixture.home.join(".codex/hooks.json").exists());
    assert_eq!(fixture.current(), release);
    assert_eq!(
        fs::read(fixture.state().join("decisions.db")).unwrap(),
        before
    );
    Fixture::success(&fixture.install(&["--final-cutover"]));
}

#[test]
fn refuses_unowned_hook_and_unreceipted_gate_without_publishing() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.home.join(".codex")).unwrap();
    write(
        &fixture.home.join(".codex/hooks.json"),
        "foreign hook",
        0o600,
    );
    Fixture::failure(&fixture.install(&["--final-cutover"]));
    assert_eq!(
        fs::read(fixture.home.join(".codex/hooks.json")).unwrap(),
        b"foreign hook"
    );
    assert!(!fixture.installed().join("current").exists());
    write(&fixture.state().join(".clockwork-maintenance"), "", 0o600);
    Fixture::failure(&fixture.install(&[]));
    assert!(fixture.state().join(".clockwork-maintenance").exists());
}

#[test]
fn changed_release_and_foreign_public_selector_are_refused() {
    let fixture = Fixture::new();
    Fixture::success(&fixture.install(&["--final-cutover"]));
    let release = fixture.current();
    write(&release.join("bin/krisis-observer"), "tampered", 0o755);
    Fixture::failure(
        &fixture
            .command()
            .arg("verify-release")
            .arg(&release)
            .output()
            .unwrap(),
    );
    Fixture::failure(&fixture.install(&[]));
    let other = Fixture::new();
    fs::create_dir_all(other.home.join(".local/bin")).unwrap();
    symlink("/bin/true", other.home.join(".local/bin/krisis")).unwrap();
    Fixture::failure(&other.install(&["--final-cutover"]));
    assert_eq!(
        fs::read_link(other.home.join(".local/bin/krisis")).unwrap(),
        Path::new("/bin/true")
    );
}

#[test]
fn failed_candidate_restores_database_before_original_publication() {
    let fixture = Fixture::new();
    Fixture::success(&fixture.install(&["--final-cutover"]));
    let release = fixture.current();
    write(
        &fixture.state().join("decisions.db"),
        "original database",
        0o600,
    );
    let mut candidate = fs::read(fixture.root.join("krisis")).unwrap();
    candidate.extend_from_slice(b"\n# candidate two\n");
    write(&fixture.root.join("krisis"), candidate, 0o755);
    write(&fixture.home.join("fail-doctor"), "", 0o600);
    Fixture::failure(&fixture.install(&["--final-cutover"]));
    assert_eq!(fixture.current(), release);
    assert_eq!(
        fs::read(fixture.state().join("decisions.db")).unwrap(),
        b"original database"
    );
    assert!(fixture.home.join(".local/bin/krisis").exists());
    assert!(!fixture.state().join(".clockwork-maintenance").exists());
}

#[test]
fn uncertain_first_schedule_selection_retains_gate_and_evidence() {
    let fixture = Fixture::new();
    write(&fixture.root.join("fail-switch"), "", 0o600);
    Fixture::failure(&fixture.install(&["--final-cutover"]));
    assert!(fixture.state().join(".clockwork-maintenance").exists());
    assert!(fs::read_dir(fixture.installed()).unwrap().any(|item| {
        item.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".transaction")
    }));
    assert!(!fixture.home.join(".local/bin/krisis").exists());
    Fixture::failure(&fixture.install(&[]));
}

fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn bundle_proofs(
    source: &Path,
    path: &Path,
    proofs: &mut serde_json::Map<String, serde_json::Value>,
) {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            bundle_proofs(source, &path, proofs);
        } else {
            proofs.insert(
                path.strip_prefix(source).unwrap().to_str().unwrap().into(),
                digest(fs::read(&path).unwrap()).into(),
            );
        }
    }
}

#[test]
fn adapter_json_owner_reaches_doctor_without_an_ambient_owner() {
    use std::io::Write as _;
    use std::process::Stdio;

    let fixture = Fixture::new();
    let annals = fixture.home.join("Library/Application Support/Annals");
    fs::create_dir_all(annals.join("decisions")).unwrap();
    fs::create_dir_all(annals.join("install/current/libexec")).unwrap();
    let config = annals.join("decisions/config.toml");
    write(
        &config,
        "[decision_feed]\nexpected_library_id = \"0123456789abcdef0123456789abcdef\"\n",
        0o600,
    );
    symlink(
        fixture.root.join("annals"),
        annals.join("install/current/libexec/annals"),
    )
    .unwrap();
    Fixture::success(&fixture.install_with_config(&["--final-cutover"], &config));
    symlink(
        fixture.root.join("clockwork"),
        fixture.home.join(".local/bin/clockwork"),
    )
    .unwrap();

    let owner = "fixture-json-owned-deployment";
    write(&fixture.home.join("expected-owner"), owner, 0o600);
    let source = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")).unwrap();
    let candidate = fixture.root.join("candidate");
    fs::create_dir_all(candidate.join("bin")).unwrap();
    let mut binaries = serde_json::Map::new();
    for (name, executable) in [
        ("krisis", fixture.root.join("krisis")),
        (
            "krisis-install",
            PathBuf::from(env!("CARGO_BIN_EXE_krisis-install")),
        ),
    ] {
        let bytes = fs::read(executable).unwrap();
        let path = format!("bin/{name}");
        write(&candidate.join(&path), &bytes, 0o755);
        binaries.insert(
            name.into(),
            serde_json::json!({
                "path":path, "sha256":digest(&bytes),
                "version":format!("{name} {}",env!("CARGO_PKG_VERSION"))
            }),
        );
    }
    let mut proofs = serde_json::Map::new();
    for bundle in ["decisions/chancery", "decisions/chancery-legacy"] {
        bundle_proofs(&source, &source.join(bundle), &mut proofs);
    }
    let mut manifest = serde_json::json!({
        "schema":1,"product":"krisis","source_commit":"fixture","source_key":"fixture",
        "binaries":binaries,"source_inputs":proofs
    });
    let encoded = format!("{}\n", serde_json::to_string(&manifest).unwrap());
    manifest["candidate_id"] = format!("sha256:{}", digest(encoded)).into();
    let request = serde_json::json!({
        "schema":1,"product":"krisis","run_id":owner,"run_dir":fixture.root,
        "source_root":source,"candidate_dir":candidate,"candidate":manifest,
        "prior":{"controls":{},"annals_library_id":"0123456789abcdef0123456789abcdef"},
        "selected_products":["krisis"],"recovery":null
    });
    let mut child = Command::new(candidate.join("bin/krisis-install"))
        .args(["adapter", "verify"])
        .env("HOME", &fixture.home)
        .env_remove("CELL_DEPLOYMENT_RUN_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&request).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    Fixture::success(&output);
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["status"], "verified");
}

// This fixture records the entire legacy format recipe for transition coverage.
#[allow(clippy::too_many_lines)]
fn legacy_decisions(fixture: &Fixture) -> PathBuf {
    let stage = fixture.root.join("legacy-stage");
    for path in ["bin", "libexec", "package", "share/chancery/decisions"] {
        fs::create_dir_all(stage.join(path)).unwrap();
    }
    let mut proofs = Vec::new();
    for (key, path) in [
        ("binary_sha256", "libexec/decisions"),
        ("frontend_sha256", "bin/decisions"),
        ("daily_runner_sha256", "bin/decisions-daily-email"),
        ("observer_runner_sha256", "bin/decisions-observer"),
    ] {
        let bytes = format!("#!/bin/sh\n# {path}\nexit 0\n");
        write(&stage.join(path), &bytes, 0o755);
        if path.starts_with("bin/") {
            write(
                &stage.join(path.replacen("bin/", "package/", 1)),
                &bytes,
                0o755,
            );
        }
        proofs.push((key.to_owned(), digest(bytes)));
    }
    for (kind, key, proof) in [
        (
            "daily-email",
            "decisions/daily-email",
            "daily_clockwork_definition_sha256",
        ),
        (
            "observer",
            "decisions/observer",
            "observer_clockwork_definition_sha256",
        ),
    ] {
        let schedule = if kind == "observer" {
            "kind = \"interval\"\nseconds = 60"
        } else {
            "kind = \"local-calendar\"\nhour = 9\nminute = 0"
        };
        let source = format!(
            r#"schema_version = 1
key = "{key}"
release_id = "__RELEASE_ID__"
release_root = "__RELEASE_ROOT__"
authority = "current-user-background"
overlap = "skip"
arguments = []
cwd = "__DECISIONS_STATE__"
[schedule]
{schedule}
run_at_load = false
[launch]
kind = "interpreted"
interpreter = "/bin/sh"
interpreter_sha256 = "__INTERPRETER_SHA256__"
script = "__RELEASE_ROOT__/bin/decisions-{kind}"
script_sha256 = "__RUNNER_SHA256__"
[environment]
HOME = "__DECISIONS_HOME__"
[output]
stdout = "__DECISIONS_LOGS__/{kind}.stdout.log"
stderr = "__DECISIONS_LOGS__/{kind}.stderr.log"
"#
        );
        write(
            &stage.join(format!("package/decisions-{kind}.clockwork.toml.in")),
            &source,
            0o644,
        );
        proofs.push((proof.to_owned(), digest(source)));
    }
    for (key, path, mode) in [
        ("hooks_sha256", "package/hooks.json", 0o600),
        ("deployer_sha256", "package/deploy-user.sh", 0o755),
        ("uninstaller_sha256", "package/uninstall-user.sh", 0o755),
    ] {
        let bytes = if mode == 0o600 {
            "{}\n"
        } else {
            "#!/bin/sh\nexit 0\n"
        };
        write(&stage.join(path), bytes, mode);
        proofs.push((key.to_owned(), digest(bytes)));
    }
    let provider = b"{\"provider\":{\"id\":\"decisions\",\"release\":\"0.3.4\"}}\n";
    write(
        &stage.join("share/chancery/decisions/provider.json"),
        provider,
        0o644,
    );
    proofs.push((
        "chancery_sha256".into(),
        digest(format!(
            "path=./provider.json\n{}  ./provider.json\n",
            digest(provider)
        )),
    ));
    let mut recipe = String::new();
    for (_, value) in &proofs {
        writeln!(recipe, "{value}").unwrap();
    }
    let id = digest(recipe);
    let mut manifest = format!("format=3\nrelease_id={id}\nversion=0.3.4\n");
    for (key, value) in proofs {
        writeln!(manifest, "{key}={value}").unwrap();
    }
    write(&stage.join("manifest.txt"), manifest, 0o444);
    fs::create_dir_all(fixture.installed().join("releases")).unwrap();
    let release = fixture.installed().join("releases").join(&id);
    fs::rename(stage, &release).unwrap();
    symlink(
        format!("releases/{id}"),
        fixture.installed().join("current"),
    )
    .unwrap();
    fs::create_dir_all(fixture.home.join(".local/bin")).unwrap();
    fs::create_dir_all(
        fixture
            .home
            .join("Library/Application Support/Chancery/providers"),
    )
    .unwrap();
    fs::create_dir_all(fixture.home.join(".codex")).unwrap();
    symlink(
        fixture.installed().join("current/bin/decisions"),
        fixture.home.join(".local/bin/decisions"),
    )
    .unwrap();
    symlink(
        fixture.installed().join("current/share/chancery/decisions"),
        fixture
            .home
            .join("Library/Application Support/Chancery/providers/decisions"),
    )
    .unwrap();
    write(&fixture.home.join(".codex/hooks.json"), "{}\n", 0o600);
    for kind in ["observer", "daily-email"] {
        let mut source =
            fs::read_to_string(release.join(format!("package/decisions-{kind}.clockwork.toml.in")))
                .unwrap();
        for (key, value) in [
            ("RELEASE_ID", id.clone()),
            ("RELEASE_ROOT", release.display().to_string()),
            ("DECISIONS_STATE", fixture.state().display().to_string()),
            ("DECISIONS_HOME", fixture.home.display().to_string()),
            (
                "DECISIONS_LOGS",
                fixture
                    .home
                    .join("Library/Logs/Decisions")
                    .display()
                    .to_string(),
            ),
            ("INTERPRETER_SHA256", digest(fs::read("/bin/sh").unwrap())),
            (
                "RUNNER_SHA256",
                digest(fs::read(release.join(format!("bin/decisions-{kind}"))).unwrap()),
            ),
        ] {
            source = source.replace(&format!("__{key}__"), &value);
        }
        let source_path = fixture.root.join(format!("{kind}.toml"));
        write(&source_path, source, 0o600);
        let output = Command::new(fixture.root.join("clockwork"))
            .args(["--json", "definition", "register"])
            .arg(source_path)
            .output()
            .unwrap();
        assert!(output.status.success());
        let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let definition = output["data"]["digest"].as_str().unwrap();
        Fixture::success(
            &Command::new(fixture.root.join("clockwork"))
                .args([
                    "--json",
                    "binding",
                    "switch",
                    &format!("decisions/{kind}"),
                    definition,
                ])
                .output()
                .unwrap(),
        );
    }
    release
}

#[test]
fn legacy_decisions_handoff_preserves_disabled_history_and_rolls_back_touched_bindings() {
    let fixture = Fixture::new();
    let old = legacy_decisions(&fixture);
    Fixture::success(
        &fixture
            .command()
            .arg("verify-release")
            .arg(&old)
            .output()
            .unwrap(),
    );
    write(&fixture.root.join("fail-disable"), "", 0o600);
    Fixture::failure(&fixture.install(&["--final-cutover"]));
    assert_eq!(fixture.current(), old);
    for kind in ["observer", "daily-email"] {
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(
                fixture
                    .root
                    .join("bindings")
                    .join(format!("decisions_{kind}")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(value["enabled"], true);
    }
    Fixture::success(&fixture.install(&["--final-cutover"]));
    assert!(!fixture.home.join(".local/bin/decisions").exists());
    assert!(fixture.home.join(".local/bin/krisis").exists());
    assert_eq!(
        fs::canonicalize(fixture.installed().join("previous")).unwrap(),
        old
    );
    for kind in ["observer", "daily-email"] {
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(
                fixture
                    .root
                    .join("bindings")
                    .join(format!("decisions_{kind}")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(value["enabled"], false);
        assert!(value["definition_digest"].is_string());
    }
}

#[test]
fn hook_replacement_during_shutdown_is_preserved_with_recovery_held() {
    let fixture = Fixture::new();
    Fixture::success(&fixture.install(&["--final-cutover"]));
    let foreign = fixture.root.join("foreign-hooks.json");
    write(&foreign, "operator hook", 0o600);
    let hook = fixture.home.join(".codex/hooks.json");
    write(
        &fixture.root.join("replace-hook-on-disable"),
        serde_json::to_vec(&serde_json::json!({"path":hook,"target":foreign})).unwrap(),
        0o600,
    );
    Fixture::failure(&fixture.install(&["--final-cutover"]));
    assert_eq!(fs::read_link(hook).unwrap(), foreign);
    assert_eq!(fs::read(foreign).unwrap(), b"operator hook");
    assert!(fixture.state().join(".clockwork-maintenance").exists());
    assert!(!fixture.home.join(".local/bin/krisis").exists());
}
