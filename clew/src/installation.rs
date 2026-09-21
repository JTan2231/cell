//! Clew program selection and schema-one initialization. Ledger rows are preserved.
use anyhow::{Context as _, Result, ensure};
use cell_install::{
    adapter::{Context, Operation},
    legacy::LegacySpec,
    simple::Spec,
    transaction::LockKind,
};
use clockwork::{
    api::{Client, Manifest},
    deployment::ScheduleState,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write as _,
    os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _},
    path::{Path, PathBuf},
};

const KEY: &str = "clew/daily-email";

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "clew",
        application: "Clew",
        source_directory: "clew",
        provider_source: "clew/chancery",
        legacy_provider_path: "share/chancery/clew",
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
        maintained: false,
    }
}

pub fn lifecycle(
    context: &Context,
    operation: Operation,
) -> cell_install::Result<serde_json::Value> {
    lifecycle_inner(context, operation)
        .map_err(|error| cell_install::Error::new(format!("{error:#}")))
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    daily_email_enabled: Option<bool>,
}

#[allow(clippy::too_many_lines)] // Keep the coordinated lifecycle and its captured intent together.
fn lifecycle_inner(context: &Context, operation: Operation) -> Result<Value> {
    let settings: Settings = serde_json::from_value(
        context
            .request
            .settings
            .clone()
            .unwrap_or_else(|| json!({})),
    )?;
    let root = crate::state_dir(&context.home);
    let gate = crate::gate(&root);
    if operation == Operation::Inspect {
        ensure!(
            gate.status()?.holds.is_empty(),
            "another operation holds Clew maintenance"
        );
        return Ok(
            json!({"state_dir":root,"email_schedule":ScheduleState::capture_installed(&context.home, KEY)?}),
        );
    }
    let prior = context
        .prior()?
        .get("lifecycle")
        .context("Clew lifecycle baseline missing")?;
    ensure!(
        prior["state_dir"] == json!(root),
        "Clew state root changed after inspection"
    );
    let schedule: ScheduleState = serde_json::from_value(prior["email_schedule"].clone())?;
    let clockwork = Client::new(context.dependency_binary("clockwork")?);
    if operation == Operation::Hold {
        gate.hold(&context.request.run_id)?;
        if context.request.recovery.is_some() {
            if ScheduleState::capture(&clockwork, KEY)?.binding.is_some() {
                clockwork.disable(KEY, None)?;
            }
        } else {
            schedule.suspend(&clockwork, KEY)?;
        }
    }
    if operation == Operation::Drain {
        let status = gate.status()?;
        ensure!(
            status.holds == [context.request.run_id.clone()],
            "Clew drain requires sole run-owned maintenance"
        );
        return Ok(json!({"waiting":!status.drained,"drained":status.drained}));
    }
    let forward = context
        .request
        .recovery
        .as_ref()
        .and_then(|r| r.get("any_apply_started"))
        == Some(&json!(true))
        && context.home.join(".local/bin/clew").exists();
    if operation == Operation::Configure || operation == Operation::Recover && forward {
        let _exclusive = gate.enter_for(&context.request.run_id)?;
        crate::store::Store::initialize(&root)?;
        if schedule.binding.is_some() || settings.daily_email_enabled.is_some() {
            let fallback = definition(&context.home, &root)?;
            let executable = fs::canonicalize(context.home.join(".local/bin/clew"))?;
            let release = executable
                .parent()
                .and_then(Path::parent)
                .context("Clew release missing")?;
            let manifest = schedule.retarget(
                fallback,
                release,
                &executable,
                cell_install::file_digest(&executable)?,
            )?;
            ScheduleState::prepare(
                &clockwork,
                &manifest,
                &context.request.run_dir.join("clew-email-definition.toml"),
            )?;
        }
    }
    if operation == Operation::Verify
        || operation == Operation::Recover && root.join(crate::store::DATABASE).exists()
    {
        crate::store::Store::open(&root, false)?.check()?;
    }
    if operation == Operation::Verify {
        platter::api::Client::new(context.home.join(".local/bin/platter")).list(None)?;
    }
    if operation == Operation::Release {
        gate.release(&context.request.run_id)?;
    }
    if operation == Operation::Activate {
        ensure!(
            gate.status()?.holds.is_empty(),
            "Clew activation requires released maintenance"
        );
        let enabled = if context.request.recovery.is_some() && !forward {
            None
        } else {
            settings.daily_email_enabled
        };
        schedule.activate(&clockwork, KEY, enabled)?;
    }
    Ok(json!({"schema_version":1,"safe_to_release":true}))
}

#[derive(clap::Parser)]
#[command(about = "Write the daily Clew Clockwork definition without activating it")]
pub struct ScheduleDefinitionArgs {
    #[arg(long)]
    state_dir: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    home: Option<PathBuf>,
}

fn definition(home: &Path, root: &Path) -> Result<Manifest> {
    ensure!(
        home.is_absolute() && root.is_absolute(),
        "schedule paths must be absolute"
    );
    let home = fs::canonicalize(home)?;
    let root = fs::canonicalize(root)?;
    crate::store::Store::open(&root, false)?.check()?;
    let spec = specification();
    let release =
        cell_install::transaction::inspect_installation(&spec.layout(), &home, &|path| {
            spec.legacy(path)
        })?
        .current
        .context("install Clew before preparing its schedule")?;
    let release_root = home
        .join("Library/Application Support/Clew/install/releases")
        .join(&release.release_id);
    let binary = release
        .files
        .get("bin/clew")
        .context("selected release has no Clew program")?;
    let logs = root.join("logs");
    if !logs.exists() {
        fs::DirBuilder::new().mode(0o700).create(&logs)?;
    }
    let metadata = fs::symlink_metadata(&logs)?;
    ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.permissions().mode() & 0o777 == 0o700,
        "Clew logs must be a private regular directory"
    );
    Ok(serde_json::from_value(json!({
        "schema_version":2,"key":KEY,"release_id":release.release_id,"release_root":release_root,
        "authority":"current-user-background","overlap":"skip",
        "failure":{"on_abend":"halt-until-approved"},"timeout_seconds":180,
        "arguments":["--state-dir",root,"email","send","--scheduled"],"cwd":root,
        "schedule":{"kind":"local-calendar","hour":9,"minute":0,"run_at_load":false},
        "launch":{"kind":"direct","program":release_root.join("bin/clew"),"sha256":binary.sha256},
        "environment":{"HOME":home,"PATH":"/usr/bin:/bin:/usr/sbin:/sbin"},
        "output":{"stdout":logs.join("daily-email.out.log"),"stderr":logs.join("daily-email.err.log")}
    }))?)
}

pub fn schedule_definition(args: ScheduleDefinitionArgs) -> Result<Value> {
    ensure!(
        args.output.is_absolute(),
        "schedule output must be absolute"
    );
    let home = args.home.map_or_else(crate::home, Ok)?;
    let manifest = definition(&home, &args.state_dir)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&args.output)?;
    file.write_all(manifest.to_toml()?.as_bytes())?;
    file.sync_all()?;
    Ok(
        json!({"key":KEY,"release_id":manifest.release_id,"definition":args.output,"registered":false,"activated":false}),
    )
}
