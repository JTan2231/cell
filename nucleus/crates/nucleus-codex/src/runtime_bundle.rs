//! Staging and integrity checks for the complete supported Codex runtime.

use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use fs2::FileExt as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{CodexError, SUPPORTED_CODEX_VERSION};

pub const RUNTIME_MANIFEST: &str = "nucleus-runtime.json";
const CODEX: &str = "codex";
const CODE_MODE_HOST: &str = "codex-code-mode-host";
const MANIFEST_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// The exact files selected together by the operator. The manifest establishes
/// file identity; the adapter's execution test establishes pair compatibility.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RuntimeBundle {
    pub executable: PathBuf,
    pub code_mode_host: PathBuf,
    pub version: String,
    pub codex_sha256: String,
    pub code_mode_host_sha256: String,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    codex_version: String,
    codex_sha256: String,
    code_mode_host_sha256: String,
}

impl RuntimeBundle {
    fn manifest(&self) -> Manifest {
        Manifest {
            schema_version: MANIFEST_VERSION,
            codex_version: self.version.clone(),
            codex_sha256: self.codex_sha256.clone(),
            code_mode_host_sha256: self.code_mode_host_sha256.clone(),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct FileStamp {
    device: u64,
    inode: u64,
    length: u64,
    mode: u32,
    modified: (i64, i64),
    changed: (i64, i64),
}

fn file_stamp(path: &Path, executable: bool) -> Result<FileStamp, CodexError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| failure(path, error))?;
    if !metadata.is_file() {
        return Err(failure(path, "must be a regular file, not a symlink"));
    }
    if executable && metadata.mode() & 0o111 == 0 {
        return Err(failure(path, "must be executable"));
    }
    Ok(FileStamp {
        device: metadata.dev(),
        inode: metadata.ino(),
        length: metadata.len(),
        mode: metadata.mode(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
    })
}

#[derive(Debug, Eq, PartialEq)]
struct BundleStamp {
    executable: PathBuf,
    codex: FileStamp,
    host: FileStamp,
    manifest: FileStamp,
}

impl BundleStamp {
    fn read(executable: &Path) -> Result<Self, CodexError> {
        let executable = resolve_executable(executable)?;
        let directory = runtime_directory(&executable)?;
        Ok(Self {
            codex: file_stamp(&executable, true)?,
            host: file_stamp(&directory.join(CODE_MODE_HOST), true)?,
            manifest: file_stamp(&directory.join(RUNTIME_MANIFEST), false)?,
            executable,
        })
    }
}

// Retain only one successful observation. Every lookup rechecks the file
// identities and metadata, including ctime, before avoiding the large hashes.
// This detects accidental replacement; it is not a same-user security boundary.
static VERIFIED: OnceLock<Mutex<Option<(BundleStamp, RuntimeBundle)>>> = OnceLock::new();

/// Verify the manifest's supported version and both executable digests.
/// A symlink to `codex` is resolved before locating its sibling runtime files.
/// This does not execute Codex; harness inspection separately checks its version.
///
/// # Errors
///
/// Returns an inspection error for incomplete, changed, or unsupported bundles.
pub fn verify_runtime(executable: &Path) -> Result<RuntimeBundle, CodexError> {
    let stamp = BundleStamp::read(executable)?;
    let cache = VERIFIED.get_or_init(|| Mutex::new(None));
    if let Some((previous, bundle)) = &*cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        && previous == &stamp
    {
        return Ok(bundle.clone());
    }
    let directory = runtime_directory(&stamp.executable)?;
    let manifest_path = directory.join(RUNTIME_MANIFEST);
    if stamp.manifest.length > MAX_MANIFEST_BYTES {
        return Err(failure(&manifest_path, "manifest is too large"));
    }
    let bytes = fs::read(&manifest_path).map_err(|error| failure(&manifest_path, error))?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|error| failure(&manifest_path, error))?;
    if manifest.schema_version != MANIFEST_VERSION {
        return Err(failure(
            &manifest_path,
            "unsupported manifest schema version",
        ));
    }
    if manifest.codex_version != SUPPORTED_CODEX_VERSION {
        return Err(failure(&manifest_path, "unsupported Codex version"));
    }
    let bundle = inspect_pair(&stamp.executable, false)?;
    if bundle.manifest() != manifest {
        return Err(failure(
            &manifest_path,
            "runtime digests or version do not match the manifest",
        ));
    }
    if BundleStamp::read(&stamp.executable)? != stamp {
        return Err(failure(
            &stamp.executable,
            "runtime changed during verification",
        ));
    }
    *cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((stamp, bundle.clone()));
    Ok(bundle)
}

/// Copy one selected Codex executable and its sibling helper into a new sealed
/// directory. Publish the complete directory with one rename. An existing
/// destination is accepted only when its verified files are identical.
///
/// # Errors
///
/// Returns an inspection error if the source pair is incomplete or unsupported,
/// staging fails, or the destination would replace another installation.
pub fn stage_runtime(source: &Path, destination: &Path) -> Result<RuntimeBundle, CodexError> {
    let source = resolve_executable(source)?;
    let source_bundle = inspect_pair(&source, true)?;
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = destination
        .file_name()
        .ok_or_else(|| failure(destination, "must name a directory"))?;
    fs::create_dir_all(parent).map_err(|error| failure(parent, error))?;
    let parent = fs::canonicalize(parent).map_err(|error| failure(parent, error))?;
    let destination = parent.join(name);
    let mut lock_name = std::ffi::OsString::from(".");
    lock_name.push(name);
    lock_name.push(".stage.lock");
    let lock_path = parent.join(lock_name);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .map_err(|error| failure(&lock_path, error))?;
    lock.lock_exclusive()
        .map_err(|error| failure(&lock_path, error))?;
    match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(failure(
                    &destination,
                    "destination must be a directory, not a symlink",
                ));
            }
            let existing = verify_runtime(&destination.join(CODEX))?;
            if existing.manifest() != source_bundle.manifest() {
                return Err(failure(
                    &destination,
                    "refusing to replace a different runtime",
                ));
            }
            return Ok(existing);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(failure(&destination, error)),
    }
    let staging = tempfile::Builder::new()
        .prefix(".codex-runtime-")
        .tempdir_in(&parent)
        .map_err(|error| failure(&parent, error))?;
    for (source, name) in [
        (&source_bundle.executable, CODEX),
        (&source_bundle.code_mode_host, CODE_MODE_HOST),
    ] {
        let target = staging.path().join(name);
        fs::copy(source, &target).map_err(|error| failure(&target, error))?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o555))
            .map_err(|error| failure(&target, error))?;
        File::open(&target)
            .and_then(|file| file.sync_all())
            .map_err(|error| failure(&target, error))?;
    }
    let manifest_path = staging.path().join(RUNTIME_MANIFEST);
    let bytes = serde_json::to_vec_pretty(&source_bundle.manifest())
        .map_err(|error| failure(&manifest_path, error))?;
    let mut manifest = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o444)
        .open(&manifest_path)
        .map_err(|error| failure(&manifest_path, error))?;
    manifest
        .write_all(&bytes)
        .and_then(|()| manifest.sync_all())
        .map_err(|error| failure(&manifest_path, error))?;
    verify_runtime(&staging.path().join(CODEX))?;
    File::open(staging.path())
        .and_then(|file| file.sync_all())
        .map_err(|error| failure(staging.path(), error))?;
    fs::set_permissions(staging.path(), fs::Permissions::from_mode(0o555))
        .map_err(|error| failure(staging.path(), error))?;
    if let Err(error) = fs::rename(staging.path(), &destination) {
        let _ = fs::set_permissions(staging.path(), fs::Permissions::from_mode(0o700));
        return Err(failure(&destination, error));
    }
    File::open(&parent)
        .and_then(|file| file.sync_all())
        .map_err(|error| failure(&parent, error))?;
    verify_runtime(&destination.join(CODEX))
}

fn resolve_executable(executable: &Path) -> Result<PathBuf, CodexError> {
    let resolved = fs::canonicalize(executable).map_err(|error| failure(executable, error))?;
    if resolved.file_name().is_none_or(|name| name != CODEX) {
        return Err(failure(&resolved, "runtime executable must be named codex"));
    }
    file_stamp(&resolved, true)?;
    Ok(resolved)
}

fn runtime_directory(executable: &Path) -> Result<&Path, CodexError> {
    executable
        .parent()
        .ok_or_else(|| failure(executable, "executable has no parent directory"))
}

fn inspect_pair(executable: &Path, check_version: bool) -> Result<RuntimeBundle, CodexError> {
    let host = runtime_directory(executable)?.join(CODE_MODE_HOST);
    let codex_before = file_stamp(executable, true)?;
    let host_before = file_stamp(&host, true)?;
    if check_version {
        check_source_version(executable, Duration::from_secs(30))?;
    }
    let bundle = RuntimeBundle {
        executable: executable.to_path_buf(),
        code_mode_host: host.clone(),
        version: SUPPORTED_CODEX_VERSION.to_owned(),
        codex_sha256: sha256(executable)?,
        code_mode_host_sha256: sha256(&host)?,
    };
    if codex_before != file_stamp(executable, true)? || host_before != file_stamp(&host, true)? {
        return Err(failure(
            executable,
            "source runtime changed during inspection",
        ));
    }
    Ok(bundle)
}

fn check_source_version(executable: &Path, timeout: Duration) -> Result<(), CodexError> {
    // A file avoids a full pipe blocking the child before timeout/reaping.
    let mut output = tempfile::tempfile().map_err(|error| failure(executable, error))?;
    let stdout = output
        .try_clone()
        .map_err(|error| failure(executable, error))?;
    let mut child = Command::new(executable)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| failure(executable, error))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failure(
                    executable,
                    match result {
                        Err(error) => error.to_string(),
                        _ => "codex --version exceeded its timeout".to_owned(),
                    },
                ));
            }
        }
    };
    output
        .seek(SeekFrom::Start(0))
        .map_err(|error| failure(executable, error))?;
    let mut bytes = Vec::new();
    output
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|error| failure(executable, error))?;
    if bytes.len() == 4097 {
        return Err(failure(executable, "codex --version output is too large"));
    }
    let line = String::from_utf8_lossy(&bytes);
    let version = line
        .trim()
        .strip_prefix("codex-cli ")
        .unwrap_or(line.trim());
    if !status.success() || version != SUPPORTED_CODEX_VERSION {
        return Err(failure(
            executable,
            format!("requires Codex {SUPPORTED_CODEX_VERSION}, received {version:?}"),
        ));
    }
    Ok(())
}

fn sha256(path: &Path) -> Result<String, CodexError> {
    let mut file = File::open(path).map_err(|error| failure(path, error))?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 8 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| failure(path, error))?;
        if read == 0 {
            return Ok(format!("{:x}", digest.finalize()));
        }
        digest.update(&buffer[..read]);
    }
}

fn failure(path: &Path, detail: impl std::fmt::Display) -> CodexError {
    CodexError::Inspection(format!("Codex runtime {}: {detail}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn source(root: &Path) -> TestResult<PathBuf> {
        fs::create_dir_all(root)?;
        let executable = root.join(CODEX);
        fs::write(
            &executable,
            format!("#!/bin/sh\nprintf 'codex-cli {SUPPORTED_CODEX_VERSION}\\n'\n"),
        )?;
        fs::write(root.join(CODE_MODE_HOST), "#!/bin/sh\nexit 0\n")?;
        for path in [&executable, &root.join(CODE_MODE_HOST)] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
        }
        Ok(executable)
    }

    fn stage(source: &Path, destination: &Path) -> TestResult<RuntimeBundle> {
        let bundle = stage_runtime(source, destination)?;
        // Allow TempDir to remove the sealed fixture after each test.
        fs::set_permissions(destination, fs::Permissions::from_mode(0o700))?;
        Ok(bundle)
    }

    fn assert_error<T>(result: Result<T, CodexError>, expected: &str) {
        match result {
            Ok(_) => panic!("expected an error containing {expected:?}"),
            Err(error) => assert!(error.to_string().contains(expected), "{error}"),
        }
    }

    #[test]
    fn stages_both_executables_and_resolves_symlink_for_verification() -> TestResult {
        let temp = tempfile::tempdir()?;
        let source = source(&temp.path().join("source"))?;
        let source_link = temp.path().join("source-link");
        symlink(&source, &source_link)?;
        let destination = temp.path().join("runtime");
        let bundle = stage(&source_link, &destination)?;
        assert_eq!(stage_runtime(&source, &destination)?, bundle);
        let alias = temp.path().join("selected-codex");
        symlink(&bundle.executable, &alias)?;
        assert_eq!(verify_runtime(&alias)?, bundle);
        assert!(destination.join(RUNTIME_MANIFEST).is_file());
        assert_eq!(fs::metadata(&bundle.code_mode_host)?.mode() & 0o777, 0o555);
        Ok(())
    }

    #[test]
    fn rejects_incomplete_source_without_publishing_destination() -> TestResult {
        let temp = tempfile::tempdir()?;
        let source = source(&temp.path().join("source"))?;
        fs::remove_file(source.with_file_name(CODE_MODE_HOST))?;
        let destination = temp.path().join("runtime");
        assert!(stage_runtime(&source, &destination).is_err());
        assert!(!destination.exists());
        Ok(())
    }

    #[test]
    fn rejects_changed_helper_even_after_cached_verification() -> TestResult {
        let temp = tempfile::tempdir()?;
        let source = source(&temp.path().join("source"))?;
        let bundle = stage(&source, &temp.path().join("runtime"))?;
        verify_runtime(&bundle.executable)?;
        fs::set_permissions(&bundle.code_mode_host, fs::Permissions::from_mode(0o755))?;
        fs::write(&bundle.code_mode_host, "#!/bin/sh\nexit 1\n")?;
        assert_error(verify_runtime(&bundle.executable), "digests");
        Ok(())
    }

    #[test]
    fn rejects_missing_non_executable_or_symlinked_helper() -> TestResult {
        let temp = tempfile::tempdir()?;
        let source = source(&temp.path().join("source"))?;
        let bundle = stage(&source, &temp.path().join("runtime"))?;
        fs::set_permissions(&bundle.code_mode_host, fs::Permissions::from_mode(0o444))?;
        assert!(verify_runtime(&bundle.executable).is_err());
        fs::remove_file(&bundle.code_mode_host)?;
        assert!(verify_runtime(&bundle.executable).is_err());
        symlink(
            source.with_file_name(CODE_MODE_HOST),
            &bundle.code_mode_host,
        )?;
        assert!(verify_runtime(&bundle.executable).is_err());
        Ok(())
    }

    #[test]
    fn rejects_manifest_version_and_refuses_existing_different_bundle() -> TestResult {
        let temp = tempfile::tempdir()?;
        let source = source(&temp.path().join("source"))?;
        let destination = temp.path().join("runtime");
        let bundle = stage(&source, &destination)?;
        fs::write(source.with_file_name(CODE_MODE_HOST), "different helper")?;
        assert_error(stage_runtime(&source, &destination), "refusing to replace");
        assert_eq!(verify_runtime(&bundle.executable)?, bundle);
        let manifest_path = destination.join(RUNTIME_MANIFEST);
        fs::set_permissions(&manifest_path, fs::Permissions::from_mode(0o644))?;
        let mut manifest = bundle.manifest();
        manifest.schema_version += 1;
        fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;
        assert_error(verify_runtime(&bundle.executable), "schema version");
        Ok(())
    }

    #[test]
    fn rejects_wrong_codex_version_and_preserves_incomplete_destination() -> TestResult {
        let temp = tempfile::tempdir()?;
        let source = source(&temp.path().join("source"))?;
        let destination = temp.path().join("runtime");
        fs::create_dir(&destination)?;
        fs::write(destination.join("sentinel"), "keep")?;
        assert!(stage_runtime(&source, &destination).is_err());
        assert_eq!(fs::read_to_string(destination.join("sentinel"))?, "keep");
        fs::write(&source, "#!/bin/sh\nprintf 'codex-cli wrong\\n'\n")?;
        assert!(stage_runtime(&source, &temp.path().join("other")).is_err());
        assert!(!temp.path().join("other").exists());
        Ok(())
    }

    #[test]
    fn source_version_probe_has_a_timeout() -> TestResult {
        let temp = tempfile::tempdir()?;
        let source = source(temp.path())?;
        fs::write(&source, "#!/bin/sh\nwhile :; do :; done\n")?;
        assert_error(
            check_source_version(&source, Duration::from_millis(25)),
            "timeout",
        );
        Ok(())
    }
}
