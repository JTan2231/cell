//! Product-owned installation layout and predecessor release proof.
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::simple::Spec;
use cell_install::transaction::LockKind;

/// Apply only Email-owned setup; never return credential bytes to the coordinator.
/// # Errors
/// Rejects malformed settings, invalid credential sources and unsafe storage.
pub fn lifecycle(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> cell_install::Result<serde_json::Value> {
    use cell_install::adapter::Operation;
    let setup: crate::settings::Setup = serde_json::from_value(
        context
            .request
            .settings
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
    )?;
    match operation {
        Operation::Inspect => crate::settings::validate(&setup),
        Operation::Configure => crate::settings::configure(&setup),
        Operation::Recover
            if context
                .request
                .recovery
                .as_ref()
                .is_some_and(|recovery| recovery["any_apply_started"] == true)
                && context.home.join(".local/bin/email").exists() =>
        {
            crate::settings::configure(&setup)
        }
        _ => Ok(()),
    }
    .map_err(|error| cell_install::Error::new(error.to_string()))?;
    Ok(
        serde_json::json!({"credential_supplied":setup.credential_file.is_some(),"receiving_domain_supplied":setup.receiving_domain.is_some()}),
    )
}

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
