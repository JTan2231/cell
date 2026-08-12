use std::error::Error;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

struct Library {
    _directory: TempDir,
    path: PathBuf,
}

impl Library {
    fn initialized() -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let library = Self {
            _directory: directory,
            path,
        };
        library.json_ok(["init"])?;
        Ok(library)
    }

    fn json_output<I, S>(&self, arguments: I) -> io::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        json_output_for(&self.path, arguments)
    }

    fn json_ok<I, S>(&self, arguments: I) -> TestResult<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.json_output(arguments)?;
        successful_json(&output)
    }

    fn json_error<I, S>(&self, arguments: I, exit_code: i32, code: &str) -> TestResult<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        json_error_for(&self.path, arguments, exit_code, code)
    }

    fn release_json_ok<I, S>(&self, arguments: I) -> TestResult<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = release_command_for(&self.path)?;
        let output = command.arg("--json").args(arguments).output()?;
        successful_json(&output)
    }

    fn seed_complete_tree(&self) -> TestResult<i64> {
        let root = mutation_id(&self.json_ok([
            "tree",
            "create",
            "--title",
            "Operations",
            "--body",
            "Operational notes",
        ])?)?;
        let root = root.to_string();
        mutation_id(&self.json_ok([
            "node",
            "add",
            "--parent",
            &root,
            "--kind",
            "source",
            "--title",
            "Runbook",
            "--body",
            "operationalneedle recovery procedure",
        ])?)
    }

    fn assert_validation_issue(&self, issue_code: &str) -> TestResult {
        assert_validation_issue_at(&self.path, issue_code)
    }

    fn reindex_and_validate(&self) -> TestResult {
        self.json_ok(["reindex"])?;
        let report = self.json_ok(["validate"])?;
        assert_eq!(report["valid"], true);
        Ok(())
    }
}

fn command_for(path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_annals"));
    command.arg("--library").arg(path);
    command
}

fn json_output_for<I, S>(path: &Path, arguments: I) -> io::Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    command_for(path).arg("--json").args(arguments).output()
}

fn json_ok_for<I, S>(path: &Path, arguments: I) -> TestResult<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = json_output_for(path, arguments)?;
    successful_json(&output)
}

fn json_error_for<I, S>(path: &Path, arguments: I, exit_code: i32, code: &str) -> TestResult<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = json_output_for(path, arguments)?;
    assert_eq!(
        output.status.code(),
        Some(exit_code),
        "unexpected error output: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let envelope = serde_json::from_slice::<Value>(&output.stderr)?;
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], code);
    Ok(envelope)
}

fn assert_validation_issue_at(path: &Path, issue_code: &str) -> TestResult {
    let envelope = json_error_for(path, ["validate"], 5, "validation_failed")?;
    let message = envelope["error"]["message"]
        .as_str()
        .ok_or_else(|| io::Error::other("validation error omitted its message"))?;
    assert!(
        message.contains(&format!("[{issue_code}]")),
        "validation did not report {issue_code}: {message}"
    );
    Ok(())
}

fn release_command_for(path: &Path) -> TestResult<Command> {
    let target_directory = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
        PathBuf::from,
    );
    let executable = if cfg!(windows) {
        "annals.exe"
    } else {
        "annals"
    };
    let binary = target_directory.join("release").join(executable);
    if !binary.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "release binary not found at {}; run `cargo build --release` first",
                binary.display()
            ),
        )
        .into());
    }
    let mut command = Command::new(binary);
    command.arg("--library").arg(path);
    Ok(command)
}

fn successful_json(output: &Output) -> TestResult<Value> {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let envelope = serde_json::from_slice::<Value>(&output.stdout)?;
    assert_eq!(envelope["ok"], true);
    Ok(envelope["data"].clone())
}

fn mutation_id(data: &Value) -> TestResult<i64> {
    data["node_ids"]
        .as_array()
        .and_then(|ids| ids.first())
        .and_then(Value::as_i64)
        .ok_or_else(|| io::Error::other("mutation response omitted its node ID").into())
}

fn assert_primary_search_result(data: &Value, node_id: i64, breadcrumb_len: usize) {
    assert_eq!(data["results"][0]["node_id"], node_id);
    assert_eq!(
        data["results"][0]["breadcrumb"].as_array().map(Vec::len),
        Some(breadcrumb_len)
    );
}

fn measure_samples(
    count: usize,
    mut operation: impl FnMut(usize) -> TestResult,
) -> TestResult<Vec<Duration>> {
    let mut samples = Vec::with_capacity(count);
    for iteration in 0..count {
        let started = Instant::now();
        operation(iteration)?;
        samples.push(started.elapsed());
    }
    Ok(samples)
}

#[must_use]
fn p95(samples: &mut [Duration]) -> Duration {
    assert!(!samples.is_empty(), "p95 requires at least one sample");
    samples.sort_unstable();
    let index = samples
        .len()
        .saturating_mul(95)
        .div_ceil(100)
        .saturating_sub(1);
    samples[index]
}

#[must_use]
fn p50(samples: &mut [Duration]) -> Duration {
    assert!(!samples.is_empty(), "p50 requires at least one sample");
    samples.sort_unstable();
    samples[(samples.len() - 1) / 2]
}

fn seed_corpus(path: &Path, unit_count: i64) -> TestResult<Duration> {
    assert!(unit_count >= 35, "the deterministic corpus needs 35 IDs");
    let started = Instant::now();
    let long_body = format!(
        "commonterm longchunkmarker {}",
        std::iter::repeat_n("longbodytoken", 1_598)
            .collect::<Vec<_>>()
            .join(" ")
    );
    assert!(long_body.split_whitespace().count() > 1_500);
    let mut connection = Connection::open(path)?;
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "INSERT INTO nodes(id, parent_id, kind, title, body, position, created_at, updated_at) \
         VALUES (1, NULL, 'topic', 'Performance corpus', 'corpus root', 0, 't', 't'); \
         INSERT INTO nodes(id, parent_id, kind, title, body, position, created_at, updated_at) \
         WITH RECURSIVE sequence(value) AS ( \
             VALUES(2) UNION ALL SELECT value + 1 FROM sequence WHERE value < 11 \
         ) \
         SELECT value, 1, 'topic', printf('Branch %02d', value - 1), \
                'deterministic branch', value - 2, 't', 't' \
         FROM sequence; \
         INSERT INTO nodes(id, parent_id, kind, title, body, position, created_at, updated_at) \
         WITH RECURSIVE sequence(value) AS ( \
             VALUES(12) UNION ALL SELECT value + 1 FROM sequence WHERE value < 31 \
         ) \
         SELECT value, CASE value WHEN 12 THEN 2 ELSE value - 1 END, 'topic', \
                printf('Deep topic %02d', value - 11), 'deep deterministic branch', \
                0, 't', 't' \
         FROM sequence; \
         INSERT INTO nodes(id, parent_id, kind, title, body, position, created_at, updated_at) \
         VALUES (32, 31, 'source', 'Deep source', 'commonterm deepmarker text', 0, 't', 't');",
    )?;
    transaction.execute(
        "INSERT INTO nodes(id, parent_id, kind, title, body, position, created_at, updated_at) \
         VALUES (33, 3, 'source', 'Long chunked source', ?1, 0, 't', 't')",
        [&long_body],
    )?;
    transaction.execute(
        "INSERT INTO nodes(id, parent_id, kind, title, body, position, created_at, updated_at) \
         WITH RECURSIVE sequence(value) AS ( \
             VALUES(35) UNION ALL SELECT value + 1 FROM sequence WHERE value < ?1 \
         ) \
         SELECT value, 2 + (value % 10), 'source', printf('Source %06d', value), \
                printf('commonterm deterministic marker%06d text', value), \
                value / 10, 't', 't' \
         FROM sequence",
        [unit_count],
    )?;
    transaction.execute(
        "INSERT INTO sources(node_id) SELECT id FROM nodes WHERE kind = 'source'",
        [],
    )?;
    transaction.commit()?;
    Ok(started.elapsed())
}

fn assert_long_source_chunked(path: &Path) -> TestResult {
    let connection = Connection::open(path)?;
    let passage_count = connection.query_row(
        "SELECT COUNT(*) FROM search_units WHERE node_id = 33 AND unit_kind = 'passage'",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    assert_eq!(passage_count, 2);
    Ok(())
}

#[must_use]
fn units_per_second(unit_count: i64, elapsed: Duration) -> u128 {
    u128::from(unit_count.unsigned_abs()).saturating_mul(1_000_000_000) / elapsed.as_nanos().max(1)
}

#[derive(Clone, Copy, Debug)]
struct Percentiles {
    p50: Duration,
    p95: Duration,
}

fn percentiles(mut samples: Vec<Duration>) -> Percentiles {
    Percentiles {
        p50: p50(&mut samples),
        p95: p95(&mut samples),
    }
}

fn measure_release_search(
    library: &Library,
    arguments: &[&str],
    verify: impl Fn(&Value),
) -> TestResult<Percentiles> {
    let warmup = library.release_json_ok(arguments.iter().copied())?;
    verify(&warmup);
    let samples = measure_samples(20, |_| {
        let results = library.release_json_ok(arguments.iter().copied())?;
        verify(&results);
        Ok(())
    })?;
    Ok(percentiles(samples))
}

#[derive(Clone, Copy, Debug)]
struct MutationTimings {
    append: Percentiles,
    edit: Percentiles,
    movement: Percentiles,
    delete: Percentiles,
}

fn measure_release_mutations(library: &Library) -> TestResult<MutationTimings> {
    let mut node_ids = Vec::new();
    let append_samples = measure_samples(20, |iteration| {
        let title = format!("Appended source {iteration:02}");
        let body = format!("commonterm appended marker {iteration:02}");
        let result = library.release_json_ok([
            "node", "add", "--parent", "2", "--kind", "source", "--title", &title, "--body", &body,
        ])?;
        let node_id = mutation_id(&result)?;
        assert!(node_id > 100_000);
        node_ids.push(node_id);
        Ok(())
    })?;
    let edit_samples = measure_samples(20, |iteration| {
        let node_id = node_ids[iteration].to_string();
        let body = format!("edited commonterm marker {iteration:02}");
        library.release_json_ok(["node", "edit", &node_id, "--body", &body])?;
        Ok(())
    })?;
    let move_samples = measure_samples(20, |iteration| {
        let node_id = node_ids[iteration].to_string();
        library.release_json_ok(["node", "move", &node_id, "--parent", "3"])?;
        Ok(())
    })?;
    let delete_samples = measure_samples(20, |iteration| {
        let node_id = node_ids[iteration].to_string();
        library.release_json_ok(["node", "delete", &node_id])?;
        Ok(())
    })?;
    Ok(MutationTimings {
        append: percentiles(append_samples),
        edit: percentiles(edit_samples),
        movement: percentiles(move_samples),
        delete: percentiles(delete_samples),
    })
}

fn report_100k_timings(
    library: &Library,
    bulk_seed_elapsed: Duration,
    reindex_elapsed: Duration,
    searches: [Percentiles; 6],
    mutations: MutationTimings,
) -> TestResult {
    let [
        global_rare,
        global_common,
        scoped_rare,
        scoped_common,
        shallow,
        deep,
    ] = searches;
    let database_bytes = std::fs::metadata(&library.path)?.len();
    let sqlite_version =
        Connection::open(&library.path)?
            .query_row("SELECT sqlite_version()", [], |row| row.get::<_, String>(0))?;
    let logical_cpus = std::thread::available_parallelism()?.get();
    eprintln!(
        "100k release smoke (10 branches, deterministic IDs 1..=100000): \
         bulk seed={bulk_seed_elapsed:?} ({} units/s), reindex={reindex_elapsed:?}, \
         global rare p50/p95={:?}/{:?}, global common p50/p95={:?}/{:?}, \
         scoped rare p50/p95={:?}/{:?}, scoped common p50/p95={:?}/{:?}, \
         shallow lookup p50/p95={:?}/{:?}, deep lookup p50/p95={:?}/{:?}, \
         append p50/p95={:?}/{:?}, edit p50/p95={:?}/{:?}, \
         move p50/p95={:?}/{:?}, delete p50/p95={:?}/{:?}, \
         database={database_bytes} bytes, fixture=deterministic, cache=warm, \
         sqlite={sqlite_version}, host={}-{} ({logical_cpus} logical CPUs)",
        units_per_second(100_000, bulk_seed_elapsed),
        global_rare.p50,
        global_rare.p95,
        global_common.p50,
        global_common.p95,
        scoped_rare.p50,
        scoped_rare.p95,
        scoped_common.p50,
        scoped_common.p95,
        shallow.p50,
        shallow.p95,
        deep.p50,
        deep.p95,
        mutations.append.p50,
        mutations.append.p95,
        mutations.edit.p50,
        mutations.edit.p95,
        mutations.movement.p50,
        mutations.movement.p95,
        mutations.delete.p50,
        mutations.delete.p95,
        std::env::consts::ARCH,
        std::env::consts::OS
    );
    Ok(())
}

#[test]
fn external_fts_drift_is_detected_and_reindex_repairs_it() -> TestResult {
    let library = Library::initialized()?;
    let source_id = library.seed_complete_tree()?;
    let backup_path = library.path.with_file_name("recovery.db");
    library.json_ok([OsStr::new("backup"), backup_path.as_os_str()])?;

    let connection = Connection::open(&backup_path)?;
    connection.execute(
        "INSERT INTO search_fts(rowid, title, breadcrumb, text) \
         VALUES (999999, 'phantom', 'phantom', 'phantomftsneedle')",
        [],
    )?;
    drop(connection);

    assert_validation_issue_at(&backup_path, "fts_integrity_error")?;
    json_ok_for(&backup_path, ["reindex"])?;
    assert_eq!(json_ok_for(&backup_path, ["validate"])?["valid"], true);
    let results = json_ok_for(&backup_path, ["search", "operationalneedle"])?;
    assert!(results["results"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["node_id"].as_i64() == Some(source_id))
    }));

    assert_eq!(library.json_ok(["validate"])?["valid"], true);
    let original_results = library.json_ok(["search", "operationalneedle"])?;
    assert_eq!(original_results["results"][0]["node_id"], source_id);
    let original = Connection::open(&library.path)?;
    let phantom_count = original.query_row(
        "SELECT COUNT(*) FROM search_fts WHERE rowid = 999999",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    assert_eq!(phantom_count, 0);
    Ok(())
}

#[test]
fn validation_detects_hash_stale_content_and_version_drift() -> TestResult {
    let library = Library::initialized()?;
    let source_id = library.seed_complete_tree()?;
    let connection = Connection::open(&library.path)?;
    connection.execute(
        "UPDATE search_units SET content_hash = 'sha256:invalid' WHERE node_id = ?1",
        [source_id],
    )?;
    drop(connection);
    library.assert_validation_issue("search_unit_stale")?;
    library.reindex_and_validate()?;

    let connection = Connection::open(&library.path)?;
    connection.execute(
        "UPDATE nodes SET body = 'canonical content changed without indexing' WHERE id = ?1",
        [source_id],
    )?;
    drop(connection);
    library.assert_validation_issue("search_unit_stale")?;
    library.reindex_and_validate()?;

    let connection = Connection::open(&library.path)?;
    connection.execute(
        "UPDATE index_metadata SET value = '999' WHERE key = 'indexer_version'",
        [],
    )?;
    drop(connection);
    library.assert_validation_issue("index_not_current")?;
    library.reindex_and_validate()?;
    Ok(())
}

#[test]
fn failed_incremental_and_full_indexing_roll_back_canonical_state() -> TestResult {
    let library = Library::initialized()?;
    let source_id = library.seed_complete_tree()?;
    let connection = Connection::open(&library.path)?;
    connection.execute_batch(
        "CREATE TRIGGER force_search_unit_failure \
         BEFORE INSERT ON search_units \
         BEGIN SELECT RAISE(ABORT, 'forced search-unit failure'); END;",
    )?;
    drop(connection);

    let source = source_id.to_string();
    library.json_error(
        ["node", "edit", &source, "--body", "replacementneedle"],
        5,
        "index_insert_failed",
    )?;
    let unchanged = library.json_ok(["node", "show", &source])?;
    assert_eq!(unchanged["body"], "operationalneedle recovery procedure");
    let old_results = library.json_ok(["search", "operationalneedle"])?;
    assert_eq!(old_results["results"][0]["node_id"], source_id);
    assert_eq!(
        library.json_ok(["search", "replacementneedle"])?["results"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );

    library.json_error(["reindex"], 5, "index_insert_failed")?;
    let still_searchable = library.json_ok(["search", "operationalneedle"])?;
    assert_eq!(still_searchable["results"][0]["node_id"], source_id);
    let connection = Connection::open(&library.path)?;
    connection.execute_batch("DROP TRIGGER force_search_unit_failure;")?;
    drop(connection);
    assert_eq!(library.json_ok(["validate"])?["valid"], true);
    Ok(())
}

#[test]
fn reader_snapshot_remains_stable_while_cli_writer_commits_under_wal() -> TestResult {
    let library = Library::initialized()?;
    library.seed_complete_tree()?;
    let mut reader = Connection::open(&library.path)?;
    let journal_mode =
        reader.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?;
    assert_eq!(journal_mode, "wal");

    let snapshot = reader.transaction()?;
    let before =
        snapshot.query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get::<_, i64>(0))?;
    assert_eq!(before, 2);
    let writer = library.json_output([
        "tree",
        "create",
        "--title",
        "Concurrent root",
        "--body",
        "writer committed",
    ])?;
    assert!(
        writer.status.success(),
        "writer failed: {}",
        String::from_utf8_lossy(&writer.stderr)
    );
    let during =
        snapshot.query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get::<_, i64>(0))?;
    assert_eq!(during, before, "the open reader snapshot changed");
    snapshot.commit()?;

    let after = reader.query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get::<_, i64>(0))?;
    assert_eq!(after, 3);
    Ok(())
}

#[test]
#[ignore = "manual deterministic 100k-unit performance smoke test"]
fn deterministic_100k_unit_reindex_and_search_smoke() -> TestResult {
    let library = Library::initialized()?;
    let bulk_seed_elapsed = seed_corpus(&library.path, 100_000)?;

    let reindex_started = Instant::now();
    let rebuilt = library.release_json_ok(["reindex"])?;
    let reindex_elapsed = reindex_started.elapsed();
    assert_eq!(rebuilt["indexed_units"], 100_000);
    assert_long_source_chunked(&library.path)?;
    assert!(reindex_elapsed < Duration::from_mins(1));

    let scoped_rare = measure_release_search(
        &library,
        &[
            "search",
            "marker100000",
            "--within",
            "2",
            "--kind",
            "source",
        ],
        |results| {
            assert_eq!(results["results"][0]["node_id"], 100_000);
        },
    )?;
    let scoped_common = measure_release_search(
        &library,
        &["search", "commonterm", "--within", "2", "--kind", "source"],
        |results| {
            assert!(results["results"].as_array().is_some_and(|items| {
                items.len() == 10
                    && items.iter().all(|item| {
                        item["node_id"]
                            .as_i64()
                            .is_some_and(|id| id == 32 || id >= 33 && id % 10 == 0)
                    })
            }));
        },
    )?;
    let global_rare = measure_release_search(
        &library,
        &["search", "marker100000", "--kind", "source"],
        |results| {
            assert_eq!(results["results"][0]["node_id"], 100_000);
        },
    )?;
    let global_common = measure_release_search(
        &library,
        &["search", "commonterm", "--kind", "source"],
        |results| {
            assert_eq!(results["results"].as_array().map(Vec::len), Some(10));
        },
    )?;
    let shallow_lookup = measure_release_search(&library, &["search", "Source 000040"], |data| {
        assert_primary_search_result(data, 40, 3);
    })?;
    let deep_lookup = measure_release_search(&library, &["search", "Deep source"], |data| {
        assert_primary_search_result(data, 32, 23);
    })?;
    let mutations = measure_release_mutations(&library)?;

    for search in [scoped_rare, scoped_common, global_rare, global_common] {
        assert!(search.p95 < Duration::from_millis(150));
    }
    for mutation in [
        mutations.append,
        mutations.edit,
        mutations.movement,
        mutations.delete,
    ] {
        assert!(mutation.p95 < Duration::from_millis(50));
    }
    report_100k_timings(
        &library,
        bulk_seed_elapsed,
        reindex_elapsed,
        [
            global_rare,
            global_common,
            scoped_rare,
            scoped_common,
            shallow_lookup,
            deep_lookup,
        ],
        mutations,
    )
}

#[test]
#[ignore = "manual deterministic 1k/10k scale samples"]
fn deterministic_1k_and_10k_scale_samples() -> TestResult {
    for unit_count in [1_000_i64, 10_000] {
        let library = Library::initialized()?;
        let bulk_seed_elapsed = seed_corpus(&library.path, unit_count)?;
        let reindex_started = Instant::now();
        let rebuilt = library.release_json_ok(["reindex"])?;
        let reindex_elapsed = reindex_started.elapsed();
        assert_eq!(rebuilt["indexed_units"], unit_count);
        assert_long_source_chunked(&library.path)?;

        let marker = format!("marker{unit_count:06}");
        let scope = (2 + unit_count % 10).to_string();
        let results =
            library.release_json_ok(["search", &marker, "--within", &scope, "--kind", "source"])?;
        assert_eq!(results["results"][0]["node_id"], unit_count);
        eprintln!(
            "{unit_count}-unit deterministic sample: bulk seed={bulk_seed_elapsed:?} \
             ({} units/s), \
             reindex={reindex_elapsed:?}",
            units_per_second(unit_count, bulk_seed_elapsed)
        );
    }
    Ok(())
}
