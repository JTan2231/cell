//! Artifact publication uses the shared maintained installation boundary.
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
        maintained: true,
    }
}

#[must_use]
pub fn main() -> std::process::ExitCode {
    if matches!(
        std::env::args().nth(1).as_deref(),
        Some("install" | "recover")
    ) {
        eprintln!("Paperboy installation and recovery require the Cell deployment coordinator");
        return std::process::ExitCode::FAILURE;
    }
    cell_install::simple::main_with_lifecycle(
        &specification(),
        env!("CARGO_PKG_VERSION"),
        lifecycle,
    )
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    enabled: Option<bool>,
}

/// Keep the daily selection coherent with the installed program.
/// # Errors
/// Refuses altered bindings and unavailable selected releases.
pub fn lifecycle(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> cell_install::Result<serde_json::Value> {
    lifecycle_inner(context, operation).map_err(|error| cell_install::Error::new(error.to_string()))
}

fn lifecycle_inner(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> anyhow::Result<serde_json::Value> {
    use anyhow::Context as _;
    use cell_install::adapter::Operation;
    use clockwork::deployment::ScheduleState;
    use serde_json::json;
    const KEY: &str = "paperboy/daily";
    let settings: Settings = serde_json::from_value(
        context
            .request
            .settings
            .clone()
            .unwrap_or_else(|| json!({})),
    )?;
    let clockwork = clockwork::api::Client::new(context.dependency_binary("clockwork")?);
    if operation == Operation::Inspect {
        return Ok(json!({"schedule":ScheduleState::capture(&clockwork, KEY)?}));
    }
    let state: ScheduleState =
        serde_json::from_value(context.prior()?["lifecycle"]["schedule"].clone())?;
    if operation == Operation::Hold {
        if context.request.recovery.is_some() {
            if ScheduleState::capture(&clockwork, KEY)?.binding.is_some() {
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
        && context.home.join(".local/bin/paperboy").exists();
    if (operation == Operation::Configure || operation == Operation::Recover && forward)
        && context.selected()
    {
        let backup = context.home.join(format!(
            "Library/Application Support/Paperboy/paperboy-pre-migration-{}.sqlite",
            context.request.run_id
        ));
        cell_install::migration::run_once(
            &context.request.run_dir.join("paperboy-migration.json"),
            &backup,
            || {
                cell_install::command::json(
                    &context.home.join(".local/bin/paperboy"),
                    &[
                        "--json".into(),
                        "migrate".into(),
                        "--backup".into(),
                        backup.clone().into_os_string(),
                    ],
                    &std::collections::BTreeMap::from([(
                        "CELL_DEPLOYMENT_RUN_ID".into(),
                        context.request.run_id.clone().into(),
                    )]),
                    std::time::Duration::from_secs(600),
                )?;
                Ok(())
            },
        )?;
    }
    if (operation == Operation::Configure || operation == Operation::Recover && forward)
        && (state.binding.is_some() || settings.enabled.is_some())
    {
        let executable = std::fs::canonicalize(context.home.join(".local/bin/paperboy"))?;
        let release = executable
            .parent()
            .and_then(std::path::Path::parent)
            .context("Paperboy release missing")?;
        let fallback = crate::operations::schedule_definition(&crate::state_root()?)?;
        let definition = state.retarget(
            fallback,
            release,
            &executable,
            cell_install::file_digest(&executable)?,
        )?;
        ScheduleState::prepare(
            &clockwork,
            &definition,
            &context.request.run_dir.join("paperboy-definition.toml"),
        )?;
    }
    if operation == Operation::Activate {
        anyhow::ensure!(
            crate::gate(&crate::state_root()?)
                .status()?
                .holds
                .is_empty(),
            "Paperboy activation requires released maintenance"
        );
        let enabled = if context.request.recovery.is_some() && !forward {
            None
        } else {
            settings.enabled
        };
        state.activate(&clockwork, KEY, enabled)?;
    }
    Ok(json!({"configured":operation == Operation::Configure}))
}
