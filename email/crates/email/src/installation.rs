//! Product-owned installation layout and predecessor release proof.
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::simple::Spec;
use cell_install::transaction::LockKind;

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "email",
        application: "Email",
        source_directory: "email",
        provider_source: "email/chancery",
        legacy_provider_path: "share/chancery/email",
        legacy: &LegacySpec {
            format: "1",
            manifest: "manifest.txt",
            metadata: &["version"],
            proofs: &[
                LegacyProof {
                    key: "payload_sha256",
                    paths: &["libexec/email"],
                },
                LegacyProof {
                    key: "frontend_sha256",
                    paths: &["bin/email", "package/email"],
                },
                LegacyProof {
                    key: "deployer_sha256",
                    paths: &["package/deploy-user.sh"],
                },
            ],
            providers: &[LegacyProvider {
                key: "provider_sha256",
                provider: "email",
                path: "share/chancery/email",
                version_key: "version",
            }],
            hash_path_lines: false,
        },
        wrapper: Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packaging/macos/email"
        ))),
        lock_kind: LockKind::Directory,
        lock_at_state: false,
        maintained: false,
    }
}
