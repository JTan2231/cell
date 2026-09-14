use std::fs::{self, Permissions};
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

pub struct ReadOnlyTree(Vec<(PathBuf, Permissions)>);

impl ReadOnlyTree {
    pub fn new(root: &Path) -> io::Result<Self> {
        let mut guard = Self(Vec::new());
        guard.collect(root)?;
        for (path, permissions) in &guard.0 {
            fs::set_permissions(path, Permissions::from_mode(permissions.mode() & !0o222))?;
        }
        Ok(guard)
    }

    fn collect(&mut self, path: &Path) -> io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        self.0.push((path.to_owned(), metadata.permissions()));
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                self.collect(&entry?.path())?;
            }
        }
        Ok(())
    }
}

impl Drop for ReadOnlyTree {
    fn drop(&mut self) {
        for (path, permissions) in &self.0 {
            let _ = fs::set_permissions(path, permissions.clone());
        }
    }
}
