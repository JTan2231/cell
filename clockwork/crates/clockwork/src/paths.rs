use std::fs;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::error::{Context as _, Error, Result};

#[derive(Debug, Clone)]
pub(crate) struct Layout {
    state_root: PathBuf,
    logs_root: PathBuf,
    agents_root: PathBuf,
    home: PathBuf,
    overridden: bool,
}

impl Layout {
    pub(crate) fn discover(state_root: Option<PathBuf>) -> Result<Self> {
        if current_uid()? == 0 {
            return Err(Error::new(
                "user_identity_unsupported",
                "Clockwork does not run as root",
            ));
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| Error::new("home_unavailable", "HOME is not set"))?;
        if !home.is_absolute() {
            return Err(Error::new("home_invalid", "HOME must be an absolute path"));
        }
        let home = canonical_existing_directory(&home, "HOME")?;
        if let Some(root) = state_root {
            if !root.is_absolute() {
                return Err(Error::new(
                    "state_root_invalid",
                    "the private state-root override must be absolute",
                ));
            }
            let root = canonical_future_path(&root, "state-root override")?;
            Ok(Self {
                logs_root: root.join("logs"),
                agents_root: root.join("launch-agents"),
                state_root: root,
                home,
                overridden: true,
            })
        } else {
            let state_root = canonical_future_path(
                &home.join("Library/Application Support/Clockwork"),
                "Clockwork state root",
            )?;
            let logs_root =
                canonical_future_path(&home.join("Library/Logs/Clockwork"), "Clockwork logs root")?;
            let agents_root =
                canonical_future_path(&home.join("Library/LaunchAgents"), "LaunchAgents root")?;
            Ok(Self {
                state_root,
                logs_root,
                agents_root,
                home,
                overridden: false,
            })
        }
    }

    #[cfg(test)]
    pub(crate) fn isolated(root: &Path) -> Self {
        let root = root
            .canonicalize()
            .expect("canonicalize isolated layout root");
        Self {
            state_root: root.join("state"),
            logs_root: root.join("logs"),
            agents_root: root.join("agents"),
            home: root.join("home"),
            overridden: true,
        }
    }

    pub(crate) fn prepare(&self) -> Result<()> {
        ensure_private_directory(&self.state_root)?;
        ensure_private_directory(&self.locks_root())?;
        ensure_private_directory(&self.logs_root)?;
        ensure_agents_directory(&self.agents_root)?;
        Ok(())
    }

    pub(crate) fn state_root(&self) -> &Path {
        &self.state_root
    }

    pub(crate) fn logs_root(&self) -> &Path {
        &self.logs_root
    }

    pub(crate) fn agents_root(&self) -> &Path {
        &self.agents_root
    }

    pub(crate) fn database(&self) -> PathBuf {
        self.state_root.join("clockwork.db")
    }

    pub(crate) fn locks_root(&self) -> PathBuf {
        self.state_root.join("locks")
    }

    pub(crate) fn gate_lock_path(&self, key: &str) -> PathBuf {
        self.locks_root()
            .join(format!("{}.gate.lock", key.replace('/', ".")))
    }

    pub(crate) fn management_lock_path(&self, key: &str) -> PathBuf {
        self.locks_root()
            .join(format!("{}.management.lock", key.replace('/', ".")))
    }

    pub(crate) fn activation_lock_path(&self, key: &str) -> PathBuf {
        self.locks_root()
            .join(format!("{}.activation.lock", key.replace('/', ".")))
    }

    pub(crate) fn transition_path(&self, key: &str) -> PathBuf {
        self.locks_root()
            .join(format!("{}.transition.json", key.replace('/', ".")))
    }

    pub(crate) fn label(key: &str) -> String {
        format!("org.clockwork.{}", key.replace('/', "."))
    }

    pub(crate) fn plist_path(&self, key: &str) -> PathBuf {
        self.agents_root.join(format!("{}.plist", Self::label(key)))
    }

    pub(crate) fn stdout_path(&self, key: &str) -> PathBuf {
        self.logs_root
            .join(format!("{}.stdout.log", key.replace('/', ".")))
    }

    pub(crate) fn stderr_path(&self, key: &str) -> PathBuf {
        self.logs_root
            .join(format!("{}.stderr.log", key.replace('/', ".")))
    }

    pub(crate) fn home(&self) -> &Path {
        &self.home
    }

    pub(crate) fn state_root_override(&self) -> Option<&Path> {
        self.overridden.then_some(self.state_root())
    }
}

pub(crate) fn ensure_private_directory(path: &Path) -> Result<()> {
    if path.exists() {
        let metadata = fs::symlink_metadata(path).context(
            "private_directory_unavailable",
            format!("inspect {}", path.display()),
        )?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::new(
                "private_directory_unsafe",
                format!("{} must be a real directory", path.display()),
            ));
        }
    } else {
        fs::create_dir_all(path).context(
            "private_directory_unavailable",
            format!("create {}", path.display()),
        )?;
    }
    require_canonical_directory(path)?;
    let metadata = fs::metadata(path).context(
        "private_directory_unavailable",
        format!("inspect ownership of {}", path.display()),
    )?;
    if metadata.uid() != current_uid()? {
        return Err(Error::new(
            "private_directory_unsafe",
            format!("{} must be owned by the current user", path.display()),
        ));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).context(
        "private_directory_unavailable",
        format!("set private permissions on {}", path.display()),
    )
}

fn ensure_agents_directory(path: &Path) -> Result<()> {
    let created = if path.exists() {
        false
    } else {
        fs::create_dir_all(path).context(
            "private_directory_unavailable",
            format!("create {}", path.display()),
        )?;
        true
    };
    require_canonical_directory(path)?;
    let metadata = fs::symlink_metadata(path).context(
        "private_directory_unavailable",
        format!("inspect {}", path.display()),
    )?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::new(
            "private_directory_unsafe",
            format!("{} must be a real directory", path.display()),
        ));
    }
    if metadata.uid() != current_uid()? || metadata.permissions().mode() & 0o022 != 0 {
        return Err(Error::new(
            "private_directory_unsafe",
            format!(
                "{} must be current-user-owned and not group- or world-writable",
                path.display()
            ),
        ));
    }
    if created {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).context(
            "private_directory_unavailable",
            format!("set private permissions on {}", path.display()),
        )?;
    }
    Ok(())
}

fn canonical_existing_directory(path: &Path, label: &str) -> Result<PathBuf> {
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(Error::new(
            "home_invalid",
            format!("{label} must be an absolute normalized path"),
        ));
    }
    let canonical =
        fs::canonicalize(path).context("home_invalid", format!("canonicalize {label}"))?;
    let metadata = fs::metadata(&canonical).context("home_invalid", format!("inspect {label}"))?;
    if !metadata.is_dir() || metadata.uid() != current_uid()? {
        return Err(Error::new(
            "home_invalid",
            format!("{label} must be a current-user-owned directory"),
        ));
    }
    Ok(canonical)
}

fn canonical_future_path(path: &Path, label: &str) -> Result<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(Error::new(
            "state_root_invalid",
            format!("{label} must be an absolute normalized path"),
        ));
    }
    let mut existing = path;
    let mut missing = Vec::new();
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            Error::new(
                "state_root_invalid",
                format!("{label} has no existing ancestor"),
            )
        })?;
        missing.push(name.to_owned());
        existing = existing.parent().ok_or_else(|| {
            Error::new(
                "state_root_invalid",
                format!("{label} has no existing ancestor"),
            )
        })?;
    }
    let mut canonical = fs::canonicalize(existing)
        .context("state_root_invalid", format!("canonicalize {label}"))?;
    for component in missing.iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

fn require_canonical_directory(path: &Path) -> Result<()> {
    let canonical = fs::canonicalize(path).context(
        "private_directory_unavailable",
        format!("canonicalize {}", path.display()),
    )?;
    if canonical != path {
        return Err(Error::new(
            "private_directory_unsafe",
            format!("{} must not traverse symbolic links", path.display()),
        ));
    }
    Ok(())
}

pub(crate) fn current_uid() -> Result<u32> {
    let output = Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .context("user_identity_unavailable", "run /usr/bin/id -u")?;
    if !output.status.success() {
        return Err(Error::new(
            "user_identity_unavailable",
            "/usr/bin/id -u failed",
        ));
    }
    std::str::from_utf8(&output.stdout)
        .context("user_identity_unavailable", "decode current uid")?
        .trim()
        .parse()
        .context("user_identity_unavailable", "parse current uid")
}
