//! Production schedules translated into immutable Clockwork activation definitions.

use anyhow::{Context, Result, ensure};
use clockwork::api::{
    Authority, BindingRecord, Client, FailurePolicy, LaunchImage, Output, OverlapPolicy,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::Path;

use crate::manifest::{Job, Manifest, Schedule};

pub type Definition = clockwork::api::Manifest;

const PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
const TIMEOUT_SECONDS: u64 = 1_380;
const INVENTORY_LIMIT: usize = 10_000;

/// Apply every production schedule while retaining enablement and failure incidents.
/// New production schedules remain disabled until explicitly enabled.
pub fn apply(root: &Path, manifest: &Manifest) -> Result<Value> {
    let home = crate::home()?;
    apply_with(
        root,
        manifest,
        &home.join(".local/bin/clockwork"),
        &home.join(".local/bin/email"),
        &home.join(".local/bin/paperboy"),
    )
}

/// Apply with exact dependency locations selected by the product installer.
pub fn apply_with(
    root: &Path,
    manifest: &Manifest,
    clockwork_binary: &Path,
    email_binary: &Path,
    executable: &Path,
) -> Result<Value> {
    apply_with_prior(
        root,
        manifest,
        clockwork_binary,
        email_binary,
        executable,
        None,
    )
}

/// Stop owned scheduled activations before changing the fixed runtime image.
pub fn suspend_for_deployment(clockwork_binary: &Path) -> Result<BTreeMap<String, BindingRecord>> {
    let client = Client::new(clockwork_binary);
    let prior = bindings(&client)?;
    for binding in prior.values() {
        client.disable(&binding.key, None)?;
    }
    Ok(prior)
}

/// Apply candidate definitions with intent captured before deployment suspension.
pub fn apply_with_prior(
    root: &Path,
    manifest: &Manifest,
    clockwork_binary: &Path,
    email_binary: &Path,
    executable: &Path,
    saved: Option<BTreeMap<String, BindingRecord>>,
) -> Result<Value> {
    manifest.validate()?;
    if !manifest.jobs.is_empty() {
        crate::runtime::executable(email_binary).context("installed Email wrapper unavailable")?;
    }
    for (id, job) in &manifest.jobs {
        crate::runtime::executable(Path::new(&job.render[0]))
            .with_context(|| format!("renderer for production {id} unavailable"))?;
    }
    let _lock = mutation_lock(root)?;
    let client = Client::new(clockwork_binary);
    let prior = match saved {
        Some(prior) => prior,
        None => bindings(&client)?,
    };
    let definitions = manifest
        .jobs
        .iter()
        .map(|(id, job)| {
            definition(root, id, job, executable, email_binary)
                .map(|definition| (id.clone(), definition))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;

    // Registration checks every artifact and definition before a binding changes.
    let directory = root.join("schedule-definitions");
    private_directory(&directory)?;
    let registered = definitions
        .iter()
        .map(|(id, definition)| {
            register(&client, &directory, id, definition).map(|digest| (id.clone(), digest))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;

    let mut removed = Vec::new();
    for binding in prior.values() {
        let id = binding.key.strip_prefix("paperboy/").unwrap_or_default();
        if !manifest.jobs.contains_key(id) {
            let disabled = if binding.enabled {
                client.disable(&binding.key, None)?
            } else {
                binding.clone()
            };
            removed.push(disabled);
        }
    }

    let mut selected = Vec::new();
    for (id, digest) in registered {
        let key = key(&id)?;
        let binding = select(&client, &key, &digest, prior.get(&key))?;
        selected.push(binding);
    }
    Ok(json!({"jobs":selected,"removed":removed}))
}

/// Change scheduled admission for a selected production without changing its snapshot.
pub fn set_enabled(root: &Path, id: &str, enabled: bool) -> Result<Value> {
    let key = key(id)?;
    let _lock = mutation_lock(root)?;
    let client = installed_client()?;
    let binding = if enabled {
        let binding = client.binding(&key)?;
        let digest = binding
            .definition_digest
            .as_deref()
            .context("apply the Paperboy production definitions before enabling this schedule")?;
        let definition = client.definition(digest)?;
        ensure!(
            definition.key == key && selected_runner(id, &definition.manifest.arguments),
            "apply the Paperboy production definitions before enabling this schedule; its selected activation definition is not a production runner"
        );
        if binding.enabled {
            binding
        } else {
            client.switch(&key, digest)?
        }
    } else {
        client.disable(&key, None)?
    };
    Ok(serde_json::to_value(binding)?)
}

/// Read recorded Clockwork state without applying or initializing a manifest.
pub fn status(_root: &Path, id: Option<&str>) -> Result<Value> {
    let client = installed_client()?;
    if let Some(id) = id {
        return Ok(serde_json::to_value(client.binding(&key(id)?)?)?);
    }
    Ok(json!({"jobs":bindings(&client)?.into_values().collect::<Vec<_>>()}))
}

/// Snapshot a production's command and subject into an exact activation definition.
pub fn definition(
    root: &Path,
    id: &str,
    job: &Job,
    executable: &Path,
    email_binary: &Path,
) -> Result<Definition> {
    job.validate(id)?;
    let root = fs::canonicalize(root).context("Paperboy state directory unavailable")?;
    let executable = fs::canonicalize(executable).context("installed Paperboy unavailable")?;
    let home = crate::home()?;
    let release =
        fs::canonicalize(home.join("Library/Application Support/Paperboy/install/current"))
            .context("installed Paperboy release directory missing")?;
    ensure!(
        executable
            == home.join("Library/Application Support/Paperboy/install/runtime/bin/paperboy"),
        "schedule application requires the stable installed Paperboy executable"
    );
    let release_id = release
        .file_name()
        .and_then(|name| name.to_str())
        .context("Paperboy release identity must be UTF-8")?;
    ensure!(
        cell_install::valid_release_id(release_id),
        "schedule application requires an immutable installed Paperboy release"
    );
    let logs = root.join("logs");
    private_directory(&logs)?;
    let environment = BTreeMap::from([
        ("HOME".to_string(), path_string(&home)?.to_string()),
        (
            "PATH".to_string(),
            format!("{PATH}:{}", home.join(".local/bin").display()),
        ),
    ]);
    let mut definition = Definition {
        schema_version: 2,
        key: key(id)?,
        release_id: release_id.to_string(),
        release_root: path_string(&release)?.to_string(),
        authority: Authority::CurrentUserBackground,
        overlap: OverlapPolicy::Skip,
        failure: FailurePolicy {
            email_cli: Some(path_string(email_binary)?.to_string()),
            ..FailurePolicy::default()
        },
        timeout_seconds: Some(TIMEOUT_SECONDS),
        arguments: arguments(id, job, email_binary)?,
        cwd: path_string(&root)?.to_string(),
        schedule: schedule(&job.schedule),
        launch: LaunchImage::Direct {
            program: path_string(&release.join("bin/paperboy"))?.to_string(),
            sha256: cell_install::file_digest(&release.join("bin/paperboy"))?,
        },
        environment,
        output: Output {
            stdout: path_string(&logs.join(format!("{id}.stdout.log")))?.to_string(),
            stderr: path_string(&logs.join(format!("{id}.stderr.log")))?.to_string(),
        },
    };
    definition.use_runtime_paths()?;
    Ok(definition)
}

fn installed_client() -> Result<Client> {
    Ok(Client::new(crate::home()?.join(".local/bin/clockwork")))
}

fn bindings(client: &Client) -> Result<BTreeMap<String, BindingRecord>> {
    let page = client.bindings_limit(INVENTORY_LIMIT)?;
    ensure!(!page.has_more, "Clockwork binding inventory is incomplete");
    Ok(page
        .items
        .into_iter()
        .filter(|binding| binding.key.starts_with("paperboy/"))
        .map(|binding| (binding.key.clone(), binding))
        .collect())
}

fn select(
    client: &Client,
    key: &str,
    digest: &str,
    prior: Option<&BindingRecord>,
) -> Result<BindingRecord> {
    if let Some(prior) = prior
        && prior.definition_digest.as_deref() == Some(digest)
    {
        let current = client.binding(key)?;
        if current.definition_digest == prior.definition_digest && current.enabled == prior.enabled
        {
            return Ok(current);
        }
    }
    if prior.is_some_and(|binding| binding.enabled) {
        Ok(client.switch(key, digest)?)
    } else {
        Ok(client.disable(key, Some(digest))?)
    }
}

fn register(
    client: &Client,
    directory: &Path,
    id: &str,
    definition: &Definition,
) -> Result<String> {
    let digest = definition.digest()?;
    let path = directory.join(format!("{id}-{digest}.toml"));
    let bytes = definition.to_toml()?;
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(bytes.as_bytes())?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            regular_private_file(&path)?;
            ensure!(
                fs::read(&path)? == bytes.as_bytes(),
                "retained Clockwork definition bytes conflict"
            );
        }
        Err(error) => return Err(error).context("prepare Clockwork definition"),
    }
    let registered = client.register(&path)?;
    ensure!(
        registered.digest == digest && registered.manifest == *definition,
        "Clockwork registration did not retain the supplied definition"
    );
    fs::remove_file(path).context("remove registered Clockwork definition file")?;
    Ok(registered.digest)
}

fn selected_runner(id: &str, arguments: &[String]) -> bool {
    let Some([command, flag, production]) = arguments.get(..3) else {
        return false;
    };
    command == "execute" && matches!(flag.as_str(), "--production" | "--job") && production == id
}

fn arguments(id: &str, job: &Job, email_binary: &Path) -> Result<Vec<String>> {
    let mut arguments = vec![
        "execute".to_string(),
        "--production".to_string(),
        id.to_string(),
        "--subject".to_string(),
        job.subject(id).to_string(),
        "--email".to_string(),
        path_string(email_binary)?.to_string(),
        "--".to_string(),
    ];
    arguments.extend(job.render.iter().cloned());
    ensure!(
        !arguments.iter().any(|argument| argument == "-c"),
        "Clockwork does not admit a literal -c argument"
    );
    Ok(arguments)
}

fn schedule(schedule: &Schedule) -> clockwork::api::Schedule {
    match schedule {
        Schedule::Interval { seconds } => clockwork::api::Schedule::Interval {
            seconds: u64::from(*seconds),
            run_at_load: false,
        },
        Schedule::LocalCalendar { hour, minute } => clockwork::api::Schedule::LocalCalendar {
            hour: *hour,
            minute: *minute,
            run_at_load: false,
        },
    }
}

fn key(id: &str) -> Result<String> {
    crate::manifest::validate_id(id)?;
    Ok(format!("paperboy/{id}"))
}

fn path_string(path: &Path) -> Result<&str> {
    ensure!(path.is_absolute(), "schedule paths must be absolute");
    path.to_str().context("schedule paths must be UTF-8")
}

fn private_directory(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute(),
        "Paperboy state directory must be absolute"
    );
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink() && fs::canonicalize(path)? == path,
        "Paperboy state must use a canonical regular directory"
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn regular_private_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.nlink() == 1
            && metadata.mode().trailing_zeros() >= 6,
        "Paperboy schedule state must be a private regular single-link file"
    );
    Ok(())
}

fn mutation_lock(root: &Path) -> Result<File> {
    private_directory(root)?;
    let path = root.join("schedule.lock");
    match fs::symlink_metadata(&path) {
        Ok(_) => regular_private_file(&path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspect Paperboy schedule lock"),
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)?;
    regular_private_file(&path)?;
    fs2::FileExt::try_lock_exclusive(&file)
        .context("another Paperboy schedule mutation is active")?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_literal_renderer_arguments_and_subject() -> Result<()> {
        let mut job = Job {
            render: vec!["/reports/digest".into(), "--window".into(), "a b $c".into()],
            subject: Some("A daily report".into()),
            schedule: Schedule::Interval { seconds: 300 },
        };
        let captured = arguments("digest", &job, Path::new("/Users/user/.local/bin/email"))?;
        job.render[2] = "changed".into();
        job.subject = Some("changed".into());
        assert_eq!(captured[4], "A daily report");
        assert_eq!(&captured[8..], ["/reports/digest", "--window", "a b $c"]);
        assert!(!captured.iter().any(|argument| argument == "--manifest"));
        Ok(())
    }

    #[test]
    fn supported_schedules_never_run_at_load() {
        assert_eq!(
            schedule(&Schedule::Interval { seconds: 3_600 }),
            clockwork::api::Schedule::Interval {
                seconds: 3_600,
                run_at_load: false
            }
        );
        assert_eq!(
            schedule(&Schedule::LocalCalendar {
                hour: 9,
                minute: 15
            }),
            clockwork::api::Schedule::LocalCalendar {
                hour: 9,
                minute: 15,
                run_at_load: false
            }
        );
    }

    #[test]
    fn binding_identity_is_production_specific_and_closed() -> Result<()> {
        assert_eq!(key("daily")?, "paperboy/daily");
        assert_eq!(key("jobs-2")?, "paperboy/jobs-2");
        for invalid in ["", "Daily", "jobs/daily", "jobs_daily", "2jobs", "../jobs"] {
            assert!(key(invalid).is_err());
        }
        assert!(key(&"x".repeat(64)).is_err());
        Ok(())
    }

    #[test]
    fn enablement_accepts_current_and_retained_production_runners() {
        assert!(selected_runner(
            "daily",
            &["execute".into(), "--production".into(), "daily".into()]
        ));
        assert!(selected_runner(
            "daily",
            &["execute".into(), "--job".into(), "daily".into()]
        ));
        assert!(!selected_runner(
            "daily",
            &["execute".into(), "--job".into(), "other".into()]
        ));
    }
}
