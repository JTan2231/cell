use anyhow::{Context as _, Result};
use clew::store::{DATABASE, Record, Reference, Store};
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
        ..Record::default()
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
        "SELECT sequence,id,recorded_at,kind,status,notes,replaces FROM entries ORDER BY sequence",
    )?;
    Ok(statement
        .query_map([], |row| (0..7).map(|index| row.get(index)).collect())?
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
        ..Record::default()
    };
    let original_entry = store.entry("old-applied")?.context("retained entry")?;
    assert_eq!(store.record(&original)?, original_entry);
    assert_eq!(original_entry.recorded_at, "2026-01-01T00:00:00Z");
    let mut other_namespace = original.clone();
    other_namespace.cast_job_id = Some("cast-job-one".into());
    other_namespace.platter_job_ref = None;
    assert!(store.existing_record(&other_namespace).is_err());
    assert!(store.record(&other_namespace).is_err());
    let old_history = store.history("old-one")?;
    assert_eq!(old_history.len(), 4);
    assert_eq!(old_history[1].superseded_by.as_deref(), Some("old-fix"));
    assert_eq!(store.current()?[0].status.as_deref(), Some("applied"));
    let new_entry = store.record(&record("cast-update", None, Some("Updated through Cast.")))?;
    assert_eq!(new_entry.application_job_id(), Some("cast-job-one"));
    assert_eq!(store.current()?.len(), 1);
    assert_eq!(store.history("old-one")?.len(), 5);
    assert_eq!(store.history("cast-job-one")?.len(), 5);
    let mut legacy_update = original.clone();
    legacy_update.id = "legacy-update".into();
    legacy_update.status = None;
    legacy_update.notes = Some("Still waiting.".into());
    let legacy_entry = store.record(&legacy_update)?;
    assert_eq!(legacy_entry.application_job_id(), Some("cast-job-one"));
    assert_eq!(store.record(&legacy_update)?, legacy_entry);
    let retraction = store.retract("undo-original", "old-applied", Some("Correction."))?;
    assert_eq!(retraction.application_job_id(), Some("cast-job-one"));
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

#[test]
fn standalone_notes_and_named_threads_do_not_need_external_references() -> Result<()> {
    let temp = private_temp()?;
    let mut store = Store::initialize(temp.path())?;
    let standalone = Record {
        id: "standalone".into(),
        notes: Some("Remember the deployment window.".into()),
        ..Record::default()
    };
    let saved = store.record(&standalone)?;
    assert_eq!(saved.thread, None);
    assert!(saved.references.is_empty());
    let started = Record {
        id: "sla-started".into(),
        thread: Some("SLA implementation".into()),
        status: Some("in progress".into()),
        notes: Some("Started the SLA implementation.\nKeep this wording.".into()),
        ..Record::default()
    };
    let first = store.record(&started)?;
    let done = Record {
        id: "sla-finished".into(),
        thread: started.thread.clone(),
        status: Some("done".into()),
        notes: Some("Finished the reporting path.".into()),
        references: vec![Reference {
            namespace: "repository.commit".into(),
            external_id: "cell/abc123".into(),
        }],
        ..Record::default()
    };
    let last = store.record(&done)?;
    assert_eq!(first.thread, last.thread);
    let thread = store.thread("SLA implementation")?;
    assert_eq!(thread.history.len(), 2);
    assert_eq!(thread.status.as_deref(), Some("done"));
    assert_eq!(thread.status_entry_id.as_deref(), Some("sla-finished"));
    assert_eq!(thread.history[0].entry.notes, started.notes);
    assert_eq!(store.search("sLa IMPLEMENTATION")?.len(), 2);
    assert_eq!(store.search("CELL/ABC123")?[0].entry.id, "sla-finished");
    assert_eq!(store.search("sla-finished")?[0].entry.id, "sla-finished");
    assert_eq!(store.search(&thread.thread.id)?.len(), 2);
    assert!(store.search("nothing matches")?.is_empty());
    assert!(store.search(" ").is_err());
    assert!(store.current()?.is_empty());
    assert!(store.thread("unknown").is_err());
    store.check()?;
    Ok(())
}

#[test]
fn neutral_job_links_do_not_enroll_applications_and_reference_sets_retry_safely() -> Result<()> {
    let temp = private_temp()?;
    let mut store = Store::initialize(temp.path())?;
    let cast = Reference {
        namespace: "cast.job".into(),
        external_id: "cast-job-one".into(),
    };
    let commit = Reference {
        namespace: "repository.commit".into(),
        external_id: "cell/abc123".into(),
    };
    let linked = Record {
        id: "research-note".into(),
        notes: Some("Compared the job listing with the migration work.".into()),
        references: vec![cast.clone(), commit.clone()],
        ..Record::default()
    };
    let original = store.record(&linked)?;
    assert_eq!(original.application_job_id(), None);
    assert!(!store.knows_job("cast-job-one")?);
    assert!(store.current()?.is_empty());
    assert!(store.history("cast-job-one")?.is_empty());
    let mut retry = linked.clone();
    retry.references = vec![commit.clone(), cast.clone(), cast.clone()];
    assert_eq!(store.record(&retry)?, original);
    retry.references.pop();
    retry.references.pop();
    assert!(store.record(&retry).is_err());
    let mut applied = record("application", Some("applied"), None);
    applied.references = vec![cast, commit];
    let app = store.record(&applied)?;
    assert_eq!(app.application_job_id(), Some("cast-job-one"));
    assert_eq!(app.references.len(), 3);
    assert_eq!(store.current()?.len(), 1);
    assert_eq!(store.history("cast-job-one")?.len(), 1);
    assert_eq!(store.search("cast-job-one")?.len(), 2);
    assert!(store.knows_job("cast-job-one")?);
    Ok(())
}

#[test]
fn corrections_keep_thread_identity_replace_complete_content_and_retry_after_retraction()
-> Result<()> {
    let temp = private_temp()?;
    let mut store = Store::initialize(temp.path())?;
    let first = Record {
        id: "started".into(),
        thread: Some("SLA implementation".into()),
        status: Some("in progress".into()),
        notes: Some("Started.".into()),
        ..Record::default()
    };
    let original = store.record(&first)?;
    let finished = Record {
        id: "finished".into(),
        thread: first.thread.clone(),
        status: Some("done".into()),
        references: vec![Reference {
            namespace: "repository.commit".into(),
            external_id: "cell/abc123".into(),
        }],
        notes: Some("Done.".into()),
        ..Record::default()
    };
    store.record(&finished)?;
    let correction = Record {
        id: "correct-finish".into(),
        replaces: Some("finished".into()),
        notes: Some("Reporting is still outstanding.".into()),
        ..Record::default()
    };
    let mut moved = correction.clone();
    moved.thread = Some("Unrelated work".into());
    assert!(store.record(&moved).is_err());
    assert!(store.thread("Unrelated work").is_err());
    let corrected = store.record(&correction)?;
    assert_eq!(corrected.thread, original.thread);
    assert_eq!(corrected.status, None);
    assert!(corrected.references.is_empty());
    let mut changed_retry = correction.clone();
    changed_retry.thread = first.thread.clone();
    assert!(store.record(&changed_retry).is_err());
    assert_eq!(
        store.thread("SLA implementation")?.status.as_deref(),
        Some("in progress")
    );
    store.retract("undo-started", "started", Some("Wrong start report."))?;
    assert_eq!(store.record(&first)?, original);
    assert_eq!(store.record(&correction)?, corrected);
    let history = store.thread("SLA implementation")?;
    assert_eq!(history.status, None);
    assert_eq!(history.history.len(), 4);
    assert_eq!(
        history.history[0].superseded_by.as_deref(),
        Some("undo-started")
    );
    assert_eq!(
        history.history[1].superseded_by.as_deref(),
        Some("correct-finish")
    );
    let same_thread = Record {
        id: "correct-again".into(),
        thread: first.thread,
        notes: Some("Reporting was finished later.".into()),
        replaces: Some("correct-finish".into()),
        ..Record::default()
    };
    assert_eq!(store.record(&same_thread)?.thread, original.thread);
    store.check()?;
    Ok(())
}

#[test]
fn entry_associations_are_atomic_and_cannot_be_added_or_changed_later() -> Result<()> {
    let temp = private_temp()?;
    let mut store = Store::initialize(temp.path())?;
    let bad = Record {
        id: "missing-alias".into(),
        platter_job_ref: Some("unmapped".into()),
        thread: Some("Must not remain".into()),
        notes: Some("Do not commit this.".into()),
        ..Record::default()
    };
    assert!(store.record(&bad).is_err());
    assert!(store.entries()?.is_empty());
    assert!(store.thread("Must not remain").is_err());
    let note = Record {
        id: "note".into(),
        notes: Some("A durable note.".into()),
        references: vec![Reference {
            namespace: "repository.commit".into(),
            external_id: "cell/abc123".into(),
        }],
        ..Record::default()
    };
    store.record(&note)?;
    let connection = rusqlite::Connection::open(temp.path().join(DATABASE))?;
    connection.execute_batch("PRAGMA foreign_keys=ON;")?;
    connection.execute(
        "INSERT INTO external_references(namespace,external_id) VALUES('cast.job','another-job')",
        [],
    )?;
    assert!(
        connection
            .execute("UPDATE entry_references SET role='application_report'", [])
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM entry_references", [])
            .is_err()
    );
    assert!(connection.execute("INSERT INTO entry_references(entry_id,reference_id,role) SELECT 'note',id,'link' FROM external_references WHERE external_id='another-job'", []).is_err());
    assert!(connection.execute("INSERT INTO entry_references(entry_id,reference_id,role) SELECT 'orphan',id,'link' FROM external_references", []).is_err());
    assert_eq!(store.entry("note")?.context("note")?.references.len(), 1);
    store.check()?;
    Ok(())
}

const SCHEMA_TWO: &str = "
CREATE TABLE legacy_references (platter_job_ref TEXT PRIMARY KEY NOT NULL,cast_job_id TEXT NOT NULL UNIQUE);
CREATE TABLE entries (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 id TEXT NOT NULL UNIQUE,
 recorded_at TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('record','retraction')),
 cast_job_id TEXT,
 platter_job_ref TEXT REFERENCES legacy_references(platter_job_ref),
 status TEXT,notes TEXT,replaces TEXT UNIQUE REFERENCES entries(id),
 CHECK(cast_job_id IS NOT NULL OR platter_job_ref IS NOT NULL),
 CHECK(kind='record' OR (status IS NULL AND replaces IS NOT NULL)),
 CHECK(kind='retraction' OR status IS NOT NULL OR notes IS NOT NULL)
);
CREATE INDEX entries_job ON entries(cast_job_id,sequence);
CREATE INDEX entries_legacy_job ON entries(platter_job_ref,sequence);
CREATE TRIGGER entries_no_update BEFORE UPDATE ON entries BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER entries_no_delete BEFORE DELETE ON entries BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER legacy_references_no_update BEFORE UPDATE ON legacy_references BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER legacy_references_no_delete BEFORE DELETE ON legacy_references BEGIN SELECT RAISE(ABORT,'immutable'); END;
PRAGMA user_version=2;
";

fn schema_two_database(root: &std::path::Path) -> Result<rusqlite::Connection> {
    use std::os::unix::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join(DATABASE))?;
    let connection = rusqlite::Connection::open(root.join(DATABASE))?;
    connection.execute_batch(SCHEMA_TWO)?;
    connection.execute_batch("INSERT INTO legacy_references VALUES('old-one','cast-job-one');
        INSERT INTO entries(id,recorded_at,kind,cast_job_id,platter_job_ref,status,notes,replaces) VALUES
        ('old-applied','2026-01-01T00:00:00Z','record',NULL,'old-one','applied','Original note.',NULL),
        ('native-applied','2026-01-02T00:00:00Z','record','cast-native',NULL,'applied',NULL,NULL),
        ('native-fix','2026-01-03T00:00:00Z','record','cast-other',NULL,'applied','Correct job.','native-applied'),
        ('native-retract','2026-01-04T00:00:00Z','retraction','cast-other',NULL,NULL,'Wrong report.','native-fix'),
        ('legacy-update','2026-01-05T00:00:00Z','record','cast-job-one','old-one',NULL,'Still waiting.',NULL);
        CREATE TRIGGER entries_require_cast BEFORE INSERT ON entries WHEN NEW.cast_job_id IS NULL BEGIN SELECT RAISE(ABORT,'reopen'); END;")?;
    Ok(connection)
}

#[test]
fn schema_two_migration_preserves_alias_retries_and_applications_without_inventing_threads()
-> Result<()> {
    let temp = private_temp()?;
    let old_connection = schema_two_database(temp.path())?;
    let original = original_rows(&old_connection)?;
    assert_eq!(Store::schema_version_at(temp.path())?, 2);
    assert!(Store::migration_needed(temp.path())?);
    assert!(Store::initialize(temp.path()).is_err());
    assert!(Store::open(temp.path(), true).is_err());
    let backup = temp.path().join("schema-two-backup.sqlite3");
    Store::migrate_current(temp.path(), &backup)?;
    assert_eq!(Store::schema_version_at(temp.path())?, 3);
    assert!(!Store::migration_needed(temp.path())?);
    assert_eq!(original_rows(&old_connection)?, original);
    assert_eq!(
        original_rows(&rusqlite::Connection::open(&backup)?)?,
        original
    );
    assert_eq!(
        std::fs::metadata(&backup)?.permissions().mode() & 0o777,
        0o600
    );
    assert!(old_connection.execute("INSERT INTO entries(id,recorded_at,kind,cast_job_id,status) VALUES('late','2026-01-06T00:00:00Z','record','cast-job-one','applied')", []).is_err());
    let mut store = Store::open(temp.path(), true)?;
    assert!(store.entries()?.iter().all(|entry| entry.thread.is_none()));
    assert_eq!(
        store.legacy_aliases()?,
        vec![("old-one".into(), "cast-job-one".into())]
    );
    assert_eq!(store.current()?.len(), 1);
    assert_eq!(store.history("cast-native")?.len(), 3);
    let old_write = Record {
        id: "old-applied".into(),
        platter_job_ref: Some("old-one".into()),
        status: Some("applied".into()),
        notes: Some("Original note.".into()),
        ..Record::default()
    };
    let saved = store.entry("old-applied")?.context("migrated entry")?;
    assert_eq!(store.record(&old_write)?, saved);
    let mut wrong_namespace = old_write.clone();
    wrong_namespace.platter_job_ref = None;
    wrong_namespace.cast_job_id = Some("cast-job-one".into());
    assert!(store.record(&wrong_namespace).is_err());
    let legacy_update = Record {
        id: "legacy-update".into(),
        platter_job_ref: Some("old-one".into()),
        notes: Some("Still waiting.".into()),
        ..Record::default()
    };
    assert_eq!(
        store.record(&legacy_update)?.application_job_id(),
        Some("cast-job-one")
    );
    let native_fix = Record {
        id: "native-fix".into(),
        cast_job_id: Some("cast-other".into()),
        status: Some("applied".into()),
        notes: Some("Correct job.".into()),
        replaces: Some("native-applied".into()),
        ..Record::default()
    };
    assert_eq!(
        store.record(&native_fix)?.recorded_at,
        "2026-01-03T00:00:00Z"
    );
    assert_eq!(
        store
            .retract("native-retract", "native-fix", Some("Wrong report."))?
            .recorded_at,
        "2026-01-04T00:00:00Z"
    );
    store.check()?;
    Ok(())
}
