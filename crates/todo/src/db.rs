use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::error::AppError;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 1;
const SCHEMA: &str = include_str!("../schema.sql");

/// Create and initialize a fresh Todo database without replacing a path.
pub(crate) fn init(path: &Path) -> Result<Connection, AppError> {
    reserve_new_file(path)?;
    match initialize_reserved_file(path) {
        Ok(connection) => Ok(connection),
        Err(error) => {
            let _ = fs::remove_file(path);
            Err(error)
        }
    }
}

pub(crate) fn open_read(path: &Path) -> Result<Connection, AppError> {
    let connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    configure_connection(&connection)?;
    require_current_schema(&connection)?;
    Ok(connection)
}

pub(crate) fn open_write(path: &Path) -> Result<Connection, AppError> {
    let connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    configure_connection(&connection)?;
    require_current_schema(&connection)?;
    enable_wal(&connection)?;
    Ok(connection)
}

fn initialize_reserved_file(path: &Path) -> Result<Connection, AppError> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| open_error(path, &error))?;
    configure_connection(&connection)?;
    connection.execute_batch(SCHEMA).map_err(|error| {
        AppError::database(
            "schema_creation_failed",
            format!("unable to create the Todo schema: {error}"),
        )
    })?;
    require_current_schema(&connection)?;
    enable_wal(&connection)?;
    Ok(connection)
}

fn open_existing(path: &Path, flags: OpenFlags) -> Result<Connection, AppError> {
    if !path.exists() {
        return Err(AppError::not_found(
            "database_not_found",
            format!("database not found: {}", path.display()),
        ));
    }
    Connection::open_with_flags(path, flags).map_err(|error| open_error(path, &error))
}

fn configure_connection(connection: &Connection) -> Result<(), AppError> {
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

fn require_current_schema(connection: &Connection) -> Result<(), AppError> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| {
            AppError::database(
                "schema_version_read_failed",
                format!("unable to read the database schema version: {error}"),
            )
        })?;
    match version.cmp(&CURRENT_SCHEMA_VERSION) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Greater => Err(AppError::database(
            "database_schema_too_new",
            format!(
                "database schema version {version} is newer than supported version {CURRENT_SCHEMA_VERSION}"
            ),
        )),
        std::cmp::Ordering::Less => Err(AppError::database(
            "database_schema_too_old",
            format!(
                "database schema version {version} is older than supported version {CURRENT_SCHEMA_VERSION}"
            ),
        )),
    }
}

fn enable_wal(connection: &Connection) -> Result<(), AppError> {
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| {
            AppError::database(
                "database_configuration_failed",
                format!("unable to enable SQLite WAL mode: {error}"),
            )
        })
}

fn reserve_new_file(path: &Path) -> Result<(), AppError> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => {
            drop(file);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(AppError::conflict(
            "database_exists",
            format!("database already exists: {}", path.display()),
        )),
        Err(error) => Err(AppError::database(
            "database_create_failed",
            format!("unable to create database {}: {error}", path.display()),
        )),
    }
}

fn open_error(path: &Path, error: &rusqlite::Error) -> AppError {
    AppError::database(
        "database_open_failed",
        format!("unable to open database {}: {error}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{CURRENT_SCHEMA_VERSION, init, open_read, open_write};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn initializes_and_reopens_a_current_database() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("todo.db");
        let connection = init(&path)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        drop(connection);
        drop(open_read(&path)?);
        drop(open_write(&path)?);
        Ok(())
    }

    #[test]
    fn init_refuses_to_replace_an_existing_file() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("todo.db");
        drop(init(&path)?);
        let Err(error) = init(&path) else {
            return Err("init unexpectedly replaced a database".into());
        };
        assert_eq!(error.code(), "database_exists");
        Ok(())
    }

    #[test]
    fn opens_reject_an_unknown_schema_version() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("todo.db");
        let connection = Connection::open(&path)?;
        connection.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION + 1)?;
        drop(connection);
        let Err(error) = open_read(&path) else {
            return Err("newer schema unexpectedly opened".into());
        };
        assert_eq!(error.code(), "database_schema_too_new");
        Ok(())
    }
}
