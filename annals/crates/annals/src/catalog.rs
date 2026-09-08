use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use fs2::FileExt as _;
use rusqlite::{Connection, OpenFlags, OptionalExtension as _, params};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::db::{self, LibraryKind};
use crate::error::{AppError, AppResult};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS libraries (
    name TEXT PRIMARY KEY,
    library_id TEXT NOT NULL UNIQUE,
    library TEXT NOT NULL UNIQUE,
    spool TEXT NOT NULL UNIQUE,
    config TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK(kind IN ('general', 'decisions')),
    state TEXT NOT NULL CHECK(state IN ('provisioning', 'ready')),
    created_at TEXT NOT NULL,
    initial_config TEXT
);
PRAGMA user_version = 1;
";

/// One registered library. `state` describes creation, not runtime readiness.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegisteredLibrary {
    pub name: String,
    pub library_id: String,
    pub library: PathBuf,
    pub spool: PathBuf,
    pub config: PathBuf,
    pub kind: String,
    pub state: String,
    /// When Annals reserved this catalog entry, not the latest source or corpus change.
    pub created_at: String,
}

/// Exclusive catalog guard shared by named creation and the installer.
pub struct CatalogLock {
    root: PathBuf,
    _file: File,
}

/// Lock the catalog for one registration or deployment operation.
///
/// # Errors
/// Returns a directory or lock error. This does not inspect deployment readiness.
pub fn lock_library_catalog(state_root: &Path) -> AppResult<CatalogLock> {
    private_directory(state_root)?;
    let root = fs::canonicalize(state_root)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(root.join(".catalog.lock"))?;
    file.try_lock_exclusive().map_err(|error| {
        AppError::conflict(
            "library_catalog_busy",
            format!("the library catalog is in use: {error}"),
        )
    })?;
    Ok(CatalogLock { root, _file: file })
}

pub(crate) fn state_root() -> AppResult<PathBuf> {
    if let Some(root) = std::env::var_os("ANNALS_STATE_DIR").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(root);
        return if path.is_absolute() {
            Ok(path)
        } else {
            Ok(std::env::current_dir()?.join(path))
        };
    }
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::invalid(
                "annals_home_missing",
                "set HOME or ANNALS_STATE_DIR to locate the library catalog",
            )
        })?;
    let home = PathBuf::from(home);
    Ok(if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Annals")
    } else {
        home.join(".local/share/annals")
    })
}

/// Read registered locations without opening or migrating their libraries.
/// An absent catalog returns an empty list.
///
/// # Errors
/// Returns a catalog read or schema error; no runtime readiness is inferred.
pub fn registered_libraries(state_root: &Path) -> AppResult<Vec<RegisteredLibrary>> {
    let Some(connection) = open_read(state_root)? else {
        return Ok(Vec::new());
    };
    let mut query = connection.prepare(
        "SELECT name, library_id, library, spool, config, kind, state, created_at FROM libraries ORDER BY name")?;
    Ok(query
        .query_map([], from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn resolve(state_root: &Path, name: &str) -> AppResult<RegisteredLibrary> {
    validate_name(name)?;
    let library = registered_libraries(state_root)?.into_iter().find(|value| value.name == name)
        .ok_or_else(|| AppError::not_found("named_library_not_found", format!("no library is registered as {name:?}; create it with `annals library create {name}`")))?;
    if library.state != "ready" {
        return Err(AppError::conflict(
            "library_provisioning",
            format!(
                "library {name:?} is not fully created; repeat `annals library create {name} --kind {}`",
                library.kind
            ),
        ));
    }
    Ok(library)
}

pub(crate) fn verify(library: &RegisteredLibrary, connection: &Connection) -> AppResult<()> {
    let actual: String = connection.query_row(
        "SELECT library_id FROM library_identity WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    if actual != library.library_id {
        return Err(AppError::conflict(
            "unexpected_library",
            "the registered path contains a different library identity",
        ));
    }
    if db::library_kind(connection)?.as_str() != library.kind {
        return Err(AppError::conflict(
            "library_kind_mismatch",
            "the registered library admission kind does not match its database",
        ));
    }
    Ok(())
}

pub(crate) fn create(
    state_root: &Path,
    name: &str,
    kind: LibraryKind,
    config: &Config,
) -> AppResult<RegisteredLibrary> {
    validate_name(name)?;
    let lock = lock_library_catalog(state_root)?;
    refuse_unfinished_install(&lock.root)?;
    let connection = open_write(&lock)?;
    let existing = lookup(&connection, name)?;
    let library = if let Some(existing) = existing {
        if existing.kind != kind.as_str() {
            return Err(AppError::conflict(
                "library_kind_mismatch",
                "this name is already reserved for a different library kind",
            ));
        }
        if existing.state == "ready" {
            verify(&existing, &db::open_read(&existing.library)?)?;
            return Ok(existing);
        }
        existing
    } else {
        let library_id: String =
            connection.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
        let directory = lock.root.join("libraries").join(&library_id);
        let created_at: String =
            connection.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |row| {
                row.get(0)
            })?;
        let library = RegisteredLibrary {
            name: name.to_owned(),
            library_id,
            library: directory.join("annals.db"),
            spool: directory.join("spool"),
            config: directory.join("config.toml"),
            kind: kind.as_str().to_owned(),
            state: "provisioning".into(),
            created_at,
        };
        let mut selected = config.clone();
        // Creation inherits execution settings, never another library's authority pin.
        selected.expected_library_id = None;
        selected.decision_feed = None;
        selected.select_named(&library)?;
        let document = toml::to_string(&selected)
            .map_err(|error| AppError::unexpected("config_write_failed", error.to_string()))?;
        insert(&connection, &library, Some(&document))?;
        library
    };
    finish_creation(&connection, library)
}

fn finish_creation(
    connection: &Connection,
    mut library: RegisteredLibrary,
) -> AppResult<RegisteredLibrary> {
    let directory = library.library.parent().ok_or_else(|| {
        AppError::database("invalid_catalog", "registered library has no directory")
    })?;
    private_directory(directory)?;
    if !library.library.exists() {
        // Publish only a complete, closed database. A killed creation may leave
        // an unused staging directory; it never publishes a partial library.
        let staging = tempfile::Builder::new()
            .prefix(".provision-")
            .tempdir_in(directory)?;
        let staged = staging.path().join("annals.db");
        let kind = if library.kind == "decisions" {
            LibraryKind::Decisions
        } else {
            LibraryKind::General
        };
        let database = db::init_with_identity(&staged, kind, &library.library_id)?;
        database
            .close()
            .map_err(|(_, error)| AppError::from(error))?;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o600))?;
        File::open(&staged)?.sync_all()?;
        rename_absent(&staged, &library.library)?;
        File::open(directory)?.sync_all()?;
    }
    verify(&library, &db::open_read(&library.library)?)?;
    for child in [
        "",
        "incoming",
        "queued",
        "processing",
        "done",
        "duplicates",
        "failed",
        "skipped",
    ] {
        private_directory(&library.spool.join(child))?;
    }
    let document: String = connection.query_row(
        "SELECT initial_config FROM libraries WHERE name = ?1",
        [&library.name],
        |row| row.get(0),
    )?;
    if library.config.exists() {
        if fs::read_to_string(&library.config)? != document {
            return Err(AppError::conflict(
                "library_provisioning_config_conflict",
                "the unfinished library configuration differs from its reserved configuration",
            ));
        }
    } else {
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        file.write_all(document.as_bytes())?;
        file.as_file().sync_all()?;
        rename_absent(file.path(), &library.config)?;
    }
    File::open(directory)?.sync_all()?;
    connection.execute("UPDATE libraries SET state = 'ready', initial_config = NULL WHERE name = ?1 AND library_id = ?2", params![library.name, library.library_id])?;
    library.state = "ready".into();
    Ok(library)
}

/// Register an existing, current-schema library during managed installation.
/// The caller must hold this root's catalog lock throughout deployment.
/// Existing names and identities cannot be rebound to another location.
///
/// # Errors
/// Returns a lock-root, identity, location, or catalog conflict.
pub fn register_existing_library(
    lock: &CatalogLock,
    state_root: &Path,
    name: &str,
    library: &Path,
    spool: &Path,
) -> AppResult<RegisteredLibrary> {
    validate_name(name)?;
    if fs::canonicalize(state_root)? != lock.root {
        return Err(AppError::invalid(
            "catalog_lock_mismatch",
            "the catalog lock belongs to a different state root",
        ));
    }
    let database = db::open_read(library)?;
    let library_id = crate::decision_feed::library_id(&database)?;
    let library = fs::canonicalize(library)?;
    let spool = fs::canonicalize(spool)?;
    let managed = lock
        .root
        .join("libraries")
        .join(&library_id)
        .join("annals.db");
    if library != lock.root.join("annals.db")
        && library != lock.root.join("decisions/annals.db")
        && library != managed
    {
        return Err(AppError::invalid(
            "unmanaged_library_path",
            "registration requires an Annals-managed database location",
        ));
    }
    let directory = library.parent().ok_or_else(|| {
        AppError::invalid("invalid_library_path", "library has no parent directory")
    })?;
    if spool != directory.join("spool") {
        return Err(AppError::invalid(
            "unmanaged_spool_path",
            "registration requires the library's sibling spool directory",
        ));
    }
    let config = directory.join("config.toml");
    if !config.is_file() {
        return Err(AppError::not_found(
            "library_config_not_found",
            "registered libraries require their sibling config.toml",
        ));
    }
    let connection = open_write(lock)?;
    if let Some(existing) = lookup(&connection, name)? {
        if existing.library_id != library_id
            || existing.library != library
            || existing.spool != spool
            || existing.config != config
            || existing.state != "ready"
        {
            return Err(AppError::conflict(
                "library_registration_conflict",
                "the registered name cannot be rebound to this library",
            ));
        }
        verify(&existing, &database)?;
        return Ok(existing);
    }
    let created_at =
        connection.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |row| {
            row.get(0)
        })?;
    let record = RegisteredLibrary {
        name: name.into(),
        library_id,
        library,
        spool,
        config,
        kind: db::library_kind(&database)?.as_str().into(),
        state: "ready".into(),
        created_at,
    };
    insert(&connection, &record, None)?;
    Ok(record)
}

fn insert(
    connection: &Connection,
    library: &RegisteredLibrary,
    document: Option<&str>,
) -> AppResult<()> {
    connection.execute("INSERT INTO libraries(name, library_id, library, spool, config, kind, state, created_at, initial_config) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![library.name, library.library_id, path_text(&library.library)?, path_text(&library.spool)?, path_text(&library.config)?, library.kind, library.state, library.created_at, document])?;
    Ok(())
}

fn lookup(connection: &Connection, name: &str) -> AppResult<Option<RegisteredLibrary>> {
    Ok(connection.query_row("SELECT name, library_id, library, spool, config, kind, state, created_at FROM libraries WHERE name = ?1", [name], from_row).optional()?)
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RegisteredLibrary> {
    Ok(RegisteredLibrary {
        name: row.get(0)?,
        library_id: row.get(1)?,
        library: PathBuf::from(row.get::<_, String>(2)?),
        spool: PathBuf::from(row.get::<_, String>(3)?),
        config: PathBuf::from(row.get::<_, String>(4)?),
        kind: row.get(5)?,
        state: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn open_read(root: &Path) -> AppResult<Option<Connection>> {
    let path = root.join("catalog.db");
    if !path.exists() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    require_schema(&connection)?;
    Ok(Some(connection))
}

fn open_write(lock: &CatalogLock) -> AppResult<Connection> {
    let path = lock.root.join("catalog.db");
    if !path.exists() {
        let staged = tempfile::NamedTempFile::new_in(&lock.root)?;
        let connection = Connection::open(staged.path())?;
        connection.execute_batch(SCHEMA)?;
        connection
            .close()
            .map_err(|(_, error)| AppError::from(error))?;
        staged.as_file().sync_all()?;
        rename_absent(staged.path(), &path)?;
        File::open(&lock.root)?.sync_all()?;
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    require_schema(&connection)?;
    Ok(connection)
}

fn require_schema(connection: &Connection) -> AppResult<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != 1 {
        return Err(AppError::database(
            "catalog_schema_incompatible",
            format!("library catalog schema {version} is unsupported"),
        ));
    }
    Ok(())
}

fn validate_name(name: &str) -> AppResult<()> {
    if name.is_empty()
        || name.len() > 64
        || matches!(name, "list" | "create" | "help")
        || !name.as_bytes()[0].is_ascii_lowercase()
        || !name.bytes().all(|value| {
            value.is_ascii_lowercase() || value.is_ascii_digit() || matches!(value, b'-' | b'_')
        })
    {
        return Err(AppError::invalid(
            "invalid_library_name",
            "library names use 1 through 64 lowercase letters, digits, '-' or '_', start with a letter, and cannot be list, create, or help",
        ));
    }
    Ok(())
}

fn path_text(path: &Path) -> AppResult<&str> {
    path.to_str().ok_or_else(|| {
        AppError::invalid(
            "invalid_library_path",
            "managed library paths must be UTF-8",
        )
    })
}

fn private_directory(path: &Path) -> AppResult<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700).create(path)?;
    Ok(())
}

fn rename_absent(source: &Path, destination: &Path) -> AppResult<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|error| AppError::Io(error.into()))
}

fn refuse_unfinished_install(root: &Path) -> AppResult<()> {
    let directory = root.join("install");
    if directory.exists() {
        for entry in fs::read_dir(directory)? {
            if entry?
                .file_name()
                .to_string_lossy()
                .starts_with("transaction.")
            {
                return Err(AppError::conflict(
                    "deployment_incomplete",
                    "recover the unfinished Annals installation before changing its library catalog",
                ));
            }
        }
    }
    Ok(())
}
