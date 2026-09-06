//! Krisis-owned state, scheduler and admission proofs used by its installer.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use cell_install::{Error, Result, file_digest};
use clockwork::api::Manifest;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::legacy::{hexadecimal, pairs};

pub const ACTIVE: &str = "krisis/observer";
pub const LEGACY_OBSERVER: &str = "decisions/observer";
pub const LEGACY_DAILY: &str = "decisions/daily-email";

pub fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::new(message))
    }
}

pub fn text(path: &Path) -> Result<&str> {
    let value = path
        .to_str()
        .ok_or_else(|| Error::new("installation path is not UTF-8"))?;
    require(
        !value.contains(['\n', '\r']),
        "installation path contains a line break",
    )?;
    Ok(value)
}

pub fn executable(path: &Path) -> Result<()> {
    require(
        path.is_absolute(),
        "installation executable must be absolute",
    )?;
    text(path)?;
    file_digest(path)?;
    require(
        fs::symlink_metadata(path)?.mode() & 0o111 != 0,
        "installation executable is not executable",
    )
}

pub fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub fn operator_uid() -> Result<u32> {
    let output = std::process::Command::new("/usr/bin/id")
        .arg("-u")
        .output()?;
    let uid = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .map_err(|_| Error::new("operator identity is unavailable"))?;
    require(
        output.status.success() && uid != 0,
        "run as the Krisis operator, not root",
    )?;
    Ok(uid)
}

pub fn owned_file(path: &Path, uid: u32, mode: Option<u32>) -> Result<()> {
    let info = fs::symlink_metadata(path)?;
    require(
        info.is_file()
            && info.nlink() == 1
            && info.uid() == uid
            && mode.is_none_or(|mode| info.mode() & 0o7777 == mode),
        "state file is not exclusively owned and private",
    )
}

pub fn directory(path: &Path, uid: u32, mode: u32) -> Result<()> {
    if !exists(path)? {
        if let Some(parent) = path.parent()
            && !exists(parent)?
        {
            directory(parent, uid, 0o755)?;
        }
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    let info = fs::symlink_metadata(path)?;
    require(
        info.is_dir() && info.uid() == uid && info.mode() & 0o022 == 0,
        "installation directory is symbolic, foreign or writable",
    )?;
    if mode == 0o700 && info.mode() & 0o7777 != mode {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("state file has no parent"))?;
    let temporary = parent.join(format!(".krisis-write-{}", uuid::Uuid::now_v7().simple()));
    let operation = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if exists(&temporary)? {
        fs::remove_file(&temporary)?;
    }
    operation
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
    pub state: PathBuf,
    pub install: PathBuf,
    pub logs: PathBuf,
    pub database: PathBuf,
    pub gate: PathBuf,
    pub hold: PathBuf,
    pub binding: PathBuf,
    pub hooks: PathBuf,
    pub uid: u32,
}

impl Paths {
    pub fn new(home: PathBuf) -> Result<Self> {
        require(home.is_absolute(), "home must be absolute")?;
        let uid = operator_uid()?;
        let info = fs::symlink_metadata(&home)?;
        require(
            info.is_dir()
                && info.uid() == uid
                && info.mode() & 0o022 == 0
                && fs::canonicalize(&home)? == home,
            "home is not the operator's regular directory",
        )?;
        text(&home)?;
        let state = home.join("Library/Application Support/Decisions");
        let install = state.join("install");
        Ok(Self {
            logs: home.join("Library/Logs/Decisions"),
            database: state.join("decisions.db"),
            gate: state.join(".clockwork-maintenance"),
            hold: install.join("krisis-maintenance-hold.txt"),
            binding: install.join("krisis-observer-binding.txt"),
            hooks: home.join(".codex/hooks.json"),
            home,
            state,
            install,
            uid,
        })
    }

    pub fn setup(&self) -> Result<()> {
        for path in [
            &self.state,
            &self.install,
            &self.logs,
            &self.install.join("releases"),
        ] {
            directory(path, self.uid, 0o700)?;
        }
        for path in [
            self.home.join(".local/bin"),
            self.home.join(".codex"),
            self.home.join("Library/LaunchAgents"),
            self.home
                .join("Library/Application Support/Chancery/providers"),
        ] {
            directory(&path, self.uid, 0o755)?;
        }
        Ok(())
    }

    pub fn validate_database(&self) -> Result<()> {
        let present = exists(&self.database)?;
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let path = PathBuf::from(format!("{}{suffix}", text(&self.database)?));
            if exists(&path)? {
                require(present, "database sidecar exists without its database")?;
                owned_file(&path, self.uid, Some(0o600))?;
            }
        }
        Ok(())
    }

    pub fn validate_logs(&self) -> Result<()> {
        for name in ["observer.stdout.log", "observer.stderr.log"] {
            let path = self.logs.join(name);
            if exists(&path)? {
                owned_file(&path, self.uid, None)?;
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            }
        }
        Ok(())
    }
}

pub fn run(
    paths: &Paths,
    executable: &Path,
    arguments: &[OsString],
    extra: &BTreeMap<OsString, OsString>,
    timeout: u64,
) -> Result<std::process::Output> {
    let mut args = vec![
        OsString::from("-i"),
        OsString::from(format!("HOME={}", text(&paths.home)?)),
        OsString::from("PATH=/usr/bin:/bin:/usr/sbin:/sbin"),
    ];
    if let Some(run_id) = std::env::var_os("CELL_DEPLOYMENT_RUN_ID") {
        let mut value = OsString::from("CELL_DEPLOYMENT_RUN_ID=");
        value.push(run_id);
        args.push(value);
    }
    for (key, value) in extra {
        let mut entry = key.clone();
        entry.push("=");
        entry.push(value);
        args.push(entry);
    }
    args.push(executable.as_os_str().to_owned());
    args.extend_from_slice(arguments);
    cell_install::command::run(
        Path::new("/usr/bin/env"),
        &args,
        &BTreeMap::new(),
        Duration::from_secs(timeout),
    )
}

pub fn checked(
    paths: &Paths,
    executable: &Path,
    arguments: &[OsString],
    extra: &BTreeMap<OsString, OsString>,
    timeout: u64,
) -> Result<Vec<u8>> {
    let output = run(paths, executable, arguments, extra, timeout)?;
    require(
        output.status.success(),
        "Krisis installation dependency operation failed",
    )?;
    Ok(output.stdout)
}

pub fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pins {
    pub annals_binary: PathBuf,
    pub annals_config: PathBuf,
    pub annals_library_id: String,
    pub codex: PathBuf,
}

impl Pins {
    pub fn validate(&self) -> Result<()> {
        require(
            hexadecimal(&self.annals_library_id, 32),
            "invalid Annals decisions-library identity",
        )?;
        for path in [&self.annals_binary, &self.annals_config, &self.codex] {
            require(path.is_absolute(), "dependency pins must be absolute")?;
            file_digest(path)?;
        }
        for path in [&self.annals_binary, &self.codex] {
            require(
                fs::symlink_metadata(path)?.mode() & 0o111 != 0,
                "dependency executable is not executable",
            )?;
        }
        Ok(())
    }

    pub fn environment(&self) -> BTreeMap<OsString, OsString> {
        BTreeMap::from([(
            OsString::from("CONVERSATIONS_CODEX"),
            self.codex.as_os_str().to_owned(),
        )])
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Binding {
    pub exists: bool,
    pub enabled: bool,
    pub definition_digest: Option<String>,
}

pub fn binding(paths: &Paths, clockwork: &Path, key: &str) -> Result<Binding> {
    let output = run(
        paths,
        clockwork,
        &args(&["--json", "binding", "show", key]),
        &BTreeMap::new(),
        180,
    )?;
    if !output.status.success() {
        let missing = [&output.stdout, &output.stderr].iter().any(|bytes| {
            serde_json::from_slice::<Value>(bytes)
                .ok()
                .and_then(|v| {
                    v.pointer("/error/code")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .as_deref()
                == Some("binding_not_found")
        });
        require(missing, "Clockwork binding inspection failed")?;
        return Ok(Binding {
            exists: false,
            enabled: false,
            definition_digest: None,
        });
    }
    let value: Value = serde_json::from_slice(&output.stdout)?;
    let data = &value["data"];
    require(
        value["ok"] == true && data["key"] == key,
        "Clockwork binding response differs",
    )?;
    let enabled = data["enabled"]
        .as_bool()
        .ok_or_else(|| Error::new("Clockwork enabled state is invalid"))?;
    let digest = if data["definition_digest"].is_null() {
        None
    } else {
        let digest = data["definition_digest"]
            .as_str()
            .ok_or_else(|| Error::new("Clockwork digest is invalid"))?;
        require(hexadecimal(digest, 64), "Clockwork digest is invalid")?;
        Some(digest.into())
    };
    require(
        !enabled || digest.is_some(),
        "enabled Clockwork binding has no definition",
    )?;
    Ok(Binding {
        exists: true,
        enabled,
        definition_digest: digest,
    })
}

pub fn definition(paths: &Paths, clockwork: &Path, digest: &str) -> Result<Manifest> {
    let bytes = checked(
        paths,
        clockwork,
        &args(&["--json", "definition", "show", digest]),
        &BTreeMap::new(),
        180,
    )?;
    let value: Value = serde_json::from_slice(&bytes)?;
    require(
        value["ok"] == true && value["data"]["digest"] == digest,
        "Clockwork definition identity differs",
    )?;
    let manifest: Manifest = serde_json::from_value(value["data"]["manifest"].clone())?;
    require(
        value["data"]["key"] == manifest.key,
        "Clockwork definition key differs",
    )?;
    Ok(manifest)
}

pub fn template(paths: &Paths, release: &Path, pins: &Pins, name: &str) -> Result<Manifest> {
    let mut input = fs::read_to_string(
        release
            .join("package")
            .join(format!("{name}.clockwork.toml.in")),
    )?;
    let id = release
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| Error::new("invalid release identity"))?;
    let values = [
        ("RELEASE_ID", id.to_owned()),
        ("RELEASE_ROOT", text(release)?.into()),
        ("KRISIS_STATE", text(&paths.state)?.into()),
        ("KRISIS_HOME", text(&paths.home)?.into()),
        ("KRISIS_LOGS", text(&paths.logs)?.into()),
        ("DECISIONS_STATE", text(&paths.state)?.into()),
        ("DECISIONS_HOME", text(&paths.home)?.into()),
        ("DECISIONS_LOGS", text(&paths.logs)?.into()),
        ("CONVERSATIONS_CODEX", text(&pins.codex)?.into()),
        ("KRISIS_ANNALS_BINARY", text(&pins.annals_binary)?.into()),
        ("KRISIS_ANNALS_CONFIG", text(&pins.annals_config)?.into()),
        ("KRISIS_ANNALS_LIBRARY_ID", pins.annals_library_id.clone()),
        ("INTERPRETER_SHA256", file_digest(Path::new("/bin/sh"))?),
        (
            "RUNNER_SHA256",
            file_digest(&release.join("bin").join(name))?,
        ),
    ];
    for (key, value) in values {
        let escaped = serde_json::to_string(&value)?;
        input = input.replace(&format!("__{key}__"), &escaped[1..escaped.len() - 1]);
    }
    require(
        !input.contains("__"),
        "definition template has unresolved fields",
    )?;
    Manifest::from_toml(&input).map_err(|_| Error::new("owned Clockwork template is invalid"))
}

pub fn switch(
    paths: &Paths,
    clockwork: &Path,
    key: &str,
    digest: &str,
    enabled: bool,
) -> Result<()> {
    let arguments = if enabled {
        args(&["--json", "binding", "switch", key, digest])
    } else {
        args(&["--json", "binding", "disable", key, "--select", digest])
    };
    checked(paths, clockwork, &arguments, &BTreeMap::new(), 180)?;
    let value = binding(paths, clockwork, key)?;
    require(
        value.enabled == enabled && value.definition_digest.as_deref() == Some(digest),
        "Clockwork selection was not established",
    )
}

pub fn disable(paths: &Paths, clockwork: &Path, key: &str, prior: &Binding) -> Result<()> {
    require(
        binding(paths, clockwork, key)? == *prior,
        "Clockwork binding changed before mutation",
    )?;
    checked(
        paths,
        clockwork,
        &args(&["--json", "binding", "disable", key]),
        &BTreeMap::new(),
        180,
    )?;
    let actual = binding(paths, clockwork, key)?;
    require(
        !actual.enabled && actual.definition_digest == prior.definition_digest,
        "Clockwork binding did not disable exactly",
    )
}

pub fn restore_binding(paths: &Paths, clockwork: &Path, key: &str, prior: &Binding) -> Result<()> {
    let digest = prior
        .definition_digest
        .as_deref()
        .ok_or_else(|| Error::new("Clockwork cannot restore a null selection"))?;
    switch(paths, clockwork, key, digest, prior.enabled)
}

pub fn binding_receipt(paths: &Paths) -> Result<BTreeMap<String, String>> {
    owned_file(&paths.binding, paths.uid, Some(0o600))?;
    let values = pairs(&paths.binding)?;
    let names = [
        "format",
        "release_id",
        "definition_digest",
        "annals_binary",
        "annals_config",
        "annals_library_id",
    ];
    require(
        values.iter().map(|(k, _)| k.as_str()).eq(names) && values[0].1 == "1",
        "invalid observer ownership receipt",
    )?;
    let result: BTreeMap<_, _> = values.into_iter().collect();
    require(
        hexadecimal(&result["release_id"], 64)
            && hexadecimal(&result["definition_digest"], 64)
            && hexadecimal(&result["annals_library_id"], 32),
        "invalid observer ownership identity",
    )?;
    Ok(result)
}

pub fn doctor(paths: &Paths, binary: &Path, pins: &Pins) -> Result<Value> {
    let bytes = checked(
        paths,
        binary,
        &args(&[
            "--database",
            text(&paths.database)?,
            "--annals-binary",
            text(&pins.annals_binary)?,
            "--annals-config",
            text(&pins.annals_config)?,
            "--annals-library-id",
            &pins.annals_library_id,
            "--json",
            "doctor",
        ]),
        &pins.environment(),
        180,
    )?;
    let value: Value = serde_json::from_slice(&bytes)?;
    require(
        value["ok"] == true
            && value["schema_version"] == 4
            && value["annals_library_id"] == pins.annals_library_id,
        "Krisis doctor did not prove schema and dedicated Annals target",
    )?;
    Ok(value)
}

pub fn maintenance(
    paths: &Paths,
    binary: &Path,
    operation: &str,
    owner: Option<&str>,
) -> Result<Value> {
    let mut arguments = args(&[
        "--database",
        text(&paths.database)?,
        "--json",
        "maintenance",
        operation,
    ]);
    if let Some(owner) = owner {
        arguments.push(owner.into());
    }
    let bytes = checked(paths, binary, &arguments, &BTreeMap::new(), 60)?;
    let value: Value = serde_json::from_slice(&bytes)?;
    let data = cell_install::command::maintenance(&value)?;
    require(
        data["protocol_version"] == 1
            && data["contract_version"] == 1
            && data["holds"].is_array()
            && data["drained"].is_boolean(),
        "installed Krisis admission is incompatible",
    )?;
    Ok(data.clone())
}

pub fn inspect_result(current: Option<&cell_install::transaction::ReleaseInfo>) -> Value {
    current.map_or_else(
        || json!({"current":"absent","release_id":null}),
        |release| {
            json!({
                "current":format!("releases/{}",release.release_id), "release_id":release.release_id
            })
        },
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixture setup failures should fail the test immediately.
mod path_tests {
    use super::*;

    #[test]
    fn dependency_execution_requires_a_regular_absolute_executable() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("dependency");
        fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(executable(&path).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        executable(&path).unwrap();
        let alias = temporary.path().join("alias");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert!(executable(&alias).is_err());
        assert!(executable(Path::new("relative")).is_err());
        assert!(text(Path::new("/line\nbreak")).is_err());
    }

    #[test]
    fn existing_owned_state_directory_is_made_private() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("state");
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        directory(&path, fs::metadata(&path).unwrap().uid(), 0o700).unwrap();
        assert_eq!(fs::metadata(path).unwrap().mode() & 0o7777, 0o700);
    }
}
