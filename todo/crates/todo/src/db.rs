use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};

use crate::error::AppError;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const FIRST_MIGRATABLE_SCHEMA_VERSION: i64 = 1;
pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 2;
const SCHEMA: &str = include_str!("../schema.sql");

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) struct MigrationOutcome {
    pub(crate) from_version: i64,
    pub(crate) to_version: i64,
    pub(crate) migrated: bool,
}

/// Create and initialize a fresh Todo database without replacing a path.
pub(crate) fn init(path: &Path) -> Result<Connection, AppError> {
    reserve_new_file(path, "database_exists", "database")?;
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

/// Explicitly upgrade the selected database after retaining a `SQLite` backup.
///
/// A current database is a true no-op: the backup argument is not inspected or
/// touched. Version 1 is the only accepted migration source.
pub(crate) fn migrate(path: &Path, backup_path: &Path) -> Result<MigrationOutcome, AppError> {
    let mut connection = open_existing(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    configure_connection(&connection)?;
    let from_version = schema_version(&connection)?;
    match from_version.cmp(&CURRENT_SCHEMA_VERSION) {
        std::cmp::Ordering::Equal => {
            return Ok(MigrationOutcome {
                from_version,
                to_version: CURRENT_SCHEMA_VERSION,
                migrated: false,
            });
        }
        std::cmp::Ordering::Greater => return Err(schema_too_new(from_version)),
        std::cmp::Ordering::Less if from_version != FIRST_MIGRATABLE_SCHEMA_VERSION => {
            return Err(schema_incompatible(from_version));
        }
        std::cmp::Ordering::Less => {}
    }

    if !backup_path.is_absolute() {
        return Err(AppError::invalid(
            "backup_path_not_absolute",
            "migration backup path must be absolute",
        ));
    }
    backup(&connection, backup_path)?;

    connection
        .pragma_update(None, "foreign_keys", false)
        .map_err(|error| configuration_error(&error))?;
    let migration = migrate_v1_transaction(&mut connection);
    let foreign_keys = connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|error| configuration_error(&error));
    migration?;
    foreign_keys?;

    require_current_schema(&connection)?;
    let violation = connection
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
        .optional()?;
    if violation.is_some() {
        return Err(AppError::database(
            "schema_migration_failed",
            "Todo migration left a foreign-key violation",
        ));
    }
    enable_wal(&connection)?;
    Ok(MigrationOutcome {
        from_version,
        to_version: CURRENT_SCHEMA_VERSION,
        migrated: true,
    })
}

fn migrate_v1_transaction(connection: &mut Connection) -> Result<(), AppError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let locked_version = schema_version(&transaction)?;
    if locked_version != FIRST_MIGRATABLE_SCHEMA_VERSION {
        return Err(if locked_version > CURRENT_SCHEMA_VERSION {
            schema_too_new(locked_version)
        } else {
            AppError::conflict(
                "database_changed_during_migration",
                format!(
                    "database schema changed from version 1 to version {locked_version} while migration was starting"
                ),
            )
        });
    }

    let old_todo_count: i64 =
        transaction.query_row("SELECT count(*) FROM todos", [], |row| row.get(0))?;
    let old_note_count: i64 =
        transaction.query_row("SELECT count(*) FROM todo_notes", [], |row| row.get(0))?;

    transaction.execute_batch(
        "DROP TRIGGER todos_content_immutable;
         DROP TRIGGER todos_cannot_be_deleted;
         DROP TRIGGER todo_notes_immutable_update;
         DROP TRIGGER todo_notes_immutable_delete;
         DROP INDEX IF EXISTS todos_status_created;
         DROP INDEX IF EXISTS todo_notes_parent_order;
         ALTER TABLE todo_notes RENAME TO legacy_v1_todo_notes;
         ALTER TABLE todos RENAME TO legacy_v1_todos;",
    )?;
    transaction.execute_batch(SCHEMA).map_err(|error| {
        AppError::database(
            "schema_migration_failed",
            format!("unable to create Todo schema version 2: {error}"),
        )
    })?;

    copy_v1_rows(&transaction)?;
    validate_v1_copy(&transaction, old_todo_count, old_note_count)?;
    transaction.execute_batch(
        "DROP TABLE legacy_v1_todo_notes;
         DROP TABLE legacy_v1_todos;",
    )?;

    let violation = transaction
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
        .optional()?;
    if violation.is_some() {
        return Err(AppError::database(
            "schema_migration_failed",
            "Todo migration produced a foreign-key violation",
        ));
    }
    transaction.commit().map_err(|error| {
        AppError::database(
            "schema_migration_failed",
            format!("unable to commit Todo schema migration: {error}"),
        )
    })
}

fn copy_v1_rows(transaction: &Transaction<'_>) -> Result<(), AppError> {
    transaction
        .execute_batch(
            "INSERT INTO todos(id, status, created_at, completed_at)
         SELECT id, status, created_at, completed_at
         FROM legacy_v1_todos;

         INSERT INTO concerns(
             id, body, source_path, status, created_at, resolved_at
         )
         SELECT id, pointer, source_path, 'attached', created_at, created_at
         FROM legacy_v1_todos;

         INSERT INTO todo_direction_revisions(
             id, todo_id, revision, title, body, source_concern_id,
             source_routing_id, provenance_kind, created_at
         )
         SELECT id, id, 1, title, pointer, id, NULL, 'legacy_v1', created_at
         FROM legacy_v1_todos;

         INSERT INTO todo_concerns(
             id, todo_id, concern_id, authorized_routing_id, attached_at
         )
         SELECT id, id, id, NULL, created_at
         FROM legacy_v1_todos;

         INSERT INTO todo_notes(id, todo_id, text, created_at)
         SELECT id, todo_id, text, created_at
         FROM legacy_v1_todo_notes;

         INSERT INTO todo_designs(
             id, todo_id, revision, assessment_id, based_on_design_id,
             agent_job_id, draft_version, state, summary, canonical_digest,
             producer_tool_call_id, created_at
         )
         SELECT id, id, 1, NULL, NULL, NULL, 1, 'legacy_unreviewed', note,
                NULL, NULL, created_at
         FROM legacy_v1_todos;",
        )
        .map_err(|error| {
            AppError::database(
                "schema_migration_failed",
                format!("unable to preserve version-1 Todo rows: {error}"),
            )
        })?;
    Ok(())
}

fn validate_v1_copy(
    transaction: &Transaction<'_>,
    old_todo_count: i64,
    old_note_count: i64,
) -> Result<(), AppError> {
    let new_todo_count: i64 =
        transaction.query_row("SELECT count(*) FROM todos", [], |row| row.get(0))?;
    let new_concern_count: i64 =
        transaction.query_row("SELECT count(*) FROM concerns", [], |row| row.get(0))?;
    let new_direction_count: i64 =
        transaction.query_row("SELECT count(*) FROM todo_direction_revisions", [], |row| {
            row.get(0)
        })?;
    let new_design_count: i64 =
        transaction.query_row("SELECT count(*) FROM todo_designs", [], |row| row.get(0))?;
    let new_note_count: i64 =
        transaction.query_row("SELECT count(*) FROM todo_notes", [], |row| row.get(0))?;
    if [
        new_todo_count,
        new_concern_count,
        new_direction_count,
        new_design_count,
    ]
    .into_iter()
    .any(|count| count != old_todo_count)
        || new_note_count != old_note_count
    {
        return Err(AppError::database(
            "schema_migration_failed",
            "Todo migration did not preserve exact row counts",
        ));
    }

    let todo_mismatch: bool = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1
             FROM legacy_v1_todos AS old
             JOIN todos AS t ON t.id = old.id
             JOIN concerns AS c ON c.id = old.id
             JOIN todo_direction_revisions AS d ON d.id = old.id
             JOIN todo_designs AS design ON design.id = old.id
             WHERE t.status IS NOT old.status
                OR t.created_at IS NOT old.created_at
                OR t.completed_at IS NOT old.completed_at
                OR c.body IS NOT old.pointer
                OR c.source_path IS NOT old.source_path
                OR c.created_at IS NOT old.created_at
                OR d.title IS NOT old.title
                OR d.body IS NOT old.pointer
                OR d.created_at IS NOT old.created_at
                OR design.summary IS NOT old.note
                OR design.created_at IS NOT old.created_at
         )",
        [],
        |row| row.get(0),
    )?;
    let note_mismatch: bool = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1
             FROM legacy_v1_todo_notes AS old
             JOIN todo_notes AS new ON new.id = old.id
             WHERE new.todo_id IS NOT old.todo_id
                OR new.text IS NOT old.text
                OR new.created_at IS NOT old.created_at
         )",
        [],
        |row| row.get(0),
    )?;
    if todo_mismatch || note_mismatch {
        return Err(AppError::database(
            "schema_migration_failed",
            "Todo migration did not preserve version-1 bytes and timestamps",
        ));
    }
    Ok(())
}

fn backup(source: &Connection, output: &Path) -> Result<(), AppError> {
    reserve_new_file(output, "backup_exists", "migration backup")?;
    let result = backup_to_reserved_file(source, output);
    if result.is_err() {
        let _ = fs::remove_file(output);
    }
    result
}

fn backup_to_reserved_file(source: &Connection, output: &Path) -> Result<(), AppError> {
    let mut destination = Connection::open_with_flags(output, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|error| {
            AppError::database(
                "backup_failed",
                format!(
                    "unable to open migration backup {}: {error}",
                    output.display()
                ),
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
        .map_err(|error| configuration_error(&error))?;
    connection.busy_timeout(BUSY_TIMEOUT).map_err(|error| {
        AppError::database(
            "database_configuration_failed",
            format!("unable to set the SQLite busy timeout: {error}"),
        )
    })
}

pub(crate) fn schema_version(connection: &Connection) -> Result<i64, AppError> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| {
            AppError::database(
                "schema_version_read_failed",
                format!("unable to read the database schema version: {error}"),
            )
        })
}

fn require_current_schema(connection: &Connection) -> Result<(), AppError> {
    let version = schema_version(connection)?;
    match version.cmp(&CURRENT_SCHEMA_VERSION) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Greater => Err(schema_too_new(version)),
        std::cmp::Ordering::Less if version == FIRST_MIGRATABLE_SCHEMA_VERSION => {
            Err(AppError::database(
                "database_schema_migration_required",
                format!(
                    "database schema version {version} must be migrated to version {CURRENT_SCHEMA_VERSION}; run `todo migrate --backup ABSOLUTE_PATH`"
                ),
            ))
        }
        std::cmp::Ordering::Less => Err(schema_incompatible(version)),
    }
}

fn schema_too_new(version: i64) -> AppError {
    AppError::database(
        "database_schema_too_new",
        format!(
            "database schema version {version} is newer than supported version {CURRENT_SCHEMA_VERSION}"
        ),
    )
}

fn schema_incompatible(version: i64) -> AppError {
    AppError::database(
        "database_schema_incompatible",
        format!(
            "database schema version {version} cannot be migrated; version {FIRST_MIGRATABLE_SCHEMA_VERSION} is the oldest supported format"
        ),
    )
}

fn enable_wal(connection: &Connection) -> Result<(), AppError> {
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| configuration_error(&error))
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

fn configuration_error(error: &rusqlite::Error) -> AppError {
    AppError::database(
        "database_configuration_failed",
        format!("unable to configure SQLite: {error}"),
    )
}

fn open_error(path: &Path, error: &rusqlite::Error) -> AppError {
    AppError::database(
        "database_open_failed",
        format!("unable to open database {}: {error}", path.display()),
    )
}

use rusqlite::OptionalExtension as _;

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{CURRENT_SCHEMA_VERSION, init, migrate, open_read, open_write, schema_version};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const V1_SCHEMA: &str = r"
        CREATE TABLE todos (
            id INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
            title TEXT NOT NULL,
            note TEXT NOT NULL,
            pointer TEXT NOT NULL,
            source_path TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'open',
            created_at TEXT NOT NULL,
            completed_at TEXT
        );
        CREATE TRIGGER todos_content_immutable
        BEFORE UPDATE OF title, note, pointer, source_path, created_at ON todos BEGIN
            SELECT RAISE(ABORT, 'todo content is immutable');
        END;
        CREATE TRIGGER todos_cannot_be_deleted
        BEFORE DELETE ON todos BEGIN SELECT RAISE(ABORT, 'no delete'); END;
        CREATE TABLE todo_notes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            todo_id INTEGER NOT NULL REFERENCES todos(id) ON DELETE RESTRICT,
            text TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX todos_status_created
            ON todos(status, created_at DESC, id DESC);
        CREATE INDEX todo_notes_parent_order
            ON todo_notes(todo_id, created_at, id);
        CREATE TRIGGER todo_notes_immutable_update
        BEFORE UPDATE ON todo_notes BEGIN SELECT RAISE(ABORT, 'no update'); END;
        CREATE TRIGGER todo_notes_immutable_delete
        BEFORE DELETE ON todo_notes BEGIN SELECT RAISE(ABORT, 'no delete'); END;
        PRAGMA user_version = 1;
    ";

    #[test]
    fn initializes_and_reopens_a_current_database() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("todo.db");
        let connection = init(&path)?;
        assert_eq!(schema_version(&connection)?, CURRENT_SCHEMA_VERSION);
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
    fn normal_opens_require_explicit_v1_migration() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("todo.db");
        let connection = Connection::open(&path)?;
        connection.execute_batch(V1_SCHEMA)?;
        drop(connection);
        let Err(error) = open_read(&path) else {
            return Err("version 1 unexpectedly opened".into());
        };
        assert_eq!(error.code(), "database_schema_migration_required");
        Ok(())
    }

    #[test]
    fn migrates_v1_byte_for_byte_and_retains_backup() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("todo.db");
        let backup = directory.path().join("todo-v1.backup");
        let connection = Connection::open(&path)?;
        connection.execute_batch(V1_SCHEMA)?;
        connection.execute(
            "INSERT INTO todos(id,title,note,pointer,source_path,status,created_at,completed_at)
             VALUES(7,?1,?2,?3,?4,'done',?5,?6)",
            rusqlite::params![
                "Hostile\u{1b} title",
                "Legacy\nbody",
                "Exact direction",
                "/tmp/source.jsonl",
                "2026-08-01T00:00:00.000Z",
                "2026-08-02T00:00:00.000Z"
            ],
        )?;
        connection.execute(
            "INSERT INTO todo_notes(id,todo_id,text,created_at) VALUES(9,7,?1,?2)",
            rusqlite::params!["note\u{0}bytes", "2026-08-03T00:00:00.000Z"],
        )?;
        drop(connection);

        let outcome = migrate(&path, &backup)?;
        assert!(outcome.migrated);
        assert_eq!(schema_version(&Connection::open(&backup)?)?, 1);
        let current = open_read(&path)?;
        let preserved: (String, String, String, String, String, i64) = current.query_row(
            "SELECT d.title, d.body, c.body, design.summary, n.text, n.id
             FROM todo_direction_revisions AS d
             JOIN concerns AS c ON c.id = 7
             JOIN todo_designs AS design ON design.id = 7
             JOIN todo_notes AS n ON n.todo_id = 7
             WHERE d.todo_id = 7",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )?;
        assert_eq!(
            preserved,
            (
                "Hostile\u{1b} title".to_owned(),
                "Exact direction".to_owned(),
                "Exact direction".to_owned(),
                "Legacy\nbody".to_owned(),
                "note\u{0}bytes".to_owned(),
                9,
            )
        );
        Ok(())
    }

    #[test]
    fn current_migrate_is_a_true_backup_no_op() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("todo.db");
        drop(init(&path)?);
        let untouched = directory.path().join("already-there");
        std::fs::write(&untouched, b"sentinel")?;
        let outcome = migrate(&path, &untouched)?;
        assert!(!outcome.migrated);
        assert_eq!(std::fs::read(&untouched)?, b"sentinel");
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
