use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

struct Installation {
    directory: TempDir,
    config: PathBuf,
    library: PathBuf,
    spool: PathBuf,
    library_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KrisisSuccessEnvelope {
    ok: bool,
    data: KrisisReceipt,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KrisisReceipt {
    contract_version: i64,
    library_id: String,
    producer: String,
    #[serde(alias = "key")]
    producer_key: String,
    source_sha256: String,
    job_id: String,
    accepted_at: String,
    acceptance: String,
}

impl Installation {
    fn new() -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let config = directory.path().join("decisions.toml");
        let library = directory.path().join("decisions/annals.db");
        let spool = directory.path().join("decisions/spool");
        fs::create_dir_all(library.parent().ok_or("library has no parent")?)?;
        fs::write(
            &config,
            format!(
                "library = {}\n[inbox]\nroot = {}\nminimum_available_bytes = 0\n",
                toml_string(&library),
                toml_string(&spool)
            ),
        )?;
        let initialized = successful_json(
            &command_with_config(&config)
                .args(["init", "--kind", "decisions"])
                .output()?,
        )?;
        assert_eq!(initialized["kind"], "decisions");
        let library_id = initialized["library_id"]
            .as_str()
            .ok_or("init omitted library_id")?
            .to_owned();
        let installation = Self {
            directory,
            config,
            library,
            spool,
            library_id,
        };
        installation.write_config(0, &installation.library_id)?;
        Ok(installation)
    }

    fn write_config(&self, minimum_available_bytes: u64, expected: &str) -> TestResult {
        fs::write(
            &self.config,
            format!(
                concat!(
                    "library = {}\n",
                    "[inbox]\n",
                    "root = {}\n",
                    "minimum_available_bytes = {minimum_available_bytes}\n",
                    "[decision_feed]\n",
                    "expected_library_id = {expected}\n",
                ),
                toml_string(&self.library),
                toml_string(&self.spool),
                minimum_available_bytes = minimum_available_bytes,
                expected = toml_literal(expected),
            ),
        )?;
        Ok(())
    }

    fn file(&self, name: &str, text: &str) -> TestResult<PathBuf> {
        let path = self.directory.path().join(name);
        fs::write(&path, text)?;
        Ok(path)
    }

    fn command(&self) -> Command {
        command_with_config(&self.config)
    }

    fn json_ok<I, S>(&self, arguments: I) -> TestResult<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        successful_json(&self.command().args(arguments).output()?)
    }

    fn accept(&self, key: &str, path: &Path) -> TestResult<Value> {
        self.json_ok([
            OsStr::new("inbox"),
            OsStr::new("accept"),
            OsStr::new("--producer"),
            OsStr::new("krisis"),
            OsStr::new("--key"),
            OsStr::new(key),
            path.as_os_str(),
        ])
    }
}

fn command_with_config(config: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_annals"));
    command
        .arg("--config")
        .arg(config)
        .arg("--json")
        .env_remove("ANNALS_LIBRARY");
    command
}

fn command_with_library(library: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_annals"));
    command
        .arg("--library")
        .arg(library)
        .arg("--json")
        .env_remove("ANNALS_CONFIG")
        .env_remove("ANNALS_LIBRARY");
    command
}

fn account(key: &str, context: &str) -> String {
    format!(
        concat!(
            "# Decision\n\nUse the dedicated decisions library.\n\n",
            "## Authority\n\n> put decisions in their own library\n\n",
            "## Context\n\n{context}\n\n",
            "## Action\n\nCreate one immutable account.\n\n",
            "## Result\n\nUnknown.\n\n",
            "## Source\n\n```json\n",
            "{{\"schema_version\":1,\"decision_id\":{key},",
            "\"occurred_at\":1788436800,\"occurred_at_precision\":\"second\",",
            "\"capture_rule_version\":\"krisis/1\",",
            "\"authority\":{{\"host_id\":\"host\",\"thread_id\":\"thread\",",
            "\"turn_id\":\"turn\",\"item_id\":\"item\",",
            "\"span\":{{\"start\":4,\"end\":42}}}}}}\n```\n",
        ),
        context = context,
        key = serde_json::to_string(key).unwrap_or_default(),
    )
}

fn successful_json(output: &Output) -> TestResult<Value> {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let envelope: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(envelope["ok"], true);
    Ok(envelope["data"].clone())
}

fn error_json(output: &Output, code: &str) -> TestResult<Value> {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    assert!(output.stdout.is_empty());
    let envelope: Value = serde_json::from_slice(&output.stderr)?;
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], code);
    Ok(envelope)
}

fn toml_string(path: &Path) -> String {
    toml_literal(&path.display().to_string())
}

fn toml_literal(value: &str) -> String {
    format!("{value:?}")
}

fn directory_count(path: &Path) -> io::Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    fs::read_dir(path)?.try_fold(0, |count, entry| {
        Ok(count + usize::from(entry?.file_type()?.is_dir()))
    })
}

fn raw_accept(binary: &Path, config: &Path, source: &Path, key: &str) -> io::Result<Output> {
    Command::new(binary)
        .arg("--config")
        .arg(config)
        .arg("--json")
        .args([
            OsStr::new("inbox"),
            OsStr::new("accept"),
            OsStr::new("--producer"),
            OsStr::new("krisis"),
            OsStr::new("--key"),
            OsStr::new(key),
            source.as_os_str(),
        ])
        .output()
}

#[test]
fn acceptance_replays_conflicts_and_pages_a_fixed_prefix() -> TestResult {
    let installation = Installation::new()?;
    let baseline = installation.json_ok(["decision-feed", "watermark"])?;
    let baseline_token = baseline["watermark"]
        .as_str()
        .ok_or("baseline watermark was not text")?
        .to_owned();
    let source = installation.file("account.md", &account("decision-1", "Initial context."))?;
    let created = installation.accept("decision-1", &source)?;
    assert_eq!(created["contract_version"], 1);
    assert_eq!(created["library_id"], installation.library_id);
    assert_eq!(created["acceptance"], "created");
    assert_eq!(created["producer"], "krisis");
    assert_eq!(created["key"], "decision-1");
    assert_eq!(created["source_sha256"].as_str().map(str::len), Some(64));
    assert!(created.get("path").is_none());
    assert!(created.get("content").is_none());
    assert_eq!(directory_count(&installation.spool.join("queued"))?, 1);

    let replayed = installation.accept("decision-1", &source)?;
    assert_eq!(replayed["acceptance"], "replayed");
    assert_eq!(replayed["job_id"], created["job_id"]);
    assert_eq!(directory_count(&installation.spool.join("queued"))?, 1);

    let connection = Connection::open(&installation.library)?;
    assert_eq!(
        connection.query_row(
            "SELECT COUNT(*) FROM decision_account_acceptances",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        1
    );
    assert_eq!(
        connection.query_row("SELECT COUNT(*) FROM ingestions", [], |row| row
            .get::<_, i64>(0))?,
        0
    );
    drop(connection);

    let changed = installation.file("changed.md", &account("decision-1", "Changed context."))?;
    error_json(
        &installation
            .command()
            .args([
                OsStr::new("inbox"),
                OsStr::new("accept"),
                OsStr::new("--producer"),
                OsStr::new("krisis"),
                OsStr::new("--key"),
                OsStr::new("decision-1"),
                changed.as_os_str(),
            ])
            .output()?,
        "decision_account_key_conflict",
    )?;

    let watermark = installation.json_ok(["decision-feed", "watermark"])?;
    let token = watermark["watermark"]
        .as_str()
        .ok_or("watermark was not text")?;
    let page = installation.json_ok([
        "decision-feed",
        "page",
        "--watermark",
        token,
        "--after",
        &baseline_token,
        "--limit",
        "1",
    ])?;
    assert_eq!(page["request_cursor"], baseline_token);
    assert_eq!(page["events"].as_array().map(Vec::len), Some(1));
    assert_eq!(page["events"][0]["account_id"], "decision-1");
    assert_eq!(page["events"][0]["occurred_at"], 1_788_436_800_i64);
    assert_eq!(page["events"][0]["authority"]["span"]["start"], 4);
    assert!(page["events"][0].get("markdown").is_none());
    let cursor = page["next_cursor"].as_str().ok_or("missing item cursor")?;
    let empty = installation.json_ok([
        "decision-feed",
        "page",
        "--watermark",
        token,
        "--after",
        cursor,
    ])?;
    assert_eq!(empty["events"].as_array().map(Vec::len), Some(0));
    assert_eq!(empty["request_cursor"], cursor);
    assert_eq!(empty["next_cursor"], cursor);
    Ok(())
}

#[test]
fn decision_feed_rejects_a_page_limit_above_200() -> TestResult {
    let installation = Installation::new()?;
    let watermark = installation.json_ok(["decision-feed", "watermark"])?;
    let token = watermark["watermark"]
        .as_str()
        .ok_or("watermark was not text")?;
    error_json(
        &installation
            .command()
            .args([
                "decision-feed",
                "page",
                "--watermark",
                token,
                "--after",
                token,
                "--limit",
                "201",
            ])
            .output()?,
        "invalid_decision_feed_limit",
    )?;
    Ok(())
}

#[test]
fn acceptance_is_physically_isolated_from_a_primary_library() -> TestResult {
    let installation = Installation::new()?;
    let primary_library = installation.directory.path().join("primary/annals.db");
    let primary_spool = installation.directory.path().join("primary/spool");
    fs::create_dir_all(
        primary_library
            .parent()
            .ok_or("primary library has no parent")?,
    )?;
    let primary_config = installation.directory.path().join("primary.toml");
    fs::write(
        &primary_config,
        format!(
            "library = {}\n[inbox]\nroot = {}\nminimum_available_bytes = 0\n",
            toml_string(&primary_library),
            toml_string(&primary_spool),
        ),
    )?;
    successful_json(&command_with_config(&primary_config).arg("init").output()?)?;

    let source = installation.file("account.md", &account("decision-isolated", "Separate."))?;
    installation.accept("decision-isolated", &source)?;
    assert_eq!(
        Connection::open(&installation.library)?.query_row(
            "SELECT COUNT(*) FROM decision_account_acceptances",
            [],
            |row| row.get::<_, i64>(0),
        )?,
        1
    );
    let primary = Connection::open(primary_library)?;
    assert_eq!(
        primary.query_row(
            "SELECT COUNT(*) FROM decision_account_acceptances",
            [],
            |row| { row.get::<_, i64>(0) }
        )?,
        0
    );
    assert_eq!(
        primary.query_row("SELECT COUNT(*) FROM ingestions", [], |row| row
            .get::<_, i64>(0))?,
        0
    );
    assert!(!primary_spool.exists());
    Ok(())
}

#[test]
fn decisions_library_rejects_generic_admission() -> TestResult {
    let installation = Installation::new()?;
    let arbitrary = installation.file("arbitrary.md", "# Not a decision account\n")?;
    error_json(
        &installation
            .command()
            .args([
                OsStr::new("inbox"),
                OsStr::new("enqueue"),
                arbitrary.as_os_str(),
            ])
            .output()?,
        "decision_feed_accept_required",
    )?;
    error_json(
        &installation
            .command()
            .args(["inbox", "register", "--settle-seconds", "0"])
            .output()?,
        "decision_feed_accept_required",
    )?;
    error_json(
        &installation
            .command()
            .args(["inbox", "import-backlog", "--from", "missing"])
            .output()?,
        "decision_feed_accept_required",
    )?;
    error_json(
        &installation
            .command()
            .args([OsStr::new("work"), OsStr::new("add"), arbitrary.as_os_str()])
            .output()?,
        "decision_feed_accept_required",
    )?;
    error_json(
        &installation
            .command()
            .arg("integrate")
            .arg(&arbitrary)
            .output()?,
        "decision_feed_accept_required",
    )?;

    fs::create_dir_all(&installation.spool)?;
    fs::write(installation.spool.join(".paused"), b"")?;
    installation.json_ok(["inbox", "run", "--settle-seconds", "0"])?;
    let incoming = installation.spool.join("incoming");
    fs::write(incoming.join("arbitrary.md"), "# Not a decision account\n")?;
    let run = installation.json_ok(["inbox", "run", "--settle-seconds", "0"])?;
    assert_eq!(run["registered"], 0);
    assert!(incoming.join("arbitrary.md").is_file());
    assert_eq!(directory_count(&installation.spool.join("queued"))?, 0);
    assert!(
        installation
            .spool
            .join(".decision-feed-library.json")
            .is_file()
    );

    let general_config = installation.directory.path().join("general.toml");
    fs::write(
        &general_config,
        format!(
            "library = {}\n[inbox]\nroot = {}\nminimum_available_bytes = 0\n",
            toml_string(&installation.library),
            toml_string(&installation.spool),
        ),
    )?;
    error_json(
        &command_with_config(&general_config)
            .args(["inbox", "run", "--settle-seconds", "0"])
            .output()?,
        "library_kind_mismatch",
    )?;
    error_json(
        &command_with_config(&general_config)
            .args([
                OsStr::new("inbox"),
                OsStr::new("enqueue"),
                arbitrary.as_os_str(),
            ])
            .output()?,
        "library_kind_mismatch",
    )?;
    Ok(())
}

#[test]
fn immutable_library_roles_close_direct_and_cross_config_ingress() -> TestResult {
    let installation = Installation::new()?;
    let arbitrary = installation.file("arbitrary.md", "Shared inbox claim.\n")?;

    for arguments in [
        vec![OsStr::new("work"), OsStr::new("add"), arbitrary.as_os_str()],
        vec![OsStr::new("integrate"), arbitrary.as_os_str()],
    ] {
        error_json(
            &command_with_library(&installation.library)
                .args(arguments)
                .output()?,
            "library_kind_mismatch",
        )?;
    }

    let alternate_spool = installation.directory.path().join("alternate-spool");
    let generic_config = installation.directory.path().join("alternate-general.toml");
    fs::write(
        &generic_config,
        format!(
            "library = {}\n[inbox]\nroot = {}\nminimum_available_bytes = 0\n",
            toml_string(&installation.library),
            toml_string(&alternate_spool),
        ),
    )?;
    error_json(
        &command_with_config(&generic_config)
            .args([
                OsStr::new("inbox"),
                OsStr::new("enqueue"),
                arbitrary.as_os_str(),
            ])
            .output()?,
        "library_kind_mismatch",
    )?;
    error_json(
        &command_with_config(&generic_config)
            .args(["inbox", "run", "--settle-seconds", "0"])
            .output()?,
        "library_kind_mismatch",
    )?;
    assert!(!alternate_spool.exists());

    let general_library = installation.directory.path().join("general.db");
    let initialized = successful_json(
        &command_with_library(&general_library)
            .arg("init")
            .output()?,
    )?;
    assert_eq!(initialized["kind"], "general");
    let general_id = initialized["library_id"]
        .as_str()
        .ok_or("general init omitted library_id")?;
    let false_decisions_spool = installation.directory.path().join("false-decisions-spool");
    let false_decisions_config = installation.directory.path().join("false-decisions.toml");
    fs::write(
        &false_decisions_config,
        format!(
            concat!(
                "library = {}\n",
                "[inbox]\nroot = {}\nminimum_available_bytes = 0\n",
                "[decision_feed]\nexpected_library_id = {:?}\n",
            ),
            toml_string(&general_library),
            toml_string(&false_decisions_spool),
            general_id,
        ),
    )?;
    error_json(
        &command_with_config(&false_decisions_config)
            .args(["decision-feed", "watermark"])
            .output()?,
        "library_kind_mismatch",
    )?;
    error_json(
        &command_with_config(&false_decisions_config)
            .args(["inbox", "run", "--settle-seconds", "0"])
            .output()?,
        "library_kind_mismatch",
    )?;
    assert!(!false_decisions_spool.exists());
    Ok(())
}

#[test]
fn decisions_spool_binding_requires_a_fresh_empty_spool() -> TestResult {
    let installation = Installation::new()?;
    let incoming = installation.spool.join("incoming");
    fs::create_dir_all(&incoming)?;
    fs::write(incoming.join("existing.md"), "existing primary input\n")?;
    error_json(
        &installation.command().args(["inbox", "run"]).output()?,
        "decision_feed_spool_not_fresh",
    )?;
    assert!(
        !installation
            .spool
            .join(".decision-feed-library.json")
            .exists()
    );
    assert!(incoming.join("existing.md").is_file());
    Ok(())
}

#[test]
fn acceptance_serializes_as_the_krisis_receipt_envelope() -> TestResult {
    let installation = Installation::new()?;
    let source = installation.file("account.md", &account("decision-5", "Receipt."))?;
    let output = raw_accept(
        Path::new(env!("CARGO_BIN_EXE_annals")),
        &installation.config,
        &source,
        "decision-5",
    )?;
    assert!(output.status.success());
    let envelope: KrisisSuccessEnvelope = serde_json::from_slice(&output.stdout)?;
    assert!(envelope.ok);
    assert_eq!(envelope.data.contract_version, 1);
    assert_eq!(envelope.data.library_id, installation.library_id);
    assert_eq!(envelope.data.producer, "krisis");
    assert_eq!(envelope.data.producer_key, "decision-5");
    assert_eq!(envelope.data.source_sha256.len(), 64);
    assert!(!envelope.data.job_id.is_empty());
    assert!(!envelope.data.accepted_at.is_empty());
    assert_eq!(envelope.data.acceptance, "created");
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn acceptance_fails_closed_for_input_identity_maintenance_and_storage() -> TestResult {
    let installation = Installation::new()?;
    let valid = installation.file("valid.md", &account("decision-2", "Context."))?;

    installation.write_config(0, "00000000000000000000000000000000")?;
    error_json(
        &installation
            .command()
            .args([
                OsStr::new("inbox"),
                OsStr::new("accept"),
                OsStr::new("--producer"),
                OsStr::new("krisis"),
                OsStr::new("--key"),
                OsStr::new("decision-2"),
                valid.as_os_str(),
            ])
            .output()?,
        "unexpected_decision_feed_library",
    )?;
    assert!(!installation.spool.exists());
    installation.write_config(0, &installation.library_id)?;

    let blank = installation.file("blank.md", " \n")?;
    error_json(
        &raw_accept(
            Path::new(env!("CARGO_BIN_EXE_annals")),
            &installation.config,
            &blank,
            "decision-2",
        )?,
        "blank_decision_account",
    )?;
    let oversized = installation.directory.path().join("oversized.md");
    fs::write(&oversized, vec![b'x'; 1_048_577])?;
    error_json(
        &raw_accept(
            Path::new(env!("CARGO_BIN_EXE_annals")),
            &installation.config,
            &oversized,
            "decision-2",
        )?,
        "decision_account_too_large",
    )?;

    let link = installation.directory.path().join("link.md");
    symlink(&valid, &link)?;
    error_json(
        &installation
            .command()
            .args([
                OsStr::new("inbox"),
                OsStr::new("accept"),
                OsStr::new("--producer"),
                OsStr::new("krisis"),
                OsStr::new("--key"),
                OsStr::new("decision-2"),
                link.as_os_str(),
            ])
            .output()?,
        "invalid_decision_account_source",
    )?;

    fs::create_dir_all(&installation.spool)?;
    fs::write(installation.spool.join(".maintenance"), b"")?;
    error_json(
        &installation
            .command()
            .args([
                OsStr::new("inbox"),
                OsStr::new("accept"),
                OsStr::new("--producer"),
                OsStr::new("krisis"),
                OsStr::new("--key"),
                OsStr::new("decision-2"),
                valid.as_os_str(),
            ])
            .output()?,
        "inbox_maintenance_active",
    )?;
    fs::remove_file(installation.spool.join(".maintenance"))?;
    installation.write_config(9_000_000_000_000_000_000, &installation.library_id)?;
    error_json(
        &installation
            .command()
            .args([
                OsStr::new("inbox"),
                OsStr::new("accept"),
                OsStr::new("--producer"),
                OsStr::new("krisis"),
                OsStr::new("--key"),
                OsStr::new("decision-2"),
                valid.as_os_str(),
            ])
            .output()?,
        "insufficient_storage",
    )?;
    assert_eq!(
        Connection::open(&installation.library)?.query_row(
            "SELECT COUNT(*) FROM decision_account_acceptances",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        0
    );
    Ok(())
}

#[test]
fn published_envelope_recovers_a_missing_acceptance_commit() -> TestResult {
    let installation = Installation::new()?;
    let source = installation.file("account.md", &account("decision-4", "Recovery."))?;
    let first = installation.accept("decision-4", &source)?;
    let connection = Connection::open(&installation.library)?;
    connection.execute_batch(
        "DROP TRIGGER decision_account_acceptances_immutable_delete;
         DELETE FROM decision_account_acceptances;
         CREATE TRIGGER decision_account_acceptances_immutable_delete
         BEFORE DELETE ON decision_account_acceptances BEGIN
             SELECT RAISE(ABORT, 'decision account acceptances are immutable');
         END;",
    )?;
    drop(connection);

    fs::write(installation.spool.join(".paused"), b"")?;
    error_json(
        &installation.command().args(["inbox", "run"]).output()?,
        "decision_account_acceptance_incomplete",
    )?;
    assert_eq!(directory_count(&installation.spool.join("queued"))?, 1);
    assert_eq!(
        Connection::open(&installation.library)?.query_row(
            "SELECT COUNT(*) FROM ingestions",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        0
    );

    let recovered = installation.accept("decision-4", &source)?;
    assert_eq!(recovered["acceptance"], "created");
    assert_eq!(recovered["job_id"], first["job_id"]);
    assert_eq!(recovered["accepted_at"], first["accepted_at"]);
    assert_eq!(directory_count(&installation.spool.join("queued"))?, 1);
    assert_eq!(
        Connection::open(&installation.library)?.query_row(
            "SELECT COUNT(*) FROM decision_account_acceptances",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        1
    );
    Ok(())
}

#[test]
fn post_publish_commit_error_leaves_an_envelope_for_exact_replay() -> TestResult {
    let installation = Installation::new()?;
    let source = installation.file("account.md", &account("decision-5", "Ambiguous commit."))?;
    Connection::open(&installation.library)?.execute_batch(
        "CREATE TABLE forced_acceptance_parent (id INTEGER PRIMARY KEY);
         CREATE TABLE forced_acceptance_child (
             id INTEGER PRIMARY KEY,
             parent_id INTEGER NOT NULL REFERENCES forced_acceptance_parent(id)
                 DEFERRABLE INITIALLY DEFERRED
         );
         CREATE TRIGGER fail_decision_account_acceptance_commit
         AFTER INSERT ON decision_account_acceptances BEGIN
             INSERT INTO forced_acceptance_child (id, parent_id) VALUES (1, 1);
         END;",
    )?;

    error_json(
        &installation
            .command()
            .args([
                OsStr::new("inbox"),
                OsStr::new("accept"),
                OsStr::new("--producer"),
                OsStr::new("krisis"),
                OsStr::new("--key"),
                OsStr::new("decision-5"),
                source.as_os_str(),
            ])
            .output()?,
        "database_error",
    )?;
    assert_eq!(directory_count(&installation.spool.join("queued"))?, 1);
    assert_eq!(
        Connection::open(&installation.library)?.query_row(
            "SELECT COUNT(*) FROM decision_account_acceptances",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        0
    );

    Connection::open(&installation.library)?.execute_batch(
        "DROP TRIGGER fail_decision_account_acceptance_commit;
         DROP TABLE forced_acceptance_child;
         DROP TABLE forced_acceptance_parent;",
    )?;
    let recovered = installation.accept("decision-5", &source)?;
    assert_eq!(recovered["acceptance"], "created");
    assert_eq!(directory_count(&installation.spool.join("queued"))?, 1);
    assert_eq!(
        Connection::open(&installation.library)?.query_row(
            "SELECT COUNT(*) FROM decision_account_acceptances",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        1
    );
    Ok(())
}

#[test]
fn concurrent_identical_acceptance_converges() -> TestResult {
    let installation = Installation::new()?;
    let source = installation.file("account.md", &account("decision-3", "Concurrent."))?;
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_annals"));
    let binary_one = binary.clone();
    let binary_two = binary;
    let config_one = installation.config.clone();
    let config_two = installation.config.clone();
    let source_one = source.clone();
    let source_two = source;
    let first =
        thread::spawn(move || raw_accept(&binary_one, &config_one, &source_one, "decision-3"));
    let second =
        thread::spawn(move || raw_accept(&binary_two, &config_two, &source_two, "decision-3"));
    let first = successful_json(&first.join().map_err(|_| "first command panicked")??)?;
    let second = successful_json(&second.join().map_err(|_| "second command panicked")??)?;
    let mut statuses = [
        first["acceptance"].as_str().unwrap_or_default(),
        second["acceptance"].as_str().unwrap_or_default(),
    ];
    statuses.sort_unstable();
    assert_eq!(statuses, ["created", "replayed"]);
    assert_eq!(first["job_id"], second["job_id"]);
    assert_eq!(directory_count(&installation.spool.join("queued"))?, 1);
    Ok(())
}
