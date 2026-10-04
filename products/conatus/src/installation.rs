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
        source_directory: "products/conatus",
        provider_source: "products/conatus/chancery",
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
    /// Prepare the daily email instead of the update runner.
    #[arg(long)]
    daily_email: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_seconds: Option<u64>,
    launch: Launch,
    environment: BTreeMap<&'static str, String>,
    output: Output,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Schedule {
    Interval {
        seconds: u64,
        run_at_load: bool,
    },
    LocalCalendar {
        hour: u8,
        minute: u8,
        run_at_load: bool,
    },
}

fn schedule_kind(daily_email: bool) -> Schedule {
    if daily_email {
        Schedule::LocalCalendar {
            hour: 9,
            minute: 0,
            run_at_load: false,
        }
    } else {
        Schedule::Interval {
            seconds: 300,
            run_at_load: true,
        }
    }
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
#[allow(clippy::too_many_lines)]
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
        transaction::inspect_installation(&spec.layout(), &home, &|path| spec.read_legacy(path))?
            .current
            .context("install Conatus before preparing its schedule")?;
    let release_root = home
        .join("Library/Application Support/Conatus/install/releases")
        .join(&release.release_id);
    release
        .files
        .get("bin/conatus")
        .context("selected release has no Conatus program")?;
    let binary = release_root.join("bin/conatus");
    let binary_hash = cell_install::file_digest(&binary)?;
    let logs = state_dir.join("logs");
    if !logs.exists() {
        DirBuilder::new().mode(0o700).create(&logs)?;
    }
    if !fs::symlink_metadata(&logs)?.is_dir() {
        bail!("Conatus logs path must be a regular directory");
    }
    let log_name = if args.daily_email {
        "daily-email"
    } else {
        "update"
    };
    let definition = Definition {
        schema_version: 2,
        key: if args.daily_email {
            "conatus/daily-email"
        } else {
            "conatus/update"
        },
        release_id: release.release_id,
        release_root: release_root.clone(),
        authority: "current-user-background",
        overlap: "skip",
        failure: clockwork::api::FailurePolicy::default(),
        arguments: [
            vec![
                "--state-dir".to_owned(),
                state_dir
                    .to_str()
                    .context("state directory must be UTF-8")?
                    .to_owned(),
            ],
            if args.daily_email {
                vec!["email".into(), "send".into(), "--scheduled".into()]
            } else {
                vec!["update".into()]
            },
        ]
        .concat(),
        cwd: state_dir,
        schedule: schedule_kind(args.daily_email),
        timeout_seconds: args.daily_email.then_some(180),
        launch: Launch {
            kind: "direct",
            program: binary,
            sha256: binary_hash,
        },
        environment: BTreeMap::from([
            (
                "HOME",
                home.to_str().context("home must be UTF-8")?.to_owned(),
            ),
            ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin".to_owned()),
        ]),
        output: Output {
            stdout: logs.join(format!("{log_name}.out.log")),
            stderr: logs.join(format!("{log_name}.err.log")),
        },
    };
    let mut definition = clockwork::api::Manifest::from_toml(&toml::to_string(&definition)?)?;
    definition.use_runtime_paths()?;
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
    daily_email_enabled: Option<bool>,
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

/// Install program files, preserve configured library identities, and publish schedules.
/// # Errors
/// Returns installation or product setup failures.
pub fn deploy(context: &cell_install::adapter::Context) -> cell_install::Result<()> {
    deploy_inner(context).map_err(|error| cell_install::Error::new(format!("{error:#}")))
}

fn deploy_inner(context: &cell_install::adapter::Context) -> Result<()> {
    use clockwork::deployment::ScheduleState;
    let settings: Settings = serde_json::from_value(
        context
            .request
            .settings
            .clone()
            .unwrap_or_else(|| json!({})),
    )?;
    let root = settings.state_dir.clone().unwrap_or(crate::state_root()?);
    anyhow::ensure!(root.is_absolute(), "Conatus state_dir must be absolute");
    let config = configuration(&root)?;
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
    let schedule = ScheduleState::capture_installed(&context.home, "conatus/update")?;
    let email_schedule = ScheduleState::capture_installed(&context.home, "conatus/daily-email")?;
    let clockwork = clockwork::api::Client::new(context.dependency_binary("clockwork")?)
        .with_home(&context.home);
    schedule.suspend(&clockwork, "conatus/update")?;
    email_schedule.suspend(&clockwork, "conatus/daily-email")?;
    cell_install::simple::deploy_program(&specification(), env!("CARGO_PKG_VERSION"), context)?;
    {
        let _admission = crate::gate(&root).enter()?;
        let annals = fs::canonicalize(context.home.join(".local/bin/annals"))?;
        if let Some(mut config) = config {
            let _runner = crate::store::runner_lock(&root)?;
            config.annals = annals;
            crate::store::Store::open(&root)?.set("config", &serde_json::to_string(&config)?)?;
        } else {
            let annals_state = settings
                .annals_state_dir
                .unwrap_or_else(|| context.home.join("Library/Application Support/Annals"));
            let decisions = settings
                .decisions_config
                .unwrap_or_else(|| annals_state.join("decisions/config.toml"));
            let library = settings.library.unwrap_or_else(|| "conatus".into());
            crate::operations::initialize(
                &root,
                &annals,
                &decisions,
                Some(&annals_state),
                &library,
            )?;
        }
    }
    let executable = fs::canonicalize(context.home.join(".local/bin/conatus"))?;
    for (saved, enabled, daily_email, filename) in [
        (
            &schedule,
            settings.enabled,
            false,
            "conatus-definition.toml",
        ),
        (
            &email_schedule,
            settings.daily_email_enabled,
            true,
            "conatus-email-definition.toml",
        ),
    ] {
        if saved.binding.is_none() && enabled.is_none() {
            continue;
        }
        let clockwork = clockwork::api::Client::new(context.dependency_binary("clockwork")?)
            .with_home(&context.home);
        let path = context.request.run_dir.join(filename);
        schedule_definition(ScheduleDefinitionArgs {
            state_dir: root.clone(),
            daily_email,
            output: path.clone(),
            home: Some(context.home.clone()),
        })?;
        let fallback = clockwork::api::Manifest::from_toml(&fs::read_to_string(&path)?)?;
        fs::remove_file(&path)?;
        let release = fs::canonicalize(
            context
                .home
                .join("Library/Application Support/Conatus/install/current"),
        )?;
        let definition = saved.retarget(
            fallback,
            &release,
            &executable,
            cell_install::file_digest(&executable)?,
        )?;
        saved.publish(&clockwork, &definition, &path, enabled)?;
    }
    Ok(())
}
