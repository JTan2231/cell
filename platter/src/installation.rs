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
        _ => cell_install::simple::main(&specification(), env!("CARGO_PKG_VERSION")),
    }
}
