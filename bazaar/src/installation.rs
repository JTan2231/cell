//! Bazaar program selection and empty-state initialization.

use cell_install::{
    adapter::{Context, Operation},
    legacy::LegacySpec,
    simple::Spec,
    transaction::LockKind,
};

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "bazaar",
        application: "Bazaar",
        source_directory: "bazaar",
        provider_source: "bazaar/chancery",
        legacy_provider_path: "share/chancery/bazaar",
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

/// Configure and verify schema-one state without adding string versions.
///
/// # Errors
/// Returns an error for unsupported settings or unavailable or incompatible state.
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
        return Err(cell_install::Error::new(
            "Bazaar has no deployment settings",
        ));
    }
    let database = crate::database_path(&context.home);
    let map_error = |error: crate::api::Error| cell_install::Error::new(error.to_string());
    if operation == Operation::Configure {
        crate::api::Writer::initialize(&database).map_err(map_error)?;
    }
    if operation == Operation::Verify || (operation == Operation::Recover && database.exists()) {
        crate::api::Reader::open(&database)
            .and_then(|reader| reader.check())
            .map_err(map_error)?;
    }
    Ok(serde_json::json!({"schema_version":1}))
}
