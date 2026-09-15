//! Clew program selection and schema-one initialization. Ledger rows are preserved.
use cell_install::{
    adapter::{Context, Operation},
    legacy::LegacySpec,
    simple::Spec,
    transaction::LockKind,
};

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "clew",
        application: "Clew",
        source_directory: "clew",
        provider_source: "clew/chancery",
        legacy_provider_path: "share/chancery/clew",
        legacy: &LegacySpec {
            format: "none",
            manifest: "manifest.txt",
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

pub fn lifecycle(
    context: &Context,
    operation: Operation,
) -> cell_install::Result<serde_json::Value> {
    if context
        .request
        .settings
        .as_ref()
        .is_some_and(|value| value != &serde_json::json!({}))
    {
        return Err(cell_install::Error::new("Clew has no deployment settings"));
    }
    let root = crate::state_dir(&context.home);
    let map_error = |error: anyhow::Error| cell_install::Error::new(format!("{error:#}"));
    if operation == Operation::Configure {
        crate::store::Store::initialize(&root).map_err(map_error)?;
    }
    if operation == Operation::Verify {
        crate::store::Store::open(&root, false)
            .and_then(|store| store.check())
            .map_err(map_error)?;
        platter::api::Client::new(context.home.join(".local/bin/platter"))
            .list(None)
            .map_err(map_error)?;
    }
    // File recovery does not restore, remove or rewrite the independent ledger.
    if operation == Operation::Recover && root.join(crate::store::DATABASE).exists() {
        crate::store::Store::open(&root, false)
            .and_then(|store| store.check())
            .map_err(map_error)?;
    }
    Ok(serde_json::json!({"schema_version":1}))
}
