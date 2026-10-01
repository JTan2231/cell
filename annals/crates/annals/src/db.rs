use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::error::AppError;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 7;
const FRESH_STATE_SCHEMA_VERSION: i64 = 3;
const SCHEMA: &str = include_str!("../schema.sql");
const MIGRATION_3_TO_4: &str = include_str!("../migrations/3-to-4.sql");
const MIGRATION_4_TO_5: &str = include_str!("../migrations/4-to-5.sql");
const MIGRATION_6_TO_7: &str = include_str!("../migrations/6-to-7.sql");
const MIGRATION_5_TO_6: &str = include_str!("../migrations/5-to-6.sql");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryKind {
    General,
    Decisions,
}

impl LibraryKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Decisions => "decisions",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationResult {
    pub from_version: i64,
    pub to_version: i64,
    pub migrated: bool,
}

/// Create and initialize a fresh Annals library without replacing a path.
pub fn init(path: &Path) -> Result<Connection, AppError> {
    init_with_kind(path, LibraryKind::General)
}

/// Create a fresh library with one immutable role.
pub(crate) fn init_with_kind(path: &Path, kind: LibraryKind) -> Result<Connection, AppError> {
    init_selected_identity(path, kind, None)
}

/// Initialize a library with the stable identity reserved by its catalog.
pub(crate) fn init_with_identity(
    path: &Path,
    kind: LibraryKind,
    library_id: &str,
) -> Result<Connection, AppError> {
    init_selected_identity(path, kind, Some(library_id))
}

fn init_selected_identity(
    path: &Path,
    kind: LibraryKind,
    library_id: Option<&str>,
) -> Result<Connection, AppError> {
    reserve_new_file(path, "library_exists", "library")?;
    match initialize_reserved_file(path, kind, library_id) {
        Ok(connection) => Ok(connection),
        Err(error) => {
            // This call exclusively created the path, so cleanup cannot remove
            // a pre-existing library.
            let _ = fs::remove_file(path);
            Err(error)
        }
    }
}

pub(crate) fn library_kind(connection: &Connection) -> Result<LibraryKind, AppError> {
    let kind = connection
        .query_row(
            "SELECT kind FROM library_profile WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| {
            AppError::database(
                "invalid_library_profile",
                format!("library has no valid immutable profile: {error}"),
            )
        })?;
    match kind.as_str() {
        "general" => Ok(LibraryKind::General),
        "decisions" => Ok(LibraryKind::Decisions),
        _ => Err(AppError::database(
            "invalid_library_profile",
            "library has an unsupported immutable profile",
        )),
    }
}

pub(crate) fn require_library_kind(
    connection: &Connection,
    expected: LibraryKind,
) -> Result<(), AppError> {
    let actual = library_kind(connection)?;
    if actual == expected {
        Ok(())
    } else {
        Err(AppError::conflict(
            "library_kind_mismatch",
            format!(
                "this operation requires a {} library; the selected library is {}",
                expected.as_str(),
                actual.as_str()
            ),
        ))
    }
}

pub(crate) fn require_path_kind(path: &Path, expected: LibraryKind) -> Result<(), AppError> {
    require_library_kind(&open_read(path)?, expected)
}

/// Open a current-format library for reads without changing journal mode.
pub fn open_read(path: &Path) -> Result<Connection, AppError> {
    let connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    configure_connection(&connection)?;
    require_current_schema(&connection)?;
    Ok(connection)
}

/// Inspect a supported library before migration without changing its journal mode.
pub fn open_migration_source(path: &Path) -> Result<Connection, AppError> {
    let connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    configure_connection(&connection)?;
    let version = schema_version(&connection)?;
    if version < FRESH_STATE_SCHEMA_VERSION {
        return Err(schema_incompatible(version));
    }
    if version > CURRENT_SCHEMA_VERSION {
        return Err(schema_too_new(version));
    }
    if version == CURRENT_SCHEMA_VERSION {
        library_kind(&connection)?;
    }
    Ok(connection)
}

/// Open a current-format library for writes.
pub fn open_write(path: &Path) -> Result<Connection, AppError> {
    let connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    configure_connection(&connection)?;
    require_current_schema(&connection)?;
    enable_wal(&connection)?;
    Ok(connection)
}

/// Migrate a version 3, 4, or 5 library to the current additive format.
///
/// Version 3 remains the deliberate fresh-state boundary. `migrate` never
/// reinterprets a library older than that boundary.
pub fn migrate(path: &Path) -> Result<MigrationResult, AppError> {
    let mut connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    configure_connection(&connection)?;
    let from_version = schema_version(&connection)?;
    let migrated = match from_version.cmp(&CURRENT_SCHEMA_VERSION) {
        std::cmp::Ordering::Equal => false,
        std::cmp::Ordering::Greater => return Err(schema_too_new(from_version)),
        std::cmp::Ordering::Less if from_version < FRESH_STATE_SCHEMA_VERSION => {
            return Err(schema_incompatible(from_version));
        }
        std::cmp::Ordering::Less => {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let locked_version = schema_version(&transaction)?;
            if locked_version == CURRENT_SCHEMA_VERSION {
                transaction.commit()?;
                false
            } else if matches!(locked_version, 3..=6) {
                if locked_version == 3 {
                    transaction.execute_batch(MIGRATION_3_TO_4).map_err(|error| {
                        AppError::database(
                            "schema_migration_failed",
                            format!(
                                "unable to migrate library schema from version 3 to version 4: {error}"
                            ),
                        )
                    })?;
                }
                if locked_version <= 4 {
                    transaction.execute_batch(MIGRATION_4_TO_5).map_err(|error| {
                        AppError::database(
                            "schema_migration_failed",
                            format!(
                                "unable to migrate library schema from version 4 to version 5: {error}"
                            ),
                        )
                    })?;
                }
                if locked_version <= 5 {
                    transaction.execute_batch(MIGRATION_5_TO_6).map_err(|error| {
                    AppError::database(
                        "schema_migration_failed",
                        format!(
                            "unable to migrate library schema from version 5 to version 6: {error}"
                        ),
                    )
                })?;
                    crate::instructions::initialize(&transaction)?;
                }
                transaction
                    .execute_batch(MIGRATION_6_TO_7)
                    .map_err(|error| {
                        AppError::database("schema_migration_failed", error.to_string())
                    })?;
                transaction.commit().map_err(|error| {
                    AppError::database(
                        "schema_migration_failed",
                        format!("unable to commit library schema migration: {error}"),
                    )
                })?;
                true
            } else if locked_version > CURRENT_SCHEMA_VERSION {
                return Err(schema_too_new(locked_version));
            } else {
                return Err(schema_incompatible(locked_version));
            }
        }
    };
    require_current_schema(&connection)?;
    enable_wal(&connection)?;
    Ok(MigrationResult {
        from_version,
        to_version: CURRENT_SCHEMA_VERSION,
        migrated,
    })
}

fn initialize_reserved_file(
    path: &Path,
    kind: LibraryKind,
    library_id: Option<&str>,
) -> Result<Connection, AppError> {
    let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| open_error(path, &error))?;
    crate::sqlite::persist_wal(&connection)?;
    configure_connection(&connection)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(SCHEMA).map_err(|error| {
        AppError::database(
            "schema_creation_failed",
            format!("unable to create the library schema: {error}"),
        )
    })?;
    transaction
        .execute(
            "INSERT INTO library_profile(singleton, kind) VALUES(1, ?1)",
            [kind.as_str()],
        )
        .map_err(|error| {
            AppError::database(
                "schema_creation_failed",
                format!("unable to establish the library profile: {error}"),
            )
        })?;
    if let Some(library_id) = library_id {
        transaction.execute(
            "UPDATE library_identity SET library_id = ?1 WHERE singleton = 1",
            [library_id],
        )?;
    }
    crate::instructions::initialize(&transaction)?;
    transaction.commit()?;
    require_current_schema(&connection)?;
    enable_wal(&connection)?;
    Ok(connection)
}

fn open_existing(path: &Path, flags: OpenFlags) -> Result<Connection, AppError> {
    if !path.exists() {
        return Err(AppError::not_found(
            "library_not_found",
            format!("library not found: {}", path.display()),
        ));
    }
    let connection =
        Connection::open_with_flags(path, flags).map_err(|error| open_error(path, &error))?;
    if flags.contains(OpenFlags::SQLITE_OPEN_READ_WRITE) {
        crate::sqlite::persist_wal(&connection)?;
    }
    Ok(connection)
}

fn configure_connection(connection: &Connection) -> Result<(), AppError> {
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|error| {
            AppError::database(
                "database_configuration_failed",
                format!("unable to enable SQLite foreign keys: {error}"),
            )
        })?;
    connection.busy_timeout(BUSY_TIMEOUT).map_err(|error| {
        AppError::database(
            "database_configuration_failed",
            format!("unable to set the SQLite busy timeout: {error}"),
        )
    })
}

fn schema_version(connection: &Connection) -> Result<i64, AppError> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| {
            AppError::database(
                "schema_version_read_failed",
                format!("unable to read library schema version: {error}"),
            )
        })
}

fn require_current_schema(connection: &Connection) -> Result<(), AppError> {
    let version = schema_version(connection)?;
    match version.cmp(&CURRENT_SCHEMA_VERSION) {
        std::cmp::Ordering::Equal => {
            library_kind(connection)?;
            crate::instructions::current(connection)?;
            Ok(())
        }
        std::cmp::Ordering::Greater => Err(schema_too_new(version)),
        std::cmp::Ordering::Less if version < FRESH_STATE_SCHEMA_VERSION => {
            Err(schema_incompatible(version))
        }
        std::cmp::Ordering::Less => Err(AppError::database(
            "library_schema_migration_required",
            format!(
                "library schema version {version} must be migrated to version {CURRENT_SCHEMA_VERSION}; run `annals migrate`"
            ),
        )),
    }
}

fn schema_too_new(version: i64) -> AppError {
    AppError::database(
        "library_schema_too_new",
        format!(
            "library schema version {version} is newer than supported version {CURRENT_SCHEMA_VERSION}"
        ),
    )
}

fn schema_incompatible(version: i64) -> AppError {
    AppError::database(
        "library_schema_incompatible",
        format!(
            "library schema version {version} predates the fresh-state format {FRESH_STATE_SCHEMA_VERSION}; create a fresh library instead of migrating this file"
        ),
    )
}

fn enable_wal(connection: &Connection) -> Result<(), AppError> {
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| {
            AppError::database(
                "database_configuration_failed",
                format!("unable to enable SQLite WAL mode: {error}"),
            )
        })?;
    // WAL opening is lazy. Materialize its coordination files while this
    // writer owns directory access, including a newly initialized library.
    schema_version(connection)?;
    Ok(())
}

fn reserve_new_file(path: &Path, code: &'static str, description: &str) -> Result<(), AppError> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => {
            drop(file);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(AppError::conflict(
            code,
            format!("{description} already exists: {}", path.display()),
        )),
        Err(error) => Err(AppError::database(
            "file_create_failed",
            format!("unable to create {description} {}: {error}", path.display()),
        )),
    }
}

fn open_error(path: &Path, error: &rusqlite::Error) -> AppError {
    AppError::database(
        "database_open_failed",
        format!("unable to open library {}: {error}", path.display()),
    )
}
