//! Stateless Iatreion installation through the shared Cell installer.

use cell_install::{legacy::LegacySpec, simple::Spec, transaction::LockKind};

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "iatreion",
        application: "Iatreion",
        source_directory: "iatreion",
        provider_source: "iatreion/chancery",
        legacy_provider_path: "share/chancery/iatreion",
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
        maintained: false,
    }
}
