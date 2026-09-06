use crate::{Error, FORMAT, InstallSpec, Installation, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

pub(crate) fn current_uid() -> Result<u32> {
    let output = std::process::Command::new("/usr/bin/id")
        .arg("-u")
        .output()?;
    let uid = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|_| Error::new("cannot establish operator identity"))?;
    if !output.status.success() || uid == 0 {
        return Err(Error::new("installation requires a non-root current user"));
    }
    Ok(uid)
}

#[derive(Clone, Debug)]
pub struct ReleaseInput {
    pub binaries: BTreeMap<String, PathBuf>,
    pub provider_dir: PathBuf,
    pub installer: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub sha256: String,
    pub mode: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub product: String,
    pub provider: String,
    pub versions: BTreeMap<String, String>,
    pub files: BTreeMap<String, FileEntry>,
    pub release_id: String,
}

#[derive(Serialize)]
struct Identity<'a> {
    format: &'a str,
    product: &'a str,
    provider: &'a str,
    versions: &'a BTreeMap<String, String>,
    files: &'a BTreeMap<String, FileEntry>,
}

pub(crate) fn identity(manifest: &Manifest) -> Result<String> {
    Ok(hash_bytes(&serde_json::to_vec(&Identity {
        format: &manifest.format,
        product: &manifest.product,
        provider: &manifest.provider,
        versions: &manifest.versions,
        files: &manifest.files,
    })?))
}

pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn regular(path: &Path) -> Result<fs::Metadata> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.nlink() != 1 {
        return Err(Error::new("artifact is not a regular single-link file"));
    }
    Ok(meta)
}

pub(crate) fn digest(path: &Path) -> Result<String> {
    regular(path)?;
    let mut input = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 65536];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Hash a regular single-link artifact without accepting a symbolic link.
///
/// # Errors
/// Returns an error for inaccessible, symbolic, hard-linked, or special files.
pub fn file_digest(path: &Path) -> Result<String> {
    digest(path)
}

pub(crate) fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

pub(crate) fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

pub(crate) fn validate_spec(spec: &InstallSpec) -> Result<()> {
    if !valid_name(spec.product)
        || !valid_name(spec.provider)
        || spec.commands.is_empty()
        || spec.commands.iter().any(|name| !valid_name(name))
        || spec.commands.iter().collect::<BTreeSet<_>>().len() != spec.commands.len()
        || spec.application.is_empty()
        || spec.application.contains(['/', '\n', '\r'])
        || matches!(spec.application, "." | "..")
    {
        return Err(Error::new("invalid installation specification"));
    }
    Ok(())
}

pub(crate) fn version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|c| c.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

pub(crate) fn directory(path: &Path) -> Result<()> {
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(Error::new("artifact directory is symbolic or invalid"));
    }
    Ok(())
}

pub(crate) fn inventory(root: &Path) -> Result<(BTreeMap<String, FileEntry>, BTreeSet<String>)> {
    directory(root)?;
    let mut files = BTreeMap::new();
    let mut dirs = BTreeSet::new();
    visit(root, root, &mut files, &mut dirs)?;
    Ok((files, dirs))
}

fn visit(
    root: &Path,
    at: &Path,
    files: &mut BTreeMap<String, FileEntry>,
    dirs: &mut BTreeSet<String>,
) -> Result<()> {
    for entry in fs::read_dir(at)? {
        let path = entry?.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| Error::new("invalid artifact path"))?
            .to_str()
            .ok_or_else(|| Error::new("artifact path is not UTF-8"))?
            .to_owned();
        if relative.contains(['\n', '\r', '\\'])
            || path.components().any(|c| matches!(c, Component::ParentDir))
        {
            return Err(Error::new("invalid artifact path"));
        }
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            dirs.insert(relative);
            visit(root, &path, files, dirs)?;
        } else {
            files.insert(
                relative,
                FileEntry {
                    sha256: digest(&path)?,
                    mode: meta.mode() & 0o7777,
                },
            );
        }
    }
    Ok(())
}

fn expected_dirs(files: impl Iterator<Item = String>) -> BTreeSet<String> {
    let mut dirs = BTreeSet::new();
    for file in files {
        let mut at = Path::new(&file).parent();
        while let Some(path) = at {
            if path.as_os_str().is_empty() {
                break;
            }
            dirs.insert(path.to_string_lossy().into_owned());
            at = path.parent();
        }
    }
    dirs
}

pub(crate) fn provider_files(
    root: &Path,
    spec: &InstallSpec,
) -> Result<BTreeMap<String, FileEntry>> {
    let (files, dirs) = inventory(root)?;
    if dirs != BTreeSet::from(["entries".to_owned(), "manuals".to_owned()])
        || !files.contains_key("provider.json")
        || !files.keys().any(|p| p.starts_with("entries/"))
        || !files.keys().any(|p| p.starts_with("manuals/"))
        || files.keys().any(|p| {
            p != "provider.json"
                && !(p.starts_with("entries/")
                    && Path::new(p).extension() == Some(OsStr::new("json")))
                && !(p.starts_with("manuals/")
                    && Path::new(p).extension() == Some(OsStr::new("md")))
        })
    {
        return Err(Error::new("provider bundle has an unsupported inventory"));
    }
    let value: serde_json::Value = serde_json::from_slice(&fs::read(root.join("provider.json"))?)?;
    if value
        .pointer("/provider/id")
        .and_then(serde_json::Value::as_str)
        != Some(spec.provider)
    {
        return Err(Error::new("provider bundle has foreign ownership"));
    }
    Ok(files)
}

/// Validate and hash the complete provider inventory without running a reader.
/// The owning source gate remains responsible for Chancery schema validation.
///
/// # Errors
/// Returns an error for unsafe files, unsupported layout, malformed metadata,
/// foreign provider identity, or inaccessible input.
pub fn provider_inventory(root: &Path, spec: &InstallSpec) -> Result<BTreeMap<String, FileEntry>> {
    provider_files(root, spec)
}

pub(crate) fn provider_version(root: &Path) -> Result<String> {
    let value: serde_json::Value = serde_json::from_slice(&fs::read(root.join("provider.json"))?)?;
    value
        .pointer("/provider/release")
        .and_then(serde_json::Value::as_str)
        .filter(|v| version(v))
        .map(str::to_owned)
        .ok_or_else(|| Error::new("invalid provider release version"))
}

pub(crate) fn copy_file(source: &Path, destination: &Path, mode: u32) -> Result<()> {
    let before = digest(source)?;
    fs::copy(source, destination)?;
    fs::set_permissions(destination, fs::Permissions::from_mode(mode))?;
    if digest(destination)? != before || digest(source)? != before {
        return Err(Error::new("candidate changed during staging"));
    }
    Ok(())
}

pub(crate) fn write_manifest(
    root: &Path,
    spec: &InstallSpec,
    versions: BTreeMap<String, String>,
) -> Result<Manifest> {
    let (files, _) = inventory(root)?;
    let mut manifest = Manifest {
        format: FORMAT.to_owned(),
        product: spec.product.to_owned(),
        provider: spec.provider.to_owned(),
        versions,
        files,
        release_id: String::new(),
    };
    manifest.release_id = identity(&manifest)?;
    fs::write(root.join("manifest.json"), serde_json::to_vec(&manifest)?)?;
    fs::set_permissions(
        root.join("manifest.json"),
        fs::Permissions::from_mode(0o444),
    )?;
    Ok(manifest)
}

/// Verify a complete retained release without executing its contents.
/// The directory's final component must be its exact content identity.
///
/// # Errors
/// Returns an error for unsafe ownership or paths, unknown formats, invalid
/// manifests, unexpected files, or any content or version mismatch.
pub fn verify_release(spec: &InstallSpec, release: &Path) -> Result<Installation> {
    validate_spec(spec)?;
    directory(release)?;
    if !release.is_absolute() || fs::canonicalize(release)? != release {
        return Err(Error::new("release path must be absolute and non-symbolic"));
    }
    verify_tree_ownership(release, current_uid()?)?;
    let id = release
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::new("invalid release path"))?;
    if !valid_hash(id) {
        return Err(Error::new("invalid release identity"));
    }
    if release.join("manifest.json").try_exists()? {
        verify_new(spec, release, id)
    } else {
        verify_legacy(spec, release, id)
    }
}

fn verify_tree_ownership(path: &Path, uid: u32) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.uid() != uid || meta.mode() & 0o022 != 0 || meta.file_type().is_symlink() {
        return Err(Error::new(
            "release artifact ownership or permissions are unsafe",
        ));
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            verify_tree_ownership(&entry?.path(), uid)?;
        }
    } else {
        regular(path)?;
    }
    Ok(())
}

fn verify_new(spec: &InstallSpec, release: &Path, id: &str) -> Result<Installation> {
    let manifest_path = release.join("manifest.json");
    regular(&manifest_path)?;
    let bytes = fs::read(&manifest_path)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    if serde_json::to_vec(&manifest)? != bytes
        || manifest.format != FORMAT
        || manifest.product != spec.product
        || manifest.provider != spec.provider
        || manifest.release_id != id
        || identity(&manifest)? != id
        || manifest
            .versions
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != spec.commands.iter().copied().collect()
        || manifest.versions.values().any(|v| !version(v))
    {
        return Err(Error::new("release manifest identity or format is invalid"));
    }
    let (mut files, dirs) = inventory(release)?;
    files.remove("manifest.json");
    if files != manifest.files || dirs != expected_dirs(manifest.files.keys().cloned()) {
        return Err(Error::new("release inventory differs from its manifest"));
    }
    let prefix = format!("share/chancery/{}/", spec.provider);
    let allowed: BTreeSet<_> = spec
        .commands
        .iter()
        .map(|name| format!("bin/{name}"))
        .chain(["package/install".to_owned()])
        .collect();
    if manifest
        .files
        .keys()
        .any(|p| !allowed.contains(p) && !p.starts_with(&prefix))
        || allowed
            .iter()
            .any(|p| manifest.files.get(p).is_none_or(|f| f.mode != 0o755))
        || manifest
            .files
            .iter()
            .any(|(p, f)| p.starts_with(&prefix) && f.mode != 0o444)
    {
        return Err(Error::new("release has an unsupported artifact layout"));
    }
    let bundle = release.join(format!("share/chancery/{}", spec.provider));
    provider_files(&bundle, spec)?;
    let version = provider_version(&bundle)?;
    if manifest.versions.get(spec.commands[0]) != Some(&version) {
        return Err(Error::new("provider and program version disagree"));
    }
    Ok(Installation {
        current: format!("releases/{id}"),
        release_id: id.to_owned(),
        version,
        format: FORMAT.to_owned(),
        manifest_sha256: digest(&manifest_path)?,
    })
}

fn verify_legacy(spec: &InstallSpec, release: &Path, id: &str) -> Result<Installation> {
    // Only the migrated Usher selector-only layout is a supported predecessor.
    if spec.product != "usher"
        || spec.provider != "usher"
        || spec.commands.first() != Some(&"usher")
    {
        return Err(Error::new("unsupported legacy release format"));
    }
    let (files, dirs) = inventory(release)?;
    let bundle = release.join("share/chancery/usher");
    let provider = provider_files(&bundle, spec)?;
    let mut expected: BTreeSet<String> = ["bin/usher", "package/deploy-user.sh", "manifest.txt"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    expected.extend(provider.keys().map(|p| format!("share/chancery/usher/{p}")));
    if files.keys().cloned().collect::<BTreeSet<_>>() != expected
        || dirs != expected_dirs(expected.into_iter())
        || files["bin/usher"].mode & 0o111 == 0
        || files["package/deploy-user.sh"].mode & 0o111 == 0
    {
        return Err(Error::new("legacy release has an unsupported inventory"));
    }
    let mut bundle_bytes = String::new();
    for (path, entry) in provider {
        let _ = write!(bundle_bytes, "path=./{path}\n{}  ./{path}\n", entry.sha256);
    }
    let bundle_hash = hash_bytes(bundle_bytes.as_bytes());
    let binary_hash = &files["bin/usher"].sha256;
    let installer_hash = &files["package/deploy-user.sh"].sha256;
    let computed =
        hash_bytes(format!("{binary_hash}\n{installer_hash}\n{bundle_hash}\n").as_bytes());
    let version = provider_version(&bundle)?;
    let expected_manifest = format!(
        "format=1\nproduct=usher\nrelease_id={id}\nversion={version}\nbinary_sha256={binary_hash}\ndeployer_sha256={installer_hash}\nchancery_sha256={bundle_hash}\n"
    );
    if computed != id || fs::read(release.join("manifest.txt"))? != expected_manifest.as_bytes() {
        return Err(Error::new(
            "legacy release manifest or content identity is invalid",
        ));
    }
    Ok(Installation {
        current: format!("releases/{id}"),
        release_id: id.to_owned(),
        version,
        format: "legacy-usher-v1".to_owned(),
        manifest_sha256: files["manifest.txt"].sha256.clone(),
    })
}
