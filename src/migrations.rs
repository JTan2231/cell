use rusqlite::{Connection, TransactionBehavior, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::AppError;

pub const CURRENT_SCHEMA_VERSION: i64 = 1;

const INITIAL_MIGRATION: &str = include_str!("../migrations/0001_initial.sql");

struct Migration {
    version: i64,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: INITIAL_MIGRATION,
}];

/// Return the greatest recorded migration version, or zero for an empty database.
pub fn schema_version(connection: &Connection) -> Result<i64, AppError> {
    let has_migration_table = connection
        .query_row(
            "SELECT EXISTS(\
                 SELECT 1 FROM sqlite_schema \
                 WHERE type = 'table' AND name = 'schema_migrations'\
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| {
            AppError::database(
                "schema_version_failed",
                format!("unable to inspect the database schema: {error}"),
            )
        })?;

    if !has_migration_table {
        return Ok(0);
    }

    connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .map_err(|error| {
            AppError::database(
                "schema_version_failed",
                format!("unable to read the database schema version: {error}"),
            )
        })
        .map(|version| version.unwrap_or(0))
}

/// Reject a database produced by a newer Annals executable.
pub fn reject_newer_schema(connection: &Connection) -> Result<i64, AppError> {
    let version = schema_version(connection)?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(AppError::database(
            "unsupported_schema_version",
            format!(
                "database schema version {version} is newer than supported version \
                 {CURRENT_SCHEMA_VERSION}"
            ),
        ));
    }
    Ok(version)
}

/// Require the exact schema understood by this executable without changing it.
pub fn require_current_schema(connection: &Connection) -> Result<(), AppError> {
    let version = reject_newer_schema(connection)?;
    if version < CURRENT_SCHEMA_VERSION {
        return Err(AppError::database(
            "migration_required",
            format!(
                "database schema version {version} requires migration to version \
                 {CURRENT_SCHEMA_VERSION}"
            ),
        ));
    }
    Ok(())
}

/// Apply all missing migrations together in one immediate transaction.
pub fn apply_pending_migrations(connection: &mut Connection) -> Result<usize, AppError> {
    let starting_version = reject_newer_schema(connection)?;
    if starting_version == CURRENT_SCHEMA_VERSION {
        return Ok(0);
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            AppError::database(
                "migration_failed",
                format!("unable to begin schema migration: {error}"),
            )
        })?;

    let mut applied = 0_usize;
    for migration in MIGRATIONS
        .iter()
        .filter(|migration| migration.version > starting_version)
    {
        transaction.execute_batch(migration.sql).map_err(|error| {
            AppError::database(
                "migration_failed",
                format!(
                    "unable to apply schema migration {}: {error}",
                    migration.version
                ),
            )
        })?;
        let applied_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| {
                AppError::database(
                    "migration_failed",
                    format!("unable to format the migration timestamp: {error}"),
                )
            })?;
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) \
                 VALUES (?1, ?2)",
                params![migration.version, applied_at],
            )
            .map_err(|error| {
                AppError::database(
                    "migration_failed",
                    format!(
                        "unable to record schema migration {}: {error}",
                        migration.version
                    ),
                )
            })?;
        applied += 1;
    }

    let foreign_key_violation = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| {
            AppError::database(
                "migration_failed",
                format!("unable to verify foreign keys after migration: {error}"),
            )
        })?;
    if foreign_key_violation {
        return Err(AppError::database(
            "migration_failed",
            "a foreign-key violation remained after schema migration",
        ));
    }

    transaction.commit().map_err(|error| {
        AppError::database(
            "migration_failed",
            format!("unable to commit schema migrations: {error}"),
        )
    })?;

    require_current_schema(connection)?;
    Ok(applied)
}
