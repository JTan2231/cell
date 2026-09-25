use anyhow::Result;
use clew::store::{DATABASE, Record, Store};
use std::os::unix::fs::PermissionsExt as _;

fn private_temp() -> Result<tempfile::TempDir> {
    let temp = tempfile::tempdir()?;
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700))?;
    Ok(temp)
}

fn record(id: &str, status: Option<&str>, notes: Option<&str>) -> Record {
    Record {
        id: id.into(),
        cast_job_id: Some("cast-job-one".into()),
        platter_job_ref: None,
        status: status.map(str::to_owned),
        notes: notes.map(str::to_owned),
        replaces: None,
    }
}

#[test]
fn notes_never_supply_status_and_retries_preserve_exact_history() -> Result<()> {
    let temp = private_temp()?;
    let mut store = Store::initialize(temp.path())?;
    store.record(&record("note", None, Some("rejected? still waiting")))?;
    assert_eq!(store.current()?[0].status, None);
    let applied = store.record(&record("applied", Some("applied"), None))?;
    assert_eq!(
        store.record(&record("applied", Some("applied"), None))?,
        applied
    );
    assert!(
        store
            .record(&record("applied", Some("rejected"), None))
            .is_err()
    );
    store.record(&record("later-note", None, Some("Posting disappeared.")))?;
    assert_eq!(store.current()?[0].status.as_deref(), Some("applied"));
    store.record(&record(
        "rejection",
        Some("rejected"),
        Some("They replied quickly."),
    ))?;
    assert_eq!(store.current()?[0].status.as_deref(), Some("rejected"));
    drop(store);
    let reopened = Store::open(temp.path(), false)?;
    assert_eq!(reopened.entries()?.len(), 4);
    assert_eq!(reopened.entry("applied")?, Some(applied));
    reopened.check()?;
    Ok(())
}

#[test]
fn corrections_and_retractions_append_without_erasing_or_retargeting_history() -> Result<()> {
    let temp = private_temp()?;
    let mut store = Store::initialize(temp.path())?;
    store.record(&record("one", Some("applied"), None))?;
    let old = store.record(&record("two", Some("rejected"), None))?;
    let retract = store.retract("undo-two", "two", Some("Wrong job."))?;
    assert_eq!(
        store.retract("undo-two", "two", Some("Wrong job."))?,
        retract
    );
    assert_eq!(store.current()?[0].status.as_deref(), Some("applied"));
    assert_eq!(store.entry("two")?, Some(old));
    assert!(store.retract("again", "two", None).is_err());
    let mut correction = record("fix-one", Some("applied"), Some("Correct role."));
    correction.replaces = Some("one".into());
    correction.cast_job_id = Some("cast-job-other".into());
    store.record(&correction)?;
    let current = store.current()?;
    assert_eq!(current.len(), 1);
    assert_eq!(
        Some(&current[0].cast_job_id),
        correction.cast_job_id.as_ref()
    );
    let history = store.history("cast-job-one")?;
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].superseded_by.as_deref(), Some("fix-one"));
    let connection = rusqlite::Connection::open(temp.path().join(DATABASE))?;
    assert!(connection.execute("DELETE FROM entries", []).is_err());
    assert!(
        connection
            .execute("UPDATE entries SET notes='changed'", [])
            .is_err()
    );
    Ok(())
}

#[test]
fn missing_and_unsupported_state_remain_errors() -> Result<()> {
    let temp = private_temp()?;
    assert!(Store::open(temp.path(), false).is_err());
    assert!(!temp.path().join(DATABASE).exists());
    let store = Store::initialize(temp.path())?;
    drop(store);
    let connection = rusqlite::Connection::open(temp.path().join(DATABASE))?;
    connection.pragma_update(None, "user_version", 99)?;
    assert!(Store::initialize(temp.path()).is_err());
    assert!(Store::open(temp.path(), true).is_err());
    Ok(())
}

const LEGACY_SCHEMA: &str = "
CREATE TABLE entries (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 id TEXT NOT NULL UNIQUE,
 recorded_at TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('record','retraction')),
 platter_job_ref TEXT NOT NULL,
 status TEXT,
 notes TEXT,
 replaces TEXT UNIQUE REFERENCES entries(id),
 CHECK(kind='record' OR (status IS NULL AND replaces IS NOT NULL)),
 CHECK(kind='retraction' OR status IS NOT NULL OR notes IS NOT NULL)
);
CREATE INDEX entries_job ON entries(platter_job_ref,sequence);
CREATE TRIGGER entries_no_update BEFORE UPDATE ON entries
 BEGIN SELECT RAISE(ABORT,'Clew entries are append-only'); END;
CREATE TRIGGER entries_no_delete BEFORE DELETE ON entries
 BEGIN SELECT RAISE(ABORT,'Clew entries are append-only'); END;
PRAGMA user_version=1;
";

fn legacy_database(root: &std::path::Path) -> Result<rusqlite::Connection> {
    use std::os::unix::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join(DATABASE))?;
    let connection = rusqlite::Connection::open(root.join(DATABASE))?;
    connection.execute_batch(LEGACY_SCHEMA)?;
    connection.execute_batch(
        "INSERT INTO entries(id,recorded_at,kind,platter_job_ref,status,notes,replaces) VALUES
         ('old-applied','2026-01-01T00:00:00Z','record','old-one','applied','Original note.',NULL),
         ('old-wrong','2026-01-02T00:00:00Z','record','old-one','rejected',NULL,NULL),
         ('old-fix','2026-01-03T00:00:00Z','record','old-two','rejected','Correct job.','old-wrong'),
         ('old-retract','2026-01-04T00:00:00Z','retraction','old-two',NULL,'Wrong report.','old-fix');",
    )?;
    Ok(connection)
}

fn original_rows(connection: &rusqlite::Connection) -> Result<Vec<Vec<rusqlite::types::Value>>> {
    let mut statement = connection.prepare(
        "SELECT sequence,id,recorded_at,kind,platter_job_ref,status,notes,replaces FROM entries ORDER BY sequence",
    )?;
    Ok(statement
        .query_map([], |row| (0..8).map(|index| row.get(index)).collect())?
        .collect::<rusqlite::Result<_>>()?)
}

#[test]
fn migration_preserves_original_rows_backups_aliases_corrections_and_exact_retries() -> Result<()> {
    use std::collections::BTreeMap;
    let temp = private_temp()?;
    let stale_connection = legacy_database(temp.path())?;
    let before = original_rows(&stale_connection)?;
    assert!(Store::migration_needed(temp.path())?);
    assert!(Store::initialize(temp.path()).is_err());
    assert!(Store::open(temp.path(), true).is_err());
    assert_eq!(
        Store::legacy_references(temp.path())?,
        ["old-one", "old-two"]
    );
    let mappings = BTreeMap::from([
        ("old-one".to_owned(), "cast-job-one".to_owned()),
        ("old-two".to_owned(), "cast-job-two".to_owned()),
    ]);
    let backup = temp.path().join("schema-one-backup.sqlite3");
    Store::migrate(temp.path(), &mappings, &backup)?;
    assert!(!Store::migration_needed(temp.path())?);
    assert_eq!(before, original_rows(&stale_connection)?);
    assert_eq!(
        std::fs::metadata(&backup)?.permissions().mode() & 0o777,
        0o600
    );
    let old = rusqlite::Connection::open(&backup)?;
    assert_eq!(before, original_rows(&old)?);
    assert_eq!(
        old.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?,
        1
    );
    // An old binary that opened before cutover cannot append with its old SQL.
    assert!(stale_connection.execute(
        "INSERT INTO entries(id,recorded_at,kind,platter_job_ref,status) VALUES('late','2026-01-05T00:00:00Z','record','old-one','applied')", [],
    ).is_err());
    let mut store = Store::open(temp.path(), true)?;
    store.check()?;
    assert_eq!(store.canonical_reference("old-one")?, "cast-job-one");
    assert!(store.knows("old-one")?);
    assert!(store.knows_job("cast-job-one")?);
    let original = Record {
        id: "old-applied".into(),
        cast_job_id: None,
        platter_job_ref: Some("old-one".into()),
        status: Some("applied".into()),
        notes: Some("Original note.".into()),
        replaces: None,
    };
    let original_entry = store.entry("old-applied")?.expect("retained entry");
    assert_eq!(store.record(&original)?, original_entry);
    assert_eq!(original_entry.recorded_at, "2026-01-01T00:00:00Z");
    let mut other_namespace = original.clone();
    other_namespace.cast_job_id = Some("cast-job-one".into());
    other_namespace.platter_job_ref = None;
    assert!(store.existing_record(&other_namespace).is_err());
    assert!(store.record(&other_namespace).is_err());
    let old_history = store.history("old-one")?;
    assert_eq!(old_history.len(), 3);
    assert_eq!(old_history[1].superseded_by.as_deref(), Some("old-fix"));
    assert_eq!(store.current()?[0].status.as_deref(), Some("applied"));
    let new_entry = store.record(&record("cast-update", None, Some("Updated through Cast.")))?;
    assert_eq!(new_entry.platter_job_ref, None);
    assert_eq!(store.current()?.len(), 1);
    assert_eq!(store.history("old-one")?.len(), 4);
    assert_eq!(store.history("cast-job-one")?.len(), 4);
    let mut legacy_update = original.clone();
    legacy_update.id = "legacy-update".into();
    legacy_update.status = None;
    legacy_update.notes = Some("Still waiting.".into());
    let legacy_entry = store.record(&legacy_update)?;
    assert_eq!(legacy_entry.cast_job_id, "cast-job-one");
    assert_eq!(store.record(&legacy_update)?, legacy_entry);
    let retraction = store.retract("undo-original", "old-applied", Some("Correction."))?;
    assert_eq!(retraction.cast_job_id, "cast-job-one");
    assert_eq!(store.record(&original)?, original_entry);
    assert_eq!(store.current()?[0].status, None);
    store.check()?;
    Ok(())
}

#[test]
fn migration_refuses_missing_or_colliding_mappings_and_preserves_existing_backup() -> Result<()> {
    use std::collections::BTreeMap;
    let temp = private_temp()?;
    let connection = legacy_database(temp.path())?;
    let before = original_rows(&connection)?;
    let backup = temp.path().join("schema-one-backup.sqlite3");
    let missing = BTreeMap::from([("old-one".to_owned(), "cast-job-one".to_owned())]);
    assert!(Store::migrate(temp.path(), &missing, &backup).is_err());
    assert!(!backup.exists());
    let collision = BTreeMap::from([
        ("old-one".to_owned(), "same-cast-job".to_owned()),
        ("old-two".to_owned(), "same-cast-job".to_owned()),
    ]);
    assert!(Store::migrate(temp.path(), &collision, &backup).is_err());
    assert!(!backup.exists());
    assert!(Store::migration_needed(temp.path())?);
    assert_eq!(before, original_rows(&connection)?);
    let valid = BTreeMap::from([
        ("old-one".to_owned(), "cast-job-one".to_owned()),
        ("old-two".to_owned(), "cast-job-two".to_owned()),
    ]);
    std::fs::write(&backup, "existing backup evidence")?;
    assert!(Store::migrate(temp.path(), &valid, &backup).is_err());
    assert_eq!(
        std::fs::read_to_string(&backup)?,
        "existing backup evidence"
    );
    assert!(Store::migration_needed(temp.path())?);
    assert_eq!(before, original_rows(&connection)?);
    Ok(())
}
