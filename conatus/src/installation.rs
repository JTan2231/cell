//! Program installation is separate from Conatus intake and schedule activation.

use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use cell_install::legacy::LegacySpec;
use cell_install::simple::Spec;
use cell_install::transaction::{self, LockKind};
use clap::Parser;
use serde::Serialize;
use serde_json::{Value, json};

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "conatus",
        application: "Conatus",
        source_directory: "conatus",
        provider_source: "conatus/chancery",
        legacy_provider_path: "share/chancery/conatus",
        legacy: &LegacySpec {
            format: "",
            manifest: "",
            metadata: &[],
            proofs: &[],
            providers: &[],
            hash_path_lines: false,
        },
        wrapper: None,
        lock_kind: LockKind::Shlock,
        lock_at_state: false,
        maintained: false,
    }
}

#[derive(Parser)]
#[command(
    about = "Write a Clockwork definition for the selected Conatus release; do not activate it"
)]
pub struct ScheduleDefinitionArgs {
    /// Existing Conatus state directory used by the scheduled update.
    #[arg(long)]
    state_dir: PathBuf,
    /// New private TOML file; existing files are not replaced.
    #[arg(long)]
    output: PathBuf,
    /// Current-user installation home; defaults to HOME.
    #[arg(long)]
    home: Option<PathBuf>,
}

#[derive(Serialize)]
struct Definition {
    schema_version: u32,
    key: &'static str,
    release_id: String,
    release_root: PathBuf,
    authority: &'static str,
    overlap: &'static str,
    failure: clockwork::api::FailurePolicy,
    arguments: Vec<String>,
    cwd: PathBuf,
    schedule: Schedule,
    launch: Launch,
    environment: BTreeMap<&'static str, String>,
    output: Output,
}

#[derive(Serialize)]
struct Schedule {
    kind: &'static str,
    seconds: u64,
    run_at_load: bool,
}

#[derive(Serialize)]
struct Launch {
    kind: &'static str,
    program: PathBuf,
    sha256: String,
}

#[derive(Serialize)]
struct Output {
    stdout: PathBuf,
    stderr: PathBuf,
}

/// Write the schedule candidate without registering or switching Clockwork.
///
/// # Errors
/// Rejects an absent or unproved release, nonabsolute paths, and existing output.
pub fn schedule_definition(args: ScheduleDefinitionArgs) -> Result<Value> {
    let home = args
        .home
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .context("HOME or --home is required")?;
    if !home.is_absolute() || !args.state_dir.is_absolute() || !args.output.is_absolute() {
        bail!("schedule paths must be absolute");
    }
    let home = fs::canonicalize(home)?;
    let state_dir = fs::canonicalize(args.state_dir)?;
    if !state_dir.is_dir() {
        bail!("--state-dir must be an existing directory");
    }
    let spec = specification();
    let release =
        transaction::inspect_installation(&spec.layout(), &home, &|path| spec.legacy(path))?
            .current
            .context("install Conatus before preparing its schedule")?;
    let release_root = home
        .join("Library/Application Support/Conatus/install/releases")
        .join(&release.release_id);
    let binary = release
        .files
        .get("bin/conatus")
        .context("selected release has no Conatus program")?;
    let logs = state_dir.join("logs");
    if !logs.exists() {
        DirBuilder::new().mode(0o700).create(&logs)?;
    }
    if !fs::symlink_metadata(&logs)?.is_dir() {
        bail!("Conatus logs path must be a regular directory");
    }
    let definition = Definition {
        schema_version: 2,
        key: "conatus/update",
        release_id: release.release_id,
        release_root: release_root.clone(),
        authority: "current-user-background",
        overlap: "skip",
        failure: clockwork::api::FailurePolicy::default(),
        arguments: vec![
            "--state-dir".to_owned(),
            state_dir
                .to_str()
                .context("state directory must be UTF-8")?
                .to_owned(),
            "update".to_owned(),
        ],
        cwd: state_dir,
        schedule: Schedule {
            kind: "interval",
            seconds: 300,
            run_at_load: true,
        },
        launch: Launch {
            kind: "direct",
            program: release_root.join("bin/conatus"),
            sha256: binary.sha256.clone(),
        },
        environment: BTreeMap::from([
            (
                "HOME",
                home.to_str().context("home must be UTF-8")?.to_owned(),
            ),
            ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin".to_owned()),
        ]),
        output: Output {
            stdout: logs.join("update.out.log"),
            stderr: logs.join("update.err.log"),
        },
    };
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&args.output)
        .context("schedule output must be a new file in an existing directory")?;
    output.write_all(toml::to_string_pretty(&definition)?.as_bytes())?;
    output.sync_all()?;
    Ok(json!({
        "key": definition.key,
        "release_id": definition.release_id,
        "definition": args.output,
        "registered": false,
        "activated": false,
    }))
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    state_dir: Option<PathBuf>,
    decisions_config: Option<PathBuf>,
    annals_state_dir: Option<PathBuf>,
    library: Option<String>,
    enabled: Option<bool>,
}

fn configuration(root: &std::path::Path) -> Result<Option<crate::Config>> {
    if !root.join("conatus.db").exists() {
        return Ok(None);
    }
    let store = crate::store::Store::open(root)?;
    store
        .setting("config")?
        .map(|value| serde_json::from_str(&value).map_err(Into::into))
        .transpose()
}

/// Configure Conatus and its exact disabled runner inside deployment maintenance.
pub fn lifecycle(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> cell_install::Result<Value> {
    lifecycle_inner(context, operation).map_err(|error| cell_install::Error::new(error.to_string()))
}

#[allow(clippy::too_many_lines)]
fn lifecycle_inner(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> Result<Value> {
    use cell_install::adapter::Operation;
    use clockwork::deployment::ScheduleState;
    const KEY: &str = "conatus/update";
    let settings: Settings = serde_json::from_value(
        context
            .request
            .settings
            .clone()
            .unwrap_or_else(|| json!({})),
    )?;
    let root = settings.state_dir.clone().unwrap_or(crate::state_root()?);
    anyhow::ensure!(root.is_absolute(), "Conatus state_dir must be absolute");
    let clockwork = clockwork::api::Client::new(context.dependency_binary("clockwork")?);
    let gate = crate::gate(&root);
    let present = configuration(&root)?.is_some();
    if operation == Operation::Inspect {
        let status = gate.status()?;
        anyhow::ensure!(
            status.holds.is_empty(),
            "another operation holds Conatus maintenance"
        );
        let config = if present {
            Some(crate::store::Store::open(&root)?.config()?)
        } else {
            None
        };
        if let Some(config) = &config {
            anyhow::ensure!(
                settings
                    .decisions_config
                    .as_ref()
                    .is_none_or(|path| path == &config.decisions_config)
                    && settings
                        .annals_state_dir
                        .as_ref()
                        .is_none_or(|path| Some(path) == config.annals_state_dir.as_ref())
                    && settings
                        .library
                        .as_ref()
                        .is_none_or(|name| name == &config.library),
                "deployment cannot replace Conatus library selections"
            );
        }
        let paused = present
            && crate::store::Store::open(&root)?
                .setting("paused")?
                .as_deref()
                == Some("true");
        return Ok(
            json!({"state_dir":root,"initialized":present,"config":config,"paused":paused,"schedule":ScheduleState::capture(&clockwork,KEY)?}),
        );
    }
    let prior = context
        .prior()?
        .get("lifecycle")
        .context("Conatus lifecycle baseline missing")?;
    anyhow::ensure!(
        prior["state_dir"] == json!(root),
        "Conatus state root changed after inspection"
    );
    let schedule: ScheduleState = serde_json::from_value(prior["schedule"].clone())?;
    if operation == Operation::Hold {
        gate.hold(&context.request.run_id)?;
        if present {
            crate::operations::pause(&root, true)?;
        }
        if context.request.recovery.is_some() {
            let now = ScheduleState::capture(&clockwork, KEY)?;
            if now.binding.is_some() {
                clockwork.disable(KEY, None)?;
            }
        } else {
            schedule.suspend(&clockwork, KEY)?;
        }
    }
    if operation == Operation::Drain {
        let status = gate.status()?;
        anyhow::ensure!(
            status.holds == [context.request.run_id.clone()],
            "Conatus drain requires sole run-owned maintenance"
        );
        let runner_idle = crate::store::runner_idle(&root)?;
        return Ok(
            json!({"waiting":!status.drained || !runner_idle,"drained":status.drained && runner_idle}),
        );
    }
    let forward = context
        .request
        .recovery
        .as_ref()
        .and_then(|r| r.get("any_apply_started"))
        == Some(&json!(true))
        && context.home.join(".local/bin/conatus").exists();
    if operation == Operation::Configure || operation == Operation::Recover && forward {
        let status = gate.status()?;
        anyhow::ensure!(
            status.holds == [context.request.run_id.clone()] && status.drained,
            "Conatus configuration requires drained run-owned maintenance"
        );
        let config = if present {
            Some(crate::store::Store::open(&root)?.config()?)
        } else {
            None
        };
        let annals_state = config
            .as_ref()
            .and_then(|c| c.annals_state_dir.clone())
            .or(settings.annals_state_dir)
            .unwrap_or_else(|| context.home.join("Library/Application Support/Annals"));
        let decisions = config
            .as_ref()
            .map(|c| c.decisions_config.clone())
            .or(settings.decisions_config)
            .unwrap_or_else(|| annals_state.join("decisions/config.toml"));
        let library = config
            .as_ref()
            .map(|c| c.library.clone())
            .or(settings.library)
            .unwrap_or_else(|| "conatus".into());
        let executable = fs::canonicalize(context.home.join(".local/bin/conatus"))?;
        let before_cursor = if present {
            crate::store::Store::open(&root)?.setting("cursor")?
        } else {
            None
        };
        cell_install::command::json(
            &executable,
            &[
                "--json".into(),
                "--state-dir".into(),
                root.clone().into_os_string(),
                "init".into(),
                "--annals".into(),
                fs::canonicalize(context.home.join(".local/bin/annals"))?.into_os_string(),
                "--decisions-config".into(),
                decisions.into_os_string(),
                "--annals-state-dir".into(),
                annals_state.into_os_string(),
                "--library".into(),
                library.into(),
            ],
            &BTreeMap::from([(
                "CELL_DEPLOYMENT_RUN_ID".into(),
                context.request.run_id.clone().into(),
            )]),
            std::time::Duration::from_secs(180),
        )?;
        let store = crate::store::Store::open(&root)?;
        if let Some(config) = config {
            let after = store.config()?;
            anyhow::ensure!(
                after.library_id == config.library_id
                    && after.decisions_library_id == config.decisions_library_id
                    && store.setting("cursor")? == before_cursor,
                "Conatus rebind changed library identity or feed cursor"
            );
        }
        if schedule.binding.is_some() || settings.enabled.is_some() {
            let path = context.request.run_dir.join("conatus-definition.toml");
            if path.exists() {
                fs::remove_file(&path)?;
            }
            schedule_definition(ScheduleDefinitionArgs {
                state_dir: root.clone(),
                output: path.clone(),
                home: Some(context.home.clone()),
            })?;
            let fallback = clockwork::api::Manifest::from_toml(&fs::read_to_string(&path)?)?;
            fs::remove_file(&path)?;
            let release = executable
                .parent()
                .and_then(std::path::Path::parent)
                .context("Conatus release missing")?;
            let definition = schedule.retarget(
                fallback,
                release,
                &executable,
                cell_install::file_digest(&executable)?,
            )?;
            ScheduleState::prepare(&clockwork, &definition, &path)?;
        }
    }
    if operation == Operation::Verify {
        let store = crate::store::Store::open(&root)?;
        let config = store.config()?;
        anyhow::ensure!(
            config.annals == fs::canonicalize(context.home.join(".local/bin/annals"))?,
            "Conatus Annals pin is not current"
        );
        config.library().current_instructions()?;
    }
    if operation == Operation::Release {
        gate.release(&context.request.run_id)?;
    }
    if operation == Operation::Activate {
        anyhow::ensure!(
            gate.status()?.holds.is_empty(),
            "Conatus activation requires released maintenance"
        );
        if present {
            crate::operations::pause(&root, prior["paused"] == true)?;
        }
        let enabled = if context.request.recovery.is_some() && !forward {
            None
        } else {
            settings.enabled
        };
        schedule.activate(&clockwork, KEY, enabled)?;
    }
    Ok(json!({"configured":operation == Operation::Configure,"safe_to_release":true}))
}
