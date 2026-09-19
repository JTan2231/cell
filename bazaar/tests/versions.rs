#![allow(clippy::unwrap_used, clippy::expect_used)]

use bazaar::api::{Error, Reader, Writer};
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::{Arc, Barrier};

fn private_directory() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

#[test]
fn versions_preserve_opaque_content_and_independent_ids() {
    let directory = private_directory();
    let database = directory.path().join("bazaar.sqlite3");
    let mut writer = Writer::initialize(&database).unwrap();
    let id = "caller/任意 ' ID";
    let original = "  Hello {{input}}\r\n\0雪\n";
    let first = writer.update(id, original).unwrap();
    let second = writer.update(id, "{not valid JSON}").unwrap();
    let restored = writer.update(id, original).unwrap();
    let duplicate = writer.update(id, original).unwrap();
    let empty = writer.update("other.config", "").unwrap();
    assert_eq!(empty.version, 1);
    assert_eq!(empty.content, "");
    let reader = Reader::open(&database).unwrap();
    assert_eq!(reader.get(id, Some(1)).unwrap(), first);
    assert_eq!(reader.get(id, Some(2)).unwrap(), second);
    assert_eq!(restored.version, 3);
    assert_eq!(reader.get(id, None).unwrap(), duplicate);
    assert_eq!(reader.history(id).unwrap(), vec![4, 3, 2, 1]);
    assert_eq!(reader.history("other.config").unwrap(), vec![1]);
    drop(writer);
    Writer::initialize(&database).unwrap();
    assert_eq!(
        Reader::open(&database).unwrap().get(id, None).unwrap(),
        duplicate
    );
}

#[test]
fn unknown_reads_and_invalid_requests_preserve_state() {
    let directory = private_directory();
    let missing = directory.path().join("absent/store.sqlite3");
    assert!(Reader::open(&missing).is_err());
    assert!(Writer::open(&missing).is_err());
    assert!(!missing.parent().unwrap().exists());
    let database = directory.path().join("bazaar.sqlite3");
    let mut writer = Writer::initialize(&database).unwrap();
    let reader = Reader::open(&database).unwrap();
    assert!(matches!(reader.get("missing", None), Err(Error::NotFound)));
    assert!(matches!(reader.history("missing"), Err(Error::NotFound)));
    assert!(matches!(writer.update("", "data"), Err(Error::EmptyId)));
    writer.update("known", "data").unwrap();
    assert!(matches!(
        reader.get("known", Some(0)),
        Err(Error::InvalidVersion)
    ));
    assert!(matches!(
        reader.get("known", Some(-1)),
        Err(Error::InvalidVersion)
    ));
    assert!(matches!(reader.get("known", Some(2)), Err(Error::NotFound)));
    assert_eq!(reader.history("known").unwrap(), vec![1]);
}

#[test]
fn reads_work_on_read_only_files_and_do_not_create_sidecars() {
    let directory = private_directory();
    let database = directory.path().join("bazaar.sqlite3");
    let mut writer = Writer::initialize(&database).unwrap();
    let saved = writer.update("example", "exact\n").unwrap();
    drop(writer);
    let before = fs::read(&database).unwrap();
    fs::set_permissions(&database, fs::Permissions::from_mode(0o400)).unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let reader = Reader::open(&database).unwrap();
    assert_eq!(reader.get("example", None).unwrap(), saved);
    assert_eq!(reader.history("example").unwrap(), vec![1]);
    reader.check().unwrap();
    assert_eq!(fs::read(&database).unwrap(), before);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    drop(reader);
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&database, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn concurrent_writers_allocate_contiguous_versions_without_lost_content() {
    let directory = private_directory();
    let database = directory.path().join("bazaar.sqlite3");
    Writer::initialize(&database).unwrap();
    let start = Arc::new(Barrier::new(6));
    let workers: Vec<_> = (0..6)
        .map(|worker| {
            let database = database.clone();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                let mut writer = Writer::open(database).unwrap();
                start.wait();
                (0..12)
                    .map(|item| {
                        writer
                            .update("shared", &format!("{worker}:{item}"))
                            .unwrap()
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let records: Vec<_> = workers
        .into_iter()
        .flat_map(|worker| worker.join().unwrap())
        .collect();
    let reader = Reader::open(&database).unwrap();
    assert_eq!(
        reader.history("shared").unwrap(),
        (1..=72).rev().collect::<Vec<_>>()
    );
    assert_eq!(
        records
            .iter()
            .map(|record| &record.content)
            .collect::<BTreeSet<_>>()
            .len(),
        72
    );
    for record in records {
        assert_eq!(reader.get("shared", Some(record.version)).unwrap(), record);
    }
}

#[test]
fn sqlite_rejects_replacement_deletion_and_version_gaps() {
    let directory = private_directory();
    let database = directory.path().join("bazaar.sqlite3");
    Writer::initialize(&database)
        .unwrap()
        .update("step", "original")
        .unwrap();
    let connection = Connection::open(&database).unwrap();
    for sql in [
        "UPDATE versions SET content = 'changed'",
        "DELETE FROM versions",
        "INSERT OR REPLACE INTO versions VALUES ('step', 1, 'replacement')",
        "INSERT INTO versions VALUES ('step', 3, 'gap')",
    ] {
        assert!(connection.execute_batch(sql).is_err(), "{sql}");
    }
    assert_eq!(
        Reader::open(database)
            .unwrap()
            .get("step", Some(1))
            .unwrap()
            .content,
        "original"
    );
}

#[test]
fn initialization_refuses_foreign_and_unsupported_state_without_rewriting_it() {
    let directory = private_directory();
    let database = directory.path().join("foreign.sqlite3");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE foreign_data (value TEXT); INSERT INTO foreign_data VALUES ('preserve');",
        )
        .unwrap();
    drop(connection);
    fs::set_permissions(&database, fs::Permissions::from_mode(0o600)).unwrap();
    let before = fs::read(&database).unwrap();
    assert!(matches!(
        Writer::initialize(&database),
        Err(Error::ForeignDatabase)
    ));
    assert!(matches!(
        Reader::open(&database),
        Err(Error::UnsupportedDatabase)
    ));
    assert_eq!(fs::read(&database).unwrap(), before);

    let database = directory.path().join("future.sqlite3");
    Writer::initialize(&database)
        .unwrap()
        .update("step", "preserve")
        .unwrap();
    Connection::open(&database)
        .unwrap()
        .execute_batch("PRAGMA user_version = 2;")
        .unwrap();
    let before = fs::read(&database).unwrap();
    assert!(matches!(
        Writer::initialize(&database),
        Err(Error::UnsupportedDatabase)
    ));
    assert!(matches!(
        Writer::open(&database),
        Err(Error::UnsupportedDatabase)
    ));
    assert_eq!(fs::read(&database).unwrap(), before);
}

#[test]
fn symlinks_and_public_files_are_refused() {
    let directory = private_directory();
    let database = directory.path().join("bazaar.sqlite3");
    Writer::initialize(&database).unwrap();
    let link = directory.path().join("link.sqlite3");
    symlink(&database, &link).unwrap();
    assert!(matches!(Reader::open(link), Err(Error::InvalidFile)));
    fs::set_permissions(&database, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(Reader::open(database), Err(Error::InvalidFile)));
}
