use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use fs2::FileExt as _;
use thiserror::Error;

const LOCK_NAME: &str = ".annals-auth.lock";
const CONFIG_NAME: &str = "config.toml";
const MAX_CONFIG_BYTES: u64 = 64 * 1024;

pub(crate) struct CredentialLease {
    _file: File,
}

impl CredentialLease {
    pub(crate) fn acquire(codex_home: &Path) -> Result<Self, AuthLeaseError> {
        let (file, lock_path) = open_lock(codex_home)?;
        file.lock_exclusive()
            .map_err(|source| AuthLeaseError::Lock {
                path: lock_path,
                source,
            })?;
        validate_config(codex_home)?;
        Ok(Self { _file: file })
    }

    pub(crate) fn try_acquire(codex_home: &Path) -> Result<Self, AuthLeaseError> {
        let (file, lock_path) = open_lock(codex_home)?;
        match file.try_lock_exclusive() {
            Ok(()) => {}
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                return Err(AuthLeaseError::Busy { path: lock_path });
            }
            Err(source) => {
                return Err(AuthLeaseError::Lock {
                    path: lock_path,
                    source,
                });
            }
        }
        validate_config(codex_home)?;
        Ok(Self { _file: file })
    }
}

fn open_lock(codex_home: &Path) -> Result<(File, PathBuf), AuthLeaseError> {
    let metadata = fs::symlink_metadata(codex_home).map_err(|source| AuthLeaseError::Home {
        path: codex_home.to_path_buf(),
        source,
    })?;
    if !metadata.file_type().is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(AuthLeaseError::UnsafeHome(codex_home.to_path_buf()));
    }

    let lock_path = codex_home.join(LOCK_NAME);
    match fs::symlink_metadata(&lock_path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(AuthLeaseError::UnsafeLock(lock_path));
        }
        Ok(_) => {}
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(AuthLeaseError::Open {
                path: lock_path,
                source,
            });
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .map_err(|source| AuthLeaseError::Open {
            path: lock_path.clone(),
            source,
        })?;
    if !file
        .metadata()
        .map_err(|source| AuthLeaseError::Open {
            path: lock_path.clone(),
            source,
        })?
        .file_type()
        .is_file()
    {
        return Err(AuthLeaseError::UnsafeLock(lock_path));
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|source| AuthLeaseError::Open {
            path: lock_path.clone(),
            source,
        })?;
    Ok((file, lock_path))
}

fn validate_config(codex_home: &Path) -> Result<(), AuthLeaseError> {
    let path = codex_home.join(CONFIG_NAME);
    let metadata = fs::symlink_metadata(&path).map_err(|source| AuthLeaseError::ConfigRead {
        path: path.clone(),
        source,
    })?;
    if !metadata.file_type().is_file()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > MAX_CONFIG_BYTES
    {
        return Err(AuthLeaseError::UnsafeConfig(path));
    }
    let document = fs::read_to_string(&path).map_err(|source| AuthLeaseError::ConfigRead {
        path: path.clone(),
        source,
    })?;
    let value =
        toml::from_str::<toml::Value>(&document).map_err(|source| AuthLeaseError::ConfigParse {
            path: path.clone(),
            source,
        })?;
    let valid = value.as_table().is_some_and(|table| {
        table.len() == 1
            && table
                .get("cli_auth_credentials_store")
                .and_then(toml::Value::as_str)
                == Some("file")
    });
    if !valid {
        return Err(AuthLeaseError::UnsupportedConfig(path));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub(crate) enum AuthLeaseError {
    #[error("unable to inspect state-local Codex home {path}: {source}")]
    Home { path: PathBuf, source: io::Error },
    #[error("state-local Codex home must be a private regular directory: {0}")]
    UnsafeHome(PathBuf),
    #[error("unable to open authentication lease {path}: {source}")]
    Open { path: PathBuf, source: io::Error },
    #[error("authentication lease path is not a regular file: {0}")]
    UnsafeLock(PathBuf),
    #[error("unable to lock state-local Codex authentication at {path}: {source}")]
    Lock { path: PathBuf, source: io::Error },
    #[error("state-local Codex authentication is currently in use: {path}")]
    Busy { path: PathBuf },
    #[error("unable to read state-local Codex configuration {path}: {source}")]
    ConfigRead { path: PathBuf, source: io::Error },
    #[error("invalid state-local Codex configuration {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("state-local Codex configuration must be a private regular file: {0}")]
    UnsafeConfig(PathBuf),
    #[error(
        "state-local Codex configuration may contain only cli_auth_credentials_store = \"file\": {0}"
    )]
    UnsupportedConfig(PathBuf),
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::{AuthLeaseError, CredentialLease};

    fn codex_home() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
        let home = tempfile::tempdir()?;
        fs::set_permissions(home.path(), fs::Permissions::from_mode(0o700))?;
        let config = home.path().join("config.toml");
        fs::write(&config, "cli_auth_credentials_store = \"file\"\n")?;
        fs::set_permissions(config, fs::Permissions::from_mode(0o600))?;
        Ok(home)
    }

    #[test]
    fn credential_lease_serializes_one_codex_home() -> Result<(), Box<dyn std::error::Error>> {
        let home = codex_home()?;
        let first = CredentialLease::acquire(home.path())?;
        let path = home.path().to_path_buf();
        let (sender, receiver) = mpsc::channel();
        let waiter = thread::spawn(move || {
            let lease = CredentialLease::acquire(&path);
            let _ = sender.send(lease.is_ok());
            lease
        });

        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        drop(first);
        assert!(receiver.recv_timeout(Duration::from_secs(1))?);
        drop(
            waiter
                .join()
                .map_err(|_| "credential lease waiter panicked")??,
        );
        assert_eq!(
            fs::metadata(home.path().join(".annals-auth.lock"))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        Ok(())
    }

    #[test]
    fn credential_lease_rejects_ambient_codex_configuration()
    -> Result<(), Box<dyn std::error::Error>> {
        let home = codex_home()?;
        fs::write(
            home.path().join("config.toml"),
            "cli_auth_credentials_store = \"file\"\nweb_search = \"live\"\n",
        )?;
        let error = CredentialLease::try_acquire(home.path())
            .err()
            .ok_or("ambient Codex configuration unexpectedly passed")?;
        assert!(matches!(error, AuthLeaseError::UnsupportedConfig(_)));
        Ok(())
    }
}
