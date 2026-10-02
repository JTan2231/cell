//! Mantic program selection and calculation-configuration initialization.

use cell_install::{adapter::Context, legacy::LegacySpec, simple::Spec, transaction::LockKind};

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "mantic",
        application: "Mantic",
        source_directory: "products/mantic",
        provider_source: "products/mantic/chancery",
        legacy_provider_path: "share/chancery/mantic",
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
    }
}

/// Select matching programs and provider, then initialize the default database.
///
/// # Errors
/// Returns installation or initialization failures. Completed effects remain.
pub fn deploy(context: &Context) -> cell_install::Result<()> {
    // Refuse unsupported settings before changing program selection.
    context.validate_settings(&[], &[])?;
    cell_install::simple::deploy_program(&specification(), env!("CARGO_PKG_VERSION"), context)?;
    let database = crate::database_path(&context.home);
    crate::store::Store::initialize(&database)
        .map_err(|error| cell_install::Error::new(format!("{error:#}")))?;
    Ok(())
}
