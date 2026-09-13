//! Verified installation layout; runtime maintenance belongs to Platter.

use anyhow::{Context, Result, ensure};
use cell_install::legacy::LegacySpec;
use cell_install::simple::Spec;
use cell_install::transaction::LockKind;
use std::{collections::BTreeMap, path::Path};

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "platter",
        application: "Platter",
        source_directory: "platter",
        provider_source: "platter/chancery",
        legacy_provider_path: "share/chancery/platter",
        // Platter has no predecessor installed release. The private prototype
        // state is handled by the runtime and is never an installer artifact.
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
        maintained: true,
    }
}

/// Generate the product-owned daily policy without registering or enabling it.
pub fn schedule_definition(home: &Path, root: &Path) -> Result<clockwork::api::Manifest> {
    use clockwork::api::{
        Authority, FailurePolicy, LaunchImage, Manifest, Output, OverlapPolicy, Schedule,
    };
    let spec = specification();
    let selected =
        std::fs::canonicalize(home.join("Library/Application Support/Platter/install/current"))?;
    let release =
        cell_install::verify_release_at(&spec.layout(), &selected, &|path| spec.legacy(path))?;
    let executable = selected.join("bin/platter");
    ensure!(
        std::fs::canonicalize(std::env::current_exe()?)? == executable,
        "schedule-definition requires the selected installed Platter executable"
    );
    let settings = crate::workflow::config(root)?;
    let text = |path: &Path| -> Result<String> {
        Ok(path
            .to_str()
            .context("schedule paths must be UTF-8")?
            .to_owned())
    };
    let mut environment = BTreeMap::from([
        ("HOME".into(), text(home)?),
        ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
    ]);
    for name in ["PLATTER_TECTONIC", "PLATTER_PYTHON"] {
        if let Some(value) = std::env::var_os(name) {
            let path = std::path::PathBuf::from(value);
            ensure!(path.is_absolute(), "renderer overrides must be absolute");
            environment.insert(name.into(), text(&path)?);
        }
    }
    Ok(Manifest {
        schema_version: 2,
        key: "platter/daily".into(),
        release_id: release.release_id,
        release_root: text(&selected)?,
        authority: Authority::CurrentUserBackground,
        overlap: OverlapPolicy::Skip,
        failure: FailurePolicy {
            email_cli: Some(text(&settings.email_executable)?),
            ..FailurePolicy::default()
        },
        timeout_seconds: None,
        arguments: vec!["run-daily".into()],
        cwd: text(root)?,
        schedule: Schedule::LocalCalendar {
            hour: 18,
            minute: 0,
            run_at_load: false,
        },
        launch: LaunchImage::Direct {
            program: text(&executable)?,
            sha256: release
                .files
                .get("bin/platter")
                .context("release has no Platter executable")?
                .sha256
                .clone(),
        },
        environment,
        output: Output {
            stdout: text(&root.join("daily.stdout.log"))?,
            stderr: text(&root.join("daily.stderr.log"))?,
        },
    })
}

/// Keep program replacement inside the coordinated maintenance boundary.
#[must_use]
pub fn main() -> std::process::ExitCode {
    let operation = std::env::args().nth(1);
    match operation.as_deref() {
        None | Some("--help" | "-h") => {
            println!(
                "platter-install {}\n\ninspect [--home ABS]\nverify --binary ABS --bundle ABS [--home ABS]\nverify-release ABS\nadapter inspect|hold|drain|apply|verify|recover|release\n\nInstallation and recovery require the Cell deployment coordinator and its run-owned maintenance hold. Direct install/recover are unavailable.",
                env!("CARGO_PKG_VERSION")
            );
            std::process::ExitCode::SUCCESS
        }
        Some("install" | "recover") => {
            println!(
                "{}",
                serde_json::json!({"ok":false,"error":{"detail":"Platter installation and recovery require the Cell deployment coordinator","disposition":"unchanged"}})
            );
            std::process::ExitCode::FAILURE
        }
        _ => cell_install::simple::main_with_lifecycle(
            &specification(),
            env!("CARGO_PKG_VERSION"),
            lifecycle,
        ),
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    resume: Option<std::path::PathBuf>,
    enabled: Option<bool>,
}

/// Initialize supplied source material and rebind the existing daily selection.
pub fn lifecycle(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> cell_install::Result<serde_json::Value> {
    lifecycle_inner(context, operation).map_err(|error| cell_install::Error::new(error.to_string()))
}

#[allow(clippy::too_many_lines)]
fn lifecycle_inner(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> Result<serde_json::Value> {
    use cell_install::adapter::Operation;
    use clockwork::deployment::ScheduleState;
    use serde_json::json;
    const KEY: &str = "platter/daily";
    let settings: Settings = serde_json::from_value(
        context
            .request
            .settings
            .clone()
            .unwrap_or_else(|| json!({})),
    )?;
    let root = crate::default_state_dir(&context.home)?;
    let clockwork = clockwork::api::Client::new(context.home.join(".local/bin/clockwork"));
    if operation == Operation::Inspect {
        let initialized = if root.join(crate::store::DATABASE).exists() {
            let path = root.join(crate::store::DATABASE);
            crate::store::regular_file(&path)?;
            let connection = rusqlite::Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let version: i64 =
                connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
            ensure!(
                matches!(version, 1 | 2 | crate::store::SCHEMA_VERSION),
                "unsupported Platter database schema"
            );
            version == 1
                || connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM settings WHERE key='config')",
                    [],
                    |row| row.get::<_, bool>(0),
                )?
        } else {
            false
        };
        if initialized {
            ensure!(
                settings.resume.is_none(),
                "deployment cannot replace Platter's retained original resume"
            );
        } else if context.selected() {
            let resume = settings.resume.as_ref().context("fresh Platter setup requires settings.resume with an absolute original resume path")?;
            ensure!(
                resume.is_absolute(),
                "Platter resume must be an absolute path"
            );
            crate::resume::ResumeTemplate::load(resume)?;
        }
        return Ok(
            json!({"initialized":initialized,"schedule":ScheduleState::capture_installed(&context.home, KEY)?}),
        );
    }
    let state: ScheduleState =
        serde_json::from_value(context.prior()?["lifecycle"]["schedule"].clone())?;
    if operation == Operation::Hold
        && (state.binding.is_some()
            || ScheduleState::capture_installed(&context.home, KEY)?
                .binding
                .is_some())
    {
        if context.request.recovery.is_some() {
            if ScheduleState::capture_installed(&context.home, KEY)?
                .binding
                .is_some()
            {
                clockwork.disable(KEY, None)?;
            }
        } else {
            state.suspend(&clockwork, KEY)?;
        }
    }
    let forward = context
        .request
        .recovery
        .as_ref()
        .and_then(|r| r.get("any_apply_started"))
        == Some(&json!(true))
        && context.home.join(".local/bin/platter").exists();
    if (operation == Operation::Configure || operation == Operation::Recover && forward)
        && (context.selected() || !context.prior()?["installation"]["current"].is_null())
    {
        if context.selected() {
            let backup = root
                .join("backups")
                .join(format!("migration-{}.sqlite", context.request.run_id));
            cell_install::migration::run_once(
                &context.request.run_dir.join("platter-migration.json"),
                &backup,
                || {
                    cell_install::command::json(
                        &context.home.join(".local/bin/platter"),
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
                    )?;
                    Ok(())
                },
            )?;
        }
        {
            let _guard =
                crate::maintenance::gate(&context.home).enter_for(&context.request.run_id)?;
            let store = crate::store::Store::open_read_only(&root)?;
            if store.setting::<serde_json::Value>("config")?.is_none() {
                crate::workflow::initialize(
                    &root,
                    settings
                        .resume
                        .as_deref()
                        .context("Platter resume input is missing")?,
                )?;
            }
            let mut config = crate::workflow::config(&root)?;
            config.cast_executable = std::fs::canonicalize(context.home.join(".local/bin/cast"))?;
            config.crm_executable = std::fs::canonicalize(context.home.join(".local/bin/crm"))?;
            config.email_executable = std::fs::canonicalize(context.home.join(".local/bin/email"))?;
            crate::store::Store::open(&root)?.set_setting("config", &config)?;
        }
        if state.binding.is_some() || settings.enabled.is_some() {
            let executable = std::fs::canonicalize(context.home.join(".local/bin/platter"))?;
            let release = executable
                .parent()
                .and_then(Path::parent)
                .context("Platter release missing")?;
            let data = cell_install::command::json(
                &executable,
                &["--json".into(), "schedule-definition".into()],
                &BTreeMap::new(),
                std::time::Duration::from_secs(60),
            )?;
            let fallback = serde_json::from_value(data["data"].clone())?;
            let mut definition = state.retarget(
                fallback,
                release,
                &executable,
                cell_install::file_digest(&executable)?,
            )?;
            definition.failure.email_cli = Some(
                crate::workflow::config(&root)?
                    .email_executable
                    .to_str()
                    .context("Email path must be UTF-8")?
                    .to_owned(),
            );
            ScheduleState::prepare(
                &clockwork,
                &definition,
                &context.request.run_dir.join("platter-definition.toml"),
            )?;
        }
    }
    if operation == Operation::Activate {
        ensure!(
            crate::maintenance::gate(&context.home)
                .status()?
                .holds
                .is_empty(),
            "Platter activation requires released maintenance"
        );
        let enabled = if context.request.recovery.is_some() && !forward {
            None
        } else {
            settings.enabled
        };
        if state.binding.is_some()
            || enabled == Some(true)
            || ScheduleState::capture_installed(&context.home, KEY)?
                .binding
                .is_some()
        {
            state.activate(&clockwork, KEY, enabled)?;
        }
    }
    Ok(json!({"configured":operation == Operation::Configure}))
}
