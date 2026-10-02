//! Product installation and the explicitly enabled EMT worker.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cell_install::legacy::LegacySpec;
use cell_install::simple::Spec;
use cell_install::transaction::{self, LockKind, ReleaseInfo};
use clockwork::api::{
    Authority, BindingRecord, Client, LaunchImage, Manifest, Output, OverlapPolicy, Schedule,
};
use serde_json::{Value, json};

use crate::{Result, fail};

pub const WORKER_KEY: &str = "emt/worker";

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "emt",
        application: "EMT",
        source_directory: "emt",
        provider_source: "emt/chancery",
        legacy_provider_path: "share/chancery/emt",
        legacy: &LegacySpec {
            format: "none",
            manifest: "manifest.txt",
            metadata: &[],
            proofs: &[],
            providers: &[],
            hash_path_lines: false,
        },
        wrapper: None,
        lock_kind: LockKind::Shlock,
        lock_at_state: false,
    }
}

fn require_standard_root(root: &Path) -> Result<PathBuf> {
    let home = crate::home()?;
    if root != home.join("Library/Application Support/EMT") {
        return Err(fail(
            "scheduled installation requires the standard EMT state root",
        ));
    }
    Ok(home)
}

fn client(home: &Path) -> Client {
    Client::new(home.join(".local/bin/clockwork"))
}

fn binding(client: &Client) -> Result<Option<BindingRecord>> {
    match client.binding(WORKER_KEY) {
        Ok(binding) => Ok(Some(binding)),
        Err(error) if error.to_string().starts_with("binding_not_found:") => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn selected_release(root: &Path) -> Result<ReleaseInfo> {
    let release = selected_release_metadata(root)?;
    let spec = specification();
    transaction::verify_release_at(
        &spec.layout(),
        &root.join("install/releases").join(&release.release_id),
        &|path| spec.legacy(path),
    )
    .map_err(Into::into)
}

fn selected_release_metadata(root: &Path) -> Result<ReleaseInfo> {
    let home = require_standard_root(root)?;
    let spec = specification();
    transaction::inspect_installation(&spec.layout(), &home, &|path| spec.read_legacy(path))?
        .current
        .ok_or_else(|| fail("install EMT before enabling its worker"))
}

fn manifest(root: &Path, release: &ReleaseInfo) -> Result<Manifest> {
    let home = require_standard_root(root)?;
    let release_root = root.join("install/releases").join(&release.release_id);
    let executable = release_root.join("bin/emt");
    release
        .files
        .get("bin/emt")
        .ok_or_else(|| fail("installed release has no EMT executable"))?;
    let executable_hash = cell_install::file_digest(&executable)?;
    Ok(Manifest {
        schema_version: 2,
        key: WORKER_KEY.into(),
        release_id: release.release_id.clone(),
        release_root: path_text(&release_root)?,
        authority: Authority::CurrentUserBackground,
        overlap: OverlapPolicy::Skip,
        failure: clockwork::api::FailurePolicy::default(),
        timeout_seconds: Some(300),
        arguments: vec!["--json".into(), "worker".into()],
        cwd: path_text(root)?,
        schedule: Schedule::Interval {
            seconds: 15,
            run_at_load: false,
        },
        launch: LaunchImage::Direct {
            program: path_text(&executable)?,
            sha256: executable_hash,
        },
        environment: BTreeMap::from([("HOME".into(), path_text(&home)?)]),
        output: Output {
            stdout: path_text(&root.join("logs/worker.stdout.log"))?,
            stderr: path_text(&root.join("logs/worker.stderr.log"))?,
        },
    })
}

fn require_owned_binding(root: &Path, client: &Client, binding: &BindingRecord) -> Result<()> {
    let Some(digest) = binding.definition_digest.as_deref() else {
        if binding.enabled {
            return Err(fail("enabled EMT binding has no selected definition"));
        }
        return Ok(());
    };
    let definition = client.definition(digest)?;
    let release_root = root
        .join("install/releases")
        .join(&definition.manifest.release_id);
    if definition.key != WORKER_KEY || definition.manifest.release_root != path_text(&release_root)?
    {
        return Err(fail(
            "emt/worker selects a definition outside EMT's owned installation",
        ));
    }
    let spec = specification();
    let release =
        transaction::verify_release_at(&spec.layout(), &release_root, &|path| spec.legacy(path))?;
    let mut expected = manifest(root, &release)?;
    // A retained schema-one selection remains owned and can be upgraded. Its
    // immutable bytes and Clockwork incident state must not be rewritten.
    expected.schema_version = definition.manifest.schema_version;
    if definition.digest != digest || definition.manifest != expected {
        return Err(fail(
            "emt/worker does not match EMT's supported worker definition",
        ));
    }
    Ok(())
}

/// Inspect program selection without running a model, sending mail, or changing selectors.
///
/// # Errors
/// Rejects a nonstandard state root or an unprovable installed selection.
pub fn installation_status(root: &Path) -> Result<Value> {
    let home = require_standard_root(root)?;
    let spec = specification();
    let snapshot =
        transaction::inspect_installation(&spec.layout(), &home, &|path| spec.legacy(path))?;
    if snapshot.current.is_some() {
        selected_release(root)?;
    }
    Ok(serde_json::to_value(snapshot)?)
}

/// Operate only EMT's Clockwork binding. Operator pause remains independent.
///
/// # Errors
/// Returns errors for unavailable Clockwork, incompatible state, unsafe paths,
/// an active conflicting operation, maintenance, or an unconfirmed selection.
pub fn schedule(root: &Path, operation: &str) -> Result<Value> {
    let home = require_standard_root(root)?;
    if !matches!(operation, "enable" | "disable" | "status") {
        return Err(fail(
            "schedule operation must be enable, disable, or status",
        ));
    }
    let client = client(&home);
    if operation == "status" {
        let observed = binding(&client)?;
        if let Some(observed) = &observed {
            require_owned_binding(root, &client, observed)?;
        }
        return Ok(json!({"key": WORKER_KEY, "binding": observed}));
    }

    // Disabling prevents future activation and remains available under a hold.
    // Enabling admits new background work and must respect every hold.
    let gate = crate::gate(root);
    let _admission = if operation == "disable" {
        gate.recover()?
    } else {
        gate.enter()?
    };
    let _runner = crate::store::runner_lock(root)?;
    let observed = binding(&client)?;
    if operation == "disable" {
        return match observed {
            None => Ok(json!({"key": WORKER_KEY, "binding": null, "enabled": false})),
            Some(_) => Ok(json!({"key": WORKER_KEY, "binding": client.disable(WORKER_KEY, None)?})),
        };
    }
    if let Some(observed) = &observed {
        require_owned_binding(root, &client, observed)?;
    }

    // Initialization is a separate product operation. Enabling may not create
    // a database or silently fill missing configuration.
    crate::store::Store::open(root)?.config()?.validate()?;
    let release = selected_release(root)?;
    let manifest = manifest(root, &release)?;
    let digest = manifest.digest()?;
    let source = manifest.to_toml()?;
    crate::store::private_directory(&root.join("logs"))?;
    private_log(&root.join("logs/worker.stdout.log"))?;
    private_log(&root.join("logs/worker.stderr.log"))?;
    let manifest_path = stage_manifest(root, &digest, &source)?;
    let registered = client.register(&manifest_path)?;
    if registered.digest != digest || registered.manifest != manifest {
        return Err(fail(
            "Clockwork registered a different EMT worker definition",
        ));
    }
    let selected = client.switch(WORKER_KEY, &digest)?;
    if !selected.enabled || selected.definition_digest.as_deref() != Some(digest.as_str()) {
        return Err(fail(
            "Clockwork did not confirm the requested EMT worker selection",
        ));
    }
    Ok(json!({"key": WORKER_KEY, "binding": selected, "manifest": manifest_path}))
}

fn private_log(path: &Path) -> Result<()> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(file) => file.sync_all()?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.nlink() != 1
                || metadata.mode() & 0o077 != 0
            {
                return Err(fail("worker logs must be private regular files"));
            }
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn stage_manifest(root: &Path, digest: &str, source: &str) -> Result<PathBuf> {
    let directory = root.join("schedules");
    crate::store::private_directory(&directory)?;
    let path = directory.join(format!("{digest}.toml"));
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.nlink() != 1
                || metadata.mode() & 0o077 != 0
                || fs::read_to_string(&path)? != source
            {
                return Err(fail(
                    "retained worker manifest has changed or is not private",
                ));
            }
            return Ok(path);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let pending = directory.join(format!(".pending-{}", crate::random_token()?));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&pending)?;
    file.write_all(source.as_bytes())?;
    file.sync_all()?;
    fs::rename(&pending, &path)?;
    File::open(&directory)?.sync_all()?;
    Ok(path)
}

fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| fail("installation paths must be UTF-8"))
}

fn require_schedule_disabled(home: &Path) -> Result<()> {
    let root = home.join("Library/Application Support/EMT");
    let executable = home.join(".local/bin/clockwork");
    if !executable.try_exists()? {
        if root.join("install/current").try_exists()? || root.join("schedules").try_exists()? {
            return Err(fail(
                "Clockwork must be available to establish that the EMT worker is disabled",
            ));
        }
        return Ok(());
    }
    if binding(&client(home))?.is_some_and(|binding| binding.enabled) {
        return Err(fail(
            "disable emt/worker with emt schedule disable before changing the installed release",
        ));
    }
    Ok(())
}

fn installer_preflight(
    arguments: &[String],
) -> Result<Option<(cell_maintenance::Admission, File)>> {
    let Some(operation) = arguments.first().map(String::as_str) else {
        return Ok(None);
    };
    if operation == "recover" {
        return Err(fail(
            "direct selector recovery is unsupported for EMT state; inspect retained effects and use EMT state and schedule interfaces for recovery",
        ));
    }
    if operation != "install" {
        return Ok(None);
    }
    let mut home = crate::home()?;
    for pair in arguments[1..].chunks(2) {
        if pair.first().is_some_and(|flag| flag == "--home") {
            home = PathBuf::from(pair.get(1).ok_or_else(|| fail("--home requires a value"))?);
        }
    }
    if !home.is_absolute() {
        return Err(fail("installer home must be absolute"));
    }
    let root = home.join("Library/Application Support/EMT");
    if root.join("emt.sqlite3").try_exists()? {
        return Err(fail(
            "initialized EMT installations require the emt-install deploy recipe",
        ));
    }
    require_schedule_disabled(&home)?;
    let admission = crate::gate(&root).enter()?;
    let runner = crate::store::runner_lock(&root)?;
    Ok(Some((admission, runner)))
}

#[must_use]
pub fn installer_main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.is_empty() || arguments == ["--help"] || arguments == ["-h"] {
        println!(
            "emt-install {}\n\ninstall --binary ABS --bundle ABS [--home ABS] [--expected-current absent|releases/ID]\ninspect [--home ABS]\ndeploy < REQUEST.json\n\nDirect install selects files for uninitialized state and leaves scheduling disabled.\nDeploy selects files and applies owned configuration and worker scheduling.\nA failed instruction retains completed effects for explicit recovery.",
            env!("CARGO_PKG_VERSION")
        );
        return ExitCode::SUCCESS;
    }
    let _guard = match installer_preflight(&arguments) {
        Ok(guard) => guard,
        Err(error) => {
            println!(
                "{}",
                json!({"ok": false, "error": {"detail": error.to_string()}})
            );
            return ExitCode::FAILURE;
        }
    };
    cell_install::simple::main_with_deployment(&specification(), env!("CARGO_PKG_VERSION"), deploy)
}

/// Install files, initialize owned state, write configuration, and publish the worker.
/// # Errors
/// Returns installation or owned setup failures.
pub fn deploy(context: &cell_install::adapter::Context) -> cell_install::Result<()> {
    deploy_inner(context).map_err(|error| cell_install::Error::new(error.to_string()))
}

fn deploy_inner(context: &cell_install::adapter::Context) -> Result<()> {
    let root = context.home.join("Library/Application Support/EMT");
    let initialized = root.join("emt.sqlite3").try_exists()?;
    let (config, enabled) = configuration(context, &root, initialized)?;
    let observed = if context
        .home
        .join("Library/Application Support/Clockwork/clockwork.db")
        .try_exists()?
        || context.home.join(".local/bin/clockwork").try_exists()?
    {
        binding(&Client::new(context.dependency_binary("clockwork")?))?
    } else {
        None
    };

    cell_install::simple::deploy_program(&specification(), env!("CARGO_PKG_VERSION"), context)?;
    let _admission = crate::gate(&root).enter()?;
    let _runner = crate::store::runner_lock(&root)?;
    crate::store::Store::initialize(&root)?;
    config.save(&root)?;
    let client = Client::new(context.dependency_binary("clockwork")?);
    Client::new(&config.clockwork_executable).configure_emt(if config.paused {
        None
    } else {
        Some(&config.receiving_domain)
    })?;
    let activate = enabled.unwrap_or_else(|| {
        observed
            .as_ref()
            .map_or(!initialized, |value| value.enabled)
    });
    if observed.is_none() && initialized && !activate {
        return Ok(());
    }
    let selected = selected_release_metadata(&root)?;
    let definition = manifest(&root, &selected)?;
    crate::store::private_directory(&root.join("logs"))?;
    private_log(&root.join("logs/worker.stdout.log"))?;
    private_log(&root.join("logs/worker.stderr.log"))?;
    let path = stage_manifest(&root, &definition.digest()?, &definition.to_toml()?)?;
    let registered = client.register(&path)?;
    if activate {
        client.switch(WORKER_KEY, &registered.digest)?;
    } else {
        client.disable(WORKER_KEY, Some(&registered.digest))?;
    }
    Ok(())
}

fn configuration(
    context: &cell_install::adapter::Context,
    root: &Path,
    initialized: bool,
) -> Result<(crate::store::Config, Option<bool>)> {
    let mut config = if initialized {
        crate::store::Config::load(root)?
    } else {
        crate::store::Config::defaults()?
    };
    if !initialized {
        config.receiving_domain.clear();
        config.paused = false;
    }
    let mut enabled = None;
    if let Some(settings) = &context.request.settings {
        let mut settings = settings
            .as_object()
            .ok_or_else(|| fail("deployment settings must be an object"))?
            .clone();
        if let Some(value) = settings.remove("enabled") {
            enabled = Some(
                value
                    .as_bool()
                    .ok_or_else(|| fail("enabled must be a boolean"))?,
            );
        }
        if settings.contains_key("poll_after") {
            return Err(fail(
                "deployment settings cannot change incoming-mail progress",
            ));
        }
        let mut merged = serde_json::to_value(&config)?;
        merged
            .as_object_mut()
            .ok_or_else(|| fail("invalid saved configuration"))?
            .extend(settings);
        config = serde_json::from_value(merged)?;
    }
    if config.receiving_domain.is_empty()
        && let Some(domain) = context
            .request
            .dependency_settings
            .get("email")
            .and_then(|settings| settings.get("receiving_domain"))
            .and_then(Value::as_str)
    {
        domain.clone_into(&mut config.receiving_domain);
    }
    if config.receiving_domain.is_empty() {
        let email = if config.email_executable == context.home.join(".local/bin/email") {
            context.dependency_inspection_binary("email")?
        } else {
            config.email_executable.clone()
        };
        let settings = tokio::runtime::Runtime::new()?
            .block_on(email::api::Client::new(&email).receiving_settings())?;
        if settings.domains.len() != 1 {
            return Err(fail(
                "configure one receiving domain through Email before deployment",
            ));
        }
        config.receiving_domain.clone_from(&settings.domains[0]);
    }
    Ok((config, enabled))
}
