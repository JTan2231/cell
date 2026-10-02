//! Product-owned program and configuration installation.
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
    }
}

#[must_use]
pub fn main() -> std::process::ExitCode {
    if std::env::args()
        .nth(1)
        .is_none_or(|value| matches!(value.as_str(), "--help" | "-h"))
    {
        println!(
            "weaver-install {}\n\ndeploy < REQUEST.json\ninspect [--home ABS]\n\nDeploy selects files and applies owned setup. Failed instructions retain completed effects. Direct install and selector-only recovery are unsupported.",
            env!("CARGO_PKG_VERSION")
        );
        return std::process::ExitCode::SUCCESS;
    }
    if matches!(
        std::env::args().nth(1).as_deref(),
        Some("install" | "recover")
    ) {
        eprintln!(
            "use weaver-install deploy for installation; inspect retained effects and use product interfaces for recovery"
        );
        return std::process::ExitCode::FAILURE;
    }
    cell_install::simple::main_with_deployment(&specification(), env!("CARGO_PKG_VERSION"), deploy)
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    annals_config: Option<std::path::PathBuf>,
}

/// Install files and write the product-owned Annals selection and state.
/// # Errors
/// Returns installation or configuration-write failures.
pub fn deploy(context: &cell_install::adapter::Context) -> cell_install::Result<()> {
    deploy_inner(context).map_err(|error| cell_install::Error::new(format!("{error:#}")))
}

fn deploy_inner(context: &cell_install::adapter::Context) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let settings: Settings = serde_json::from_value(
        context
            .request
            .settings
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
    )?;
    let root = context.home.join("Library/Application Support/Weaver");
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
    cell_install::simple::deploy_program(&specification(), env!("CARGO_PKG_VERSION"), context)?;
    crate::operations::install_config(&root, &config)?;
    Ok(())
}
