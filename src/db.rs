use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};

use crate::error::AppError;
use crate::migrations;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Create and initialize a new Annals library without replacing an existing path.
pub fn init(path: &Path) -> Result<Connection, AppError> {
    reserve_new_file(path, "library_exists", "library")?;

    match initialize_reserved_file(path) {
        Ok(connection) => Ok(connection),
        Err(error) => {
            // The path was created by this call, so removing it cannot delete a
            // pre-existing library. Ignore cleanup failure and preserve the cause.
            let _cleanup_result = fs::remove_file(path);
            Err(error)
        }
    }
}

/// Open a library for reads without running migrations or changing journal mode.
pub fn open_read(path: &Path) -> Result<Connection, AppError> {
    let connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    configure_connection(&connection)?;
    migrations::require_current_schema(&connection)?;
    Ok(connection)
}

/// Open an existing library for validation without applying migrations.
///
/// Validation normally reads data, but FTS5's external-content integrity
/// command is issued as an insert into the virtual table. The command does not
/// modify indexed content, but `SQLite` requires a read-write connection for it.
pub fn open_validation(path: &Path) -> Result<Connection, AppError> {
    let connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    configure_connection(&connection)?;
    migrations::require_current_schema(&connection)?;
    Ok(connection)
}

/// Open a library for writes, applying supported migrations before returning it.
pub fn open_write(path: &Path) -> Result<Connection, AppError> {
    let mut connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    configure_connection(&connection)?;

    // Compatibility is checked before any persistent pragma or migration, so a
    // newer database is rejected without writes.
    migrations::reject_newer_schema(&connection)?;
    migrations::apply_pending_migrations(&mut connection)?;
    enable_wal(&connection)?;
    Ok(connection)
}

/// Copy a consistent `SQLite` snapshot without replacing an existing output path.
pub fn backup(source: &Connection, output: &Path) -> Result<(), AppError> {
    reserve_new_file(output, "backup_exists", "backup output")?;

    let result = backup_to_reserved_file(source, output);
    if result.is_err() {
        let _cleanup_result = fs::remove_file(output);
    }
    result
}

fn initialize_reserved_file(path: &Path) -> Result<Connection, AppError> {
    let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| open_error(path, &error))?;
    configure_connection(&connection)?;
    probe_fts5(&connection)?;
    migrations::apply_pending_migrations(&mut connection)?;
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

fn probe_fts5(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "CREATE VIRTUAL TABLE temp.annals_fts5_probe USING fts5(value); \
             DROP TABLE temp.annals_fts5_probe;",
        )
        .map_err(|error| {
            AppError::database(
                "fts5_unavailable",
                format!("this SQLite build does not provide working FTS5 support: {error}"),
            )
        })
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

fn backup_to_reserved_file(source: &Connection, output: &Path) -> Result<(), AppError> {
    let mut destination = Connection::open_with_flags(output, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| {
            AppError::database(
                "backup_failed",
                format!("unable to open backup output {}: {error}", output.display()),
            )
        })?;
    let backup = Backup::new(source, &mut destination).map_err(|error| {
        AppError::database(
            "backup_failed",
            format!("unable to start SQLite backup: {error}"),
        )
    })?;
    backup
        .run_to_completion(128, Duration::from_millis(10), None)
        .map_err(|error| {
            AppError::database(
                "backup_failed",
                format!("unable to complete SQLite backup: {error}"),
            )
        })
}

fn open_error(path: &Path, error: &rusqlite::Error) -> AppError {
    AppError::database(
        "database_open_failed",
        format!("unable to open library {}: {error}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn initializes_and_reopens_a_library() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");

        let connection = init(&path)?;
        assert_eq!(migrations::schema_version(&connection)?, 1);
        assert_eq!(
            connection.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))?,
            1
        );
        assert_eq!(
            connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?,
            "wal"
        );
        drop(connection);

        let read_connection = open_read(&path)?;
        assert_eq!(migrations::schema_version(&read_connection)?, 1);
        drop(read_connection);

        let write_connection = open_write(&path)?;
        assert_eq!(migrations::schema_version(&write_connection)?, 1);
        Ok(())
    }

    #[test]
    fn init_and_backup_refuse_to_overwrite() -> TestResult {
        let directory = tempfile::tempdir()?;
        let library_path = directory.path().join("annals.db");
        let backup_path = directory.path().join("backup.db");
        let connection = init(&library_path)?;

        let Err(init_error) = init(&library_path) else {
            return Err("init unexpectedly replaced its library".into());
        };
        assert_eq!(init_error.code(), "library_exists");

        backup(&connection, &backup_path)?;
        let Err(backup_error) = backup(&connection, &backup_path) else {
            return Err("backup unexpectedly replaced its output".into());
        };
        assert_eq!(backup_error.code(), "backup_exists");

        let backup_connection = open_read(&backup_path)?;
        assert_eq!(migrations::schema_version(&backup_connection)?, 1);
        Ok(())
    }

    #[test]
    fn newer_schema_is_rejected_without_changes() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let connection = init(&path)?;
        connection.execute("UPDATE schema_migrations SET version = 2", [])?;
        drop(connection);

        let Err(error) = open_write(&path) else {
            return Err("a newer schema was unexpectedly opened for writing".into());
        };
        assert_eq!(error.code(), "unsupported_schema_version");

        let connection = Connection::open(&path)?;
        assert_eq!(migrations::schema_version(&connection)?, 2);
        Ok(())
    }
}
