//! Product-owned admission fences for deployment.
//!
//! Products select a private gate directory, hold an [`Admission`] throughout
//! the relevant work, and augment [`Status::drained`] with durable domain work.
//! A hold never changes a product's operator pause. Releasing one owner leaves
//! every other owner's hold intact. The deployment runner is not a domain writer.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("deployment maintenance prevents new work")]
    Held,
    #[error("previously admitted work is still active")]
    Busy,
    #[error("invalid deployment hold owner")]
    InvalidOwner,
    #[error("deployment maintenance: {0}")]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Resolve a database identity before its product chooses a maintenance gate.
/// Existing symbolic aliases converge on the same regular, single-link file.
/// An absent database retains its normal filename beneath the resolved parent.
///
/// # Errors
/// Returns an I/O error for a special or hardlinked database, an inaccessible
/// ancestor, or an unresolvable relative path.
pub fn canonical_database_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    match fs::metadata(&absolute) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(invalid_state().into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt as _;
                if metadata.nlink() != 1 {
                    return Err(invalid_state().into());
                }
            }
            Ok(absolute.canonicalize()?)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut missing = Vec::new();
            let mut ancestor = absolute.as_path();
            loop {
                match fs::symlink_metadata(ancestor) {
                    Ok(_) => break,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        missing.push(ancestor.file_name().ok_or_else(invalid_state)?.to_owned());
                        ancestor = ancestor.parent().ok_or_else(invalid_state)?;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            let mut resolved = ancestor.canonicalize()?;
            if !resolved.is_dir() {
                return Err(invalid_state().into());
            }
            for component in missing.into_iter().rev() {
                resolved.push(component);
            }
            Ok(resolved)
        }
        Err(error) => Err(error.into()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub contract_version: u32,
    pub holds: Vec<String>,
    pub drained: bool,
}

/// A live admission. Dropping it releases the activity lock, including after a
/// process exits. Durable holds themselves never expire on process death.
#[derive(Debug)]
pub struct Admission {
    _activity: File,
}

#[derive(Debug, Clone)]
pub struct Gate {
    root: PathBuf,
}

impl Gate {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Admit new product work atomically with respect to creation of a hold.
    ///
    /// # Errors
    /// Returns `Held` if any owner has a hold, or an I/O error for an unprovable
    /// gate. Keep the returned guard alive until the admitted work settles.
    pub fn enter(&self) -> Result<Admission> {
        let _control = self.control()?;
        if !self.holds()?.is_empty() {
            return Err(Error::Held);
        }
        let activity = self.activity()?;
        fs2::FileExt::try_lock_shared(&activity).map_err(map_busy)?;
        Ok(Admission {
            _activity: activity,
        })
    }

    /// Continue previously admitted durable work during maintenance.
    ///
    /// The product must establish the existing work identity first and must
    /// never use this operation to create a new domain attempt.
    ///
    /// # Errors
    /// Returns `Busy` during exclusive installation, or an I/O error.
    pub fn recover(&self) -> Result<Admission> {
        let _control = self.control()?;
        self.holds()?;
        let activity = self.activity()?;
        fs2::FileExt::try_lock_shared(&activity).map_err(map_busy)?;
        Ok(Admission {
            _activity: activity,
        })
    }

    /// Admit an installation under its sole matching hold.
    /// This never bypasses another owner, and requires previous activity drained.
    ///
    /// # Errors
    /// Returns `InvalidOwner`, `Held`, `Busy`, or an I/O error.
    pub fn enter_for(&self, owner: &str) -> Result<Admission> {
        validate_owner(owner)?;
        let _control = self.control()?;
        if self.holds()? != [owner] {
            return Err(Error::Held);
        }
        let activity = self.activity()?;
        fs2::FileExt::try_lock_exclusive(&activity).map_err(map_busy)?;
        Ok(Admission {
            _activity: activity,
        })
    }

    /// Durably prevent new admission. Existing admissions continue to settle.
    /// Repeating this operation with the same owner is idempotent.
    ///
    /// # Errors
    /// Returns `InvalidOwner` or an I/O error, including invalid existing state.
    pub fn hold(&self, owner: &str) -> Result<Status> {
        validate_owner(owner)?;
        let _control = self.control()?;
        let path = self.root.join("holds").join(owner);
        if !self.holds()?.iter().any(|value| value == owner) {
            let mut pending = tempfile::NamedTempFile::new_in(self.root.join("holds"))?;
            writeln!(pending, "{owner}")?;
            pending.as_file().sync_all()?;
            pending
                .persist(&path)
                .map_err(|error| Error::Io(error.error))?;
            File::open(self.root.join("holds"))?.sync_all()?;
        }
        self.status_locked()
    }

    /// Release only this owner's hold. Absence is an idempotent success.
    ///
    /// # Errors
    /// Returns `InvalidOwner` or an I/O error for unprovable state.
    pub fn release(&self, owner: &str) -> Result<Status> {
        validate_owner(owner)?;
        let _control = self.control()?;
        if self.holds()?.iter().any(|value| value == owner) {
            fs::remove_file(self.root.join("holds").join(owner))?;
            File::open(self.root.join("holds"))?.sync_all()?;
        }
        self.status_locked()
    }

    /// Inspect without creating an absent gate. A product must also account
    /// for its durable admitted work before reporting domain quiescence.
    ///
    /// # Errors
    /// Returns an I/O error for invalid, inaccessible, or unprovable state.
    pub fn status(&self) -> Result<Status> {
        match fs::symlink_metadata(&self.root) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Status {
                    contract_version: 1,
                    holds: vec![],
                    drained: true,
                });
            }
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        require_private_directory(&self.root)?;
        require_private_directory(&self.root.join("holds"))?;
        let control = private_file_read(&self.root.join("control.lock"))?;
        fs2::FileExt::lock_shared(&control)?;
        self.status_with_activity(&private_file_read(&self.root.join("activity.lock"))?)
    }

    fn status_locked(&self) -> Result<Status> {
        let activity = self.activity()?;
        self.status_with_activity(&activity)
    }

    fn status_with_activity(&self, activity: &File) -> Result<Status> {
        let drained = match fs2::FileExt::try_lock_exclusive(activity) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => false,
            Err(error) => return Err(error.into()),
        };
        Ok(Status {
            contract_version: 1,
            holds: self.holds()?,
            drained,
        })
    }

    fn control(&self) -> Result<File> {
        private_directory(&self.root)?;
        private_directory(&self.root.join("holds"))?;
        let control = private_file(&self.root.join("control.lock"))?;
        fs2::FileExt::lock_exclusive(&control)?;
        Ok(control)
    }

    fn activity(&self) -> Result<File> {
        private_file(&self.root.join("activity.lock"))
    }

    fn holds(&self) -> Result<Vec<String>> {
        let mut result = Vec::new();
        for entry in fs::read_dir(self.root.join("holds"))? {
            let entry = entry?;
            let owner = entry
                .file_name()
                .into_string()
                .map_err(|_| invalid_state())?;
            // Unpublished tempfile bytes cannot establish a hold.
            if owner.starts_with('.') {
                continue;
            }
            validate_owner(&owner).map_err(|_| invalid_state())?;
            let file = private_file_read(&entry.path())?;
            let mut body = String::new();
            file.take(130).read_to_string(&mut body)?;
            if body != format!("{owner}\n") {
                return Err(invalid_state().into());
            }
            result.push(owner);
        }
        result.sort();
        Ok(result)
    }
}

fn validate_owner(owner: &str) -> Result<()> {
    if owner.is_empty()
        || owner.len() > 128
        || owner.starts_with('.')
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        return Err(Error::InvalidOwner);
    }
    Ok(())
}

fn invalid_state() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "unprovable deployment maintenance state",
    )
}

fn map_busy(error: io::Error) -> Error {
    if error.kind() == io::ErrorKind::WouldBlock {
        Error::Busy
    } else {
        Error::Io(error)
    }
}

fn private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(invalid_state().into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt as _;
                builder.mode(0o700);
            }
            builder.create(path)?;
        }
        Err(error) => return Err(error.into()),
    }
    require_private_directory(path)
}

fn require_private_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid_state().into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "maintenance directory must be private",
            )
            .into());
        }
    }
    Ok(())
}

fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    validate_file(&file)?;
    Ok(file)
}

fn private_file_read(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    validate_file(&file)?;
    Ok(file)
}

fn validate_file(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(invalid_state().into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if metadata.nlink() != 1 || metadata.mode() & 0o077 != 0 {
            return Err(invalid_state().into());
        }
    }
    Ok(())
}
