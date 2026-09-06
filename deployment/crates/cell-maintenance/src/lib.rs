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

    /// Admit an installation or isolated canary under its sole matching hold.
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
        let _control = self.control()?;
        self.status_locked()
    }

    fn status_locked(&self) -> Result<Status> {
        let activity = self.activity()?;
        let drained = match fs2::FileExt::try_lock_exclusive(&activity) {
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if fs::symlink_metadata(path)?.permissions().mode() & 0o077 != 0 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_fences_new_work_but_existing_admission_can_drain() -> Result<()> {
        let root = tempfile::tempdir()?;
        let gate = Gate::new(root.path().join("gate"));
        let admitted = gate.enter()?;
        assert!(!gate.hold("run-a")?.drained);
        assert!(matches!(gate.enter(), Err(Error::Held)));
        assert!(matches!(gate.enter_for("run-a"), Err(Error::Busy)));
        drop(admitted);
        assert!(gate.status()?.drained);
        let install = gate.enter_for("run-a")?;
        assert!(!gate.status()?.drained);
        drop(install);
        assert!(gate.release("run-a")?.holds.is_empty());
        let _resumed = gate.enter()?;
        Ok(())
    }

    #[test]
    fn independent_holds_survive_reopen_and_release_only_their_owner() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("gate");
        let gate = Gate::new(&path);
        gate.hold("deployment")?;
        gate.hold("operator")?;
        gate.hold("deployment")?;
        let reopened = Gate::new(path);
        assert!(matches!(reopened.enter_for("deployment"), Err(Error::Held)));
        assert_eq!(reopened.release("deployment")?.holds, ["operator"]);
        assert_eq!(reopened.release("deployment")?.holds, ["operator"]);
        assert!(matches!(reopened.enter(), Err(Error::Held)));
        reopened.release("operator")?;
        let _admission = reopened.enter()?;
        Ok(())
    }

    #[test]
    fn absent_status_is_read_only_and_owner_paths_are_closed() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("gate");
        let gate = Gate::new(&path);
        assert!(gate.status()?.drained);
        assert!(!path.exists());
        for owner in ["", "../escape", ".hidden", "with space", "a/b"] {
            assert!(matches!(gate.hold(owner), Err(Error::InvalidOwner)));
        }
        Ok(())
    }

    #[test]
    fn damaged_hold_fails_closed() -> Result<()> {
        let root = tempfile::tempdir()?;
        let gate = Gate::new(root.path().join("gate"));
        gate.hold("run")?;
        fs::write(gate.path().join("holds/run"), b"wrong-owner\n")?;
        assert!(gate.status().is_err());
        assert!(gate.enter().is_err());
        assert!(gate.recover().is_err());
        assert!(gate.release("run").is_err());
        Ok(())
    }

    #[test]
    fn existing_work_can_recover_while_held_but_never_during_installation() -> Result<()> {
        let root = tempfile::tempdir()?;
        let gate = Gate::new(root.path().join("gate"));
        gate.hold("run")?;
        let recovered = gate.recover()?;
        assert!(!gate.status()?.drained);
        assert!(matches!(gate.enter(), Err(Error::Held)));
        assert!(matches!(gate.enter_for("run"), Err(Error::Busy)));
        drop(recovered);
        let installation = gate.enter_for("run")?;
        assert!(matches!(gate.recover(), Err(Error::Busy)));
        drop(installation);
        assert!(gate.status()?.drained);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_gate_and_linked_hold_are_rejected() -> Result<()> {
        let root = tempfile::tempdir()?;
        let target = root.path().join("target");
        let linked = root.path().join("linked");
        fs::create_dir(&target)?;
        std::os::unix::fs::symlink(&target, &linked)?;
        assert!(Gate::new(linked).hold("run").is_err());
        let gate = Gate::new(root.path().join("gate"));
        gate.hold("run")?;
        fs::hard_link(gate.path().join("holds/run"), root.path().join("alias"))?;
        assert!(gate.enter().is_err());
        assert!(gate.status().is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn database_aliases_share_identity_and_hardlinks_are_refused() -> Result<()> {
        let root = tempfile::tempdir()?;
        let database = root.path().join("data.sqlite");
        fs::write(&database, b"fixture")?;
        let alias = root.path().join("alias.sqlite");
        std::os::unix::fs::symlink(&database, &alias)?;
        assert_eq!(
            canonical_database_path(&database)?,
            canonical_database_path(&alias)?
        );
        let future = root.path().join("future/data.sqlite");
        assert_eq!(
            canonical_database_path(&future)?,
            root.path().canonicalize()?.join("future/data.sqlite")
        );
        assert!(!future.exists());
        fs::hard_link(&database, root.path().join("hardlink.sqlite"))?;
        assert!(canonical_database_path(&database).is_err());
        assert!(canonical_database_path(&alias).is_err());
        Ok(())
    }
}
