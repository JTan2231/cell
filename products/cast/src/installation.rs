//! Product-owned program installation and state setup.
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::simple::Spec;
use cell_install::transaction::LockKind;

/// Initialize missing accepted-record state.
/// # Errors
/// Rejects unknown setup fields and failed initialization.
fn configure(context: &cell_install::adapter::Context) -> cell_install::Result<()> {
    #[derive(Default, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Settings {
        state_dir: Option<std::path::PathBuf>,
    }
    let settings: Settings = serde_json::from_value(
        context
            .request
            .settings
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
    )?;
    if settings
        .state_dir
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err(cell_install::Error::new(
            "Cast state directory must be absolute",
        ));
    }
    let directory = settings
        .state_dir
        .or_else(|| {
            std::env::var_os("CAST_STATE_DIR")
                .filter(|path| !path.is_empty())
                .map(std::path::PathBuf::from)
        })
        .unwrap_or_else(|| context.home.join(".local/share/cast"));
    let store = crate::store::Store::init(&directory)
        .map_err(|_| cell_install::Error::new("Cast state initialization failed"))?;
    let _lock = store
        .lock()
        .map_err(|_| cell_install::Error::new("Cast setup could not acquire the product lock"))?;
    Ok(())
}

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "cast",
        application: "Cast",
        source_directory: "products/cast",
        provider_source: "products/cast/chancery",
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
    }
}

/// Install declared files and apply this product's requested setup.
/// # Errors
/// Returns installation or setup failures.
pub fn deploy(context: &cell_install::adapter::Context) -> cell_install::Result<()> {
    cell_install::simple::deploy_program(&specification(), env!("CARGO_PKG_VERSION"), context)?;
    configure(context)?;
    Ok(())
}
