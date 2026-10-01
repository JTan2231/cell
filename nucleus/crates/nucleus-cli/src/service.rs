use std::env;
use std::ffi::OsStr;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nucleus_codex::validate_auth_document;
use thiserror::Error;

pub const SERVICE_LABEL: &str = "org.nucleus.daemon";

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("the Nucleus service is supported only on macOS")]
    UnsupportedPlatform,
    #[error("HOME is unavailable or is not an absolute path")]
    MissingHome,
    #[error("unable to locate nucleusd; pass --daemon or place it beside nucleus")]
    DaemonNotFound,
    #[error("unable to locate Codex; pass --codex or set NUCLEUS_CODEX")]
    CodexNotFound,
    #[error("Codex runtime is incomplete or changed: {0}")]
    InvalidCodexRuntime(String),
    #[error("Codex home must be an existing absolute directory: {0}")]
    InvalidCodexHome(PathBuf),
    #[error("a loaded {SERVICE_LABEL} service has no managed plist at {0}")]
    UnmanagedLoadedService(PathBuf),
    #[error("{operation} failed for {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{program} failed with status {status}: {stderr}")]
    Command {
        program: &'static str,
        status: i32,
        stderr: String,
    },
    #[error("unexpected output from id -u: {0:?}")]
    InvalidUid(String),
    #[error(
        "installation failed: {install}; restoring the previous installation also failed: {rollback}"
    )]
    InstallRollback { install: String, rollback: String },
    #[error("unable to inspect database schema at {path}: {source}")]
    Database {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error(
        "automatic binary rollback is unsafe because the database schema changed from {before:?} to {after:?}; keep the candidate binaries and restore a matching pre-cutover database only as an explicit recovery operation"
    )]
    SchemaRollbackUnsafe {
        before: Option<i64>,
        after: Option<i64>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServicePaths {
    pub home: PathBuf,
    pub state_dir: PathBuf,
    pub socket: PathBuf,
    pub database: PathBuf,
    pub codex_home: PathBuf,
    pub log_dir: PathBuf,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
    pub launch_agent: PathBuf,
    pub daemon: PathBuf,
    pub cli: PathBuf,
}

impl ServicePaths {
    pub fn for_current_user() -> Result<Self, ServiceError> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or(ServiceError::MissingHome)?;
        Ok(Self::under_home(&home))
    }

    pub fn under_home(home: &Path) -> Self {
        let state_dir = home.join("Library/Application Support/Nucleus");
        let log_dir = home.join("Library/Logs/Nucleus");
        Self {
            home: home.to_path_buf(),
            socket: state_dir.join("nucleus.sock"),
            database: state_dir.join("nucleus.db"),
            codex_home: state_dir.join("codex-home"),
            stdout_log: log_dir.join("nucleusd.stdout.log"),
            stderr_log: log_dir.join("nucleusd.stderr.log"),
            launch_agent: home
                .join("Library/LaunchAgents")
                .join(format!("{SERVICE_LABEL}.plist")),
            daemon: home.join(".local/libexec/nucleusd"),
            cli: home.join(".local/bin/nucleus"),
            state_dir,
            log_dir,
        }
    }

    fn create_directories(&self) -> Result<(), ServiceError> {
        create_private_dir(&self.state_dir)?;
        create_private_dir(&self.codex_home)?;
        create_private_dir(&self.log_dir)?;
        create_dir(parent(&self.launch_agent)?, 0o700)?;
        create_dir(parent(&self.daemon)?, 0o755)?;
        create_dir(parent(&self.cli)?, 0o755)
    }
}

#[derive(Debug)]
pub struct InstallResult {
    pub paths: ServicePaths,
    pub codex: PathBuf,
    pub codex_home: PathBuf,
    previous: PreviousInstallation,
    target: String,
}

impl InstallResult {
    pub fn rollback(&self) -> Result<(), ServiceError> {
        self.previous.restore(&self.paths, &self.target, true)
    }
}

#[derive(Debug)]
struct FileSnapshot {
    bytes: Vec<u8>,
    mode: u32,
}

#[derive(Debug)]
struct PreviousInstallation {
    daemon: Option<FileSnapshot>,
    cli: Option<FileSnapshot>,
    launch_agent: Option<FileSnapshot>,
    database_schema_version: Option<i64>,
    was_loaded: bool,
}

impl PreviousInstallation {
    fn capture(paths: &ServicePaths, was_loaded: bool) -> Result<Self, ServiceError> {
        Ok(Self {
            daemon: snapshot_file(&paths.daemon)?,
            cli: snapshot_file(&paths.cli)?,
            launch_agent: snapshot_file(&paths.launch_agent)?,
            database_schema_version: database_schema_version(&paths.database)?,
            was_loaded,
        })
    }

    fn restore(
        &self,
        paths: &ServicePaths,
        target: &str,
        replace_service: bool,
    ) -> Result<(), ServiceError> {
        let current_schema = database_schema_version(&paths.database)?;
        if current_schema != self.database_schema_version {
            return Err(ServiceError::SchemaRollbackUnsafe {
                before: self.database_schema_version,
                after: current_schema,
            });
        }
        if replace_service
            && launchctl([OsStr::new("print"), OsStr::new(target)])?
                .status
                .success()
        {
            bootout(target)?;
        }

        restore_file(&paths.daemon, self.daemon.as_ref())?;
        restore_file(&paths.cli, self.cli.as_ref())?;
        restore_file(&paths.launch_agent, self.launch_agent.as_ref())?;

        // Authentication is forward-only. Either the old daemon can refresh it
        // before bootout or the replacement can refresh it before a later
        // health-check rollback. Restoring a byte snapshot here could therefore
        // resurrect a stale, already-consumed refresh token.

        if self.was_loaded && replace_service {
            command_success(
                "/bin/launchctl",
                &Command::new("/bin/launchctl")
                    .arg("bootstrap")
                    .arg(target_domain()?)
                    .arg(&paths.launch_agent)
                    .output()
                    .map_err(|source| ServiceError::Io {
                        operation: "restore prior LaunchAgent",
                        path: paths.launch_agent.clone(),
                        source,
                    })?,
            )?;
        }
        Ok(())
    }
}

fn database_schema_version(path: &Path) -> Result<Option<i64>, ServiceError> {
    if !path.exists() {
        return Ok(None);
    }
    let connection = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|source| ServiceError::Database {
        path: path.to_path_buf(),
        source,
    })?;
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map(Some)
        .map_err(|source| ServiceError::Database {
            path: path.to_path_buf(),
            source,
        })
}

/// A stopped service can be installed under an existing deployment hold only
/// when its retained jobs and attempts have already settled. This read never
/// creates a database, migrates state, or changes an attempt.
pub fn offline_deployment_ready(paths: &ServicePaths) -> Result<bool, ServiceError> {
    if status()?.loaded {
        return Ok(false);
    }
    settled_database(&paths.database)
}

fn settled_database(database: &Path) -> Result<bool, ServiceError> {
    if !database.exists() {
        return Ok(true);
    }
    let connection =
        rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|source| ServiceError::Database {
                path: database.to_owned(),
                source,
            })?;
    let unfinished: i64 = connection.query_row(
        "SELECT (SELECT COUNT(*) FROM jobs WHERE state NOT IN ('completed','failed','cancelled')) + (SELECT COUNT(*) FROM attempts WHERE completed_at IS NULL OR state NOT IN ('completed','failed','cancelled','timed_out','lost'))", [], |row| row.get(0))
        .map_err(|source| ServiceError::Database { path: database.to_owned(), source })?;
    Ok(unfinished == 0)
}

#[derive(Debug)]
pub struct ServiceStatus {
    pub loaded: bool,
    pub target: String,
    pub details: String,
}

pub fn install(
    paths: ServicePaths,
    daemon_source: Option<&Path>,
    codex_source: Option<&Path>,
    codex_home_source: Option<&Path>,
) -> Result<InstallResult, ServiceError> {
    require_macos()?;
    paths.create_directories()?;

    let cli_source = canonical_current_executable()?;
    let daemon_source = find_daemon(&cli_source, daemon_source)?;
    let codex = find_codex(codex_source)?;
    nucleus_codex::runtime_bundle::verify_runtime(&codex)
        .map_err(|error| ServiceError::InvalidCodexRuntime(error.to_string()))?;
    let source_codex_home = find_codex_home(codex_home_source)?;
    let target = service_target()?;
    let was_loaded = launchctl([OsStr::new("print"), OsStr::new(&target)])?
        .status
        .success();
    let previous = PreviousInstallation::capture(&paths, was_loaded)?;
    if was_loaded && previous.launch_agent.is_none() {
        return Err(ServiceError::UnmanagedLoadedService(
            paths.launch_agent.clone(),
        ));
    }

    let mut service_replaced = false;
    let operation = (|| {
        // Stage every file while an existing service continues running its old
        // executable inode. Only a completely staged installation is stopped.
        copy_executable(&daemon_source, &paths.daemon)?;
        if cli_source != paths.cli {
            copy_executable(&cli_source, &paths.cli)?;
        }
        let plist = render_plist(&paths, &codex);
        atomic_write(&paths.launch_agent, plist.as_bytes(), 0o600)?;
        validate_plist(&paths.launch_agent)?;
        command_success(
            "/bin/launchctl",
            &launchctl([OsStr::new("enable"), OsStr::new(&target)])?,
        )?;

        if was_loaded {
            bootout(&target)?;
        }
        service_replaced = true;
        // Credential authority may be refreshed by a running daemon. Import it
        // only after the previous service is fully stopped. Authentication is
        // deliberately excluded from rollback so later refreshes remain
        // authoritative even if bootstrap or the health check fails.
        prepare_owned_codex_home(&paths.codex_home, source_codex_home.as_deref())?;
        command_success(
            "/bin/launchctl",
            &Command::new("/bin/launchctl")
                .arg("bootstrap")
                .arg(target_domain()?)
                .arg(&paths.launch_agent)
                .output()
                .map_err(|source| ServiceError::Io {
                    operation: "run launchctl bootstrap",
                    path: PathBuf::from("/bin/launchctl"),
                    source,
                })?,
        )
    })();

    if let Err(error) = operation {
        if let Err(rollback) = previous.restore(&paths, &target, service_replaced) {
            return Err(ServiceError::InstallRollback {
                install: error.to_string(),
                rollback: rollback.to_string(),
            });
        }
        return Err(error);
    }

    let owned_codex_home = paths.codex_home.clone();
    Ok(InstallResult {
        paths,
        codex,
        codex_home: owned_codex_home,
        previous,
        target,
    })
}

pub fn status() -> Result<ServiceStatus, ServiceError> {
    require_macos()?;
    let target = service_target()?;
    let output = launchctl([OsStr::new("print"), OsStr::new(&target)])?;
    let loaded = output.status.success();
    let mut details = if loaded {
        String::from_utf8_lossy(&output.stdout).into_owned()
    } else {
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    if details.trim().is_empty() {
        details = if loaded {
            "service is loaded".to_owned()
        } else {
            "service is not loaded".to_owned()
        };
    }
    Ok(ServiceStatus {
        loaded,
        target,
        details,
    })
}

pub fn restart() -> Result<(), ServiceError> {
    require_macos()?;
    let target = service_target()?;
    let output = launchctl([
        OsStr::new("kickstart"),
        OsStr::new("-k"),
        OsStr::new(&target),
    ])?;
    command_success("/bin/launchctl", &output)
}

pub fn uninstall(paths: &ServicePaths) -> Result<(), ServiceError> {
    require_macos()?;
    let target = service_target()?;
    let loaded = launchctl([OsStr::new("print"), OsStr::new(&target)])?
        .status
        .success();
    if loaded && !is_regular_file(&paths.launch_agent) {
        return Err(ServiceError::UnmanagedLoadedService(
            paths.launch_agent.clone(),
        ));
    }
    if loaded {
        bootout(&target)?;
    }

    remove_installed_file(&paths.launch_agent)?;
    remove_installed_file(&paths.daemon)?;
    remove_installed_file(&paths.cli)?;
    Ok(())
}

pub fn find_daemon(
    current_executable: &Path,
    requested: Option<&Path>,
) -> Result<PathBuf, ServiceError> {
    let candidate = requested.map(Path::to_path_buf).or_else(|| {
        env::var_os("NUCLEUSD_BINARY")
            .map(PathBuf::from)
            .or_else(|| current_executable.parent().map(|dir| dir.join("nucleusd")))
    });
    let Some(candidate) = candidate else {
        return Err(ServiceError::DaemonNotFound);
    };
    if !candidate.is_file() {
        return Err(ServiceError::DaemonNotFound);
    }
    fs::canonicalize(&candidate).map_err(|source| ServiceError::Io {
        operation: "resolve daemon executable",
        path: candidate,
        source,
    })
}

pub fn find_codex(requested: Option<&Path>) -> Result<PathBuf, ServiceError> {
    if let Some(candidate) = requested {
        return resolve_executable(candidate);
    }
    if let Some(candidate) = env::var_os("NUCLEUS_CODEX") {
        return resolve_executable(Path::new(&candidate));
    }
    let candidate = path_candidates("codex")
        .chain([
            PathBuf::from("/opt/homebrew/bin/codex"),
            PathBuf::from("/usr/local/bin/codex"),
        ])
        .find(|path| is_executable(path))
        .ok_or(ServiceError::CodexNotFound)?;
    resolve_executable(&candidate)
}

pub fn find_codex_home(requested: Option<&Path>) -> Result<Option<PathBuf>, ServiceError> {
    let Some(candidate) = requested else {
        return Ok(None);
    };
    if !candidate.is_absolute() || !candidate.is_dir() {
        return Err(ServiceError::InvalidCodexHome(candidate.to_path_buf()));
    }
    let resolved = fs::canonicalize(candidate).map_err(|source| ServiceError::Io {
        operation: "resolve Codex home",
        path: candidate.to_path_buf(),
        source,
    })?;
    if !resolved.is_dir() {
        return Err(ServiceError::InvalidCodexHome(resolved));
    }
    Ok(Some(resolved))
}

fn prepare_owned_codex_home(destination: &Path, source: Option<&Path>) -> Result<(), ServiceError> {
    create_private_dir(destination)?;
    atomic_write(
        &destination.join("config.toml"),
        b"cli_auth_credentials_store = \"file\"\n",
        0o600,
    )?;
    let Some(source) = source else {
        return Ok(());
    };
    let source_auth = source.join("auth.json");
    let metadata = fs::symlink_metadata(&source_auth).map_err(|source_error| ServiceError::Io {
        operation: "inspect source Codex authentication",
        path: source_auth.clone(),
        source: source_error,
    })?;
    if !metadata.file_type().is_file()
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() == 0
        || metadata.len() > 4 * 1024 * 1024
    {
        return Err(ServiceError::Io {
            operation: "validate source Codex authentication",
            path: source_auth,
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "auth.json must be a nonempty mode-0600 regular file no larger than 4194304 bytes",
            ),
        });
    }
    let destination_auth = destination.join("auth.json");
    let bytes = fs::read(&source_auth).map_err(|source_error| ServiceError::Io {
        operation: "read source Codex authentication",
        path: source_auth.clone(),
        source: source_error,
    })?;
    validate_auth_document(&bytes).map_err(|source_error| ServiceError::Io {
        operation: "validate source Codex authentication",
        path: source_auth,
        source: io::Error::new(io::ErrorKind::InvalidData, source_error.to_string()),
    })?;
    if fs::canonicalize(source).is_ok_and(|source| source == destination) {
        return Ok(());
    }
    atomic_write(&destination_auth, &bytes, 0o600)
}

pub fn prepare_codex_home(paths: &ServicePaths) -> Result<(), ServiceError> {
    prepare_owned_codex_home(&paths.codex_home, None)
}

fn resolve_executable(candidate: &Path) -> Result<PathBuf, ServiceError> {
    if !is_executable(candidate) {
        return Err(ServiceError::CodexNotFound);
    }
    fs::canonicalize(candidate).map_err(|source| ServiceError::Io {
        operation: "resolve Codex executable",
        path: candidate.to_path_buf(),
        source,
    })
}

pub fn render_plist(paths: &ServicePaths, codex: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{daemon}</string>
        <string>serve</string>
        <string>--codex</string>
        <string>{codex}</string>
    </array>
    <key>WorkingDirectory</key>
    <string>{state}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>{home}</string>
        <key>CODEX_HOME</key>
        <string>{codex_home}</string>
        <key>PATH</key>
        <string>{path}</string>
    </dict>
    <key>KeepAlive</key>
    <true/>
    <key>Umask</key>
    <integer>63</integer>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
</dict>
</plist>
"#,
        label = SERVICE_LABEL,
        daemon = xml_escape(&paths.daemon.to_string_lossy()),
        codex = xml_escape(&codex.to_string_lossy()),
        state = xml_escape(&paths.state_dir.to_string_lossy()),
        home = xml_escape(&paths.home.to_string_lossy()),
        codex_home = xml_escape(&paths.codex_home.to_string_lossy()),
        path = xml_escape(&service_path(&paths.home)),
        stdout = xml_escape(&paths.stdout_log.to_string_lossy()),
        stderr = xml_escape(&paths.stderr_log.to_string_lossy()),
    )
}

fn service_path(home: &Path) -> String {
    [
        home.join(".local/bin"),
        home.join(".cargo/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ]
    .iter()
    .map(|path| path.to_string_lossy())
    .collect::<Vec<_>>()
    .join(":")
}

fn canonical_current_executable() -> Result<PathBuf, ServiceError> {
    let executable = env::current_exe().map_err(|source| ServiceError::Io {
        operation: "locate current executable",
        path: PathBuf::from("nucleus"),
        source,
    })?;
    fs::canonicalize(&executable).map_err(|source| ServiceError::Io {
        operation: "resolve current executable",
        path: executable,
        source,
    })
}

fn path_candidates(executable: &str) -> impl Iterator<Item = PathBuf> {
    env::var_os("PATH")
        .map(|value| {
            env::split_paths(&value)
                .map(|directory| directory.join(executable))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn create_dir(path: &Path, mode: u32) -> Result<(), ServiceError> {
    if path.is_dir() {
        return Ok(());
    }
    if path.exists() {
        return Err(ServiceError::Io {
            operation: "create directory",
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::AlreadyExists, "path is not a directory"),
        });
    }
    DirBuilder::new()
        .recursive(true)
        .mode(mode)
        .create(path)
        .map_err(|source| ServiceError::Io {
            operation: "create directory",
            path: path.to_path_buf(),
            source,
        })
}

fn create_private_dir(path: &Path) -> Result<(), ServiceError> {
    create_dir(path, 0o700)?;
    let metadata = fs::symlink_metadata(path).map_err(|source| ServiceError::Io {
        operation: "inspect private directory",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(ServiceError::Io {
            operation: "secure private directory",
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "directory must not be a symlink",
            ),
        });
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        ServiceError::Io {
            operation: "secure private directory",
            path: path.to_path_buf(),
            source,
        }
    })
}

fn parent(path: &Path) -> Result<&Path, ServiceError> {
    path.parent().ok_or_else(|| ServiceError::Io {
        operation: "resolve parent directory",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
    })
}

fn copy_executable(source: &Path, destination: &Path) -> Result<(), ServiceError> {
    let bytes = fs::read(source).map_err(|source_error| ServiceError::Io {
        operation: "read executable",
        path: source.to_path_buf(),
        source: source_error,
    })?;
    atomic_write(destination, &bytes, 0o755)
}

fn snapshot_file(path: &Path) -> Result<Option<FileSnapshot>, ServiceError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ServiceError::Io {
                operation: "inspect installed file",
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if !metadata.file_type().is_file() {
        return Err(ServiceError::Io {
            operation: "snapshot installed file",
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "path is not a regular file"),
        });
    }
    let bytes = fs::read(path).map_err(|source| ServiceError::Io {
        operation: "snapshot installed file",
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Some(FileSnapshot {
        bytes,
        mode: metadata.permissions().mode() & 0o777,
    }))
}

fn restore_file(path: &Path, snapshot: Option<&FileSnapshot>) -> Result<(), ServiceError> {
    if let Some(snapshot) = snapshot {
        atomic_write(path, &snapshot.bytes, snapshot.mode)
    } else {
        remove_installed_file(path)
    }
}

fn validate_plist(path: &Path) -> Result<(), ServiceError> {
    let output = Command::new("/usr/bin/plutil")
        .arg("-lint")
        .arg(path)
        .output()
        .map_err(|source| ServiceError::Io {
            operation: "validate LaunchAgent plist",
            path: path.to_path_buf(),
            source,
        })?;
    command_success("/usr/bin/plutil", &output)
}

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<(), ServiceError> {
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("nucleus");
    let temporary = path.with_file_name(format!(".{file_name}.install-{}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(mode)
            .open(&temporary)
            .map_err(|source| ServiceError::Io {
                operation: "create temporary installation file",
                path: temporary.clone(),
                source,
            })?;
        file.write_all(bytes).map_err(|source| ServiceError::Io {
            operation: "write installation file",
            path: temporary.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| ServiceError::Io {
            operation: "sync installation file",
            path: temporary.clone(),
            source,
        })?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(mode)).map_err(|source| {
            ServiceError::Io {
                operation: "set installation file permissions",
                path: temporary.clone(),
                source,
            }
        })?;
        fs::rename(&temporary, path).map_err(|source| ServiceError::Io {
            operation: "install file",
            path: path.to_path_buf(),
            source,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn remove_installed_file(path: &Path) -> Result<(), ServiceError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ServiceError::Io {
            operation: "remove installed file",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn launchctl<const N: usize>(arguments: [&OsStr; N]) -> Result<Output, ServiceError> {
    Command::new("/bin/launchctl")
        .args(arguments)
        .output()
        .map_err(|source| ServiceError::Io {
            operation: "run launchctl",
            path: PathBuf::from("/bin/launchctl"),
            source,
        })
}

fn bootout(target: &str) -> Result<(), ServiceError> {
    // launchd can accept bootout before removing the job. Waiting prevents a
    // replacement bootstrap from racing that transition.
    command_success("/bin/launchctl", &launchctl(bootout_arguments(target))?)
}

fn bootout_arguments(target: &str) -> [&OsStr; 3] {
    [
        OsStr::new("bootout"),
        OsStr::new("--wait"),
        OsStr::new(target),
    ]
}

fn command_success(program: &'static str, output: &Output) -> Result<(), ServiceError> {
    if output.status.success() {
        return Ok(());
    }
    Err(ServiceError::Command {
        program,
        status: output.status.code().unwrap_or(-1),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

fn target_domain() -> Result<String, ServiceError> {
    Ok(format!("gui/{}", current_uid()?))
}

fn service_target() -> Result<String, ServiceError> {
    Ok(format!("{}/{SERVICE_LABEL}", target_domain()?))
}

fn current_uid() -> Result<u32, ServiceError> {
    let output = Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .map_err(|source| ServiceError::Io {
            operation: "determine user id",
            path: PathBuf::from("/usr/bin/id"),
            source,
        })?;
    if !output.status.success() {
        return Err(ServiceError::Command {
            program: "/usr/bin/id",
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    text.parse().map_err(|_| ServiceError::InvalidUid(text))
}

fn require_macos() -> Result<(), ServiceError> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(ServiceError::UnsupportedPlatform)
    }
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_paths_are_per_user_and_absolute() {
        let paths = ServicePaths::under_home(Path::new("/Users/Test Person"));
        assert_eq!(
            paths.socket,
            PathBuf::from("/Users/Test Person/Library/Application Support/Nucleus/nucleus.sock")
        );
        assert_eq!(
            paths.launch_agent,
            PathBuf::from("/Users/Test Person/Library/LaunchAgents/org.nucleus.daemon.plist")
        );
        assert_eq!(
            paths.daemon,
            PathBuf::from("/Users/Test Person/.local/libexec/nucleusd")
        );
    }

    #[test]
    fn bootout_waits_for_launchd_to_finish_removing_the_service() {
        assert_eq!(
            bootout_arguments("gui/501/org.nucleus.daemon"),
            [
                OsStr::new("bootout"),
                OsStr::new("--wait"),
                OsStr::new("gui/501/org.nucleus.daemon"),
            ]
        );
    }

    #[test]
    fn plist_uses_only_absolute_paths_and_escapes_them() {
        let paths = ServicePaths::under_home(Path::new("/Users/a&b"));
        let plist = render_plist(&paths, Path::new("/opt/homebrew/bin/codex"));
        assert!(plist.contains("<string>/Users/a&amp;b/.local/libexec/nucleusd</string>"));
        assert!(plist.contains("<string>serve</string>"));
        assert!(plist.contains("<string>--codex</string>"));
        assert!(plist.contains("<string>/opt/homebrew/bin/codex</string>"));
        assert!(plist.contains("<key>HOME</key>"));
        assert!(plist.contains("<key>CODEX_HOME</key>"));
        assert!(plist.contains(
            "<string>/Users/a&amp;b/Library/Application Support/Nucleus/codex-home</string>"
        ));
        assert!(plist.contains("/Users/a&amp;b/.cargo/bin"));
        assert!(plist.contains("<integer>63</integer>"));
        assert!(!plist.contains("ProcessType"));
        assert!(!plist.contains('~'));
    }

    #[test]
    fn plist_always_uses_nucleus_owned_codex_home() {
        let paths = ServicePaths::under_home(Path::new("/Users/example"));
        let plist = render_plist(&paths, Path::new("/opt/homebrew/bin/codex"));

        assert!(plist.contains("<key>CODEX_HOME</key>"));
        assert!(plist.contains(
            "<string>/Users/example/Library/Application Support/Nucleus/codex-home</string>"
        ));
    }
}
