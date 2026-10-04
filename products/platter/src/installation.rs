//! Installation layout; runtime maintenance belongs to Platter.

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
        source_directory: "products/platter",
        provider_source: "products/platter/chancery",
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
    }
}

/// Generate the product-owned daily policy without registering or enabling it.
pub fn schedule_definition(home: &Path, root: &Path) -> Result<clockwork::api::Manifest> {
    let spec = specification();
    let selected =
        std::fs::canonicalize(home.join("Library/Application Support/Platter/install/current"))?;
    let release =
        cell_install::verify_release_at(&spec.layout(), &selected, &|path| spec.legacy(path))?;
    let executable = home.join("Library/Application Support/Platter/install/runtime/bin/platter");
    ensure!(
        std::fs::canonicalize(std::env::current_exe()?)? == executable,
        "schedule-definition requires the selected installed Platter executable"
    );
    schedule_manifest(home, root, &selected, release)
}

fn schedule_manifest(
    home: &Path,
    root: &Path,
    selected: &Path,
    release: cell_install::transaction::ReleaseInfo,
) -> Result<clockwork::api::Manifest> {
    use clockwork::api::{
        Authority, FailurePolicy, LaunchImage, Manifest, Output, OverlapPolicy, Schedule,
    };
    let executable = selected.join("bin/platter");
    release
        .files
        .get("bin/platter")
        .context("release has no Platter executable")?;
    let executable_hash = cell_install::file_digest(&executable)?;
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
    let mut manifest = Manifest {
        schema_version: 2,
        key: "platter/daily".into(),
        release_id: release.release_id,
        release_root: text(selected)?,
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
            sha256: executable_hash,
        },
        environment,
        output: Output {
            stdout: text(&root.join("daily.stdout.log"))?,
            stderr: text(&root.join("daily.stderr.log"))?,
        },
    };
    manifest.use_runtime_paths()?;
    Ok(manifest)
}

/// Run this product's deployment recipe.
#[must_use]
pub fn main() -> std::process::ExitCode {
    let operation = std::env::args().nth(1);
    match operation.as_deref() {
        None | Some("--help" | "-h") => {
            println!(
                "platter-install {}\n\ninspect [--home ABS]\ndeploy (recipe request on stdin)\n\nDeploy performs the owned installation and setup recipe. Direct install/recover are unavailable.",
                env!("CARGO_PKG_VERSION")
            );
            std::process::ExitCode::SUCCESS
        }
        Some("install" | "recover") => {
            println!(
                "{}",
                serde_json::json!({"ok":false,"error":{"detail":"use platter-install deploy for installation; inspect retained effects and use product interfaces for recovery","disposition":"unchanged"}})
            );
            std::process::ExitCode::FAILURE
        }
        _ => cell_install::simple::main_with_deployment(
            &specification(),
            env!("CARGO_PKG_VERSION"),
            deploy,
        ),
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    resume: Option<std::path::PathBuf>,
    projects_template: Option<std::path::PathBuf>,
    enabled: Option<bool>,
}

/// Install files, migrate owned state, update provider pins, and publish the daily schedule.
/// # Errors
/// Returns installation, migration, or setup failures.
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
            .unwrap_or_else(|| serde_json::json!({})),
    )?;
    let root = crate::default_state_dir(&context.home)?;
    let state = ScheduleState::capture_installed(&context.home, "platter/daily")?;
    let clockwork = clockwork::api::Client::new(context.dependency_binary("clockwork")?)
        .with_home(&context.home);
    state.suspend(&clockwork, "platter/daily")?;
    cell_install::simple::deploy_program(&specification(), env!("CARGO_PKG_VERSION"), context)?;
    {
        let (_admission, _runner) = crate::maintenance::install_admission(&context.home, &root)?;
        crate::migration::migrate(&root)?;
        let store = crate::store::Store::open(&root)?;
        if store.setting::<serde_json::Value>("config")?.is_none() {
            let resume = settings.resume.as_deref().context("fresh Platter setup requires settings.resume with an absolute original resume path")?;
            ensure!(resume.is_absolute(), "Platter resume must be absolute");
            crate::workflow::initialize(&root, resume)?;
        } else {
            ensure!(
                settings.resume.is_none(),
                "deployment cannot replace Platter's retained original resume"
            );
        }
        let mut config = crate::workflow::config(&root)?;
        config.milieu_executable = std::fs::canonicalize(context.home.join(".local/bin/milieu"))?;
        config.email_executable = std::fs::canonicalize(context.home.join(".local/bin/email"))?;
        config.weaver_executable = std::fs::canonicalize(context.home.join(".local/bin/weaver"))?;
        store.set_setting("config", &config)?;
        if let Some(path) = &settings.projects_template {
            crate::workflow::import_projects_template(&root, path)?;
        }
    }
    if state.binding.is_some() || settings.enabled.is_some() {
        let clockwork = clockwork::api::Client::new(context.dependency_binary("clockwork")?)
            .with_home(&context.home);
        let executable = std::fs::canonicalize(context.home.join(".local/bin/platter"))?;
        let release = std::fs::canonicalize(
            context
                .home
                .join("Library/Application Support/Platter/install/current"),
        )?;
        let spec = specification();
        let metadata = cell_install::read_release_at(&spec.layout(), &release, &|path| {
            spec.read_legacy(path)
        })?;
        let mut definition = state.retarget(
            schedule_manifest(&context.home, &root, &release, metadata)?,
            &release,
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
        state.publish(
            &clockwork,
            &definition,
            &context.request.run_dir.join("platter-definition.toml"),
            settings.enabled,
        )?;
    }
    Ok(())
}
