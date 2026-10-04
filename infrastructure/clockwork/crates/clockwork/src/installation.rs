//! Product-owned installation layout and predecessor release proof.
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::simple::Spec;
use cell_install::transaction::LockKind;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

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

fn installed_bindings(home: &Path) -> cell_install::Result<Vec<crate::api::BindingRecord>> {
    let state = home.join("Library/Application Support/Clockwork");
    let database = state.join("clockwork.db");
    if !database.try_exists()? && !database.is_symlink() {
        for (path, transition) in [
            (state.join("locks"), true),
            (home.join("Library/LaunchAgents"), false),
        ] {
            if !path.try_exists()? && !path.is_symlink() {
                continue;
            }
            if !std::fs::symlink_metadata(&path)?.is_dir() {
                return Err(cell_install::Error::new(
                    "Clockwork locks or LaunchAgents path is not a regular directory",
                ));
            }
            for entry in std::fs::read_dir(path)? {
                let name = entry?.file_name();
                let name = name.to_string_lossy();
                if transition && name.ends_with(".transition.json")
                    || !transition && name.starts_with("org.clockwork.") && name.ends_with(".plist")
                {
                    return Err(cell_install::Error::new(
                        "Clockwork database is absent while retained runtime state remains",
                    ));
                }
            }
        }
        return Ok(Vec::new());
    }
    complete_bindings(&crate::api::Client::new(home.join(".local/bin/clockwork")).with_home(home))
        .map_err(|error| cell_install::Error::new(error.to_string()))
}

/// Refuse recovery bytes whose broker cannot operate from the fixed runtime path.
/// # Errors
/// Rejects unavailable release metadata and pre-runtime installation formats.
pub fn require_runtime_recovery(release: &Path) -> cell_install::Result<()> {
    let spec = specification();
    let info = cell_install::transaction::read_release_at(&spec.layout(), release, &|root| {
        spec.read_legacy(root)
    })?;
    if info.format != cell_install::transaction::TRANSACTION_FORMAT {
        return Err(cell_install::Error::new(
            "Clockwork recovery requires cell-install-v4 runtime-aware programs; rebuild historical source with fixed-runtime support",
        ));
    }
    Ok(())
}

/// Require disabled bindings and settle their existing activations before direct replacement.
/// # Errors
/// Refuses enabled schedules, unavailable inventory, or unproved activation exit.
pub fn require_quiescent_installation(home: &Path) -> cell_install::Result<()> {
    let bindings = installed_bindings(home)?;
    if bindings.iter().any(|binding| binding.enabled) {
        return Err(cell_install::Error::new(
            "disable Clockwork bindings before direct program installation or recovery",
        ));
    }
    let client = crate::api::Client::new(home.join(".local/bin/clockwork")).with_home(home);
    for binding in bindings {
        client
            .disable(&binding.key, None)
            .map_err(|error| cell_install::Error::new(error.to_string()))?;
    }
    Ok(())
}

/// Drain bindings, replace broker files, and restore their prior enabled intent.
/// # Errors
/// Returns installation or Clockwork selection failures.
pub fn deploy(context: &cell_install::adapter::Context) -> cell_install::Result<()> {
    let bindings = installed_bindings(&context.home)?;
    let mut intent = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(
            context
                .request
                .run_dir
                .join("clockwork-binding-intent.json"),
        )?;
    intent.write_all(&serde_json::to_vec_pretty(&bindings)?)?;
    intent.sync_all()?;
    std::fs::File::open(&context.request.run_dir)?.sync_all()?;
    let client =
        crate::api::Client::new(context.home.join(".local/bin/clockwork")).with_home(&context.home);
    for binding in &bindings {
        client
            .disable(&binding.key, None)
            .map_err(|error| cell_install::Error::new(error.to_string()))?;
    }
    cell_install::simple::deploy_program(&specification(), env!("CARGO_PKG_VERSION"), context)?;
    for binding in bindings.into_iter().filter(|binding| binding.enabled) {
        let digest = binding.definition_digest.as_deref().ok_or_else(|| {
            cell_install::Error::new("enabled binding has no selected definition")
        })?;
        client
            .switch(&binding.key, digest)
            .map_err(|error| cell_install::Error::new(error.to_string()))?;
    }
    Ok(())
}
