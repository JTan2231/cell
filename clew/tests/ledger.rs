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
        platter_job_ref: "ashby:acme:engineer".into(),
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
    correction.platter_job_ref = "ashby:other:engineer".into();
    store.record(&correction)?;
    let current = store.current()?;
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].platter_job_ref, correction.platter_job_ref);
    let history = store.history("ashby:acme:engineer")?;
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
