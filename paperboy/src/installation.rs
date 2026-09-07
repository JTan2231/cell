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
    cell_install::simple::main(&specification(), env!("CARGO_PKG_VERSION"))
}
