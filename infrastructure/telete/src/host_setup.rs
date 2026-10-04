//! Shared host setup only. Legacy journals are observed under their existing
//! locks; this module never runs the Python manager or changes its records.
use crate::paths::{self, FileLock, Paths};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub(crate) fn uid() -> u32 {
    rustix::process::getuid().as_raw()
}

fn account_home(output: &str, expected_uid: u32) -> Result<PathBuf> {
    let mut directories = Vec::new();
    for record in output.split("\n\n") {
        let fields: Vec<_> = record
            .lines()
            .filter_map(|line| line.split_once(": "))
            .collect();
        if fields
            .iter()
            .any(|(key, value)| *key == "uid" && value.parse::<u32>().ok() == Some(expected_uid))
        {
            directories.extend(
                fields
                    .iter()
                    .filter(|(key, _)| *key == "dir")
                    .map(|(_, value)| PathBuf::from(value)),
            );
        }
    }
    ensure!(
        directories.len() == 1 && directories[0].is_absolute(),
        "cannot establish the current user's account home"
    );
    Ok(directories.remove(0))
}

pub(crate) fn home() -> Result<PathBuf> {
    static HOME_PATH: OnceLock<PathBuf> = OnceLock::new();
    if let Some(home) = HOME_PATH.get() {
        return Ok(home.clone());
    }
    ensure!(cfg!(target_os = "macos"), "Cell host setup requires macOS");
    let result = Command::new("/usr/bin/dscacheutil")
        .args(["-q", "user", "-a", "uid", &uid().to_string()])
        .output()
        .context("cannot read the current user's account")?;
    ensure!(
        result.status.success(),
        "cannot read the current user's account"
    );
    let home = account_home(&String::from_utf8(result.stdout)?, uid())?;
    ensure!(
        fs::metadata(&home)?.uid() == uid(),
        "account home is not owned by the current user"
    );
    let _ = HOME_PATH.set(home.clone());
    Ok(home)
}

pub(crate) fn setup_lock() -> Result<FileLock> {
    paths::lock(
        &home()?.join("Library/Application Support/Cell/host-setup.lock"),
        false,
    )
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn legacy_loaded() -> Result<bool> {
    let result = Command::new("/bin/launchctl")
        .args(["print", &format!("gui/{}/dev.cell.ci-manager", uid())])
        .output()
        .context("cannot inspect the existing CI manager service")?;
    ensure!(
        result.status.success() || result.status.code() == Some(113),
        "cannot establish whether the existing CI manager service is stopped"
    );
    Ok(result.status.success())
}

// Keep the schema-one compatibility read local to host setup. Do not create,
// migrate, or reinterpret a legacy journal as Telete queue state.
fn queue_guard(root: &Path, storage_cutover: bool) -> Result<Vec<FileLock>> {
    let database = root.join("queue.sqlite3");
    if !exists(&database)? {
        return Ok(Vec::new());
    }
    let mut guards = vec![paths::lock(&root.join("admission.lock"), false)?];
    if storage_cutover {
        guards.push(paths::lock(&root.join("worker.lock"), false)?);
    }
    let metadata = fs::symlink_metadata(&database)?;
    ensure!(
        metadata.is_file() && metadata.uid() == uid() && metadata.mode() & 0o777 == 0o600,
        "legacy CI journal must be an owned private regular file"
    );
    let connection = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    ensure!(
        connection.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))? == 1,
        "unsupported legacy CI journal schema; use its own recovery procedure"
    );
    let paused: Option<String> = connection
        .query_row("SELECT value FROM control WHERE key='paused'", [], |row| {
            row.get(0)
        })
        .optional()?;
    ensure!(
        paused
            .as_deref()
            .map(serde_json::from_str::<Value>)
            .transpose()?
            == Some(Value::Bool(true)),
        "pause the existing CI manager before host setup"
    );
    let unsettled: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM jobs WHERE phase NOT IN ('succeeded','failed','cancelled','already_included'))", [], |row| row.get(0))?;
    ensure!(
        !unsettled,
        "settle all active and queued CI jobs before host setup"
    );
    if storage_cutover {
        let held: bool =
            connection.query_row("SELECT EXISTS(SELECT 1 FROM holds)", [], |row| row.get(0))?;
        ensure!(
            !held,
            "settle existing CI maintenance owners before selecting storage"
        );
    } else {
        let config: String =
            connection.query_row("SELECT value FROM control WHERE key='config'", [], |row| {
                row.get(0)
            })?;
        let config: Value = serde_json::from_str(&config)?;
        let common = Path::new(
            config["common_git_dir"]
                .as_str()
                .context("legacy CI repository identity is missing")?,
        );
        ensure!(
            common.is_absolute(),
            "legacy CI repository identity must be absolute"
        );
        for name in [
            "cell-release-publication.lock",
            "cell-release-publication.lock.d",
        ] {
            ensure!(
                !exists(&common.join(name))?,
                "settle release publication before changing signing configuration"
            );
        }
    }
    Ok(guards)
}

fn deployment_guard(root: &Path) -> Result<FileLock> {
    let guard = paths::lock(&root.join("deployment.lock"), false)?;
    ensure!(
        !exists(&root.join("active"))?,
        "settle retained Cell deployment recovery before host setup"
    );
    Ok(guard)
}

fn storage_guard_at(home: &Path, loaded: bool) -> Result<Vec<FileLock>> {
    ensure!(
        !loaded,
        "stop the existing CI manager service before selecting storage"
    );
    let legacy = home.join("Library/Application Support/Cell");
    let mut guards = queue_guard(&legacy.join("ci-manager"), true)?;
    let deployments = legacy.join("deployments");
    if exists(&deployments)? {
        guards.push(deployment_guard(&deployments)?);
    }
    Ok(guards)
}

pub(crate) fn storage_cutover_guard() -> Result<Vec<FileLock>> {
    storage_guard_at(&home()?, legacy_loaded()?)
}

pub(crate) fn signing_guard(paths: &Paths) -> Result<Vec<FileLock>> {
    let mut guards = queue_guard(&paths.workspace.join("ci-manager"), false)?;
    guards.push(deployment_guard(&paths.workspace.join("deployments"))?);
    Ok(guards)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    // Locks intentionally survive exec. Start each lock fixture in its own
    // process so parallel Git/process tests cannot inherit its descriptors.
    fn isolated(test: &str) -> bool {
        const CHILD: &str = "TELETE_TEST_HOST_SETUP_CHILD";
        if std::env::var_os(CHILD).as_deref() == Some(std::ffi::OsStr::new(test)) {
            return false;
        }
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(CHILD, test)
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "isolated host setup test failed: {test}");
                return true;
            }
            if started.elapsed() >= std::time::Duration::from_secs(30) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("isolated host setup test timed out: {test}");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn journal(root: &Path) -> Connection {
        paths::ensure_private(root).unwrap();
        let database = root.join("queue.sqlite3");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("PRAGMA user_version=1; CREATE TABLE control(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE jobs(phase TEXT); CREATE TABLE holds(owner TEXT); INSERT INTO control VALUES('paused','true');").unwrap();
        connection
            .execute(
                "INSERT INTO control VALUES('config',?1)",
                [serde_json::json!({"common_git_dir":root.join("git")}).to_string()],
            )
            .unwrap();
        fs::set_permissions(database, fs::Permissions::from_mode(0o600)).unwrap();
        connection
    }

    #[test]
    fn home_uses_exact_account_identity() {
        assert_eq!(
            account_home("uid: 501\ndir: /Users/fixture\n", 501).unwrap(),
            PathBuf::from("/Users/fixture")
        );
        assert!(account_home("uid: 502\ndir: /Users/fixture\n", 501).is_err());
        assert!(account_home("uid: 501\ndir: relative\n", 501).is_err());
        assert!(account_home("uid: 501\ndir: /one\ndir: /two\n", 501).is_err());
    }

    #[test]
    fn storage_preserves_legacy_pause_jobs_holds_and_worker_checks() {
        if isolated(
            "host_setup::tests::storage_preserves_legacy_pause_jobs_holds_and_worker_checks",
        ) {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("ci-manager");
        let connection = journal(&root);
        assert!(queue_guard(&root, true).is_ok());
        connection
            .execute("UPDATE control SET value='false' WHERE key='paused'", [])
            .unwrap();
        assert!(queue_guard(&root, true).is_err());
        connection
            .execute("UPDATE control SET value='true' WHERE key='paused'", [])
            .unwrap();
        for phase in ["queued", "checking", "blocked", "notifying"] {
            connection
                .execute("INSERT INTO jobs VALUES(?1)", [phase])
                .unwrap();
            assert!(queue_guard(&root, true).is_err());
            connection.execute("DELETE FROM jobs", []).unwrap();
        }
        connection
            .execute("INSERT INTO holds VALUES('maintenance')", [])
            .unwrap();
        assert!(queue_guard(&root, true).is_err());
        connection.execute("DELETE FROM holds", []).unwrap();
        let worker = paths::lock(&root.join("worker.lock"), false).unwrap();
        assert!(queue_guard(&root, true).is_err());
        drop(worker);
        let guards = queue_guard(&root, true).unwrap();
        assert!(paths::lock(&root.join("admission.lock"), false).is_err());
        assert!(paths::lock(&root.join("worker.lock"), false).is_err());
        drop(guards);
        assert!(paths::lock(&root.join("admission.lock"), false).is_ok());
    }

    #[test]
    fn shared_signing_checks_queue_and_release_publication() {
        if isolated("host_setup::tests::shared_signing_checks_queue_and_release_publication") {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("ci-manager");
        let connection = journal(&root);
        // A requester maintenance hold is not a signing prohibition in Python.
        connection
            .execute("INSERT INTO holds VALUES('maintenance')", [])
            .unwrap();
        assert!(queue_guard(&root, false).is_ok());
        fs::create_dir(root.join("git")).unwrap();
        for name in [
            "cell-release-publication.lock",
            "cell-release-publication.lock.d",
        ] {
            fs::write(root.join("git").join(name), b"held").unwrap();
            assert!(queue_guard(&root, false).is_err());
            fs::remove_file(root.join("git").join(name)).unwrap();
        }
        connection.execute_batch("PRAGMA user_version=2;").unwrap();
        assert!(queue_guard(&root, false).is_err());
    }

    #[test]
    fn storage_cutover_checks_service_and_retained_deployment() {
        if isolated("host_setup::tests::storage_cutover_checks_service_and_retained_deployment") {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        assert!(storage_guard_at(temporary.path(), true).is_err());
        assert!(storage_guard_at(temporary.path(), false).is_ok());
        let root = temporary
            .path()
            .join("Library/Application Support/Cell/deployments");
        paths::ensure_private(&root).unwrap();
        fs::write(root.join("active"), b"retained").unwrap();
        assert!(storage_guard_at(temporary.path(), false).is_err());
        fs::remove_file(root.join("active")).unwrap();
        let guards = storage_guard_at(temporary.path(), false).unwrap();
        assert!(paths::lock(&root.join("deployment.lock"), false).is_err());
        drop(guards);
        assert!(paths::lock(&root.join("deployment.lock"), false).is_ok());
    }

    #[test]
    fn guards_do_not_create_or_migrate_legacy_queues() {
        if isolated("host_setup::tests::guards_do_not_create_or_migrate_legacy_queues") {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("missing");
        assert!(queue_guard(&root, true).unwrap().is_empty());
        assert!(!root.exists());
        let connection = journal(&root);
        let before = fs::read(root.join("queue.sqlite3")).unwrap();
        drop(queue_guard(&root, true).unwrap());
        assert_eq!(fs::read(root.join("queue.sqlite3")).unwrap(), before);
        drop(connection);
    }
}
