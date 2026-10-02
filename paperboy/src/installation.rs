//! Product-owned program and schedule installation.
use cell_install::{legacy::LegacySpec, simple::Spec, transaction::LockKind};

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "paperboy",
        application: "Paperboy",
        source_directory: "paperboy",
        provider_source: "paperboy/chancery",
        legacy_provider_path: "share/chancery/paperboy",
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

#[must_use]
pub fn main() -> std::process::ExitCode {
    if std::env::args()
        .nth(1)
        .is_none_or(|value| matches!(value.as_str(), "--help" | "-h"))
    {
        println!(
            "paperboy-install {}\n\ndeploy < REQUEST.json\ninspect [--home ABS]\n\nDeploy selects files and applies owned setup. Failed instructions retain completed effects. Direct install and selector-only recovery are unsupported.",
            env!("CARGO_PKG_VERSION")
        );
        return std::process::ExitCode::SUCCESS;
    }
    if matches!(
        std::env::args().nth(1).as_deref(),
        Some("install" | "recover")
    ) {
        eprintln!(
            "use paperboy-install deploy for installation; inspect retained effects and use product interfaces for recovery"
        );
        return std::process::ExitCode::FAILURE;
    }
    cell_install::simple::main_with_deployment(&specification(), env!("CARGO_PKG_VERSION"), deploy)
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    enabled: Option<bool>,
}

/// Install program files, initialize owned state, and publish the daily schedule.
/// # Errors
/// Returns installation or product setup failures.
pub fn deploy(context: &cell_install::adapter::Context) -> cell_install::Result<()> {
    deploy_inner(context).map_err(|error| cell_install::Error::new(format!("{error:#}")))
}

fn deploy_inner(context: &cell_install::adapter::Context) -> anyhow::Result<()> {
    use anyhow::Context as _;
    use clockwork::deployment::ScheduleState;
    use serde_json::json;
    let settings: Settings = serde_json::from_value(
        context
            .request
            .settings
            .clone()
            .unwrap_or_else(|| json!({})),
    )?;
    let state = ScheduleState::capture_installed(&context.home, "paperboy/daily")?;
    cell_install::simple::deploy_program(&specification(), env!("CARGO_PKG_VERSION"), context)?;
    let root = crate::state_root()?;
    let _admission = crate::gate(&root).enter()?;
    let _runner = crate::store::runner_lock(&root)?;
    crate::store::Store::initialize(&root)?;
    if state.binding.is_some() || settings.enabled.is_some() {
        let clockwork = clockwork::api::Client::new(context.dependency_binary("clockwork")?);
        let executable = std::fs::canonicalize(context.home.join(".local/bin/paperboy"))?;
        let release = executable
            .parent()
            .and_then(std::path::Path::parent)
            .context("Paperboy release missing")?;
        let definition = state.retarget(
            crate::operations::schedule_definition(&root)?,
            release,
            &executable,
            cell_install::file_digest(&executable)?,
        )?;
        state.publish(
            &clockwork,
            &definition,
            &context.request.run_dir.join("paperboy-definition.toml"),
            settings.enabled,
        )?;
    }
    Ok(())
}
