//! Weaver's small lifecycle around the shared Cell file transaction.
use cell_install::{legacy::LegacySpec, simple::Spec, transaction::LockKind};

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "weaver",
        application: "Weaver",
        source_directory: "weaver-narrative",
        provider_source: "weaver-narrative/chancery",
        legacy_provider_path: "share/chancery/weaver",
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
        eprintln!("Weaver installation and recovery require the Cell deployment coordinator");
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
    annals_config: Option<std::path::PathBuf>,
}

pub fn lifecycle(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> cell_install::Result<serde_json::Value> {
    lifecycle_inner(context, operation).map_err(|e| cell_install::Error::new(e.to_string()))
}

fn lifecycle_inner(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> anyhow::Result<serde_json::Value> {
    use anyhow::Context as _;
    use cell_install::adapter::Operation;
    use serde_json::json;
    let settings: Settings = serde_json::from_value(
        context
            .request
            .settings
            .clone()
            .unwrap_or_else(|| json!({})),
    )?;
    let root = context.home.join("Library/Application Support/Weaver");
    if operation == Operation::Inspect {
        let annals_config = if let Some(path) = settings.annals_config {
            path
        } else {
            crate::Config::read(&root)
                .context("first Weaver installation requires settings.weaver.annals_config")?
                .annals_config
        };
        let config = crate::Config {
            annals_binary: context.home.join(".local/bin/annals"),
            annals_config,
        };
        config.validate()?;
        annals_api::Client::new(
            context.dependency_inspection_binary("annals")?,
            &config.annals_config,
        )
        .start()?;
        return Ok(json!({"config":config}));
    }
    let forward = context
        .request
        .recovery
        .as_ref()
        .and_then(|r| r.get("any_apply_started"))
        == Some(&json!(true))
        && context.home.join(".local/bin/weaver").exists();
    if context.selected()
        && (operation == Operation::Configure || operation == Operation::Recover && forward)
    {
        let config: crate::Config =
            serde_json::from_value(context.prior()?["lifecycle"]["config"].clone())?;
        // The public initializer owns SQLite and configuration writes under the exact hold.
        cell_install::command::json(
            &context.home.join(".local/bin/weaver"),
            &[
                "--json".into(),
                "init".into(),
                "--annals-config".into(),
                config.annals_config.into_os_string(),
                "--annals-binary".into(),
                config.annals_binary.into_os_string(),
            ],
            &std::collections::BTreeMap::from([(
                "CELL_DEPLOYMENT_RUN_ID".into(),
                context.request.run_id.clone().into(),
            )]),
            std::time::Duration::from_secs(180),
        )?;
    }
    Ok(json!({}))
}
