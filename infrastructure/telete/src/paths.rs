use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub(crate) struct Paths {
    pub(crate) root: PathBuf,
    pub(crate) workspace: PathBuf,
}

#[derive(Deserialize)]
struct Workspace {
    schema_version: u32,
    volume: PathBuf,
    volume_uuid: String,
    directory: String,
}

pub(crate) fn default_root() -> Result<PathBuf> {
    Ok(workspace()?.join("telete"))
}

fn workspace() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is unavailable")?;
    let config_path = PathBuf::from(home).join("Library/Application Support/Cell/workspace.json");
    ensure!(
        !fs::symlink_metadata(&config_path)?.file_type().is_symlink(),
        "symbolic workspace configuration"
    );
    let config: Workspace = serde_json::from_slice(&fs::read(&config_path)?)?;
    ensure!(
        config.schema_version == 1 && config.directory == "cell",
        "unsupported workspace selection"
    );
    ensure!(
        config.volume.is_absolute()
            && !fs::symlink_metadata(&config.volume)?
                .file_type()
                .is_symlink(),
        "work volume is missing or symbolic"
    );
    let plist = Command::new("/usr/sbin/diskutil")
        .args(["info", "-plist"])
        .arg(&config.volume)
        .output()
        .context("external APFS workspace requires macOS diskutil")?;
    ensure!(
        plist.status.success(),
        "cannot inspect selected external volume"
    );
    let mut converter = Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    converter
        .stdin
        .take()
        .context("plutil stdin missing")?
        .write_all(&plist.stdout)?;
    let json = converter.wait_with_output()?;
    ensure!(json.status.success(), "invalid volume observation");
    let info: serde_json::Value = serde_json::from_slice(&json.stdout)?;
    ensure!(
        info["MountPoint"].as_str() == config.volume.to_str()
            && info["Internal"] == false
            && info["FilesystemType"] == "apfs"
            && info["WritableVolume"] == true
            && info["GlobalPermissionsEnabled"] == true
            && info["VolumeUUID"].as_str() == Some(config.volume_uuid.as_str()),
        "external volume identity, APFS or ownership mismatch"
    );
    let base = config.volume.join("cell");
    ensure!(
        base.is_dir()
            && !fs::symlink_metadata(&base)?.file_type().is_symlink()
            && fs::metadata(&base)?.dev() == fs::metadata(&config.volume)?.dev(),
        "external workspace is missing or replaced"
    );
    Ok(base)
}

impl Paths {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let workspace = workspace()?;
        ensure!(
            root.is_absolute() && root.starts_with(&workspace) && root != workspace,
            "Telete state must be inside the selected external workspace"
        );
        ensure!(
            !root
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir)),
            "state path is not normalized"
        );
        let relative = root.strip_prefix(&workspace)?;
        let first = relative
            .components()
            .next()
            .context("empty state path")?
            .as_os_str();
        ensure!(
            first
                .to_str()
                .is_some_and(|v| v == "telete" || v.starts_with("telete-")),
            "state must use a separate telete directory"
        );
        let paths = Self { root, workspace };
        paths.initialize()?;
        Ok(paths)
    }

    fn initialize(&self) -> Result<()> {
        ensure_private(&self.root)?;
        for name in ["jobs", "tmp", "targets", "broker", "deployments", "logs"] {
            ensure_private(&self.root.join(name))?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn for_test(root: PathBuf) -> Result<Self> {
        let paths = Self {
            workspace: root.clone(),
            root,
        };
        paths.initialize()?;
        Ok(paths)
    }

    pub(crate) fn job(&self, id: &str) -> PathBuf {
        self.root.join("jobs").join(id)
    }
    pub(crate) fn targets(&self) -> PathBuf {
        self.root.join("targets")
    }
    pub(crate) fn require_capacity(&self) -> Result<()> {
        #[cfg(not(test))]
        ensure!(
            workspace()? == self.workspace,
            "external workspace selection changed"
        );
        ensure!(
            fs2::available_space(&self.workspace)? >= 2 * 1024 * 1024 * 1024,
            "external workspace has less than 2 GiB free"
        );
        Ok(())
    }
    pub(crate) fn environment(&self) -> BTreeMap<String, String> {
        let temporary = self.root.join("tmp").display().to_string();
        let cache = self.root.join("cache").display().to_string();
        let home = std::env::var("HOME").unwrap_or_default();
        BTreeMap::from([
            (
                "PATH".into(),
                format!("{home}/.cargo/bin:{home}/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin"),
            ),
            ("RUSTUP_HOME".into(), format!("{home}/.rustup")),
            ("TMPDIR".into(), temporary.clone()),
            ("TMP".into(), temporary.clone()),
            ("TEMP".into(), temporary),
            (
                "CARGO_HOME".into(),
                self.workspace.join("cargo").display().to_string(),
            ),
            (
                "CARGO_TARGET_DIR".into(),
                self.targets().display().to_string(),
            ),
            ("XDG_CACHE_HOME".into(), cache.clone()),
            ("CLANG_MODULE_CACHE_PATH".into(), format!("{cache}/clang")),
            ("SWIFT_MODULECACHE_PATH".into(), format!("{cache}/swift")),
            ("CHANCERY_USAGE_INTERNAL".into(), "1".into()),
        ])
    }
}

pub(crate) fn ensure_private(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "private directory must be absolute");
    ensure!(
        !path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir)),
        "private directory must be normalized"
    );
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            ensure!(
                !metadata.file_type().is_symlink(),
                "symbolic private path: {}",
                current.display()
            );
        }
    }
    if !path.exists() {
        fs::create_dir_all(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let metadata = fs::metadata(path)?;
    let owner = fs::metadata(std::env::var_os("HOME").context("HOME unavailable")?)?.uid();
    ensure!(
        metadata.is_dir() && metadata.uid() == owner && metadata.mode() & 0o777 == 0o700,
        "private directory must be user-owned mode 0700: {}",
        path.display()
    );
    Ok(())
}

pub(crate) fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("record has no parent")?;
    ensure_private(parent)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "record is not a regular file"
        );
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub(crate) struct FileLock {
    _file: File,
}

pub(crate) fn lock(path: &Path, blocking: bool) -> Result<FileLock> {
    ensure_private(path.parent().context("lock has no parent")?)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    ensure!(
        file.metadata()?.is_file() && file.metadata()?.mode() & 0o777 == 0o600,
        "lock must be a private regular file"
    );
    if blocking {
        file.lock_exclusive()?;
    } else if let Err(error) = file.try_lock_exclusive() {
        bail!("operation already owned: {}: {error}", path.display());
    }
    // Retain process ownership through child execution. Closing the parent's
    // descriptor does not release an inherited flock while a child holds it.
    rustix::io::fcntl_setfd(&file, rustix::io::FdFlags::empty())?;
    Ok(FileLock { _file: file })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn child_retains_lock_after_parent_descriptor_closes() {
        let directory = tempfile::tempdir().unwrap();
        let private = directory.path().join("private");
        ensure_private(&private).unwrap();
        let path = private.join("owner.lock");
        let owner = lock(&path, false).unwrap();
        let mut child = Command::new("/bin/sleep").arg("1").spawn().unwrap();
        drop(owner);
        assert!(lock(&path, false).is_err());
        assert!(child.wait().unwrap().success());
        assert!(lock(&path, false).is_ok());
    }
}
