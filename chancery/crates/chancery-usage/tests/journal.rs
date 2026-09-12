use std::error::Error;
use std::fs;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::time::{Duration, Instant};

use chancery_usage::{Filter, Store};
use rusqlite::Connection;

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn registrations_are_idempotent_and_events_are_append_only() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("usage.sqlite3");
    let store = Store::initialize(&path)?;
    store.register_system("alpha")?;
    store.register_system("alpha")?;
    store.register_command("alpha", "show")?;
    store.register_command("alpha", "show")?;
    store.register_command("alpha", "unused")?;
    assert!(store.register_command("unknown", "show").is_err());
    assert!(store.record("alpha", "unknown", None).is_err());
    assert_eq!(store.record("alpha", "show", Some("thread-a"))?, 1);
    assert_eq!(store.record("alpha", "show", Some("thread-a"))?, 2);
    assert_eq!(store.record("alpha", "show", None)?, 3);
    let counts = store.counts(&Filter::default())?;
    assert_eq!(counts.len(), 2);
    assert_eq!(counts[0].invocations, 3);
    assert_eq!(counts[1].invocations, 0);
    assert_eq!(counts[1].last_recorded_at, None);
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    let sql = Connection::open(&path)?;
    for table in ["usage", "commands", "systems"] {
        assert!(sql.execute(&format!("DELETE FROM {table}"), []).is_err());
        let field = if table == "usage" { "command_id" } else { "id" };
        assert!(
            sql.execute(&format!("UPDATE {table} SET {field}='changed'"), [])
                .is_err()
        );
    }
    drop(store);
    let reopened = Store::initialize(&path)?;
    assert_eq!(reopened.events(&Filter::default(), 0, 100)?.items.len(), 3);
    Ok(())
}

#[test]
fn filters_and_pages_preserve_unknown_threads_and_zero_counts() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = Store::initialize(&directory.path().join("usage.sqlite3"))?;
    for system in ["alpha", "beta"] {
        store.register_system(system)?;
        store.register_command(system, "show")?;
    }
    store.record("alpha", "show", Some("thread-a"))?;
    store.record("alpha", "show", None)?;
    store.record("beta", "show", Some("thread-b"))?;
    let first = store.events(&Filter::default(), 0, 2)?;
    assert_eq!(first.items.len(), 2);
    assert!(first.has_more);
    let second = store.events(&Filter::default(), first.next_cursor, 2)?;
    assert_eq!(second.items.len(), 1);
    assert!(!second.has_more);
    let thread = Filter {
        thread: Some("thread-a".into()),
        ..Filter::default()
    };
    let counts = store.counts(&thread)?;
    assert_eq!(counts[0].invocations, 1);
    assert_eq!(counts[1].invocations, 0);
    let unlinked = Filter {
        unattributed: true,
        ..Filter::default()
    };
    assert_eq!(store.events(&unlinked, 0, 100)?.items.len(), 1);
    let outside = Filter {
        until: Some(1),
        ..Filter::default()
    };
    assert!(store.counts(&outside)?.iter().all(|c| c.invocations == 0));
    assert!(
        store
            .events(
                &Filter {
                    since: Some(2),
                    until: Some(1),
                    ..Filter::default()
                },
                0,
                100
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn opens_never_create_and_initialization_refuses_foreign_or_newer_state() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("usage.sqlite3");
    assert!(Store::open(&path).is_err());
    assert!(Store::read(&path).is_err());
    assert!(!path.exists());
    let store = Store::initialize(&path)?;
    drop(store);
    let sql = Connection::open(&path)?;
    sql.pragma_update(None, "user_version", 99)?;
    assert!(Store::open(&path).is_err());
    assert!(Store::initialize(&path).is_err());
    assert_eq!(
        sql.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?,
        99
    );
    let link = directory.path().join("linked.sqlite3");
    symlink(&path, &link)?;
    assert!(Store::initialize(&link).is_err());
    let foreign = directory.path().join("foreign.sqlite3");
    let connection = Connection::open(&foreign)?;
    fs::set_permissions(&foreign, fs::Permissions::from_mode(0o600))?;
    connection.execute("CREATE TABLE unrelated (id INTEGER)", [])?;
    assert!(Store::initialize(&foreign).is_err());
    Ok(())
}

#[test]
fn contention_returns_an_error_without_waiting_for_the_command() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("usage.sqlite3");
    let store = Store::initialize(&path)?;
    store.register_system("alpha")?;
    store.register_command("alpha", "show")?;
    let blocker = Connection::open(&path)?;
    blocker.execute_batch("BEGIN IMMEDIATE")?;
    let start = Instant::now();
    assert!(store.record("alpha", "show", None).is_err());
    assert!(start.elapsed() < Duration::from_secs(2));
    blocker.execute_batch("ROLLBACK")?;
    store.record("alpha", "show", None)?;
    assert_eq!(store.events(&Filter::default(), 0, 100)?.items.len(), 1);
    Ok(())
}

#[derive(clap::Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    #[command(alias = "read")]
    Show { private_argument: String },
    #[command(subcommand)]
    Repository(Repository),
}

#[derive(clap::Subcommand)]
enum Repository {
    Show,
    List,
}

#[test]
fn command_identity_comes_from_declarations_not_argument_values() -> TestResult {
    use clap::CommandFactory as _;
    let ids = chancery_usage::cli::command_ids(&Cli::command(), "");
    assert!(ids.contains(&"repository.show".into()));
    assert!(ids.contains(&"show".into()));
    assert!(!ids.contains(&"read".into()));
    let (_, selected) = chancery_usage::cli::parse_command_from::<Cli>(
        ["example", "read", "private source text"].map(Into::into),
        "",
    )?;
    assert_eq!(selected.as_deref(), Some("show"));
    assert!(
        chancery_usage::cli::parse_command_from::<Cli>(["example", "show"].map(Into::into), "")
            .is_err()
    );
    Ok(())
}
