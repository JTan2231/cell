//! Verified installation layout; runtime maintenance belongs to Platter.

use cell_install::legacy::LegacySpec;
use cell_install::simple::Spec;
use cell_install::transaction::LockKind;

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
