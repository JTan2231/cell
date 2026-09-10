//! Product-owned installation layout and predecessor release proof.
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::simple::Spec;
use cell_install::transaction::LockKind;

/// Initialize absent state through CRM under its exact deployment hold.
/// # Errors
/// Rejects setup settings and any failed initialization.
pub fn lifecycle(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> cell_install::Result<serde_json::Value> {
    use cell_install::adapter::Operation;
    if context
        .request
        .settings
        .as_ref()
        .is_some_and(|value| value != &serde_json::json!({}))
    {
        return Err(cell_install::Error::new(
            "CRM deployment has no configurable setup fields",
        ));
    }
    let database = std::env::var_os("CRM_DATABASE").map_or_else(
        || context.home.join("Library/Application Support/CRM/crm.db"),
        std::path::PathBuf::from,
    );
    let forward = context
        .request
        .recovery
        .as_ref()
        .and_then(|r| r.get("any_apply_started"))
        == Some(&serde_json::json!(true))
        && context.home.join(".local/bin/crm").exists();
    if (operation == Operation::Configure || operation == Operation::Recover && forward)
        && !database.exists()
    {
        cell_install::command::json(
            &context.home.join(".local/bin/crm"),
            &["--json".into(), "init".into()],
            &std::collections::BTreeMap::from([(
                "CELL_DEPLOYMENT_RUN_ID".into(),
                context.request.run_id.clone().into(),
            )]),
            std::time::Duration::from_secs(60),
        )?;
    }
    if (operation == Operation::Configure || operation == Operation::Recover && forward)
        && context.selected()
    {
        cell_install::command::json(
            &context.home.join(".local/bin/crm"),
            &[
                "--json".into(),
                "migrate".into(),
                "--backup".into(),
                context
                    .home
                    .join(format!(
                        "Library/Application Support/CRM/crm-pre-migration-{}.sqlite",
                        context.request.run_id
                    ))
                    .into_os_string(),
            ],
            &std::collections::BTreeMap::from([(
                "CELL_DEPLOYMENT_RUN_ID".into(),
                context.request.run_id.clone().into(),
            )]),
            std::time::Duration::from_secs(600),
        )?;
    }
    Ok(serde_json::json!({"initialized":database.exists()}))
}

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "crm",
        application: "CRM",
        source_directory: "crm",
        provider_source: "crm/chancery",
        legacy_provider_path: "share/chancery/crm",
        legacy: &LegacySpec {
            format: "1",
            manifest: "manifest.txt",
            metadata: &["version", "product"],
            proofs: &[
                LegacyProof {
                    key: "binary_sha256",
                    paths: &["bin/crm"],
                },
                LegacyProof {
                    key: "deployer_sha256",
                    paths: &["package/deploy-user.sh"],
                },
            ],
            providers: &[LegacyProvider {
                key: "chancery_sha256",
                provider: "crm",
                path: "share/chancery/crm",
                version_key: "version",
            }],
            hash_path_lines: true,
        },
        wrapper: None,
        lock_kind: LockKind::Shlock,
        lock_at_state: false,
        maintained: true,
    }
}
