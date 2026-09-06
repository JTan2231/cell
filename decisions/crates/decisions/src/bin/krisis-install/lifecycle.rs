//! Product-owned admission, scheduler cutover, and database compensation.
use super::{
    Control, Install, home, legacy, package,
    support::{
        ACTIVE, Binding, LEGACY_DAILY, LEGACY_OBSERVER, Paths, Pins, args, atomic_write, binding,
        binding_receipt, checked, definition, directory, disable, doctor, executable, exists,
        inspect_result, owned_file, require, restore_binding, run, switch, template, text,
    },
};
use cell_install::transaction::{self, InstallSnapshot, SelectionReceipt};
use cell_install::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedFile {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
    mode: u32,
    uid: u32,
}

impl SavedFile {
    fn capture(path: &Path, uid: u32, mode: u32) -> Result<Self> {
        let bytes = if exists(path)? {
            owned_file(path, uid, Some(mode))?;
            Some(fs::read(path)?)
        } else {
            None
        };
        Ok(Self {
            path: path.into(),
            bytes,
            mode,
            uid,
        })
    }
    fn restore(&self) -> Result<()> {
        if exists(&self.path)? {
            owned_file(&self.path, self.uid, Some(self.mode))?;
        }
        if let Some(bytes) = &self.bytes {
            atomic_write(&self.path, bytes, self.mode)
        } else {
            if exists(&self.path)? {
                fs::remove_file(&self.path)?;
            }
            Ok(())
        }
    }
    fn unchanged(&self) -> Result<()> {
        if exists(&self.path)? {
            owned_file(&self.path, self.uid, Some(self.mode))?;
        }
        require(
            if let Some(bytes) = &self.bytes {
                exists(&self.path)? && fs::read(&self.path)? == *bytes
            } else {
                !exists(&self.path)?
            },
            "captured product state changed before mutation",
        )
    }

    fn remove_owned_image(&self, candidate: &[u8]) -> Result<()> {
        if exists(&self.path)? {
            owned_file(&self.path, self.uid, Some(self.mode))?;
            let bytes = fs::read(&self.path)?;
            require(
                self.bytes.as_deref() == Some(bytes.as_slice()) || bytes == candidate,
                "hook changed outside the installation transaction",
            )?;
            fs::remove_file(&self.path)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Service {
    file: SavedFile,
    target: String,
    loaded: bool,
}

fn loaded(paths: &Paths, launchctl: &Path, target: &str) -> Result<bool> {
    Ok(run(
        paths,
        launchctl,
        &args(&["print", target]),
        &BTreeMap::new(),
        30,
    )?
    .status
    .success())
}

fn services(paths: &Paths, launchctl: &Path, prior: &InstallSnapshot) -> Result<Vec<Service>> {
    let mut result = vec![];
    for name in ["observer", "daily-email"] {
        let label = format!("org.decisions.{name}");
        let path = paths
            .home
            .join("Library/LaunchAgents")
            .join(format!("{label}.plist"));
        let file = SavedFile::capture(&path, paths.uid, 0o644)?;
        let target = format!("gui/{}/{label}", paths.uid);
        let loaded = loaded(paths, launchctl, &target)?;
        require(
            !loaded || file.bytes.is_some(),
            "loaded legacy service has no owned recoverable plist",
        )?;
        if let Some(bytes) = &file.bytes {
            let old = prior
                .current
                .as_ref()
                .ok_or_else(|| Error::new("legacy service has no owned release"))?;
            require(
                old.format == "legacy-2",
                "legacy service is not owned by the current Decisions release",
            )?;
            let root = package::root(paths, old);
            let source = root.join("package").join(format!("{label}.plist"));
            let mut expected = fs::read_to_string(source)?;
            let replacements = [
                (
                    "DECISIONS_OBSERVER_RUNNER",
                    root.join("bin/decisions-observer"),
                ),
                ("DECISIONS_RUNNER", root.join("bin/decisions-daily-email")),
                ("DECISIONS_STATE_DIR", paths.state.clone()),
                ("DECISIONS_HOME", paths.home.clone()),
                (
                    "DECISIONS_OBSERVER_STDOUT",
                    paths.logs.join("observer.stdout.log"),
                ),
                (
                    "DECISIONS_OBSERVER_STDERR",
                    paths.logs.join("observer.stderr.log"),
                ),
                (
                    "DECISIONS_STDOUT",
                    paths.logs.join("daily-email.stdout.log"),
                ),
                (
                    "DECISIONS_STDERR",
                    paths.logs.join("daily-email.stderr.log"),
                ),
            ];
            for (key, value) in replacements {
                expected = expected.replace(&format!("__{key}__"), text(&value)?);
            }
            require(
                expected.as_bytes() == bytes,
                "legacy LaunchAgent is foreign or modified",
            )?;
        }
        result.push(Service {
            file,
            target,
            loaded,
        });
    }
    Ok(result)
}

fn assert_closed(paths: &Paths) -> Result<()> {
    paths.validate_database()?;
    if exists(&paths.database)? {
        let output = run(
            paths,
            Path::new("/usr/sbin/lsof"),
            &args(&["-t", "--", text(&paths.database)?]),
            &BTreeMap::new(),
            30,
        )?;
        require(
            output.status.code() == Some(1),
            "Krisis database quiescence could not be proved",
        )?;
    }
    Ok(())
}

fn gate_identity(paths: &Paths) -> Result<(u64, u64)> {
    owned_file(&paths.gate, paths.uid, Some(0o600))?;
    let info = fs::symlink_metadata(&paths.gate)?;
    Ok((info.dev(), info.ino()))
}

fn hold_contents(id: &str, digest: &str, pins: &Pins, identity: (u64, u64)) -> Result<Vec<u8>> {
    Ok(format!("format=1\nkey={ACTIVE}\nrelease_id={id}\ndefinition_digest={digest}\nannals_binary={}\nannals_config={}\nannals_library_id={}\ngate_device={}\ngate_inode={}\n",
        text(&pins.annals_binary)?,text(&pins.annals_config)?,pins.annals_library_id,identity.0,identity.1).into_bytes())
}

fn authenticate_hold(
    paths: &Paths,
    id: &str,
    digest: &str,
    pins: &Pins,
    allow_absent: bool,
) -> Result<(u64, u64)> {
    owned_file(&paths.hold, paths.uid, Some(0o600))?;
    let values = legacy::pairs(&paths.hold)?;
    let names = [
        "format",
        "key",
        "release_id",
        "definition_digest",
        "annals_binary",
        "annals_config",
        "annals_library_id",
        "gate_device",
        "gate_inode",
    ];
    require(
        values.iter().map(|(key, _)| key.as_str()).eq(names),
        "maintenance receipt is not canonical",
    )?;
    let identity = (
        values[7]
            .1
            .parse()
            .map_err(|_| Error::new("invalid gate device"))?,
        values[8]
            .1
            .parse()
            .map_err(|_| Error::new("invalid gate inode"))?,
    );
    require(
        fs::read(&paths.hold)? == hold_contents(id, digest, pins, identity)?,
        "maintenance receipt belongs to another candidate",
    )?;
    if exists(&paths.gate)? {
        require(
            gate_identity(paths)? == identity,
            "maintenance gate was replaced",
        )?;
    } else {
        require(allow_absent, "authenticated maintenance gate is absent")?;
    }
    Ok(identity)
}

fn engage(paths: &Paths, id: &str, digest: &str, pins: &Pins) -> Result<bool> {
    if exists(&paths.gate)? {
        authenticate_hold(paths, id, digest, pins, false)?;
        Ok(false)
    } else {
        use std::os::unix::fs::OpenOptionsExt as _;
        let gate = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&paths.gate)?;
        gate.sync_all()?;
        atomic_write(
            &paths.hold,
            &hold_contents(id, digest, pins, gate_identity(paths)?)?,
            0o600,
        )?;
        Ok(true)
    }
}

fn release_hold(paths: &Paths, id: &str, digest: &str, pins: &Pins) -> Result<()> {
    authenticate_hold(paths, id, digest, pins, true)?;
    if exists(&paths.gate)? {
        fs::remove_file(&paths.gate)?;
        fs::File::open(&paths.state)?.sync_all()?;
    }
    Ok(())
}

pub fn pins_from_receipt(
    paths: &Paths,
    clockwork: &Path,
    current: &transaction::ReleaseInfo,
) -> Result<Pins> {
    pins_from_receipt_with_fallback(paths, clockwork, current, None)
}

fn pins_from_receipt_with_fallback(
    paths: &Paths,
    clockwork: &Path,
    current: &transaction::ReleaseInfo,
    fallback: Option<&Path>,
) -> Result<Pins> {
    let receipt = binding_receipt(paths)?;
    require(
        receipt["release_id"] == current.release_id,
        "Annals pin receipt belongs to another release",
    )?;
    let definition = definition(paths, clockwork, &receipt["definition_digest"])?;
    let codex = definition
        .environment
        .get("CONVERSATIONS_CODEX")
        .map(String::as_str)
        .or_else(|| fallback.and_then(Path::to_str))
        .ok_or_else(|| Error::new("installed Codex pin is absent"))?;
    let pins = Pins {
        annals_binary: receipt["annals_binary"].clone().into(),
        annals_config: receipt["annals_config"].clone().into(),
        annals_library_id: receipt["annals_library_id"].clone(),
        codex: codex.into(),
    };
    pins.validate()?;
    require(
        definition
            == template(
                paths,
                &package::root(paths, current),
                &pins,
                "krisis-observer",
            )?,
        "installed observer definition is not exact",
    )?;
    Ok(pins)
}

fn prove_bindings(
    paths: &Paths,
    clockwork: &Path,
    snapshot: &InstallSnapshot,
    pins: &Pins,
    candidate: &Path,
    candidate_digest: &str,
) -> Result<BTreeMap<String, Binding>> {
    let mut result = BTreeMap::new();
    for key in [ACTIVE, LEGACY_OBSERVER, LEGACY_DAILY] {
        result.insert(key.into(), binding(paths, clockwork, key)?);
    }
    require(
        !(result[ACTIVE].enabled && result[LEGACY_OBSERVER].enabled),
        "both current and legacy observer are enabled",
    )?;
    if let Some(digest) = &result[ACTIVE].definition_digest {
        let old = snapshot
            .current
            .as_ref()
            .ok_or_else(|| Error::new("selected observer has no installed release"))?;
        let receipt = binding_receipt(paths)?;
        require(
            receipt["release_id"] == old.release_id
                && receipt["definition_digest"] == *digest
                && receipt["annals_library_id"] == pins.annals_library_id,
            "selected observer differs from installed receipt or persistent library identity",
        )?;
        if digest == candidate_digest {
            require(
                definition(paths, clockwork, digest)?
                    == template(paths, candidate, pins, "krisis-observer")?,
                "candidate definition differs",
            )?;
        } else {
            let old = snapshot
                .current
                .as_ref()
                .ok_or_else(|| Error::new("observer binding has no current release"))?;
            require(
                old.format == "legacy-4" || old.format == transaction::TRANSACTION_FORMAT,
                "observer binding is not owned by current release",
            )?;
            let old_pins =
                pins_from_receipt_with_fallback(paths, clockwork, old, Some(&pins.codex))?;
            require(
                old_pins.annals_library_id == pins.annals_library_id,
                "ordinary installation cannot replace the Annals library",
            )?;
            require(
                binding_receipt(paths)?["definition_digest"] == *digest,
                "observer selection differs from ownership receipt",
            )?;
        }
    } else {
        require(
            snapshot
                .current
                .as_ref()
                .is_none_or(|old| matches!(old.format.as_str(), "legacy-2" | "legacy-3")),
            "owned current release has no observer definition",
        )?;
    }
    for (key, name) in [
        (LEGACY_OBSERVER, "decisions-observer"),
        (LEGACY_DAILY, "decisions-daily-email"),
    ] {
        if result[key].enabled {
            let old = snapshot
                .current
                .as_ref()
                .ok_or_else(|| Error::new("legacy binding has no current release"))?;
            require(
                old.format == "legacy-3",
                "enabled legacy binding is not owned by current Decisions release",
            )?;
            let digest = result[key]
                .definition_digest
                .as_deref()
                .ok_or_else(|| Error::new("legacy digest is absent"))?;
            require(
                definition(paths, clockwork, digest)?
                    == template(paths, &package::root(paths, old), pins, name)?,
                "legacy definition is foreign or modified",
            )?;
        }
    }
    Ok(result)
}

fn installed_hook(paths: &Paths, snapshot: &InstallSnapshot) -> Result<SavedFile> {
    let file = SavedFile::capture(&paths.hooks, paths.uid, 0o600)?;
    if let Some(bytes) = &file.bytes {
        let current = snapshot
            .current
            .as_ref()
            .ok_or_else(|| Error::new("hook exists without an owned current release"))?;
        require(
            *bytes == fs::read(package::root(paths, current).join("package/hooks.json"))?,
            "Codex hooks are foreign or modified",
        )?;
    }
    Ok(file)
}

fn register(paths: &Paths, clockwork: &Path, release: &Path, pins: &Pins) -> Result<String> {
    let manifest = template(paths, release, pins, "krisis-observer")?;
    let digest = manifest
        .digest()
        .map_err(|_| Error::new("definition digest failed"))?;
    let path = paths.install.join(format!(
        ".definition-{}.toml",
        uuid::Uuid::now_v7().simple()
    ));
    let source = manifest
        .to_toml()
        .map_err(|_| Error::new("definition encoding failed"))?;
    atomic_write(&path, source.as_bytes(), 0o600)?;
    let result = (|| {
        let bytes = checked(
            paths,
            clockwork,
            &args(&["--json", "definition", "register", text(&path)?]),
            &BTreeMap::new(),
            180,
        )?;
        let value: Value = serde_json::from_slice(&bytes)?;
        require(
            value["ok"] == true && value["data"]["digest"] == digest,
            "Clockwork registered a different definition",
        )?;
        require(
            definition(paths, clockwork, &digest)? == manifest,
            "registered observer definition is not exact",
        )?;
        Ok(digest)
    })();
    fs::remove_file(path)?;
    result
}

pub fn no_unfinished_transaction(paths: &Paths) -> Result<()> {
    if exists(&paths.install)? {
        for item in fs::read_dir(&paths.install)? {
            let name = item?.file_name();
            require(
                !name.to_string_lossy().starts_with(".transaction"),
                "unfinished Krisis transaction requires retained product recovery evidence",
            )?;
        }
    }
    Ok(())
}

// Admission, held-candidate proof, and cutover share one ordered lock scope.
#[allow(clippy::too_many_lines)]
pub fn install(options: &Install, deployment_run_id: Option<&str>) -> Result<Value> {
    let mut paths = Paths::new(home(options.home.clone())?)?;
    if let Some(owner) = deployment_run_id {
        paths.deployment_run_id = Some(owner.into());
    }
    let pins = Pins {
        annals_binary: options.annals.clone(),
        annals_config: options.annals_config.clone(),
        annals_library_id: options.annals_library_id.clone(),
        codex: options.codex.clone(),
    };
    executable(&options.binary)?;
    executable(&options.clockwork)?;
    require(
        options.launchctl.is_absolute(),
        "launchctl path must be absolute",
    )?;
    if options.final_cutover || options.release_maintenance {
        executable(&options.launchctl)?;
    }
    pins.validate()?;
    paths.setup()?;
    paths.validate_database()?;
    paths.validate_logs()?;
    no_unfinished_transaction(&paths)?;
    let prepared = package::prepare(&paths, options)?;
    let layout = package::layout();
    let verifier = |root: &Path| package::legacy_info(root, paths.uid);
    let mut transaction = transaction::lock_installation(&layout, &paths.home, &verifier)?;
    let prior = transaction::inspect_detached_installation(&layout, &paths.home, &verifier)?;
    if let Some(expected) = &options.expected_current {
        require(
            inspect_result(prior.current.as_ref())["current"] == *expected,
            "current release differs from expected selection",
        )?;
    }
    let digest = register(&paths, &options.clockwork, &prepared.root, &pins)?;
    let prior_hold = SavedFile::capture(&paths.hold, paths.uid, 0o600)?;
    if options.release_maintenance {
        require(
            package::inspect(&paths)? == prior,
            "held candidate public selectors are incomplete",
        )?;
        owned_file(&paths.hooks, paths.uid, Some(0o600))?;
        let current = prior
            .current
            .as_ref()
            .ok_or_else(|| Error::new("maintenance release requires an installed candidate"))?;
        package::matches_candidate(&paths, current, options)?;
        require(
            fs::read(&paths.hooks)? == fs::read(prepared.root.join("package/hooks.json"))?,
            "installed hooks differ from held candidate",
        )?;
        let receipt = binding_receipt(&paths)?;
        require(
            receipt["release_id"] == prepared.info.release_id
                && receipt["definition_digest"] == digest,
            "installed binding receipt differs from held candidate",
        )?;
        let active = binding(&paths, &options.clockwork, ACTIVE)?;
        require(
            active.enabled && active.definition_digest.as_deref() == Some(&digest),
            "observer selection differs from held candidate",
        )?;
        require(
            pins_from_receipt(&paths, &options.clockwork, current)? == pins,
            "held candidate dependency pins differ",
        )?;
        for key in [LEGACY_OBSERVER, LEGACY_DAILY] {
            require(
                !binding(&paths, &options.clockwork, key)?.enabled,
                "legacy schedule remains enabled",
            )?;
        }
        for service in services(&paths, &options.launchctl, &prior)? {
            require(
                !service.loaded && service.file.bytes.is_none(),
                "legacy service remains installed",
            )?;
        }
        release_hold(&paths, &prepared.info.release_id, &digest, &pins)?;
        return Ok(
            json!({"ok":true,"data":{"release_id":prepared.info.release_id,"maintenance":false}}),
        );
    }
    if options.keep_maintenance {
        authenticate_hold(&paths, &prepared.info.release_id, &digest, &pins, false)?;
    }
    let created = engage(&paths, &prepared.info.release_id, &digest, &pins)?;
    if !options.final_cutover {
        return Ok(
            json!({"ok":true,"data":{"release_id":prepared.info.release_id,"prepared":true,"maintenance":true}}),
        );
    }
    let result = cutover(
        &paths,
        options,
        &pins,
        &prepared,
        &digest,
        &prior,
        &mut transaction,
    );
    match result {
        Ok(()) => {
            if !options.keep_maintenance {
                release_hold(&paths, &prepared.info.release_id, &digest, &pins)?;
            }
            Ok(
                json!({"ok":true,"data":{"release_id":prepared.info.release_id,"maintenance":options.keep_maintenance}}),
            )
        }
        Err(error) => {
            if no_unfinished_transaction(&paths).is_ok() {
                if created {
                    release_hold(&paths, &prepared.info.release_id, &digest, &pins)?;
                }
                prior_hold.restore()?;
            }
            Err(error)
        }
    }
}

// Keep each forward transition beside its database-first compensation sequence.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn cutover(
    paths: &Paths,
    options: &Install,
    pins: &Pins,
    prepared: &transaction::PreparedRelease,
    digest: &str,
    prior: &InstallSnapshot,
    transaction: &mut transaction::InstallTransaction<'_>,
) -> Result<()> {
    let controls = prove_bindings(
        paths,
        &options.clockwork,
        prior,
        pins,
        &prepared.root,
        digest,
    )?;
    let services = services(paths, &options.launchctl, prior)?;
    for (service, key) in services.iter().zip([LEGACY_OBSERVER, LEGACY_DAILY]) {
        require(
            !(service.loaded && controls[key].enabled),
            "legacy scheduler is active through two owners",
        )?;
    }
    let hook = installed_hook(paths, prior)?;
    let receipt = SavedFile::capture(&paths.binding, paths.uid, 0o600)?;
    let evidence = paths
        .install
        .join(format!(".transaction-{}", uuid::Uuid::now_v7().simple()));
    directory(&evidence, paths.uid, 0o700)?;
    atomic_write(
        &evidence.join("prior.json"),
        &serde_json::to_vec(
            &json!({"selection":prior,"controls":controls,"services":services,"hook":hook,"binding_receipt":receipt}),
        )?,
        0o600,
    )?;
    let mut touched = Vec::new();
    let mut stopped = Vec::new();
    let mut suspended: Option<SelectionReceipt> = None;
    let mut database: Option<Vec<SavedFile>> = None;
    let result = (|| {
        transaction.recheck(prior)?;
        hook.unchanged()?;
        receipt.unchanged()?;
        for key in [ACTIVE, LEGACY_OBSERVER, LEGACY_DAILY] {
            if controls[key].enabled {
                touched.push(key);
                disable(paths, &options.clockwork, key, &controls[key])?;
            }
        }
        for service in &services {
            service.file.unchanged()?;
            if service.loaded {
                stopped.push(service);
                checked(
                    paths,
                    &options.launchctl,
                    &args(&["bootout", &service.target]),
                    &BTreeMap::new(),
                    30,
                )?;
            }
            if service.file.bytes.is_some() {
                fs::remove_file(&service.file.path)?;
            }
        }
        let mut paths_to_suspend = vec![PathBuf::from(".local/bin/krisis")];
        if prior.current.as_ref().is_some_and(|info| {
            info.public
                .iter()
                .any(|entry| entry.path == Path::new(".local/bin/decisions"))
        }) {
            paths_to_suspend.push(PathBuf::from(".local/bin/decisions"));
        }
        suspended = Some(transaction.suspend(prior, &paths_to_suspend)?);
        atomic_write(
            &evidence.join("suspension.json"),
            &serde_json::to_vec(&suspended)?,
            0o600,
        )?;
        hook.unchanged()?;
        if hook.bytes.is_some() {
            fs::remove_file(&paths.hooks)?;
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
        assert_closed(paths)?;
        let backup = ["", "-wal", "-shm", "-journal"]
            .into_iter()
            .map(|suffix| {
                SavedFile::capture(
                    &PathBuf::from(format!("{}{suffix}", text(&paths.database)?)),
                    paths.uid,
                    0o600,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        atomic_write(
            &evidence.join("database.json"),
            &serde_json::to_vec(&backup)?,
            0o600,
        )?;
        database = Some(backup);
        doctor(paths, &prepared.root.join("libexec/krisis"), pins)?;
        checked(
            paths,
            &prepared.root.join("libexec/krisis"),
            &args(&["--database", text(&paths.database)?, "observe", "activate"]),
            &pins.environment(),
            180,
        )?;
        let before = &suspended
            .as_ref()
            .ok_or_else(|| Error::new("missing suspension proof"))?
            .after;
        let published = transaction.publish(prepared, before, |_| Ok(()))?;
        atomic_write(
            &evidence.join("publication.json"),
            &serde_json::to_vec(&published)?,
            0o600,
        )?;
        atomic_write(
            &paths.hooks,
            &fs::read(prepared.root.join("package/hooks.json"))?,
            0o600,
        )?;
        atomic_write(&paths.binding,format!("format=1\nrelease_id={}\ndefinition_digest={digest}\nannals_binary={}\nannals_config={}\nannals_library_id={}\n",prepared.info.release_id,text(&pins.annals_binary)?,text(&pins.annals_config)?,pins.annals_library_id).as_bytes(),0o600)?;
        if !touched.contains(&ACTIVE) {
            touched.push(ACTIVE);
        }
        switch(paths, &options.clockwork, ACTIVE, digest, true)?;
        for key in [LEGACY_OBSERVER, LEGACY_DAILY] {
            let mut expected = controls[key].clone();
            expected.enabled = false;
            if !controls[key].enabled {
                expected = controls[key].clone();
            }
            require(
                binding(paths, &options.clockwork, key)? == expected,
                "legacy schedule changed during cutover",
            )?;
        }
        authenticate_hold(paths, &prepared.info.release_id, digest, pins, false)?;
        Ok(())
    })();
    if let Err(error) = result {
        let rollback: Result<()> = (|| {
            // Public candidate access must be removed before restoring database bytes.
            if let Some(selection) = &suspended {
                transaction.recover(&selection.after, prepared, false, |_| Ok(()))?;
            }
            hook.remove_owned_image(&fs::read(prepared.root.join("package/hooks.json"))?)?;
            for key in touched.iter().rev() {
                let selected = binding(paths, &options.clockwork, key)?;
                if selected.enabled {
                    disable(paths, &options.clockwork, key, &selected)?;
                }
            }
            if let Some(backup) = &database {
                assert_closed(paths)?;
                for file in backup {
                    file.restore()?;
                }
            }
            receipt.restore()?;
            for key in touched.iter().rev() {
                restore_binding(paths, &options.clockwork, key, &controls[*key])?;
            }
            for service in &services {
                service.file.restore()?;
            }
            for service in stopped {
                checked(
                    paths,
                    &options.launchctl,
                    &args(&[
                        "bootstrap",
                        &format!("gui/{}", paths.uid),
                        text(&service.file.path)?,
                    ]),
                    &BTreeMap::new(),
                    30,
                )?;
            }
            if let Some(selection) = &suspended {
                transaction.restore(selection, |_| Ok(()))?;
            }
            hook.remove_owned_image(&fs::read(prepared.root.join("package/hooks.json"))?)?;
            hook.restore()?;
            Ok(())
        })();
        if rollback.is_err() {
            return Err(Error::new(format!(
                "Krisis cutover failed; maintenance and recovery evidence retained at {}",
                text(&evidence)?
            )));
        }
        fs::remove_dir_all(evidence)?;
        return Err(error);
    }
    fs::remove_dir_all(evidence)?;
    Ok(())
}

pub fn uninstall(options: &Control) -> Result<Value> {
    let paths = Paths::new(home(options.home.clone())?)?;
    let clockwork = options.clockwork.clone().map_or_else(
        || fs::canonicalize(paths.home.join(".local/bin/clockwork")),
        Ok,
    )?;
    executable(&clockwork)?;
    executable(&options.launchctl)?;
    no_unfinished_transaction(&paths)?;
    let layout = package::layout();
    let verifier = |root: &Path| package::legacy_info(root, paths.uid);
    let mut tx = transaction::lock_installation(&layout, &paths.home, &verifier)?;
    let prior = transaction::inspect_detached_installation(&layout, &paths.home, &verifier)?;
    let current = prior
        .current
        .as_ref()
        .ok_or_else(|| Error::new("no retained Krisis installation"))?;
    require(
        matches!(
            current.format.as_str(),
            "legacy-4" | transaction::TRANSACTION_FORMAT
        ),
        "legacy Decisions removal requires its retained lifecycle",
    )?;
    let pins = pins_from_receipt(&paths, &clockwork, current)?;
    let receipt = binding_receipt(&paths)?;
    let active = binding(&paths, &clockwork, ACTIVE)?;
    require(
        active.definition_digest.as_deref() == Some(&receipt["definition_digest"]),
        "observer selection differs from its owned receipt",
    )?;
    let hook = installed_hook(&paths, &prior)?;
    for key in [LEGACY_OBSERVER, LEGACY_DAILY] {
        require(
            !binding(&paths, &clockwork, key)?.enabled,
            "enabled legacy binding requires explicit retirement",
        )?;
    }
    for service in services(&paths, &options.launchctl, &prior)? {
        require(
            !service.loaded && service.file.bytes.is_none(),
            "legacy service requires explicit retirement",
        )?;
    }
    if exists(&paths.gate)? {
        gate_identity(&paths)?;
    } else {
        engage(
            &paths,
            &current.release_id,
            &receipt["definition_digest"],
            &pins,
        )?;
    }
    if active.enabled {
        disable(&paths, &clockwork, ACTIVE, &active)?;
    }
    hook.unchanged()?;
    if hook.bytes.is_some() {
        fs::remove_file(&paths.hooks)?;
    }
    let public = prior
        .current
        .as_ref()
        .map(|info| {
            info.public
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    tx.suspend(&prior, &public)?;
    Ok(
        json!({"ok":true,"data":{"uninstalled":true,"retained_release":current.release_id,"maintenance":true}}),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixture setup failures should fail the test immediately.
mod file_tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    #[test]
    fn captured_state_refuses_symbolic_shared_or_public_replacements() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state");
        let other = directory.path().join("other");
        fs::write(&path, b"captured").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let uid = fs::symlink_metadata(&path).unwrap().uid();
        let saved = SavedFile::capture(&path, uid, 0o600).unwrap();
        fs::write(&other, b"captured").unwrap();
        fs::set_permissions(&other, fs::Permissions::from_mode(0o600)).unwrap();
        fs::remove_file(&path).unwrap();
        symlink(&other, &path).unwrap();
        assert!(saved.unchanged().is_err());
        assert!(saved.restore().is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&other, &path).unwrap();
        assert!(saved.unchanged().is_err());
        assert!(saved.restore().is_err());
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"captured").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(saved.unchanged().is_err());
        assert!(saved.restore().is_err());
        assert_eq!(fs::read(&other).unwrap(), b"captured");
    }

    #[test]
    fn hook_compensation_preserves_an_unrecognized_edit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hooks.json");
        fs::write(&path, b"old hook").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let saved = SavedFile::capture(&path, fs::metadata(&path).unwrap().uid(), 0o600).unwrap();
        fs::write(&path, b"operator edit").unwrap();
        assert!(saved.remove_owned_image(b"candidate hook").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"operator edit");
    }
}
