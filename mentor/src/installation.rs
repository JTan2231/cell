//! Content-addressed program installation and the one explicitly enabled worker.

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

pub const WORKER_KEY: &str = "mentor/worker";

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "mentor",
        application: "MentorMail",
        source_directory: "mentor",
        provider_source: "mentor/chancery",
        legacy_provider_path: "share/chancery/mentor",
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
        maintained: true,
    }
}

fn require_standard_root(root: &Path) -> Result<PathBuf> {
    let home = crate::home()?;
    if root != home.join("Library/Application Support/MentorMail") {
        return Err(fail(
            "scheduled installation requires the standard MentorMail state root",
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
    let home = require_standard_root(root)?;
    let spec = specification();
    transaction::inspect_installation(&spec.layout(), &home, &|path| spec.legacy(path))?
        .current
        .ok_or_else(|| fail("install Mentor before enabling its worker"))
}

fn manifest(root: &Path, release: &ReleaseInfo) -> Result<Manifest> {
    let home = require_standard_root(root)?;
    let release_root = root.join("install/releases").join(&release.release_id);
    let executable = release_root.join("bin/mentor");
    let executable_hash = release
        .files
        .get("bin/mentor")
        .ok_or_else(|| fail("installed release has no Mentor executable"))?
        .sha256
        .clone();
    Ok(Manifest {
        schema_version: 2,
        key: WORKER_KEY.into(),
        release_id: release.release_id.clone(),
        release_root: path_text(&release_root)?,
        authority: Authority::CurrentUserBackground,
        overlap: OverlapPolicy::Skip,
        failure: clockwork::api::FailurePolicy::default(),
        timeout_seconds: Some(90),
        arguments: vec!["--json".into(), "worker".into()],
        cwd: path_text(root)?,
        schedule: Schedule::Interval {
            seconds: 3600,
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
            return Err(fail("enabled Mentor binding has no selected definition"));
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
            "mentor/worker selects a definition outside Mentor's owned installation",
        ));
    }
    let spec = specification();
    let release =
        transaction::verify_release_at(&spec.layout(), &release_root, &|path| spec.legacy(path))?;
    let mut expected = manifest(root, &release)?;
    // A retained schema-one selection remains owned and can be upgraded. Its
    // immutable bytes and Clockwork incident state must not be rewritten.
    expected.schema_version = definition.manifest.schema_version;
    // Keep the previous verified interval recognizable during an hourly upgrade.
    if definition.manifest.schedule
        == (Schedule::Interval {
            seconds: 60,
            run_at_load: false,
        })
    {
        expected.schedule = definition.manifest.schedule.clone();
    }
    if definition.digest != digest || definition.manifest != expected {
        return Err(fail(
            "mentor/worker does not match Mentor's supported worker definition",
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
    Ok(serde_json::to_value(snapshot)?)
}

/// Operate only Mentor's Clockwork binding. Operator pause remains independent.
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
            "Clockwork registered a different Mentor worker definition",
        ));
    }
    let selected = client.switch(WORKER_KEY, &digest)?;
    if !selected.enabled || selected.definition_digest.as_deref() != Some(digest.as_str()) {
        return Err(fail(
            "Clockwork did not confirm the requested Mentor worker selection",
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
    let root = home.join("Library/Application Support/MentorMail");
    let executable = home.join(".local/bin/clockwork");
    if !executable.try_exists()? {
        if root.join("install/current").try_exists()? || root.join("schedules").try_exists()? {
            return Err(fail(
                "Clockwork must be available to establish that the Mentor worker is disabled",
            ));
        }
        return Ok(());
    }
    if binding(&client(home))?.is_some_and(|binding| binding.enabled) {
        return Err(fail(
            "disable mentor/worker with mentor schedule disable before changing the installed release",
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
            "direct selector recovery is unsupported for Mentor state; use coordinated deployment recovery",
        ));
    }
    if operation == "adapter" {
        return Ok(None);
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
    let root = home.join("Library/Application Support/MentorMail");
    if root.join("mentor.sqlite3").try_exists()? {
        return Err(fail(
            "initialized Mentor installations must be updated through coordinated deployment",
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
            "mentor-install {}\n\ninstall --binary ABS --bundle ABS [--home ABS] [--expected-current absent|releases/HASH]\ninspect [--home ABS]\nverify --binary ABS --bundle ABS [--home ABS]\nverify-release ABS\n\nDirect install is for uninitialized state and leaves scheduling disabled.\nInitialized updates and recovery use the Cell maintained deployment coordinator.\nCoordinated deployment preserves worker intent and activates after verification.",
            env!("CARGO_PKG_VERSION")
        );
        return ExitCode::SUCCESS;
    }
    let _guard = match installer_preflight(&arguments) {
        Ok(guard) => guard,
        Err(error) if arguments.first().is_some_and(|value| value == "adapter") => {
            return cell_install::adapter::finish(Err(cell_install::Error::new(error.to_string())));
        }
        Err(error) => {
            println!(
                "{}",
                json!({"ok": false, "error": {"detail": error.to_string()}})
            );
            return ExitCode::FAILURE;
        }
    };
    cell_install::simple::main_with_lifecycle(
        &specification(),
        env!("CARGO_PKG_VERSION"),
        deployment_lifecycle,
    )
}

fn deployment_lifecycle(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> cell_install::Result<Value> {
    deployment_operation(context, operation)
        .map_err(|error| cell_install::Error::new(error.to_string()))
}

#[allow(clippy::too_many_lines)] // Keep captured intent and its ordered recovery together.
fn deployment_operation(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> Result<Value> {
    use cell_install::adapter::Operation;
    let root = context.home.join("Library/Application Support/MentorMail");
    let client = client(&context.home);
    if operation == Operation::Inspect {
        let initialized = root.join("mentor.sqlite3").try_exists()?;
        let mut config = if initialized {
            crate::store::Store::open(&root)?.config()?
        } else {
            crate::store::Config::defaults()?
        };
        if !initialized {
            config.receiving_domain.clear();
            config.paused = false;
        }
        let mut activate = None;
        if let Some(settings) = &context.request.settings {
            let mut settings = settings
                .as_object()
                .ok_or_else(|| fail("deployment settings must be an object"))?
                .clone();
            if let Some(enabled) = settings.remove("enabled") {
                activate = Some(
                    enabled
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
        config.validate()?;
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
        if let Some(value) = &observed {
            require_owned_binding(&root, &client, value)?;
        }
        return Ok(
            json!({"binding":observed,"config":config,"initialized":initialized,
            "activate":activate.unwrap_or_else(|| observed.as_ref().map_or(!initialized, |value| value.enabled))}),
        );
    }
    let prior = &context.prior()?["lifecycle"];
    let before: Option<BindingRecord> = serde_json::from_value(prior["binding"].clone())?;
    let forward = context.request.recovery.as_ref().is_none_or(|recovery| {
        recovery["any_apply_started"] == true && context.home.join(".local/bin/mentor").is_file()
    });
    if operation == Operation::Hold {
        let observed =
            if before.is_none() && !context.home.join(".local/bin/clockwork").try_exists()? {
                None
            } else {
                binding(&client)?
            };
        if context.request.recovery.is_none() {
            require_prior_selection(before.as_ref(), observed.as_ref())?;
        }

        if observed.is_some() {
            client.disable(WORKER_KEY, None)?;
        }
        if context.request.recovery.is_some() && forward && prior["initialized"] == false {
            // Repair only initialization authorized by the original absent-state
            // baseline before the coordinator asks this product to drain again.
            let gate = crate::gate(&root);
            gate.hold(&context.request.run_id)?;
            let _admission = gate.enter_for(&context.request.run_id)?;
            let _runner = crate::store::runner_lock(&root)?;
            crate::store::Store::initialize(&root)?;
        }
    }
    if operation == Operation::Configure || operation == Operation::Recover && forward {
        configure_worker(&root, &client, context, prior, before.as_ref())?;
    }
    if operation == Operation::Recover && !forward {
        // No publication began. Keep the original generation inactive until release.
        if let Some(before) = &before {
            client.disable(WORKER_KEY, before.definition_digest.as_deref())?;
        }
    }
    if operation == Operation::Activate {
        let activate = if forward {
            prior["activate"] == true
        } else {
            before.as_ref().is_some_and(|value| value.enabled)
        };
        if forward {
            let _admission = crate::gate(&root).enter()?;
            let _runner = crate::store::runner_lock(&root)?;
            let store = crate::store::Store::open(&root)?;
            let mut config = store.config()?;
            config.paused = prior["config"]["paused"]
                .as_bool()
                .ok_or_else(|| fail("saved pause intent is missing"))?;
            config.validate()?;
            store.set_config(&config)?;
        }
        if activate {
            let observed =
                binding(&client)?.ok_or_else(|| fail("worker activation has no binding"))?;
            let digest = observed
                .definition_digest
                .as_deref()
                .ok_or_else(|| fail("worker activation has no selected definition"))?;
            let selected = client.switch(WORKER_KEY, digest)?;
            if !selected.enabled || selected.definition_digest.as_deref() != Some(digest) {
                return Err(fail("Clockwork did not confirm worker activation"));
            }
        }
    }
    if matches!(
        operation,
        Operation::Verify | Operation::Recover | Operation::Activate
    ) {
        let observed = if context.home.join(".local/bin/clockwork").is_file() {
            binding(&client)?
        } else {
            None
        };
        if forward
            && (before.is_some() || prior["initialized"] == false || prior["activate"] == true)
            && observed.is_none()
        {
            return Err(fail("configured worker binding is absent"));
        }
        if let Some(observed) = observed {
            require_owned_binding(&root, &client, &observed)?;
            if before.as_ref().is_some_and(|before| {
                before.halted_incident.is_some()
                    && observed.halted_incident != before.halted_incident
            }) {
                return Err(fail("deployment changed the worker failure halt"));
            }
            if operation != Operation::Activate && observed.enabled {
                return Err(fail("worker became enabled before deployment activation"));
            }
        }
    }
    Ok(json!({}))
}

fn require_prior_selection(
    before: Option<&BindingRecord>,
    observed: Option<&BindingRecord>,
) -> Result<()> {
    let unchanged = match (before, observed) {
        (None, None) => true,
        (Some(before), Some(observed)) => {
            before.definition_digest == observed.definition_digest
                && before.halted_incident == observed.halted_incident
                && (before.enabled || !observed.enabled)
        }
        _ => false,
    };
    if !unchanged {
        return Err(fail("worker binding changed since deployment inspection"));
    }
    Ok(())
}

fn configure_worker(
    root: &Path,
    client: &Client,
    context: &cell_install::adapter::Context,
    prior: &Value,
    before: Option<&BindingRecord>,
) -> Result<()> {
    migrate_for_configuration(root, context)?;
    let gate = crate::gate(root);
    let _admission = gate.enter_for(&context.request.run_id)?;
    let _runner = crate::store::runner_lock(root)?;
    let mut config: crate::store::Config = serde_json::from_value(prior["config"].clone())?;
    config.validate()?;
    if prior["initialized"] == false {
        config.paused = true;
    }
    crate::store::Store::open(root)?.set_config(&config)?;
    // Preserve an existing installation's absent binding as operator intent.
    if before.is_none() && prior["initialized"] == true && prior["activate"] != true {
        return Ok(());
    }
    let selected = selected_release(root)?;
    let definition = manifest(root, &selected)?;
    let digest = definition.digest()?;
    crate::store::private_directory(&root.join("logs"))?;
    private_log(&root.join("logs/worker.stdout.log"))?;
    private_log(&root.join("logs/worker.stderr.log"))?;
    let path = stage_manifest(root, &digest, &definition.to_toml()?)?;
    let registered = client.register(&path)?;
    if registered.digest != digest || registered.manifest != definition {
        return Err(fail("registered worker differs from its owned definition"));
    }
    let selected = client.disable(WORKER_KEY, Some(&digest))?;
    if selected.enabled || selected.definition_digest.as_deref() != Some(&digest) {
        return Err(fail("Clockwork did not confirm disabled worker selection"));
    }
    Ok(())
}

fn migrate_for_configuration(root: &Path, context: &cell_install::adapter::Context) -> Result<()> {
    let directory = &context.request.run_dir;
    crate::store::private_directory(directory)?;
    let marker = directory.join("mentor-migration.json");
    let backup = root.join(format!(
        "mentor-pre-migration-{}.sqlite",
        context.request.run_id
    ));
    if marker.try_exists()? {
        let metadata = fs::symlink_metadata(&marker)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.nlink() != 1
            || metadata.mode() & 0o077 != 0
        {
            return Err(fail("migration receipt must be a private regular file"));
        }
        let receipt: Value = serde_json::from_slice(&fs::read(&marker)?)?;
        if receipt["run_id"] != context.request.run_id || receipt["schema_version"] != 1 {
            return Err(fail("migration receipt does not match this deployment"));
        }
        for (path, hash) in receipt["backups"]
            .as_object()
            .ok_or_else(|| fail("migration backup evidence is missing"))?
        {
            if cell_install::file_digest(Path::new(path))?
                != hash
                    .as_str()
                    .ok_or_else(|| fail("migration backup digest is invalid"))?
            {
                return Err(fail("retained migration backup changed"));
            }
        }
        return Ok(());
    }
    let receipt = if context.prior()?["lifecycle"]["initialized"] == false {
        let gate = crate::gate(root);
        let _admission = gate.enter_for(&context.request.run_id)?;
        let _runner = crate::store::runner_lock(root)?;
        crate::store::Store::initialize(root)?;
        json!({"data":{"schema_version":1,"backup":null}})
    } else {
        cell_install::command::json(
            &context.home.join(".local/bin/mentor"),
            &[
                "--json".into(),
                "migrate".into(),
                "--backup".into(),
                backup.clone().into_os_string(),
            ],
            &BTreeMap::from([(
                "CELL_DEPLOYMENT_RUN_ID".into(),
                context.request.run_id.clone().into(),
            )]),
            std::time::Duration::from_secs(600),
        )?
    };
    let mut backups = serde_json::Map::new();
    for name in ["backup", "config_backup"] {
        if let Some(path) = receipt["data"][name].as_str() {
            backups.insert(
                path.to_owned(),
                json!(cell_install::file_digest(Path::new(path))?),
            );
        }
    }
    let receipt = json!({"run_id":context.request.run_id,"schema_version":1,"backups":backups});
    let pending = directory.join(format!(".mentor-migration-{}", crate::random_token()?));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&pending)?;
    file.write_all(&serde_json::to_vec(&receipt)?)?;
    file.sync_all()?;
    fs::rename(&pending, &marker)?;
    File::open(directory)?.sync_all()?;
    Ok(())
}
