//! Product-owned program and schedule installation.
use anyhow::Result;
use cell_install::{legacy::LegacySpec, simple::Spec, transaction::LockKind};

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "paperboy",
        application: "Paperboy",
        source_directory: "products/paperboy",
        provider_source: "products/paperboy/chancery",
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

/// Install programs, initialize absent production definitions, and refresh schedules.
/// # Errors
/// Returns installation or product setup failures.
pub fn deploy(context: &cell_install::adapter::Context) -> cell_install::Result<()> {
    deploy_inner(context).map_err(|error| cell_install::Error::new(format!("{error:#}")))
}

fn deploy_inner(context: &cell_install::adapter::Context) -> Result<()> {
    context.validate_settings(&[], &[])?;
    let clockwork_binary = context.dependency_binary("clockwork")?;
    let email_binary = context.dependency_binary("email")?;
    let root = context.home.join("Library/Application Support/Paperboy");
    let manifest_path = root.join("paperboy.toml");
    // Refuse invalid existing configuration before changing the installed program.
    if manifest_path.try_exists()? {
        crate::manifest::Manifest::load(&manifest_path)?;
    }
    let prior = crate::schedule::suspend_for_deployment(&clockwork_binary)?;
    cell_install::simple::deploy_program(&specification(), env!("CARGO_PKG_VERSION"), context)?;
    crate::manifest::Manifest::initialize(&manifest_path)?;
    let manifest = crate::manifest::Manifest::load(&manifest_path)?;
    let executable = std::fs::canonicalize(context.home.join(".local/bin/paperboy"))?;
    crate::schedule::apply_with_prior(
        &root,
        &manifest,
        &clockwork_binary,
        &email_binary,
        &executable,
        Some(prior),
    )?;
    Ok(())
}
