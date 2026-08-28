use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::error::AppError;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 4;
const FRESH_STATE_SCHEMA_VERSION: i64 = 3;
const SCHEMA: &str = include_str!("../schema.sql");
const MIGRATION_3_TO_4: &str = include_str!("../migrations/3-to-4.sql");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationResult {
    pub from_version: i64,
    pub to_version: i64,
    pub migrated: bool,
}

/// Create and initialize a fresh Annals library without replacing a path.
pub fn init(path: &Path) -> Result<Connection, AppError> {
    reserve_new_file(path, "library_exists", "library")?;
    match initialize_reserved_file(path) {
        Ok(connection) => Ok(connection),
        Err(error) => {
            // This call exclusively created the path, so cleanup cannot remove
            // a pre-existing library.
            let _ = fs::remove_file(path);
            Err(error)
        }
    }
}

/// Open a current-format library for reads without changing journal mode.
pub fn open_read(path: &Path) -> Result<Connection, AppError> {
    let connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    configure_connection(&connection)?;
    require_current_schema(&connection)?;
    Ok(connection)
}

/// Open a library at the fresh-state boundary or current version solely as a
/// consistent pre-migration backup source.
pub fn open_backup_source(path: &Path) -> Result<Connection, AppError> {
    let connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    configure_connection(&connection)?;
    let version = schema_version(&connection)?;
    if version < FRESH_STATE_SCHEMA_VERSION {
        return Err(schema_incompatible(version));
    }
    if version > CURRENT_SCHEMA_VERSION {
        return Err(schema_too_new(version));
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

/// Migrate a version 3 library to the current additive format.
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
            } else if locked_version == FRESH_STATE_SCHEMA_VERSION {
                transaction.execute_batch(MIGRATION_3_TO_4).map_err(|error| {
                    AppError::database(
                        "schema_migration_failed",
                        format!(
                            "unable to migrate library schema from version {locked_version} to version {CURRENT_SCHEMA_VERSION}: {error}"
                        ),
                    )
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

/// Copy a consistent `SQLite` backup without replacing an existing output path.
pub fn backup(source: &Connection, output: &Path) -> Result<(), AppError> {
    reserve_new_file(output, "backup_exists", "backup output")?;
    let result = backup_to_reserved_file(source, output);
    if result.is_err() {
        let _ = fs::remove_file(output);
    }
    result
}

fn initialize_reserved_file(path: &Path) -> Result<Connection, AppError> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| open_error(path, &error))?;
    configure_connection(&connection)?;
    connection.execute_batch(SCHEMA).map_err(|error| {
        AppError::database(
            "schema_creation_failed",
            format!("unable to create the library schema: {error}"),
        )
    })?;
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
        std::cmp::Ordering::Equal => Ok(()),
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
    fn initializes_and_reopens_a_current_library() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let connection = init(&path)?;
        assert_eq!(schema_version(&connection)?, CURRENT_SCHEMA_VERSION);
        assert_eq!(head_revision(&connection)?, 0);
        drop(connection);
        assert_eq!(head_revision(&open_read(&path)?)?, 0);
        assert_eq!(head_revision(&open_write(&path)?)?, 0);
        Ok(())
    }

    #[test]
    fn normal_open_and_migrate_reject_legacy_without_mutating_it() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("legacy.db");
        let connection = Connection::open(&path)?;
        connection.execute_batch("CREATE TABLE legacy(value TEXT); PRAGMA user_version = 2;")?;
        drop(connection);

        let Err(read_error) = open_read(&path) else {
            return Err("legacy library unexpectedly opened for reads".into());
        };
        let Err(write_error) = open_write(&path) else {
            return Err("legacy library unexpectedly opened for writes".into());
        };
        let Err(backup_error) = open_backup_source(&path) else {
            return Err("legacy library unexpectedly opened for backup".into());
        };
        let Err(migrate_error) = migrate(&path) else {
            return Err("legacy library unexpectedly migrated".into());
        };
        for error in [read_error, write_error, backup_error, migrate_error] {
            assert_eq!(error.code(), "library_schema_incompatible");
        }
        let connection = Connection::open(&path)?;
        assert_eq!(schema_version(&connection)?, 2);
        assert!(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'legacy')",
            [],
            |row| row.get::<_, bool>(0)
        )?);
        Ok(())
    }

    #[test]
    fn migrates_version_three_additively_and_preserves_deliveries() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let connection = init(&path)?;
        connection.execute(
            "INSERT INTO works(label, normalized_label, text, sha256, created_at)
             VALUES('work', 'work', 'source', ?1, 'now')",
            ["0".repeat(64)],
        )?;
        connection.execute(
            "INSERT INTO ingestions(
                 delivery_key, source_name, channel, first_seen_at, ingested_at,
                 completed_at, status, work_id, new_work, error_code, error_message
             ) VALUES(
                 'inbox:j00000000000000000001:seen', 'source.txt', 'inbox', 'seen',
                 'ingested', 'completed', 'failed', 1, 1, 'model_runner_failed',
                 'source delivery failed'
             )",
            [],
        )?;
        connection.execute_batch(
            "DROP TABLE inbox_retry_items;
             DROP TABLE inbox_retry_events;
             PRAGMA user_version = 3;",
        )?;
        drop(connection);

        let Err(error) = open_read(&path) else {
            return Err("version 3 library unexpectedly opened before migration".into());
        };
        assert_eq!(error.code(), "library_schema_migration_required");
        assert_eq!(
            open_backup_source(&path)?
                .query_row("SELECT COUNT(*) FROM ingestions", [], |row| row
                    .get::<_, i64>(0))?,
            1
        );
        assert_eq!(
            Connection::open(&path)?
                .pragma_query_value(None, "user_version", |row| { row.get::<_, i64>(0) })?,
            3
        );

        assert_eq!(
            migrate(&path)?,
            MigrationResult {
                from_version: 3,
                to_version: CURRENT_SCHEMA_VERSION,
                migrated: true,
            }
        );
        let connection = open_read(&path)?;
        assert_eq!(schema_version(&connection)?, CURRENT_SCHEMA_VERSION);
        assert_eq!(
            connection.query_row(
                "SELECT error_code FROM ingestions WHERE id = 1",
                [],
                |row| { row.get::<_, String>(0) }
            )?,
            "model_runner_failed"
        );
        for table in ["inbox_retry_events", "inbox_retry_items"] {
            assert!(connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get::<_, bool>(0)
            )?);
        }
        Ok(())
    }

    #[test]
    fn failed_version_three_migration_is_atomic() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let connection = init(&path)?;
        connection.execute_batch(
            "DROP TABLE inbox_retry_items;
             DROP TABLE inbox_retry_events;
             CREATE TABLE inbox_retry_events(blocker TEXT);
             PRAGMA user_version = 3;",
        )?;
        drop(connection);

        let Err(error) = migrate(&path) else {
            return Err("conflicting migration unexpectedly succeeded".into());
        };
        assert_eq!(error.code(), "schema_migration_failed");
        let connection = Connection::open(&path)?;
        assert_eq!(schema_version(&connection)?, 3);
        assert_eq!(
            connection.query_row(
                "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'inbox_retry_events'",
                [],
                |row| row.get::<_, String>(0)
            )?,
            "CREATE TABLE inbox_retry_events(blocker TEXT)"
        );
        assert!(!connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'inbox_retry_items')",
            [],
            |row| row.get::<_, bool>(0)
        )?);
        Ok(())
    }

    #[test]
    fn migrate_is_idempotent_for_current_format() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        drop(init(&path)?);
        assert_eq!(
            migrate(&path)?,
            MigrationResult {
                from_version: CURRENT_SCHEMA_VERSION,
                to_version: CURRENT_SCHEMA_VERSION,
                migrated: false,
            }
        );
        Ok(())
    }

    #[test]
    fn finalized_typed_request_rows_are_sealed() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let connection = init(&path)?;
        connection.execute(
            "INSERT INTO works(label, normalized_label, text, sha256, created_at)
             VALUES('work', 'work', 'source', ?1, 'now')",
            ["0".repeat(64)],
        )?;
        connection.execute("INSERT INTO concept_identities DEFAULT VALUES", [])?;
        connection.execute(
            "INSERT INTO reconciliation_requests(work_id, base_revision, summary, created_at)
             VALUES(1, 0, 'summary', 'now')",
            [],
        )?;
        connection.execute(
            "INSERT INTO request_annotations(request_id, ordinal, text)
             VALUES(1, 0, 'annotation')",
            [],
        )?;
        connection.execute(
            "INSERT INTO request_operations(
                 request_id, slot, ordinal, action, status,
                 created_version, last_changed_version
             ) VALUES(1, 1, 0, 'add_evidence', 'staged', 1, 1)",
            [],
        )?;
        connection.execute(
            "INSERT INTO operation_selectors(
                 operation_id, role, ordinal, selector_kind, concept_id
             ) VALUES(1, 'concept', 0, 'existing', 1)",
            [],
        )?;
        connection.execute(
            "INSERT INTO operation_evidence(
                 operation_id, ordinal, quote
             ) VALUES(1, 0, 'source')",
            [],
        )?;
        connection.execute(
            "INSERT INTO operation_evidence_headings(evidence_id, ordinal, component)
             VALUES(1, 0, 'heading')",
            [],
        )?;
        connection.execute(
            "INSERT INTO reconciliations(request_id, status, actor, created_at)
             VALUES(1, 'pending', 'test', 'now')",
            [],
        )?;

        assert!(
            connection
                .execute(
                    "UPDATE reconciliation_requests SET summary = 'changed' WHERE id = 1",
                    [],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "UPDATE request_annotations SET text = 'changed' WHERE request_id = 1",
                    [],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "UPDATE request_operations SET hint = 'changed' WHERE id = 1",
                    [],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "UPDATE operation_selectors SET ordinal = 1 WHERE operation_id = 1",
                    [],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "UPDATE operation_evidence SET quote = 'changed' WHERE id = 1",
                    [],
                )
                .is_err()
        );
        assert!(connection
            .execute(
                "UPDATE operation_evidence_headings SET component = 'changed' WHERE evidence_id = 1",
                [],
            )
            .is_err());
        Ok(())
    }

    #[test]
    fn init_and_backup_refuse_to_overwrite() -> TestResult {
        let directory = tempfile::tempdir()?;
        let library_path = directory.path().join("annals.db");
        let backup_path = directory.path().join("backup.db");
        let connection = init(&library_path)?;
        let Err(init_error) = init(&library_path) else {
            return Err("init unexpectedly replaced a library".into());
        };
        assert_eq!(init_error.code(), "library_exists");
        backup(&connection, &backup_path)?;
        let Err(backup_error) = backup(&connection, &backup_path) else {
            return Err("backup unexpectedly replaced its output".into());
        };
        assert_eq!(backup_error.code(), "backup_exists");
        assert_eq!(head_revision(&open_read(&backup_path)?)?, 0);
        Ok(())
    }

    fn head_revision(connection: &Connection) -> rusqlite::Result<i64> {
        connection.query_row("SELECT revision FROM library_state", [], |row| row.get(0))
    }
}
