//! Product-owned program and schedule installation.
use anyhow::{Context as _, Result, ensure};
use cell_install::{legacy::LegacySpec, simple::Spec, transaction::LockKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

/// Install program files, initialize an empty manifest, and refresh selected jobs.
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
    retire_legacy(context, &clockwork_binary)?;
    cell_install::simple::deploy_program(&specification(), env!("CARGO_PKG_VERSION"), context)?;
    crate::manifest::Manifest::initialize(&manifest_path)?;
    let manifest = crate::manifest::Manifest::load(&manifest_path)?;
    let executable = std::fs::canonicalize(context.home.join(".local/bin/paperboy"))?;
    crate::schedule::apply_with(
        &root,
        &manifest,
        &clockwork_binary,
        &email_binary,
        &executable,
    )?;
    Ok(())
}

fn retire_legacy(context: &cell_install::adapter::Context, clockwork_binary: &Path) -> Result<()> {
    use clockwork::api::LaunchImage;
    use clockwork::deployment::ScheduleState;

    let spec = specification();
    let installation =
        cell_install::transaction::inspect_installation(&spec.layout(), &context.home, &|root| {
            spec.read_legacy(root)
        })?;
    let clockwork = clockwork::api::Client::new(clockwork_binary);
    let state = ScheduleState::capture(&clockwork, "paperboy/daily")?;
    let legacy_definition = state
        .definition
        .as_ref()
        .filter(|definition| is_legacy_arguments(&definition.manifest.arguments));
    // A selected new release proves that its predecessor passed this drain.
    // Do not require retired dependencies again unless the old binding was enabled.
    let program = if installation.current.as_ref().is_some_and(|release| {
        release
            .versions
            .get("paperboy")
            .is_some_and(|version| is_legacy_version(version))
    }) {
        Some(std::fs::canonicalize(
            context.home.join(".local/bin/paperboy"),
        )?)
    } else if let Some(definition) = legacy_definition.filter(|_| {
        installation.current.is_none()
            || state
                .binding
                .as_ref()
                .is_some_and(|binding| binding.enabled)
    }) {
        match &definition.manifest.launch {
            LaunchImage::Direct { program, .. } => Some(PathBuf::from(program)),
            LaunchImage::Interpreted { .. } => {
                anyhow::bail!("legacy Paperboy retirement requires its direct installed program")
            }
        }
    } else {
        None
    };
    let Some(program) = program else {
        return Ok(());
    };
    legacy_maintenance(&program, "hold", Some(&context.request.run_id))?;
    if state.binding.is_some() {
        state.suspend(&clockwork, "paperboy/daily")?;
    }
    let drained = legacy_maintenance(&program, "drain", None)?;
    require_legacy_drain(&drained)?;
    // Keep this retirement hold. Retained legacy programs must not admit new work.
    Ok(())
}

fn require_legacy_drain(drained: &serde_json::Value) -> Result<()> {
    ensure!(
        drained.get("drained") == Some(&serde_json::Value::Bool(true))
            && drained.get("nonterminal_jobs") == Some(&serde_json::json!(0)),
        "legacy Paperboy has active or unresolved work; the installed legacy program, maintenance hold, disabled schedule, and database are retained for explicit recovery"
    );
    Ok(())
}

fn is_legacy_version(version: &str) -> bool {
    version.starts_with("0.1.") || version.starts_with("0.2.")
}

fn is_legacy_arguments(arguments: &[String]) -> bool {
    let mut arguments = arguments.iter().map(String::as_str);
    let first = arguments.next();
    let command = if first == Some("--json") {
        arguments.next()
    } else {
        first
    };
    command == Some("run")
        && arguments
            .take_while(|argument| *argument != "--")
            .any(|argument| argument == "--scheduled")
}

fn legacy_maintenance(
    program: &Path,
    operation: &str,
    owner: Option<&str>,
) -> Result<serde_json::Value> {
    let mut arguments = vec!["--json".into(), "maintenance".into(), operation.into()];
    if let Some(owner) = owner {
        arguments.push(owner.into());
    }
    let environment = BTreeMap::from([("CHANCERY_USAGE_INTERNAL".into(), "1".into())]);
    let response =
        cell_install::command::json(program, &arguments, &environment, Duration::from_secs(45))
            .context(
                "legacy Paperboy maintenance failed; retain its program and state for recovery",
            )?;
    Ok(cell_install::command::maintenance(&response)?.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_arguments_do_not_identify_a_legacy_runner() {
        let strings = |arguments: &[&str]| {
            arguments
                .iter()
                .map(|s| (*s).to_string())
                .collect::<Vec<_>>()
        };
        assert!(is_legacy_arguments(&strings(&["run", "--scheduled"])));
        assert!(is_legacy_arguments(&strings(&[
            "--json",
            "run",
            "--scheduled"
        ])));
        assert!(!is_legacy_arguments(&strings(&[
            "execute",
            "--job",
            "daily",
            "--",
            "/render",
            "--scheduled",
        ])));
        assert!(!is_legacy_arguments(&strings(&["run", "daily"])));
        assert!(!is_legacy_arguments(&strings(&[
            "run",
            "daily",
            "--",
            "--scheduled"
        ])));
    }

    #[test]
    fn retirement_refuses_active_or_unresolved_legacy_work() {
        use serde_json::json;
        assert!(require_legacy_drain(&json!({"drained":true,"nonterminal_jobs":0})).is_ok());
        for state in [
            json!({"drained":false,"nonterminal_jobs":0}),
            json!({"drained":true,"nonterminal_jobs":1}),
            json!({"drained":true,"nonterminal_jobs":null}),
            json!({"drained":true}),
        ] {
            assert!(require_legacy_drain(&state).is_err());
        }
    }
}
