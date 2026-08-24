use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::error::AppError;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const CURRENT_SCHEMA_VERSION: i64 = 2;
const SCHEMA: &str = include_str!("../schema.sql");
const MIGRATION_0_TO_1: &str = include_str!("../migrations/0001_ingestions.sql");
const MIGRATION_1_TO_2: &str = include_str!("../migrations/0002_reconciliation_drafts.sql");
type SchemaObject = (String, String, String, Option<String>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationResult {
    pub from_version: i64,
    pub to_version: i64,
    pub migrated: bool,
}

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

/// Open a library for reads without changing journal mode.
pub fn open_read(path: &Path) -> Result<Connection, AppError> {
    let connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    configure_connection(&connection)?;
    Ok(connection)
}

/// Open an existing library for writes.
pub fn open_write(path: &Path) -> Result<Connection, AppError> {
    let connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    configure_connection(&connection)?;
    enable_wal(&connection)?;
    Ok(connection)
}

/// Upgrade an existing library to the schema understood by this executable.
pub fn migrate(path: &Path) -> Result<MigrationResult, AppError> {
    let mut connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    configure_connection(&connection)?;

    let from_version = schema_version(&connection)?;
    if from_version > CURRENT_SCHEMA_VERSION {
        return Err(AppError::database(
            "library_schema_too_new",
            format!(
                "library schema version {from_version} is newer than supported version {CURRENT_SCHEMA_VERSION}"
            ),
        ));
    }
    enable_wal(&connection)?;
    if from_version == CURRENT_SCHEMA_VERSION {
        return Ok(MigrationResult {
            from_version,
            to_version: CURRENT_SCHEMA_VERSION,
            migrated: false,
        });
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            AppError::database(
                "schema_migration_failed",
                format!("unable to begin library schema migration: {error}"),
            )
        })?;
    if from_version == 0 {
        migrate_zero_to_one(&transaction)?;
    }
    if from_version <= 1 {
        migrate_one_to_two(&transaction)?;
    }
    transaction
        .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
        .map_err(|error| {
            AppError::database(
                "schema_migration_failed",
                format!(
                    "unable to record library schema version {CURRENT_SCHEMA_VERSION}: {error}"
                ),
            )
        })?;
    transaction.commit().map_err(|error| {
        AppError::database(
            "schema_migration_failed",
            format!("unable to commit library schema migration: {error}"),
        )
    })?;

    Ok(MigrationResult {
        from_version,
        to_version: CURRENT_SCHEMA_VERSION,
        migrated: true,
    })
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
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| open_error(path, &error))?;
    configure_connection(&connection)?;
    connection.execute_batch(SCHEMA).map_err(|error| {
        AppError::database(
            "schema_creation_failed",
            format!("unable to create the library schema: {error}"),
        )
    })?;
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

fn migrate_zero_to_one(connection: &Connection) -> Result<(), AppError> {
    let existing = ingestion_schema_objects(connection)?;
    if existing.is_empty() {
        return connection.execute_batch(MIGRATION_0_TO_1).map_err(|error| {
            AppError::database(
                "schema_migration_failed",
                format!("unable to migrate library schema from version 0 to 1: {error}"),
            )
        });
    }

    let reference = Connection::open_in_memory().map_err(|error| {
        AppError::database(
            "schema_migration_failed",
            format!("unable to prepare the version 1 schema reference: {error}"),
        )
    })?;
    reference.execute_batch(MIGRATION_0_TO_1).map_err(|error| {
        AppError::database(
            "schema_migration_failed",
            format!("unable to prepare the version 1 schema reference: {error}"),
        )
    })?;
    if existing != ingestion_schema_objects(&reference)? {
        return Err(AppError::database(
            "schema_migration_failed",
            "version 0 library has an unexpected ingestions schema",
        ));
    }

    Ok(())
}

fn migrate_one_to_two(connection: &Connection) -> Result<(), AppError> {
    let already_present = connection.query_row(
        "SELECT \
             EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' \
                    AND name = 'reconciliation_drafts') \
             AND EXISTS(SELECT 1 FROM pragma_table_info('reconciliations') \
                        WHERE name = 'reconciliation_draft_id')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if already_present {
        return Ok(());
    }
    connection.execute_batch(MIGRATION_1_TO_2).map_err(|error| {
        AppError::database(
            "schema_migration_failed",
            format!("unable to migrate library schema from version 1 to 2: {error}"),
        )
    })
}

fn ingestion_schema_objects(connection: &Connection) -> Result<Vec<SchemaObject>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_schema \
             WHERE name = 'ingestions' OR tbl_name = 'ingestions' \
             ORDER BY type, name, tbl_name, coalesce(sql, '')",
        )
        .map_err(|error| {
            AppError::database(
                "schema_migration_failed",
                format!("unable to inspect the version 0 ingestions schema: {error}"),
            )
        })?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(|error| {
            AppError::database(
                "schema_migration_failed",
                format!("unable to inspect the version 0 ingestions schema: {error}"),
            )
        })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|error| {
        AppError::database(
            "schema_migration_failed",
            format!("unable to inspect the version 0 ingestions schema: {error}"),
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
        assert_eq!(library_revision(&connection)?, 0);
        assert_eq!(
            connection.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))?,
            1
        );
        assert_eq!(
            connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?,
            "wal"
        );
        assert_eq!(schema_version(&connection)?, CURRENT_SCHEMA_VERSION);
        drop(connection);

        let read_connection = open_read(&path)?;
        assert_eq!(library_revision(&read_connection)?, 0);
        drop(read_connection);

        let write_connection = open_write(&path)?;
        assert_eq!(library_revision(&write_connection)?, 0);
        Ok(())
    }

    #[test]
    fn migrates_a_version_zero_library_without_ingestions() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let connection = version_zero_library(&path)?;
        connection.execute(
            "INSERT INTO works(label, normalized_label, text, sha256, created_at) \
             VALUES('Legacy', 'legacy', 'text', ?1, '2026-08-17T00:00:00Z')",
            ["0".repeat(64)],
        )?;
        drop(connection);

        let result = migrate(&path)?;
        assert_eq!(
            result,
            MigrationResult {
                from_version: 0,
                to_version: CURRENT_SCHEMA_VERSION,
                migrated: true,
            }
        );

        let connection = open_read(&path)?;
        assert_eq!(schema_version(&connection)?, CURRENT_SCHEMA_VERSION);
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM works", [], |row| row.get::<_, i64>(0))?,
            1
        );
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM ingestions", [], |row| {
                row.get::<_, i64>(0)
            })?,
            0
        );
        assert_eq!(
            connection.query_row(
                "SELECT COUNT(*) FROM sqlite_schema \
                 WHERE type = 'index' AND name LIKE 'ingestions_%'",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            6
        );
        Ok(())
    }

    #[test]
    fn migrates_a_version_zero_library_with_existing_ingestions() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let connection = init(&path)?;
        connection.execute(
            "INSERT INTO ingestions(\
                 id, delivery_key, source_name, channel, source_size_bytes, source_created_at, \
                 source_modified_at, first_seen_at, status\
             ) VALUES(41, 'legacy-delivery', 'legacy.jsonl', 'inbox', 123, \
                      '2026-08-16T23:58:00Z', '2026-08-16T23:59:00Z', \
                      '2026-08-17T00:00:00Z', 'processing')",
            [],
        )?;
        connection.pragma_update(None, "user_version", 0)?;
        drop(connection);

        assert_eq!(
            migrate(&path)?,
            MigrationResult {
                from_version: 0,
                to_version: CURRENT_SCHEMA_VERSION,
                migrated: true,
            }
        );

        let connection = open_read(&path)?;
        assert_eq!(schema_version(&connection)?, CURRENT_SCHEMA_VERSION);
        let receipt: (
            i64,
            String,
            String,
            String,
            i64,
            String,
            String,
            String,
            String,
        ) = connection.query_row(
            "SELECT id, delivery_key, source_name, channel, source_size_bytes, \
                        source_created_at, source_modified_at, first_seen_at, status \
                 FROM ingestions",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )?;
        assert_eq!(
            receipt,
            (
                41,
                "legacy-delivery".to_owned(),
                "legacy.jsonl".to_owned(),
                "inbox".to_owned(),
                123,
                "2026-08-16T23:58:00Z".to_owned(),
                "2026-08-16T23:59:00Z".to_owned(),
                "2026-08-17T00:00:00Z".to_owned(),
                "processing".to_owned(),
            )
        );
        assert_eq!(
            connection.query_row(
                "SELECT seq FROM sqlite_sequence WHERE name = 'ingestions'",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            41
        );
        Ok(())
    }

    #[test]
    fn migration_refuses_an_unexpected_version_zero_ingestions_schema() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let connection = init(&path)?;
        connection.execute_batch(
            "ALTER TABLE ingestions ADD COLUMN unexpected TEXT;
             PRAGMA user_version = 0;",
        )?;
        drop(connection);

        let Err(error) = migrate(&path) else {
            return Err("unexpected version 0 ingestions schema was accepted".into());
        };
        assert_eq!(error.code(), "schema_migration_failed");

        let connection = open_read(&path)?;
        assert_eq!(schema_version(&connection)?, 0);
        assert_eq!(
            connection.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('ingestions') \
                 WHERE name = 'unexpected'",
                [],
                |row| row.get::<_, i64>(0),
            )?,
            1
        );
        Ok(())
    }

    #[test]
    fn migration_is_idempotent_at_the_current_version() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        drop(version_zero_library(&path)?);

        assert!(migrate(&path)?.migrated);
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
    fn migrates_version_one_model_provenance_into_a_finalized_draft() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let connection = version_one_library(&path)?;
        let request = r#"{
            "summary":"Legacy model request",
            "operations":[{
                "action":"create_concept",
                "ref":"legacy",
                "label":"Legacy",
                "parents":[],
                "evidence":[{"quote":"Legacy source."}]
            }],
            "annotations":["Retained annotation"]
        }"#;
        connection.execute(
            "INSERT INTO works(id, label, normalized_label, text, sha256, created_at) \
             VALUES(1, 'Legacy', 'legacy', 'Legacy source.', ?1, '2026-08-17T00:00:00Z')",
            ["0".repeat(64)],
        )?;
        connection.execute(
            "INSERT INTO model_runs(\
                 id, token, work_id, base_revision, status, model, reasoning_effort, \
                 prompt_version, created_at, completed_at\
             ) VALUES(1, 'legacy-run', 1, 0, 'submitted', 'legacy-model', 'high', \
                      'liaison-v3', '2026-08-17T00:00:00Z', '2026-08-17T00:01:00Z')",
            [],
        )?;
        connection.execute(
            "INSERT INTO tool_calls(\
                 model_run_id, sequence, tool_name, arguments, result, succeeded, created_at\
             ) VALUES(1, 0, 'submit_reconciliation', ?1, '{\"recorded\":true}', 1, \
                      '2026-08-17T00:00:30Z')",
            [request],
        )?;
        connection.execute(
            "INSERT INTO reconciliations(\
                 id, work_id, base_revision, model_run_id, status, summary, submitted_request, \
                 resolved_reconciliation, actor, created_at\
             ) VALUES(1, 1, 0, 1, 'recorded', 'Legacy model request', ?1, '{}', 'model', \
                      '2026-08-17T00:00:30Z')",
            [request],
        )?;
        drop(connection);

        let result = migrate(&path)?;
        assert_eq!(
            result,
            MigrationResult {
                from_version: 1,
                to_version: CURRENT_SCHEMA_VERSION,
                migrated: true,
            }
        );
        let connection = open_read(&path)?;
        assert_eq!(
            connection.query_row("SELECT status FROM reconciliation_drafts", [], |row| row
                .get::<_, String>(
                0
            ))?,
            "finalized"
        );
        assert_eq!(
            connection.query_row(
                "SELECT operation ->> '$.ref' FROM reconciliation_draft_operations",
                [],
                |row| row.get::<_, String>(0)
            )?,
            "legacy"
        );
        assert!(connection.query_row(
            "SELECT reconciliation_draft_id IS NOT NULL FROM reconciliations",
            [],
            |row| row.get::<_, bool>(0)
        )?);
        Ok(())
    }

    #[test]
    fn migration_refuses_a_future_schema_version() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let connection = init(&path)?;
        connection.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION + 1)?;
        drop(connection);

        let Err(error) = migrate(&path) else {
            return Err("future schema version was accepted".into());
        };
        assert_eq!(error.code(), "library_schema_too_new");
        let connection = open_read(&path)?;
        assert_eq!(schema_version(&connection)?, CURRENT_SCHEMA_VERSION + 1);
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
        assert_eq!(library_revision(&backup_connection)?, 0);
        Ok(())
    }

    #[test]
    fn canonical_graph_constraints_are_enforced() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let connection = init(&path)?;

        connection.execute(
            "INSERT INTO concepts(label) VALUES('Same'), ('Same'), ('Shared'), ('Leaf')",
            [],
        )?;
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM concepts", [], |row| row
                .get::<_, i64>(0))?,
            4
        );
        connection.execute(
            "INSERT INTO concept_edges(parent_id, child_id) VALUES(1, 3), (2, 3), (3, 4)",
            [],
        )?;
        assert!(
            connection
                .execute(
                    "INSERT INTO concept_edges(parent_id, child_id) VALUES(1, 1)",
                    [],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "INSERT INTO concept_edges(parent_id, child_id) VALUES(999, 1)",
                    [],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "INSERT INTO concept_edges(parent_id, child_id) VALUES(1, 3)",
                    [],
                )
                .is_err()
        );

        connection.execute(
            "INSERT INTO works(label, normalized_label, text, sha256, created_at) \
             VALUES(?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["Source", "source", "x", "0".repeat(64), "now"],
        )?;
        connection.execute(
            "INSERT INTO evidence(concept_id, work_id, start_byte, end_byte) VALUES(3, 1, 0, 1)",
            [],
        )?;
        assert!(
            connection
                .execute(
                    "INSERT INTO evidence(concept_id, work_id, start_byte, end_byte) \
                     VALUES(3, 1, 0, 1)",
                    [],
                )
                .is_err()
        );

        connection.execute("DELETE FROM concepts WHERE id = 3", [])?;
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM concept_edges", [], |row| {
                row.get::<_, i64>(0)
            })?,
            0
        );
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM evidence", [], |row| row
                .get::<_, i64>(0))?,
            0
        );
        Ok(())
    }

    fn library_revision(connection: &Connection) -> rusqlite::Result<i64> {
        connection.query_row(
            "SELECT revision FROM library_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
    }

    fn version_zero_library(path: &Path) -> Result<Connection, AppError> {
        let connection = init(path)?;
        connection.execute_batch(
            "DROP INDEX ingestions_one_new_work_per_work;
             DROP INDEX ingestions_by_completed;
             DROP INDEX ingestions_by_ingested;
             DROP INDEX ingestions_by_first_seen;
             DROP INDEX ingestions_by_modified;
             DROP INDEX ingestions_by_created;
             DROP TABLE ingestions;
             PRAGMA user_version = 0;",
        )?;
        Ok(connection)
    }

    fn version_one_library(path: &Path) -> Result<Connection, AppError> {
        let connection = init(path)?;
        connection.execute_batch(
            "DROP INDEX reconciliations_one_per_draft;
             ALTER TABLE reconciliations DROP COLUMN reconciliation_draft_id;
             DROP TABLE reconciliation_draft_operations;
             DROP TABLE reconciliation_drafts;
             CREATE UNIQUE INDEX tool_calls_one_successful_submission
                 ON tool_calls(model_run_id)
                 WHERE tool_name = 'submit_reconciliation' AND succeeded = 1;
             PRAGMA user_version = 1;",
        )?;
        Ok(connection)
    }
}
