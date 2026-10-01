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

#[derive(Deserialize, Serialize)]
struct InstallReceipt {
    schema: u32,
    backup: PathBuf,
}

/// Record migration completion so a resumed deployment does not repeat it.
/// The product's migration must itself recover an interrupted call when no
/// receipt exists. Later configuration and admitted work may change live state.
///
/// # Errors
/// Refuses invalid receipt paths or migration failure.
pub fn install_once(
    receipt: &Path,
    backup: &Path,
    migrate: impl FnOnce() -> Result<()>,
) -> Result<()> {
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
            let saved: InstallReceipt = serde_json::from_slice(&std::fs::read(receipt)?)?;
            if saved.schema != 1 || saved.backup != backup {
                return Err(Error::new("retained migration evidence changed"));
            }
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    migrate()?;
    let saved = InstallReceipt {
        schema: 1,
        backup: backup.to_owned(),
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
