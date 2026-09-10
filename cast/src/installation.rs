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
        let executable = context.home.join(".local/bin/cast");
        let mut arguments: Vec<std::ffi::OsString> = vec!["--json".into()];
        if let Some(path) = settings.state_dir {
            arguments.extend(["--state-dir".into(), path.into_os_string()]);
        }
        let mut init = arguments.clone();
        init.push("init".into());
        cell_install::command::json(
            &executable,
            &init,
            &std::collections::BTreeMap::new(),
            std::time::Duration::from_secs(60),
        )?;
        if let Some(path) = settings.config_file {
            arguments.extend([
                "config".into(),
                "set".into(),
                "--file".into(),
                path.into_os_string(),
            ]);
            cell_install::command::json(
                &executable,
                &arguments,
                &std::collections::BTreeMap::new(),
                std::time::Duration::from_secs(60),
            )?;
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
