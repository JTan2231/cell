use std::fs::{File, OpenOptions};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};

use fs2::FileExt;

use crate::error::{Context as _, Error, Result};
use crate::paths::{Layout, current_uid};

pub(crate) struct KeyLock {
    #[allow(dead_code)]
    file: File,
}

impl KeyLock {
    pub(crate) fn acquire_management(layout: &Layout, key: &str) -> Result<Self> {
        let file = open(&layout.management_lock_path(key))?;
        FileExt::lock_exclusive(&file)
            .context("key_lock_unavailable", format!("lock management for {key}"))?;
        Ok(Self { file })
    }

    pub(crate) fn try_acquire_transition(layout: &Layout, key: &str) -> Result<Option<Self>> {
        let file = open(&layout.gate_lock_path(key))?;
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => Ok(Some(Self { file })),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(Error::new(
                "key_lock_unavailable",
                format!("lock {key}: {error}"),
            )),
        }
    }

    pub(crate) fn acquire_transition(layout: &Layout, key: &str) -> Result<Self> {
        let file = open(&layout.gate_lock_path(key))?;
        FileExt::lock_exclusive(&file).context("key_lock_unavailable", format!("lock {key}"))?;
        Ok(Self { file })
    }

    pub(crate) fn acquire_activation_gate(layout: &Layout, key: &str) -> Result<Self> {
        let file = open(&layout.gate_lock_path(key))?;
        FileExt::lock_shared(&file).context(
            "key_lock_unavailable",
            format!("enter activation gate for {key}"),
        )?;
        Ok(Self { file })
    }

    pub(crate) fn try_acquire_activation(layout: &Layout, key: &str) -> Result<Option<Self>> {
        let file = open(&layout.activation_lock_path(key))?;
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => Ok(Some(Self { file })),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(Error::new(
                "key_lock_unavailable",
                format!("lock activation {key}: {error}"),
            )),
        }
    }
}

fn open(path: &std::path::Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .context("key_lock_unavailable", format!("open {}", path.display()))?;
    let metadata = file
        .metadata()
        .context("key_lock_unavailable", "inspect key lock")?;
    if !metadata.is_file() {
        return Err(Error::new(
            "key_lock_unavailable",
            format!("{} is not a regular file", path.display()),
        ));
    }
    if metadata.uid() != current_uid()? {
        return Err(Error::new(
            "key_lock_unavailable",
            format!("{} must be owned by the current user", path.display()),
        ));
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .context("key_lock_unavailable", "make key lock private")?;
    Ok(file)
}
