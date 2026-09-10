//! Durable completion receipts for product-owned deployment migrations.

use crate::{Error, Result, file_digest};
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: u32,
    backup: PathBuf,
    sha256: Option<String>,
}

/// Replay a completed migration by checking its retained backup identity.
/// The product's migration must itself recover an interrupted call when no
/// receipt exists. Later configuration and admitted work may change live state.
///
/// # Errors
/// Refuses changed backup evidence, unsafe receipt paths, or migration failure.
pub fn run_once(receipt: &Path, backup: &Path, migrate: impl FnOnce() -> Result<()>) -> Result<()> {
    if !receipt.is_absolute() || !backup.is_absolute() {
        return Err(Error::new("migration evidence paths must be absolute"));
    }
    let parent = receipt
        .parent()
        .ok_or_else(|| Error::new("migration receipt parent missing"))?;
    let owner = std::fs::symlink_metadata(parent)?;
    if !owner.is_dir() || owner.file_type().is_symlink() || owner.mode() & 0o077 != 0 {
        return Err(Error::new("migration receipt directory must be private"));
    }
    match std::fs::symlink_metadata(receipt) {
        Ok(metadata) => {
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.nlink() != 1
                || metadata.uid() != owner.uid()
                || metadata.mode() & 0o077 != 0
            {
                return Err(Error::new("migration receipt must be a private owned file"));
            }
            let saved: Receipt = serde_json::from_slice(&std::fs::read(receipt)?)?;
            if saved.schema != 1
                || saved.backup != backup
                || saved.sha256 != backup_digest(backup, owner.uid())?
            {
                return Err(Error::new("retained migration evidence changed"));
            }
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    migrate()?;
    let saved = Receipt {
        schema: 1,
        backup: backup.to_owned(),
        sha256: backup_digest(backup, owner.uid())?,
    };
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    pending.write_all(&serde_json::to_vec(&saved)?)?;
    pending.as_file().sync_all()?;
    pending
        .persist_noclobber(receipt)
        .map_err(|error| error.error)?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn backup_digest(path: &Path, owner: u32) -> Result<Option<String>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.nlink() != 1
                || metadata.uid() != owner
                || metadata.mode() & 0o077 != 0
            {
                return Err(Error::new("migration backup must be a private owned file"));
            }
            file_digest(path).map(Some)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn completed_migration_keeps_backup_after_live_state_changes() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().canonicalize()?;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        let receipt = root.join("migration.json");
        let backup = root.join("backup.sqlite");
        run_once(&receipt, &backup, || {
            std::fs::write(&backup, b"original snapshot")?;
            std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o600))?;
            Ok(())
        })?;
        std::fs::write(root.join("live.sqlite"), b"later admitted work")?;
        run_once(&receipt, &backup, || {
            Err(Error::new("must not repeat migration"))
        })?;
        std::fs::write(&backup, b"changed backup")?;
        assert!(run_once(&receipt, &backup, || Ok(())).is_err());
        Ok(())
    }

    #[test]
    fn absent_receipt_calls_product_recovery_for_a_committed_migration() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().canonicalize()?;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        let receipt = root.join("migration.json");
        let backup = root.join("backup.sqlite");
        // The product committed its backup before the receipt could be written.
        std::fs::write(&backup, b"already committed")?;
        std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o600))?;
        run_once(&receipt, &backup, || {
            assert_eq!(std::fs::read(&backup)?, b"already committed");
            Ok(())
        })?;
        assert!(receipt.is_file());
        assert_eq!(std::fs::read(&backup)?, b"already committed");
        Ok(())
    }
}
