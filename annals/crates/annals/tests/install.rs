#![cfg(target_os = "macos")]

use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn executable(path: &Path, source: &str) -> Result {
    fs::write(path, source)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

struct Fixture {
    root: tempfile::TempDir,
    home: PathBuf,
    product: PathBuf,
    usage: PathBuf,
    clockwork: PathBuf,
    launchctl: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self> {
        let root = tempfile::Builder::new()
            .prefix("annals-install-test-")
            .tempdir_in("/private/tmp")?;
        let home = root.path().join("home");
        fs::create_dir(&home)?;
        let product = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let usage = root.path().join("annals-usage");
        let provider: Value = serde_json::from_slice(&fs::read(
            product.join("chancery/annals-usage/provider.json"),
        )?)?;
        let version = provider
            .pointer("/provider/release")
            .and_then(Value::as_str)
            .ok_or("usage version missing")?;
        executable(
            &usage,
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n --version) echo 'annals-usage {version}' ;;\n --help) echo help ;;\n doctor) [ ! -f \"$HOME/fail-doctor\" ] ;;\n *) exit 64 ;;\nesac\n"
            ),
        )?;
        let launchctl = root.path().join("launchctl");
        executable(&launchctl, "#!/bin/sh\n[ \"$1\" != print ]\n")?;
        let clockwork = root.path().join("clockwork");
        executable(
            &clockwork,
            r"#!/usr/bin/env python3
import hashlib,json,os,pathlib,sys,tomllib
path=pathlib.Path(os.environ['HOME'])/'clockwork.json'
state=json.loads(path.read_text()) if path.exists() else {'definitions':{},'bindings':{}}
args=sys.argv[1:]
if args[0]=='--json': args=args[1:]
data=None
if args[:2]==['definition','register']:
 manifest=tomllib.loads(pathlib.Path(args[2]).read_text()); digest=hashlib.sha256(json.dumps(manifest,sort_keys=True).encode()).hexdigest()
 if not pathlib.Path(manifest['cwd']).is_dir() or not all(pathlib.Path(manifest['output'][channel]).is_file() for channel in ['stdout','stderr']):
  print(json.dumps({'ok':False,'error':{'code':'unsafe_definition_path'}}),file=sys.stderr);sys.exit(1)
 data={'digest':digest,'key':manifest['key'],'manifest':manifest};state['definitions'][digest]=data
elif args[:2]==['definition','show']: data=state['definitions'][args[2]]
elif args[:2]==['binding','show']:
 if args[2] not in state['bindings']:
  print(json.dumps({'ok':False,'error':{'code':'binding_not_found'}}),file=sys.stderr);sys.exit(1)
 data=state['bindings'][args[2]]
elif args[:2] in (['binding','disable'],['binding','switch']):
 key=args[2]; prior=state['bindings'].get(key,{'key':key,'enabled':False,'definition_digest':None})
 digest=args[3] if args[1]=='switch' else (args[4] if len(args)>4 else prior['definition_digest'])
 data={'key':key,'enabled':args[1]=='switch','definition_digest':digest};state['bindings'][key]=data
else: sys.exit(64)
path.write_text(json.dumps(state));print(json.dumps({'ok':True,'data':data}))
",
        )?;
        Ok(Self {
            root,
            home,
            product,
            usage,
            clockwork,
            launchctl,
        })
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_annals-install"));
        command
            .env("HOME", &self.home)
            .env_remove("CELL_DEPLOYMENT_RUN_ID")
            .env_remove("ANNALS_CONFIG")
            .env_remove("ANNALS_LIBRARY")
            .env_remove("ANNALS_STATE_DIR");
        command
    }

    fn install(&self) -> Result<Output> {
        Ok(self
            .command()
            .args(["install", "--binary"])
            .arg(env!("CARGO_BIN_EXE_annals"))
            .arg("--usage-binary")
            .arg(&self.usage)
            .arg("--bundle")
            .arg(self.product.join("chancery/annals"))
            .arg("--usage-bundle")
            .arg(self.product.join("chancery/annals-usage"))
            .arg("--nucleus")
            .arg(self.root.path().join("nucleus"))
            .arg("--nucleus-socket")
            .arg(self.root.path().join("nucleus.sock"))
            .arg("--clockwork")
            .arg(&self.clockwork)
            .arg("--launchctl")
            .arg(&self.launchctl)
            .output()?)
    }

    fn state(&self) -> PathBuf {
        self.home.join("Library/Application Support/Annals")
    }
    fn release(&self) -> Result<PathBuf> {
        Ok(self
            .state()
            .join("install")
            .join(fs::read_link(self.state().join("install/current"))?))
    }

    fn provision(&self, keep: bool) -> Result<Output> {
        let mut command = self.command();
        command
            .args(["provision-decisions", "--release-root"])
            .arg(self.release()?)
            .arg("--nucleus-socket")
            .arg(self.root.path().join("nucleus.sock"))
            .arg("--clockwork")
            .arg(&self.clockwork);
        if keep {
            command.arg("--keep-maintenance");
        }
        Ok(command.output()?)
    }
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn native_release_installs_primary_and_independent_decisions_library() -> Result {
    let fixture = Fixture::new()?;
    success(&fixture.install()?);
    let current = fixture.release()?;
    success(
        &fixture
            .command()
            .arg("verify-release")
            .arg(&current)
            .output()?,
    );
    let output = Command::new(fixture.home.join(".local/bin/annals"))
        .args(["--json", "stats"])
        .env("HOME", &fixture.home)
        .env_remove("ANNALS_CONFIG")
        .env_remove("ANNALS_LIBRARY")
        .output()?;
    success(&output);
    success(&fixture.provision(true)?);
    let decisions = fixture.state().join("decisions");
    assert!(decisions.join("spool/.maintenance").is_file());
    assert!(decisions.join(".provision-maintenance.json").is_file());
    success(&fixture.provision(false)?);
    assert!(!decisions.join("spool/.maintenance").exists());
    let config: toml::Value = toml::from_str(&fs::read_to_string(decisions.join("config.toml"))?)?;
    assert!(config.get("decision_feed").is_some());
    assert_ne!(
        fs::read(fixture.state().join("annals.db"))?,
        fs::read(decisions.join("annals.db"))?
    );
    Ok(())
}

#[test]
fn native_schedules_upgrade_schema_one_bindings_to_default_failure_policy() -> Result {
    let fixture = Fixture::new()?;
    success(&fixture.install()?);
    success(&fixture.provision(false)?);
    let state_path = fixture.home.join("clockwork.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    let keys = ["annals/inbox", "annals/decisions-inbox"];
    for key in keys {
        let digest = state["bindings"][key]["definition_digest"]
            .as_str()
            .ok_or("missing selected definition")?
            .to_owned();
        let mut definition = state["definitions"][&digest].clone();
        let manifest: clockwork::api::Manifest =
            serde_json::from_value(definition["manifest"].clone())?;
        assert_eq!(manifest.schema_version, 2);
        assert_eq!(manifest.failure, clockwork::api::FailurePolicy::default());
        definition["manifest"]["schema_version"] = serde_json::json!(1);
        let legacy: clockwork::api::Manifest =
            serde_json::from_value(definition["manifest"].clone())?;
        let legacy_digest = legacy.digest()?;
        definition["digest"] = serde_json::json!(legacy_digest);
        state["definitions"][&legacy_digest] = definition;
        state["bindings"][key]["definition_digest"] = serde_json::json!(legacy_digest);
    }
    fs::write(&state_path, serde_json::to_vec(&state)?)?;
    success(&fixture.install()?);
    success(&fixture.provision(false)?);
    let state: Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    for key in keys {
        let digest = state["bindings"][key]["definition_digest"]
            .as_str()
            .ok_or("missing selected definition")?;
        let manifest: clockwork::api::Manifest =
            serde_json::from_value(state["definitions"][digest]["manifest"].clone())?;
        assert_eq!(manifest.schema_version, 2);
        assert_eq!(manifest.failure, clockwork::api::FailurePolicy::default());
    }
    Ok(())
}

#[test]
fn failed_update_restores_programs_database_and_own_maintenance() -> Result {
    let fixture = Fixture::new()?;
    success(&fixture.install()?);
    let prior = fixture.release()?;
    let config = fs::read(fixture.state().join("config.toml"))?;
    fs::write(fixture.home.join("fail-doctor"), b"fail")?;
    let failed = fixture.install()?;
    assert!(!failed.status.success());
    assert_eq!(fixture.release()?, prior);
    assert_eq!(fs::read(fixture.state().join("config.toml"))?, config);
    assert!(!fixture.state().join("spool/.maintenance").exists());
    let output = Command::new(fixture.home.join(".local/bin/annals"))
        .args(["--json", "maintenance", "status"])
        .env("HOME", &fixture.home)
        .env_remove("ANNALS_CONFIG")
        .env_remove("ANNALS_LIBRARY")
        .output()?;
    success(&output);
    let value: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(value.pointer("/data/holds"), Some(&serde_json::json!([])));
    Ok(())
}

#[test]
fn changed_installed_artifact_is_refused_before_lifecycle_effects() -> Result {
    let fixture = Fixture::new()?;
    success(&fixture.install()?);
    let runner = fixture.release()?.join("bin/annals-inbox");
    fs::write(&runner, b"foreign")?;
    assert!(!fixture.install()?.status.success());
    assert!(!fixture.state().join("spool/.maintenance").exists());
    assert!(!fixture.state().join("install/.update-lock").exists());
    Ok(())
}

#[test]
fn committed_recovery_retains_gate_when_scheduler_evidence_changed() -> Result {
    let fixture = Fixture::new()?;
    success(&fixture.install()?);
    let archive = fs::read_dir(fixture.state().join("backups/deployments"))?
        .next()
        .ok_or("no completed transaction")??
        .path();
    let retained = fixture
        .state()
        .join("install")
        .join(archive.file_name().ok_or("missing archive name")?);
    fs::rename(archive, &retained)?;
    let gate = fixture.state().join("spool/.maintenance");
    fs::write(&gate, b"")?;
    fs::set_permissions(&gate, fs::Permissions::from_mode(0o600))?;
    let clockwork = fixture.home.join("clockwork.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&clockwork)?)?;
    state["bindings"]["annals/inbox"]["enabled"] = Value::Bool(false);
    fs::write(clockwork, serde_json::to_vec(&state)?)?;
    let result = fixture.command().arg("recover").arg(&retained).output()?;
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("scheduler control changed"));
    assert!(gate.is_file());
    assert!(retained.is_dir());
    Ok(())
}
