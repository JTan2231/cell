//! Product-owned installation layout and predecessor release proof.
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::simple::Spec;
use cell_install::transaction::LockKind;

/// Initialize missing discovery state without collecting from any provider.
/// # Errors
/// Rejects unknown setup fields and failed initialization.
pub fn lifecycle(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> cell_install::Result<serde_json::Value> {
    use cell_install::adapter::Operation;
    #[derive(Default, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Settings {
        state_dir: Option<std::path::PathBuf>,
        config_file: Option<std::path::PathBuf>,
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
        || settings
            .config_file
            .as_ref()
            .is_some_and(|path| !path.is_absolute() || !path.is_file())
    {
        return Err(cell_install::Error::new(
            "Cast setup paths must be absolute existing inputs",
        ));
    }
    if operation == Operation::Configure
        || operation == Operation::Recover
            && context
                .request
                .recovery
                .as_ref()
                .is_some_and(|recovery| recovery["any_apply_started"] == true)
            && context.home.join(".local/bin/cast").exists()
    {
        // Recovery can select a retained CLI with an older output interface.
        // Setup uses the same state APIs and product lock as the current CLI.
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
        let _lock = store.lock().map_err(|_| {
            cell_install::Error::new("Cast setup could not acquire the product lock")
        })?;
        if let Some(path) = settings.config_file {
            let config = serde_json::from_slice(&std::fs::read(path)?)?;
            store
                .set_config(&config)
                .map_err(|_| cell_install::Error::new("Cast configuration replacement failed"))?;
        }
    }
    Ok(serde_json::json!({"configured":operation == Operation::Configure}))
}

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
