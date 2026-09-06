//! Product-owned installation layout and predecessor release proof.
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::simple::Spec;
use cell_install::transaction::LockKind;

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "cast",
        application: "Cast",
        source_directory: "cast",
        provider_source: "cast/chancery",
        legacy_provider_path: "share/chancery/cast",
        legacy: &LegacySpec {
            format: "1",
            manifest: "manifest.txt",
            metadata: &["version"],
            proofs: &[
                LegacyProof {
                    key: "payload_sha256",
                    paths: &["libexec/cast"],
                },
                LegacyProof {
                    key: "frontend_sha256",
                    paths: &["bin/cast", "package/cast"],
                },
                LegacyProof {
                    key: "deployer_sha256",
                    paths: &["package/deploy-user.sh"],
                },
            ],
            providers: &[LegacyProvider {
                key: "provider_sha256",
                provider: "cast",
                path: "share/chancery/cast",
                version_key: "version",
            }],
            hash_path_lines: false,
        },
        wrapper: Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packaging/macos/cast"
        ))),
        lock_kind: LockKind::Directory,
        lock_at_state: false,
        maintained: false,
    }
}
