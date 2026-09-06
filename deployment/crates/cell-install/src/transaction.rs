//! Explicit file installation transactions. Product lifecycle remains in the caller.

use crate::artifact::{
    copy_file, current_uid, digest, hash_bytes, inventory, valid_hash, verify_tree_ownership,
};
use crate::{Disposition, Error, FileEntry, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Component, Path, PathBuf};

pub const TRANSACTION_FORMAT: &str = "cell-install-v2";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockKind {
    Shlock,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockSpec {
    pub path: PathBuf,
    pub kind: LockKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicKind {
    Symlink,
    Copy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicEntry {
    /// Path relative to the operator home.
    pub path: PathBuf,
    /// File or provider directory relative to the immutable release.
    pub artifact: String,
    pub kind: PublicKind,
    pub mode: u32,
}

#[derive(Clone, Debug)]
pub struct InstallLayout {
    pub product: String,
    pub application: String,
    pub product_lock: LockSpec,
    pub catalog_lock: Option<LockSpec>,
    pub public: Vec<PublicEntry>,
}

#[derive(Clone, Debug)]
pub struct SourceFile {
    pub source: PathBuf,
    pub mode: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSpec {
    pub path: String,
    pub version: String,
}

#[derive(Clone, Debug)]
pub struct ReleasePlan {
    pub files: BTreeMap<String, SourceFile>,
    pub versions: BTreeMap<String, String>,
    pub providers: BTreeMap<String, ProviderSpec>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseInfo {
    pub release_id: String,
    pub format: String,
    pub versions: BTreeMap<String, String>,
    pub files: BTreeMap<String, FileEntry>,
    pub public: Vec<PublicEntry>,
}

#[derive(Clone, Debug)]
pub struct PreparedRelease {
    pub root: PathBuf,
    pub info: ReleaseInfo,
}

pub type LegacyVerifier<'a> = dyn Fn(&Path) -> Result<ReleaseInfo> + 'a;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum CapturedEntry {
    Absent,
    Link(PathBuf),
    Copy(FileEntry),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallSnapshot {
    pub current: Option<ReleaseInfo>,
    pub previous: Option<ReleaseInfo>,
    entries: BTreeMap<PathBuf, CapturedEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SelectionReceipt {
    pub before: InstallSnapshot,
    pub after: InstallSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    product: String,
    versions: BTreeMap<String, String>,
    providers: BTreeMap<String, ProviderSpec>,
    files: BTreeMap<String, FileEntry>,
    public: Vec<PublicEntry>,
    release_id: String,
}

#[derive(Serialize)]
struct Identity<'a> {
    format: &'a str,
    product: &'a str,
    versions: &'a BTreeMap<String, String>,
    providers: &'a BTreeMap<String, ProviderSpec>,
    files: &'a BTreeMap<String, FileEntry>,
    public: &'a [PublicEntry],
}

impl Manifest {
    fn identity(&self) -> Result<String> {
        Ok(hash_bytes(&serde_json::to_vec(&Identity {
            format: &self.format,
            product: &self.product,
            versions: &self.versions,
            providers: &self.providers,
            files: &self.files,
            public: &self.public,
        })?))
    }
    fn info(&self) -> ReleaseInfo {
        ReleaseInfo {
            release_id: self.release_id.clone(),
            format: self.format.clone(),
            versions: self.versions.clone(),
            files: self.files.clone(),
            public: self.public.clone(),
        }
    }
}

fn relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || path.to_str().is_none_or(|p| p.contains(['\n', '\r', '\\']))
    {
        return Err(Error::new(
            "installation path must be a literal relative path",
        ));
    }
    Ok(())
}

fn install_root(layout: &InstallLayout, home: &Path) -> PathBuf {
    home.join("Library/Application Support")
        .join(&layout.application)
        .join("install")
}

fn validate_layout(layout: &InstallLayout, home: &Path) -> Result<u32> {
    if !crate::artifact::valid_name(&layout.product)
        || layout.application.is_empty()
        || layout.application.contains(['/', '\n', '\r'])
        || matches!(layout.application.as_str(), "." | "..")
        || !home.is_absolute()
        || fs::canonicalize(home)? != home
    {
        return Err(Error::new("invalid installation layout or operator home"));
    }
    let uid = current_uid()?;
    safe_directory(home, uid)?;
    relative(&layout.product_lock.path)?;
    if let Some(lock) = &layout.catalog_lock {
        relative(&lock.path)?;
    }
    validate_public(&layout.public)?;
    Ok(uid)
}

fn validate_public(entries: &[PublicEntry]) -> Result<()> {
    let mut paths = BTreeSet::new();
    for entry in entries {
        relative(&entry.path)?;
        relative(Path::new(&entry.artifact))?;
        if !paths.insert(&entry.path) || entry.mode & !0o777 != 0 || entry.mode & 0o022 != 0 {
            return Err(Error::new("invalid or duplicate public installation entry"));
        }
    }
    Ok(())
}

fn safe_directory(path: &Path, uid: u32) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o022 != 0 {
        return Err(Error::new("unsafe installation directory"));
    }
    Ok(())
}

fn ensure_path(home: &Path, relative_path: &Path, uid: u32, create: bool) -> Result<()> {
    relative(relative_path)?;
    let mut path = home.to_owned();
    for component in relative_path.components() {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(_) => safe_directory(&path, uid)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                match fs::create_dir(&path) {
                    Ok(()) => fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e.into()),
                }
                safe_directory(&path, uid)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

enum HeldLock {
    Pid {
        _guard: crate::installation::Lock,
    },
    Directory {
        path: PathBuf,
        device: u64,
        inode: u64,
        owner: Vec<u8>,
    },
}

impl HeldLock {
    fn acquire(home: &Path, spec: &LockSpec, uid: u32) -> Result<Self> {
        relative(&spec.path)?;
        if let Some(parent) = spec.path.parent() {
            ensure_path(home, parent, uid, true)?;
        }
        let path = home.join(&spec.path);
        match spec.kind {
            LockKind::Shlock => Ok(Self::Pid {
                _guard: crate::installation::Lock::acquire(path, uid)?,
            }),
            LockKind::Directory => {
                if let Err(error) = fs::create_dir(&path) {
                    if error.kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(error.into());
                    }
                    reclaim_directory_lock(&path, uid)?;
                    fs::create_dir(&path)?;
                }
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
                let owner = format!("cell-install-lock-v1\n{}\n", std::process::id()).into_bytes();
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path.join(LOCK_OWNER))?;
                file.write_all(&owner)?;
                file.sync_all()?;
                let metadata = fs::symlink_metadata(&path)?;
                Ok(Self::Directory {
                    path,
                    device: metadata.dev(),
                    inode: metadata.ino(),
                    owner,
                })
            }
        }
    }
}

const LOCK_OWNER: &str = ".cell-install-owner";

fn directory_lock_owner(path: &Path, uid: u32) -> Result<(fs::Metadata, Vec<u8>)> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o777 != 0o700 {
        return Err(Error::new("unsafe product directory lock retained"));
    }
    let children = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
    if children.len() != 1 || children[0].file_name() != LOCK_OWNER {
        return Err(Error::new(
            "unrecognized product directory lock requires explicit recovery",
        ));
    }
    let marker = path.join(LOCK_OWNER);
    let info = fs::symlink_metadata(&marker)?;
    if !info.is_file()
        || info.uid() != uid
        || info.mode() & 0o777 != 0o600
        || info.nlink() != 1
        || info.len() > 64
    {
        return Err(Error::new("unsafe product directory lock owner retained"));
    }
    Ok((metadata, fs::read(marker)?))
}

fn reclaim_directory_lock(path: &Path, uid: u32) -> Result<()> {
    let (metadata, owner) = directory_lock_owner(path, uid)?;
    let text =
        std::str::from_utf8(&owner).map_err(|_| Error::new("unknown lock owner retained"))?;
    let pid: u32 = text
        .strip_prefix("cell-install-lock-v1\n")
        .and_then(|value| value.strip_suffix('\n'))
        .and_then(|value| value.parse().ok())
        .filter(|pid| *pid > 0 && i32::try_from(*pid).is_ok())
        .ok_or_else(|| Error::new("unknown lock owner retained"))?;
    let status = std::process::Command::new("/bin/kill")
        .arg("-0")
        .arg(pid.to_string())
        .env("LC_ALL", "C")
        .output()?;
    if status.status.success()
        || !String::from_utf8_lossy(&status.stderr).contains("No such process")
    {
        return Err(Error::new("product directory lock is held"));
    }
    let (again, bytes) = directory_lock_owner(path, uid)?;
    if again.dev() != metadata.dev() || again.ino() != metadata.ino() || bytes != owner {
        return Err(Error::new("product directory lock changed during recovery"));
    }
    fs::remove_file(path.join(LOCK_OWNER))?;
    fs::remove_dir(path)?;
    Ok(())
}

impl Drop for HeldLock {
    fn drop(&mut self) {
        if let Self::Directory {
            path,
            device,
            inode,
            owner,
        } = self
            && let Ok(uid) = current_uid()
            && let Ok((metadata, bytes)) = directory_lock_owner(path, uid)
            && metadata.dev() == *device
            && metadata.ino() == *inode
            && bytes == *owner
        {
            let _ = fs::remove_file(path.join(LOCK_OWNER));
            let _ = fs::remove_dir(path);
        }
    }
}

fn verify_manifest(root: &Path, product: Option<&str>) -> Result<Manifest> {
    if !root.is_absolute() || fs::canonicalize(root)? != root {
        return Err(Error::new("release path must be canonical"));
    }
    verify_tree_ownership(root, current_uid()?)?;
    let bytes = fs::read(root.join("manifest.json"))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    if manifest.format != TRANSACTION_FORMAT
        || product.is_some_and(|p| p != manifest.product)
        || !valid_hash(&manifest.release_id)
        || root.file_name().and_then(|p| p.to_str()) != Some(manifest.release_id.as_str())
        || manifest.identity()? != manifest.release_id
        || serde_json::to_vec(&manifest)? != bytes
    {
        return Err(Error::new(
            "invalid release format, product, or content identity",
        ));
    }
    validate_public(&manifest.public)?;
    let (mut files, dirs) = inventory(root)?;
    files.remove("manifest.json");
    if files != manifest.files {
        return Err(Error::new("release files differ from the sealed inventory"));
    }
    let mut expected_dirs = BTreeSet::new();
    for path in manifest.files.keys() {
        relative(Path::new(path))?;
        for parent in Path::new(path)
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            expected_dirs.insert(parent.to_string_lossy().into_owned());
        }
    }
    if dirs != expected_dirs {
        return Err(Error::new("release has unmanifested directories"));
    }
    for entry in &manifest.public {
        if !manifest.files.contains_key(&entry.artifact) && !dirs.contains(&entry.artifact) {
            return Err(Error::new("public entry names a missing artifact"));
        }
        if entry.kind == PublicKind::Copy
            && manifest
                .files
                .get(&entry.artifact)
                .is_none_or(|file| file.mode != entry.mode)
        {
            return Err(Error::new(
                "copied public entry has an invalid artifact or mode",
            ));
        }
    }
    for (id, provider) in &manifest.providers {
        relative(Path::new(&provider.path))?;
        let data: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(&provider.path).join("provider.json"))?)?;
        if data
            .pointer("/provider/id")
            .and_then(serde_json::Value::as_str)
            != Some(id.as_str())
            || data
                .pointer("/provider/release")
                .and_then(serde_json::Value::as_str)
                != Some(provider.version.as_str())
        {
            return Err(Error::new(
                "provider identity or independent version mismatch",
            ));
        }
    }
    Ok(manifest)
}

/// Read and prove a retained release; legacy proof belongs to its product.
/// # Errors
/// Rejects unsafe paths, incompatible manifests, changed bytes, or rejected legacy proof.
pub fn verify_release_at(
    layout: &InstallLayout,
    root: &Path,
    legacy: &LegacyVerifier<'_>,
) -> Result<ReleaseInfo> {
    if !root.is_absolute() || fs::canonicalize(root)? != root {
        return Err(Error::new("release path must be canonical"));
    }
    verify_tree_ownership(root, current_uid()?)?;
    let result = if root.join("manifest.json").try_exists()? {
        let format: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("manifest.json"))?)?;
        if format.get("format").and_then(serde_json::Value::as_str) == Some(TRANSACTION_FORMAT) {
            let manifest = verify_manifest(root, Some(&layout.product))?;
            if manifest.public != layout.public {
                return Err(Error::new(
                    "release public declarations differ from product layout",
                ));
            }
            manifest.info()
        } else {
            legacy(root)?
        }
    } else {
        legacy(root)?
    };
    if root.file_name().and_then(|name| name.to_str()) != Some(result.release_id.as_str())
        || !valid_hash(&result.release_id)
    {
        return Err(Error::new("legacy proof returned an invalid identity"));
    }
    validate_public(&result.public)?;
    Ok(result)
}

/// Stage explicit payloads and admit an immutable release under the product lock.
/// # Errors
/// Rejects unsafe paths/files, changing inputs, invalid versions/providers, or lock failures.
pub fn prepare_release(
    layout: &InstallLayout,
    home: &Path,
    plan: &ReleasePlan,
) -> Result<PreparedRelease> {
    let uid = validate_layout(layout, home)?;
    let root = install_root(layout, home);
    ensure_path(
        home,
        &root
            .strip_prefix(home)
            .map_err(|_| Error::new("invalid install root"))?
            .join("releases"),
        uid,
        true,
    )?;
    let stage = tempfile::Builder::new()
        .prefix(&format!(
            ".cell-install-stage-{}-{}-",
            layout.product,
            std::process::id()
        ))
        .tempdir_in(root.join("releases"))?;
    for (path, file) in &plan.files {
        relative(Path::new(path))?;
        if path == "manifest.json"
            || file.mode & !0o777 != 0
            || file.mode & 0o022 != 0
            || !file.source.is_absolute()
        {
            return Err(Error::new("invalid release input path or mode"));
        }
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(stage.path().join(parent))?;
        }
        copy_file(&file.source, &stage.path().join(path), file.mode)?;
    }
    let (files, dirs) = inventory(stage.path())?;
    for dir in dirs {
        fs::set_permissions(stage.path().join(dir), fs::Permissions::from_mode(0o755))?;
    }
    let mut manifest = Manifest {
        format: TRANSACTION_FORMAT.to_owned(),
        product: layout.product.clone(),
        versions: plan.versions.clone(),
        providers: plan.providers.clone(),
        files,
        public: layout.public.clone(),
        release_id: String::new(),
    };
    manifest.release_id = manifest.identity()?;
    fs::write(
        stage.path().join("manifest.json"),
        serde_json::to_vec(&manifest)?,
    )?;
    fs::set_permissions(
        stage.path().join("manifest.json"),
        fs::Permissions::from_mode(0o444),
    )?;
    let _lock = HeldLock::acquire(home, &layout.product_lock, uid)?;
    crate::installation::clean_scratch_at(
        &layout.product,
        uid,
        &[root.join("releases")].into_iter().collect(),
        None,
    )?;
    let destination = root.join("releases").join(&manifest.release_id);
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            verify_manifest(&destination, Some(&layout.product))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::rename(stage.path(), &destination)?;
        }
        Err(e) => return Err(e.into()),
    }
    let verified = verify_manifest(&destination, Some(&layout.product))?;
    Ok(PreparedRelease {
        root: destination,
        info: verified.info(),
    })
}

/// Revalidate the exact prepared release without executing any payload.
/// # Errors
/// Returns an error for changed or unsafe prepared bytes or identity.
pub fn verify_prepared(prepared: &PreparedRelease) -> Result<()> {
    if verify_manifest(&prepared.root, None)?.info() != prepared.info {
        return Err(Error::new("prepared release evidence changed"));
    }
    Ok(())
}

fn capture(path: &Path) -> Result<CapturedEntry> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.uid() != current_uid()? => {
            Err(Error::new("foreign public installation entry"))
        }
        Ok(meta) if meta.file_type().is_symlink() => Ok(CapturedEntry::Link(fs::read_link(path)?)),
        Ok(meta) if meta.is_file() && meta.mode() & 0o022 == 0 => {
            Ok(CapturedEntry::Copy(FileEntry {
                sha256: digest(path)?,
                mode: meta.mode() & 0o7777,
            }))
        }
        Ok(_) => Err(Error::new("public path has an unsupported type")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CapturedEntry::Absent),
        Err(e) => Err(e.into()),
    }
}

fn selected(
    layout: &InstallLayout,
    home: &Path,
    name: &str,
    legacy: &LegacyVerifier<'_>,
) -> Result<Option<ReleaseInfo>> {
    match capture(&install_root(layout, home).join(name))? {
        CapturedEntry::Absent => Ok(None),
        CapturedEntry::Link(target) => {
            let text = target
                .to_str()
                .ok_or_else(|| Error::new("invalid selection"))?;
            let id = text
                .strip_prefix("releases/")
                .filter(|id| valid_hash(id))
                .ok_or_else(|| Error::new("invalid selected release"))?;
            Ok(Some(verify_release_at(
                layout,
                &install_root(layout, home).join("releases").join(id),
                legacy,
            )?))
        }
        CapturedEntry::Copy(_) => Err(Error::new("release selector is not a symbolic link")),
    }
}

fn expected_entry(
    layout: &InstallLayout,
    home: &Path,
    info: &ReleaseInfo,
    entry: &PublicEntry,
) -> Result<CapturedEntry> {
    match entry.kind {
        PublicKind::Symlink => Ok(CapturedEntry::Link(
            install_root(layout, home)
                .join("current")
                .join(&entry.artifact),
        )),
        PublicKind::Copy => Ok(CapturedEntry::Copy(
            info.files
                .get(&entry.artifact)
                .filter(|file| file.mode == entry.mode)
                .cloned()
                .ok_or_else(|| Error::new("copied artifact lacks exact release proof"))?,
        )),
    }
}

fn snapshot(
    layout: &InstallLayout,
    home: &Path,
    legacy: &LegacyVerifier<'_>,
    strict: bool,
) -> Result<InstallSnapshot> {
    let uid = validate_layout(layout, home)?;
    let root = install_root(layout, home);
    ensure_path(
        home,
        root.strip_prefix(home)
            .map_err(|_| Error::new("invalid installation path"))?,
        uid,
        false,
    )?;
    let current = selected(layout, home, "current", legacy)?;
    let previous = selected(layout, home, "previous", legacy)?;
    let mut public = BTreeMap::new();
    for entry in layout
        .public
        .iter()
        .chain(current.iter().flat_map(|r| &r.public))
        .chain(previous.iter().flat_map(|r| &r.public))
    {
        public.insert(entry.path.clone(), entry);
    }
    let mut entries = BTreeMap::new();
    for path in public.keys() {
        if let Some(parent) = path.parent() {
            ensure_path(home, parent, uid, false)?;
        }
        let actual = capture(&home.join(path))?;
        let required = current
            .as_ref()
            .and_then(|r| r.public.iter().find(|e| &e.path == path));
        let expected = if let (Some(info), Some(entry)) = (&current, required) {
            expected_entry(layout, home, info, entry)?
        } else {
            CapturedEntry::Absent
        };
        if strict && actual != expected {
            return Err(Error::new(
                "foreign or incoherent public installation entry",
            ));
        }
        entries.insert(path.clone(), actual);
    }
    if strict && current.is_none() && previous.is_some() {
        return Err(Error::new("previous selector exists without current"));
    }
    Ok(InstallSnapshot {
        current,
        previous,
        entries,
    })
}

/// Inspect complete owned installation state without changing or executing it.
/// # Errors
/// Rejects foreign/incoherent public paths, unsafe directories, or invalid releases.
pub fn inspect_installation(
    layout: &InstallLayout,
    home: &Path,
    legacy: &LegacyVerifier<'_>,
) -> Result<InstallSnapshot> {
    snapshot(layout, home, legacy, true)
}

/// Inspect selected releases while allowing their owned public entries to be absent.
/// The product decides whether its runtime state permits repair or reinstallation.
/// # Errors
/// Rejects unsafe releases, foreign public paths, or entries other than absent/exact selected bytes.
pub fn inspect_detached_installation(
    layout: &InstallLayout,
    home: &Path,
    legacy: &LegacyVerifier<'_>,
) -> Result<InstallSnapshot> {
    let state = snapshot(layout, home, legacy, false)?;
    for (path, value) in &state.entries {
        if *value == CapturedEntry::Absent {
            continue;
        }
        let current = state
            .current
            .as_ref()
            .ok_or_else(|| Error::new("public entry has no selected release"))?;
        let entry = current
            .public
            .iter()
            .find(|entry| &entry.path == path)
            .ok_or_else(|| Error::new("public entry is not owned by selected release"))?;
        if *value != expected_entry(layout, home, current, entry)? {
            return Err(Error::new("foreign public installation entry"));
        }
    }
    Ok(state)
}

pub struct InstallTransaction<'a> {
    layout: InstallLayout,
    home: PathBuf,
    legacy: &'a LegacyVerifier<'a>,
    uid: u32,
    _product_lock: HeldLock,
}

/// Hold the product's exact existing installation lock across its lifecycle.
/// # Errors
/// Rejects unsafe paths, invalid layout, or an unavailable product lock.
pub fn lock_installation<'a>(
    layout: &InstallLayout,
    home: &Path,
    legacy: &'a LegacyVerifier<'a>,
) -> Result<InstallTransaction<'a>> {
    let uid = validate_layout(layout, home)?;
    let lock = HeldLock::acquire(home, &layout.product_lock, uid)?;
    Ok(InstallTransaction {
        layout: layout.clone(),
        home: home.to_owned(),
        legacy,
        uid,
        _product_lock: lock,
    })
}

impl InstallTransaction<'_> {
    /// Check captured state again while holding the product lock.
    /// # Errors
    /// Returns an error for changed content, selectors, or captured public paths.
    pub fn recheck(&self, expected: &InstallSnapshot) -> Result<()> {
        if snapshot(&self.layout, &self.home, self.legacy, false)? != *expected {
            return Err(Error::new("stale installation snapshot"));
        }
        Ok(())
    }

    fn catalog(&self) -> Result<Option<HeldLock>> {
        self.layout
            .catalog_lock
            .as_ref()
            .map(|spec| HeldLock::acquire(&self.home, spec, self.uid))
            .transpose()
    }

    /// Remove owned public views before product-owned migration; keep release selectors.
    /// # Errors
    /// Refuses stale snapshots or failed detachment, with compensation disposition.
    pub fn suspend(
        &mut self,
        expected: &InstallSnapshot,
        paths: &[PathBuf],
    ) -> Result<SelectionReceipt> {
        let _catalog = self.catalog()?;
        self.recheck(expected)?;
        let mut after = expected.clone();
        for path in paths {
            *after
                .entries
                .get_mut(path)
                .ok_or_else(|| Error::new("suspension path is not a captured public entry"))? =
                CapturedEntry::Absent;
        }
        self.change(expected, &after, || Ok(()))
    }

    /// Detach all owned public and release selectors; retain releases and runtime state.
    /// # Errors
    /// Refuses stale snapshots or failed detachment, with compensation disposition.
    pub fn detach(&mut self, expected: &InstallSnapshot) -> Result<SelectionReceipt> {
        let _catalog = self.catalog()?;
        self.recheck(expected)?;
        let mut after = expected.clone();
        after.current = None;
        after.previous = None;
        for value in after.entries.values_mut() {
            *value = CapturedEntry::Absent;
        }
        self.change(expected, &after, || Ok(()))
    }

    /// Publish then verify while holding the catalog lock. Verification must not migrate state.
    /// # Errors
    /// Failed file publication or verification compensates the captured selector/file view.
    pub fn publish(
        &mut self,
        prepared: &PreparedRelease,
        expected: &InstallSnapshot,
        verify: impl FnOnce(&ReleaseInfo) -> Result<()>,
    ) -> Result<SelectionReceipt> {
        if verify_release_at(&self.layout, &prepared.root, self.legacy)? != prepared.info {
            return Err(Error::new("prepared or retained release evidence changed"));
        }
        if prepared.root
            != install_root(&self.layout, &self.home)
                .join("releases")
                .join(&prepared.info.release_id)
        {
            return Err(Error::new(
                "prepared release belongs to another installation",
            ));
        }
        let _catalog = self.catalog()?;
        self.recheck(expected)?;
        let after = self.publication(expected, &prepared.info)?;
        self.change(expected, &after, || verify(&prepared.info))
    }

    fn publication(
        &self,
        expected: &InstallSnapshot,
        info: &ReleaseInfo,
    ) -> Result<InstallSnapshot> {
        let mut after = expected.clone();
        if after.current.as_ref() != Some(info) {
            after.previous.clone_from(&after.current);
        }
        after.current = Some(info.clone());
        for value in after.entries.values_mut() {
            *value = CapturedEntry::Absent;
        }
        for entry in &info.public {
            after.entries.insert(
                entry.path.clone(),
                expected_entry(&self.layout, &self.home, info, entry)?,
            );
        }
        Ok(after)
    }

    /// Repair an interrupted publication using captured prior and exact candidate evidence.
    /// The product explicitly chooses the compatible program selection after its state recovery.
    /// # Errors
    /// Rejects unproved releases or paths outside the prior/candidate/absent views.
    pub fn recover(
        &mut self,
        prior: &InstallSnapshot,
        candidate: &PreparedRelease,
        select_candidate: bool,
        verify: impl FnOnce(&ReleaseInfo) -> Result<()>,
    ) -> Result<SelectionReceipt> {
        if verify_release_at(&self.layout, &candidate.root, self.legacy)? != candidate.info
            || candidate.root
                != install_root(&self.layout, &self.home)
                    .join("releases")
                    .join(&candidate.info.release_id)
        {
            return Err(Error::new("recovery candidate evidence changed"));
        }
        for info in prior.current.iter().chain(&prior.previous) {
            if verify_release_at(
                &self.layout,
                &install_root(&self.layout, &self.home)
                    .join("releases")
                    .join(&info.release_id),
                self.legacy,
            )? != *info
            {
                return Err(Error::new("recovery prior evidence changed"));
            }
        }
        let target = self.publication(prior, &candidate.info)?;
        let _catalog = self.catalog()?;
        let actual = snapshot(&self.layout, &self.home, self.legacy, false)?;
        if actual.current.is_some()
            && actual.current != prior.current
            && actual.current != target.current
            || actual.previous.is_some()
                && actual.previous != prior.previous
                && actual.previous != target.previous
        {
            return Err(Error::new("unattributable recovery release selection"));
        }
        for (path, value) in &actual.entries {
            if *value != CapturedEntry::Absent
                && Some(value) != prior.entries.get(path)
                && Some(value) != target.entries.get(path)
            {
                return Err(Error::new("unattributable recovery public path"));
            }
        }
        let selected = if select_candidate { &target } else { prior };
        self.change(&actual, selected, || {
            if let Some(info) = &selected.current {
                verify(info)?;
            }
            Ok(())
        })
    }

    /// Restore a prior file selection only after the product proves lifecycle compatibility.
    /// # Errors
    /// Refuses changed selections or unproved prior bytes and reports restoration uncertainty.
    pub fn restore(
        &mut self,
        receipt: &SelectionReceipt,
        verify: impl FnOnce(&ReleaseInfo) -> Result<()>,
    ) -> Result<()> {
        let _catalog = self.catalog()?;
        self.recheck(&receipt.after)?;
        self.change(&receipt.after, &receipt.before, || {
            if let Some(prior) = &receipt.before.current {
                verify(prior)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    fn change(
        &self,
        before: &InstallSnapshot,
        after: &InstallSnapshot,
        verify: impl FnOnce() -> Result<()>,
    ) -> Result<SelectionReceipt> {
        self.clean_scratch(before, after)?;
        let result = self
            .write_view(after)
            .and_then(|()| self.recheck(after))
            .and_then(|()| verify());
        if let Err(mut error) = result {
            error.disposition = if self.attributable(before, after).is_ok()
                && self
                    .write_view(before)
                    .and_then(|()| self.recheck(before))
                    .is_ok()
            {
                Disposition::Restored
            } else {
                Disposition::Uncertain
            };
            return Err(error);
        }
        Ok(SelectionReceipt {
            before: before.clone(),
            after: after.clone(),
        })
    }

    fn clean_scratch(&self, before: &InstallSnapshot, after: &InstallSnapshot) -> Result<()> {
        let root = install_root(&self.layout, &self.home);
        let mut parents = BTreeSet::from([root.clone()]);
        let mut targets = BTreeSet::new();
        for snapshot in [before, after] {
            for (path, value) in &snapshot.entries {
                if let Some(parent) = path.parent() {
                    parents.insert(self.home.join(parent));
                }
                if let CapturedEntry::Link(target) = value {
                    targets.insert(target.clone());
                }
            }
        }
        crate::installation::clean_scratch_at(
            &self.layout.product,
            self.uid,
            &parents,
            Some(&targets),
        )?;
        crate::installation::clean_scratch_at(
            &self.layout.product,
            self.uid,
            &BTreeSet::from([root.join("releases")]),
            None,
        )
    }

    fn attributable(&self, before: &InstallSnapshot, after: &InstallSnapshot) -> Result<()> {
        let current = snapshot(&self.layout, &self.home, self.legacy, false)?;
        if current.current != before.current && current.current != after.current
            || current.previous != before.previous && current.previous != after.previous
        {
            return Err(Error::new("unattributable release selection"));
        }
        for (path, value) in current.entries {
            if Some(&value) != before.entries.get(&path) && Some(&value) != after.entries.get(&path)
            {
                return Err(Error::new("unattributable public path"));
            }
        }
        Ok(())
    }

    fn write_view(&self, target: &InstallSnapshot) -> Result<()> {
        let root = install_root(&self.layout, &self.home);
        ensure_path(
            &self.home,
            root.strip_prefix(&self.home)
                .map_err(|_| Error::new("invalid installation root"))?,
            self.uid,
            true,
        )?;
        for (path, value) in &target.entries {
            if let Some(parent) = path.parent() {
                ensure_path(&self.home, parent, self.uid, true)?;
            }
            let destination = self.home.join(path);
            match value {
                CapturedEntry::Absent => remove_owned(&destination)?,
                CapturedEntry::Link(link) => atomic_link(link, &destination, &self.layout.product)?,
                CapturedEntry::Copy(proof) => {
                    let selected = target
                        .current
                        .as_ref()
                        .ok_or_else(|| Error::new("copied public path has no selected release"))?;
                    let entry = selected
                        .public
                        .iter()
                        .find(|e| &e.path == path && e.kind == PublicKind::Copy)
                        .ok_or_else(|| Error::new("missing copied path declaration"))?;
                    let source = root
                        .join("releases")
                        .join(&selected.release_id)
                        .join(&entry.artifact);
                    if digest(&source)? != proof.sha256 {
                        return Err(Error::new("prior copied artifact changed"));
                    }
                    let temporary = tempfile::Builder::new()
                        .prefix(&format!(
                            ".cell-install-selector-{}-{}-",
                            self.layout.product,
                            std::process::id()
                        ))
                        .tempdir_in(
                            destination
                                .parent()
                                .ok_or_else(|| Error::new("missing public parent"))?,
                        )?;
                    let file = temporary.path().join("copy");
                    copy_file(&source, &file, proof.mode)?;
                    fs::rename(&file, &destination)?;
                }
            }
        }
        for (name, value) in [("previous", &target.previous), ("current", &target.current)] {
            if let Some(info) = value {
                atomic_link(
                    &PathBuf::from(format!("releases/{}", info.release_id)),
                    &root.join(name),
                    &self.layout.product,
                )?;
            } else {
                remove_owned(&root.join(name))?;
            }
        }
        Ok(())
    }
}

fn remove_owned(path: &Path) -> Result<()> {
    if capture(path)? != CapturedEntry::Absent {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn atomic_link(target: &Path, destination: &Path, product: &str) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| Error::new("invalid selector parent"))?;
    let temporary = tempfile::Builder::new()
        .prefix(&format!(
            ".cell-install-selector-{product}-{}-",
            std::process::id()
        ))
        .tempdir_in(parent)?;
    let link = temporary.path().join("link");
    symlink(target, &link)?;
    fs::rename(link, destination)?;
    Ok(())
}

#[cfg(test)]
mod tests;
