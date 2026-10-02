//! Product-owned installation layout and predecessor release proof.
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::simple::Spec;
use cell_install::transaction::LockKind;

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "clockwork",
        application: "Clockwork",
        source_directory: "infrastructure/clockwork",
        provider_source: "infrastructure/clockwork/chancery",
        legacy_provider_path: "share/chancery/clockwork",
        legacy: &LegacySpec {
            format: "1",
            manifest: "manifest.txt",
            metadata: &["version", "product"],
            proofs: &[
                LegacyProof {
                    key: "binary_sha256",
                    paths: &["bin/clockwork"],
                },
                LegacyProof {
                    key: "deployer_sha256",
                    paths: &["package/deploy-user.sh"],
                },
                LegacyProof {
                    key: "uninstaller_sha256",
                    paths: &["package/uninstall-user.sh"],
                },
            ],
            providers: &[LegacyProvider {
                key: "chancery_sha256",
                provider: "clockwork",
                path: "share/chancery/clockwork",
                version_key: "version",
            }],
            hash_path_lines: true,
        },
        wrapper: None,
        lock_kind: LockKind::Shlock,
        lock_at_state: true,
    }
}

fn complete_bindings(
    client: &crate::api::Client,
) -> Result<Vec<crate::api::BindingRecord>, crate::api::Error> {
    // The public list interface supplies a bounded prefix, not an offset cursor.
    let mut limit = 20;
    loop {
        let page = client.bindings_limit(limit)?;
        if !page.has_more {
            return Ok(page.items);
        }
        limit = limit
            .checked_mul(2)
            .ok_or_else(|| crate::api::Error("Clockwork binding inventory is too large".into()))?;
    }
}

/// Replace broker files and refresh enabled bindings with the installed broker.
/// # Errors
/// Returns installation or Clockwork selection failures.
pub fn deploy(context: &cell_install::adapter::Context) -> cell_install::Result<()> {
    cell_install::simple::deploy_program(&specification(), env!("CARGO_PKG_VERSION"), context)?;
    if context
        .home
        .join("Library/Application Support/Clockwork/clockwork.db")
        .try_exists()?
    {
        let client = crate::api::Client::new(context.home.join(".local/bin/clockwork"));
        let bindings = complete_bindings(&client)
            .map_err(|error| cell_install::Error::new(error.to_string()))?;
        for binding in bindings.into_iter().filter(|binding| binding.enabled) {
            let digest = binding.definition_digest.as_deref().ok_or_else(|| {
                cell_install::Error::new("enabled binding has no selected definition")
            })?;
            client
                .switch(&binding.key, digest)
                .map_err(|error| cell_install::Error::new(error.to_string()))?;
        }
    }
    Ok(())
}
