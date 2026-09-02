use std::collections::BTreeSet;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, File, FileTimes};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, params};
use serde_json::{Value, json};
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const DATABASE_LABEL: &str = "Database systems";
const CONCURRENCY_LABEL: &str = "Concurrency control";
const SERIALIZABLE_LABEL: &str = "Serializable execution";
const LOCKING_LABEL: &str = "Predicate locking";

struct Library {
    directory: TempDir,
    path: PathBuf,
}

struct IngestionTimes<'a> {
    created: Option<&'a str>,
    modified: Option<&'a str>,
    first_seen: &'a str,
    ingested: Option<&'a str>,
    completed: Option<&'a str>,
}

impl Library {
    fn initialized() -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let library = Self { directory, path };
        library.json_ok(["init"])?;
        Ok(library)
    }

    fn file(&self, name: &str, contents: &str) -> TestResult<PathBuf> {
        let path = self.directory.path().join(name);
        fs::write(&path, contents)?;
        Ok(path)
    }

    fn output<I, S>(&self, arguments: I) -> io::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        command(&self.path).args(arguments).output()
    }

    fn json_ok<I, S>(&self, arguments: I) -> TestResult<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        successful_json(&self.output(arguments)?)
    }

    fn json_error<I, S>(&self, arguments: I, code: &str) -> TestResult<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        error_json(&self.output(arguments)?, code)
    }

    fn human_ok<I, S>(&self, arguments: I) -> TestResult<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = Command::new(env!("CARGO_BIN_EXE_annals"))
            .arg("--library")
            .arg(&self.path)
            .args(arguments)
            .output()?;
        assert!(
            output.status.success(),
            "command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        Ok(String::from_utf8(output.stdout)?)
    }

    fn human_with_input<I, S>(&self, arguments: I, input: &str) -> TestResult<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut child = Command::new(env!("CARGO_BIN_EXE_annals"))
            .arg("--library")
            .arg(&self.path)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take().ok_or("child had no standard input")?;
        stdin.write_all(input.as_bytes())?;
        drop(stdin);
        Ok(child.wait_with_output()?)
    }

    fn add_work(&self, name: &str, filename: &str, text: &str) -> TestResult<Value> {
        let input = self.file(filename, text)?;
        self.json_ok([
            OsStr::new("work"),
            OsStr::new("add"),
            input.as_os_str(),
            OsStr::new("--name"),
            OsStr::new(name),
        ])
    }

    fn submit(
        &self,
        work: &str,
        base: i64,
        filename: &str,
        reconciliation: &Value,
    ) -> TestResult<Value> {
        let request = self.file(filename, &serde_json::to_string_pretty(reconciliation)?)?;
        self.json_ok([
            OsStr::new("change"),
            OsStr::new("submit"),
            request.as_os_str(),
            OsStr::new("--work"),
            OsStr::new(work),
            OsStr::new("--base"),
            OsStr::new(&base.to_string()),
        ])
    }

    fn set_ingestion_times(&self, source_name: &str, times: &IngestionTimes<'_>) -> TestResult {
        let connection = Connection::open(&self.path)?;
        let updated = connection.execute(
            "UPDATE ingestions SET source_created_at = ?1, source_modified_at = ?2, \
                 first_seen_at = ?3, ingested_at = ?4, completed_at = ?5 \
             WHERE source_name = ?6",
            params![
                times.created,
                times.modified,
                times.first_seen,
                times.ingested,
                times.completed,
                source_name,
            ],
        )?;
        assert_eq!(updated, 1, "expected one ingestion named {source_name:?}");
        Ok(())
    }
}

fn command(path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_annals"));
    command.arg("--library").arg(path).arg("--json");
    command
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
    let data = envelope["data"].clone();
    assert_no_storage_coordinates(&data);
    Ok(data)
}

fn error_json(output: &Output, code: &str) -> TestResult<Value> {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    assert!(output.stdout.is_empty());
    let envelope = serde_json::from_slice::<Value>(&output.stderr)?;
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], code);
    Ok(envelope)
}

fn only_delivery(report: &Value) -> TestResult<&Value> {
    let deliveries = report["deliveries"]
        .as_array()
        .ok_or("lately deliveries were not an array")?;
    assert_eq!(deliveries.len(), 1, "expected one delivery: {report}");
    deliveries
        .first()
        .ok_or_else(|| "lately report had no delivery".into())
}

fn assert_no_storage_coordinates(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                assert!(
                    !matches!(key.as_str(), "position" | "start_byte" | "end_byte"),
                    "public JSON exposed storage coordinate {key:?}: {value}"
                );
                assert_no_storage_coordinates(child);
            }
        }
        Value::Array(array) => {
            for child in array {
                assert_no_storage_coordinates(child);
            }
        }
        _ => {}
    }
}

#[test]
fn lately_reports_manual_new_and_duplicate_deliveries_without_source_content() -> TestResult {
    const SENTINEL: &str = "SOURCE-CONTENT-SENTINEL-7f3f9a";

    let library = Library::initialized()?;
    let text = format!("A retained source contains {SENTINEL}.\n");
    let first_input = library.file("first-source.txt", &text)?;
    File::open(&first_input)?.set_times(
        FileTimes::new().set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_577_934_245)),
    )?;
    let first = library.json_ok([
        OsStr::new("work"),
        OsStr::new("add"),
        first_input.as_os_str(),
        OsStr::new("--name"),
        OsStr::new("Original source"),
    ])?;
    assert_eq!(first["retention"], "new");
    assert!(first["first_retained_at"].as_str().is_some());
    assert!(first.get("created_at").is_none());

    let second = library.add_work("Requested duplicate", "second-source.txt", &text)?;
    assert_eq!(second["work"], "Original source");
    assert_eq!(second["retention"], "duplicate");

    let report = library.json_ok([
        "lately",
        "--since",
        "2000-01-01",
        "--until",
        "2100-01-01",
        "--channel",
        "manual",
    ])?;
    assert_eq!(report["time_basis"], "ingested");
    assert_eq!(report["channel"], "manual");
    assert_eq!(report["delivery_count"], 2);
    assert_eq!(report["completed_count"], 2);
    assert_eq!(report["processing_count"], 0);
    assert_eq!(report["failed_count"], 0);
    assert_eq!(report["new_work_count"], 1);
    assert_eq!(report["duplicate_count"], 1);
    assert_eq!(report["missing_time_count"], 0);

    let deliveries = report["deliveries"]
        .as_array()
        .ok_or("lately deliveries were not an array")?;
    let first_delivery = deliveries
        .iter()
        .find(|delivery| delivery["source_name"] == "first-source.txt")
        .ok_or("first delivery was missing")?;
    assert_eq!(first_delivery["channel"], "manual");
    assert_eq!(first_delivery["status"], "completed");
    assert_eq!(first_delivery["retention"], "new");
    assert_eq!(first_delivery["result"], "retained");
    assert_eq!(first_delivery["work"], "Original source");
    assert_eq!(first_delivery["size_bytes"], text.len());
    assert_eq!(first_delivery["source_modified_at"], "2020-01-02T03:04:05Z");
    assert!(
        first_delivery["source_created_at"].is_null()
            || first_delivery["source_created_at"].is_string()
    );
    assert!(first_delivery["first_seen_at"].as_str().is_some());
    assert!(first_delivery["ingested_at"].as_str().is_some());
    assert!(first_delivery["completed_at"].as_str().is_some());
    assert!(first_delivery["error"].is_null());

    let duplicate_delivery = deliveries
        .iter()
        .find(|delivery| delivery["source_name"] == "second-source.txt")
        .ok_or("duplicate delivery was missing")?;
    assert_eq!(duplicate_delivery["retention"], "duplicate");
    assert_eq!(duplicate_delivery["result"], "retained");
    assert_eq!(duplicate_delivery["work"], "Original source");

    let report_json = serde_json::to_string(&report)?;
    assert!(!report_json.contains(SENTINEL));
    let human = library.human_ok([
        "lately",
        "--since",
        "2000-01-01",
        "--until",
        "2100-01-01",
        "--channel",
        "manual",
    ])?;
    assert!(human.contains("2 deliveries:"));
    assert!(human.contains("1 new work"));
    assert!(human.contains("1 duplicate"));
    assert!(!human.contains(SENTINEL));

    let shown = library.json_ok(["work", "show", "Original source"])?;
    assert!(shown["first_retained_at"].as_str().is_some());
    assert!(shown.get("created_at").is_none());
    let shown_human = library.human_ok(["work", "show", "Original source"])?;
    assert!(shown_human.contains("\nFirst retained: "));
    assert!(!shown_human.contains("\nCreated: "));
    assert_eq!(library.json_ok(["stats"])?["work_count"], 1);
    Ok(())
}

#[test]
fn lately_uses_half_open_windows_for_each_timestamp_and_counts_missing_time() -> TestResult {
    let library = Library::initialized()?;
    library.add_work("Earlier delivery", "earlier.txt", "Earlier metadata.\n")?;
    library.add_work("Boundary delivery", "boundary.txt", "Boundary metadata.\n")?;

    library.set_ingestion_times(
        "earlier.txt",
        &IngestionTimes {
            created: None,
            modified: Some("2026-08-01T00:00:00Z"),
            first_seen: "2026-08-02T00:00:00Z",
            ingested: Some("2026-08-03T00:00:00Z"),
            completed: Some("2026-08-04T00:00:00Z"),
        },
    )?;
    library.set_ingestion_times(
        "boundary.txt",
        &IngestionTimes {
            created: Some("2026-08-02T00:00:00Z"),
            modified: Some("2026-08-02T00:00:00Z"),
            first_seen: "2026-08-03T00:00:00Z",
            ingested: Some("2026-08-04T00:00:00Z"),
            completed: Some("2026-08-05T00:00:00Z"),
        },
    )?;

    for (basis, since, until) in [
        ("modified", "2026-08-01T00:00:00Z", "2026-08-02T00:00:00Z"),
        ("first-seen", "2026-08-02T00:00:00Z", "2026-08-03T00:00:00Z"),
        ("ingested", "2026-08-03T00:00:00Z", "2026-08-04T00:00:00Z"),
        ("completed", "2026-08-04T00:00:00Z", "2026-08-05T00:00:00Z"),
    ] {
        let report = library.json_ok([
            "lately",
            "--since",
            since,
            "--until",
            until,
            "--by",
            basis,
            "--channel",
            "manual",
        ])?;
        assert_eq!(only_delivery(&report)?["source_name"], "earlier.txt");
        assert_eq!(report["time_basis"], basis);
        assert_eq!(report["delivery_count"], 1);
        assert_eq!(report["missing_time_count"], 0);
    }

    let created = library.json_ok([
        "lately",
        "--since",
        "2026-08-01T00:00:00Z",
        "--until",
        "2026-08-03T00:00:00Z",
        "--by",
        "created",
        "--channel",
        "manual",
    ])?;
    assert_eq!(created["time_basis"], "created");
    assert_eq!(only_delivery(&created)?["source_name"], "boundary.txt");
    assert_eq!(created["missing_time_count"], 1);
    Ok(())
}

#[test]
fn lately_preserves_a_failed_invalid_utf8_delivery() -> TestResult {
    const ERROR_SENTINEL: &str = "ERROR-DIAGNOSTIC-SENTINEL-31d4d8";
    let library = Library::initialized()?;
    let input = library.directory.path().join("invalid-source.bin");
    fs::write(&input, [0xff, 0xfe])?;
    let output = library.output([
        OsStr::new("work"),
        OsStr::new("add"),
        input.as_os_str(),
        OsStr::new("--name"),
        OsStr::new("Invalid source"),
    ])?;
    error_json(&output, "input_not_utf8")?;
    Connection::open(&library.path)?.execute(
        "UPDATE ingestions SET error_message = ?1 WHERE source_name = 'invalid-source.bin'",
        [ERROR_SENTINEL],
    )?;

    let completed = library.json_ok([
        "lately",
        "--since",
        "2000-01-01",
        "--until",
        "2100-01-01",
        "--by",
        "completed",
        "--status",
        "failed",
        "--channel",
        "manual",
    ])?;
    assert_eq!(completed["delivery_count"], 1);
    assert_eq!(completed["failed_count"], 1);
    assert_eq!(completed["missing_time_count"], 0);
    let delivery = only_delivery(&completed)?;
    assert_eq!(delivery["source_name"], "invalid-source.bin");
    assert_eq!(delivery["channel"], "manual");
    assert_eq!(delivery["status"], "failed");
    assert!(delivery["retention"].is_null());
    assert!(delivery["result"].is_null());
    assert!(delivery["work"].is_null());
    assert!(delivery["ingested_at"].is_null());
    assert!(delivery["completed_at"].as_str().is_some());
    assert_eq!(delivery["error"]["code"], "input_not_utf8");
    assert!(!serde_json::to_string(&completed)?.contains(ERROR_SENTINEL));

    let ingested = library.json_ok([
        "lately",
        "--since",
        "2000-01-01",
        "--until",
        "2100-01-01",
        "--status",
        "failed",
        "--channel",
        "manual",
    ])?;
    assert_eq!(ingested["delivery_count"], 0);
    assert_eq!(ingested["missing_time_count"], 1);
    Ok(())
}

#[test]
fn a_new_manual_delivery_finalizes_an_interrupted_predecessor() -> TestResult {
    let library = Library::initialized()?;
    Connection::open(&library.path)?.execute(
        "INSERT INTO ingestions(source_name, channel, first_seen_at, status) \
         VALUES('interrupted.txt', 'manual', '2026-08-01T00:00:00Z', 'processing')",
        [],
    )?;

    library.add_work(
        "Recovered workflow",
        "recovered.txt",
        "A later manual delivery starts cleanly.\n",
    )?;

    let connection = Connection::open(&library.path)?;
    let interrupted = connection.query_row(
        "SELECT status, error_code FROM ingestions WHERE source_name = 'interrupted.txt'",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    assert_eq!(
        interrupted,
        (
            "failed".to_owned(),
            "manual_ingestion_interrupted".to_owned()
        )
    );
    assert_eq!(
        connection.query_row(
            "SELECT COUNT(*) FROM ingestions WHERE status = 'processing'",
            [],
            |row| row.get::<_, i64>(0),
        )?,
        0
    );
    Ok(())
}

#[test]
fn command_requires_a_selected_library_and_ignores_empty_environment_values() -> TestResult {
    let directory = tempfile::tempdir()?;

    let output = Command::new(env!("CARGO_BIN_EXE_annals"))
        .args(["--json", "init"])
        .env_remove("ANNALS_CONFIG")
        .env_remove("ANNALS_LIBRARY")
        .current_dir(directory.path())
        .output()?;
    error_json(&output, "library_not_configured")?;
    assert!(!directory.path().join("annals.db").exists());

    let config = directory.path().join("annals.toml");
    let configured_library = directory.path().join("configured.db");
    fs::write(&config, "library = \"configured.db\"\n")?;
    let output = Command::new(env!("CARGO_BIN_EXE_annals"))
        .arg("--config")
        .arg(&config)
        .args(["--json", "init"])
        .env("ANNALS_LIBRARY", "")
        .current_dir(directory.path())
        .output()?;
    successful_json(&output)?;
    assert!(configured_library.is_file());

    let explicit_library = directory.path().join("explicit.db");
    let output = Command::new(env!("CARGO_BIN_EXE_annals"))
        .arg("--library")
        .arg(&explicit_library)
        .args(["--json", "init"])
        .env("ANNALS_CONFIG", "")
        .env_remove("ANNALS_LIBRARY")
        .current_dir(directory.path())
        .output()?;
    successful_json(&output)?;
    assert!(explicit_library.is_file());
    Ok(())
}

fn assert_public_id(id: &str) {
    let Some(digits) = id.strip_prefix('c') else {
        panic!("public concept ID does not start with c: {id:?}");
    };
    assert!(
        !digits.is_empty(),
        "public concept ID has no digits: {id:?}"
    );
    assert!(
        !digits.starts_with('0'),
        "public concept ID is not canonical: {id:?}"
    );
    assert!(
        digits.bytes().all(|byte| byte.is_ascii_digit()),
        "public concept ID is not decimal: {id:?}"
    );
}

fn find_concept_id(items: &Value, label: &str) -> Option<String> {
    items.as_array()?.iter().find_map(|item| {
        let concept = item.get("concept").unwrap_or(item);
        if concept["label"].as_str() == Some(label) {
            concept["id"].as_str().map(ToOwned::to_owned)
        } else {
            None
        }
    })
}

fn concept_id(items: &Value, label: &str) -> TestResult<String> {
    let id = find_concept_id(items, label)
        .ok_or_else(|| format!("no concept labelled {label:?} in {items}"))?;
    assert_public_id(&id);
    Ok(id)
}

fn concept_id_across_pages(first: &Value, second: &Value, label: &str) -> TestResult<String> {
    let id = find_concept_id(first, label)
        .or_else(|| find_concept_id(second, label))
        .ok_or_else(|| format!("no concept labelled {label:?} across root pages"))?;
    assert_public_id(&id);
    Ok(id)
}

fn concept_ids(items: &Value) -> TestResult<BTreeSet<String>> {
    let entries = items
        .as_array()
        .ok_or_else(|| format!("expected concept array, got {items}"))?;
    let mut ids = BTreeSet::new();
    for entry in entries {
        let concept = entry.get("concept").unwrap_or(entry);
        let id = concept["id"]
            .as_str()
            .ok_or_else(|| format!("concept has no public ID: {concept}"))?;
        assert_public_id(id);
        ids.insert(id.to_owned());
    }
    Ok(ids)
}

fn edge_labels(edges: &Value) -> TestResult<BTreeSet<(String, String)>> {
    let edges = edges
        .as_array()
        .ok_or_else(|| format!("expected edge array, got {edges}"))?;
    edges
        .iter()
        .map(|edge| {
            let parent = edge["parent"]["label"]
                .as_str()
                .ok_or_else(|| format!("edge has no parent label: {edge}"))?;
            let child = edge["child"]["label"]
                .as_str()
                .ok_or_else(|| format!("edge has no child label: {edge}"))?;
            Ok((parent.to_owned(), child.to_owned()))
        })
        .collect()
}

fn edge_ids(edges: &Value) -> TestResult<BTreeSet<(String, String)>> {
    let edges = edges
        .as_array()
        .ok_or_else(|| format!("expected edge array, got {edges}"))?;
    edges
        .iter()
        .map(|edge| {
            let parent = edge["parent_id"]
                .as_str()
                .ok_or_else(|| format!("edge has no parent ID: {edge}"))?;
            let child = edge["child_id"]
                .as_str()
                .ok_or_else(|| format!("edge has no child ID: {edge}"))?;
            Ok((parent.to_owned(), child.to_owned()))
        })
        .collect()
}

fn diamond_source() -> &'static str {
    concat!(
        "Database systems organize persistent information.\n",
        "Concurrency control coordinates overlapping transactions.\n",
        "Serializable transactions behave as some serial execution.\n",
        "Predicate locks prevent phantom inserts.\n"
    )
}

fn diamond_reconciliation() -> Value {
    json!({
        "summary": "Build a shared serializability concept",
        "operations": [
            {
                "action": "create_concept",
                "ref": "database",
                "label": DATABASE_LABEL,
                "parents": [],
                "evidence": [{
                    "quote": "Database systems organize persistent information."
                }]
            },
            {
                "action": "create_concept",
                "ref": "concurrency",
                "label": CONCURRENCY_LABEL,
                "parents": [],
                "evidence": [{
                    "quote": "Concurrency control coordinates overlapping transactions."
                }]
            },
            {
                "action": "create_concept",
                "ref": "serializable",
                "label": SERIALIZABLE_LABEL,
                "parents": [{"new": "database"}, {"new": "concurrency"}],
                "evidence": [{
                    "quote": "Serializable transactions behave as some serial execution."
                }]
            },
            {
                "action": "create_concept",
                "ref": "locking",
                "label": LOCKING_LABEL,
                "parents": [{"new": "serializable"}],
                "evidence": [{
                    "quote": "Predicate locks prevent phantom inserts."
                }]
            }
        ],
        "annotations": [
            "Serializable execution belongs to both broader scopes."
        ]
    })
}

fn seed_diamond(library: &Library, work: &str) -> TestResult {
    library.add_work(work, "diamond.txt", diamond_source())?;
    library.submit(work, 0, "diamond.json", &diamond_reconciliation())?;
    let applied = library.json_ok(["change", "apply", "--work", work])?;
    assert_eq!(applied["revision"], 1);
    Ok(())
}

fn complete_order_reconciliation() -> Value {
    json!({
        "summary": "Build a complete conceptual order",
        "operations": [
            {
                "action": "create_concept",
                "ref": "alpha",
                "label": "Alpha scope",
                "parents": [],
                "evidence": [{"quote": "Alpha is the broadest scope."}]
            },
            {
                "action": "create_concept",
                "ref": "beta",
                "label": "Beta scope",
                "parents": [{"new": "alpha"}],
                "evidence": [{"quote": "Beta is narrower than alpha."}]
            },
            {
                "action": "create_concept",
                "ref": "gamma",
                "label": "Gamma scope",
                "parents": [{"new": "alpha"}, {"new": "beta"}],
                "evidence": [{"quote": "Gamma is narrower than beta."}]
            },
            {
                "action": "create_concept",
                "ref": "delta",
                "label": "Delta claim",
                "parents": [
                    {"new": "alpha"},
                    {"new": "beta"},
                    {"new": "gamma"}
                ],
                "evidence": [{"quote": "Delta is the narrowest claim."}]
            }
        ]
    })
}

fn search_concept_id(library: &Library, revision: i64, label: &str) -> TestResult<String> {
    let revision = revision.to_string();
    let search = library.json_ok(["search", label, "--at", revision.as_str(), "--limit", "20"])?;
    concept_id(&search["results"]["items"], label)
}

#[test]
fn init_work_add_and_show_retain_exact_source_without_changing_corpus() -> TestResult {
    let library = Library::initialized()?;
    let text = concat!(
        "# Concurrency\n",
        "Serializable transactions behave as some serial execution.\n\n",
        "# Predicate locking\n",
        "Predicate locks prevent phantom inserts.\n"
    );

    let added = library.add_work("Serializable execution", "paper.md", text)?;
    assert_eq!(added["work"], "Serializable execution");
    assert_eq!(added["size_bytes"], u64::try_from(text.len())?);
    assert_eq!(added["corpus_revision"], 0);

    let duplicate = library.add_work("Duplicate label", "copy.md", text)?;
    assert_eq!(duplicate["work"], "Serializable execution");
    assert_eq!(duplicate["sha256"], added["sha256"]);

    let shown = library.json_ok(["work", "show", "Serializable execution"])?;
    assert_eq!(shown["work"], "Serializable execution");
    assert_eq!(shown["text"], text);
    assert_eq!(shown["headings"][0]["path"], json!(["Concurrency"]));

    let overview = library.json_ok(["overview"])?;
    assert_eq!(overview["revision"], 0);
    assert_eq!(overview["concept_count"], 0);
    assert_eq!(overview["edge_count"], 0);
    let stats = library.json_ok(["stats"])?;
    assert_eq!(stats["revision"], 0);
    assert_eq!(stats["work_count"], 1);
    assert_eq!(stats["edge_count"], 0);
    assert_eq!(stats["commit_count"], 0);
    Ok(())
}

#[test]
fn repeated_evidence_quote_expands_to_exact_ranges_and_reverts() -> TestResult {
    let library = Library::initialized()?;
    let essay = "# Repeated essay\n\nA repeated passage supports one concept.\n\nA repeated passage supports one concept.\n\n";
    library.add_work("Repeated source", "repeated.md", &essay.repeat(3))?;

    let submitted = library.submit(
        "Repeated source",
        0,
        "repeated.json",
        &json!({
            "summary": "Represent one claim repeated throughout the source",
            "operations": [{
                "action": "create_concept",
                "ref": "repeated_claim",
                "label": "Repeated source claim",
                "parents": [],
                "evidence": [{
                    "quote": "A repeated passage supports one concept."
                }]
            }]
        }),
    )?;
    assert_eq!(submitted["status"], "pending");

    let validated = library.json_ok(["change", "validate"])?;
    assert_eq!(validated["status"], "valid");
    assert_eq!(
        validated["operations"][0]["evidence"][0]["occurrence_count"],
        6
    );
    let validated_human = library.human_ok(["change", "validate"])?;
    assert!(validated_human.contains("6 occurrences"));

    let applied = library.json_ok(["change", "apply"])?;
    assert_eq!(applied["revision"], 1);
    let concept = search_concept_id(&library, 1, "Repeated source claim")?;
    let evidence = library.json_ok([
        "concept", "evidence", &concept, "--at", "1", "--limit", "10",
    ])?;
    assert_eq!(evidence["evidence"]["page"]["total"], 6);
    assert_eq!(
        evidence["evidence"]["items"].as_array().map(Vec::len),
        Some(6)
    );

    let diff = library.json_ok(["diff", "0", "1"])?;
    let evidence_additions = diff["entries"]
        .as_array()
        .ok_or("diff entries were not an array")?
        .iter()
        .filter(|entry| entry["kind"] == "evidence_added")
        .count();
    assert_eq!(evidence_additions, 6);

    let reverted = library.json_ok(["revert", "1"])?;
    assert_eq!(reverted["revision"], 2);
    assert_eq!(library.json_ok(["overview"])?["concept_count"], 0);
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn diamond_reconciliation_supports_local_browsing_edge_history_and_revert() -> TestResult {
    let library = Library::initialized()?;
    library.add_work("Graph source", "graph.txt", diamond_source())?;

    let submitted = library.submit("Graph source", 0, "initial.json", &diamond_reconciliation())?;
    assert_eq!(submitted["base_revision"], 0);
    assert_eq!(submitted["status"], "pending");
    assert_eq!(submitted["operation_count"], 4);
    assert_eq!(
        submitted["reconciliation"]["operations"][2]["parents"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );

    let shown = library.json_ok(["change", "show"])?;
    assert_eq!(shown["reconciliation"], diamond_reconciliation());
    let validated = library.json_ok(["change", "validate"])?;
    assert_eq!(validated["status"], "valid");
    assert_eq!(validated["operations"].as_array().map(Vec::len), Some(4));

    let applied = library.json_ok(["change", "apply"])?;
    assert_eq!(applied["revision"], 1);
    assert_eq!(applied["status"], "applied");

    let genesis = library.json_ok(["overview", "--at", "0"])?;
    assert_eq!(genesis["concept_count"], 0);
    assert_eq!(genesis["edge_count"], 0);
    let overview = library.json_ok(["overview", "--at", "1"])?;
    assert_eq!(overview["revision"], 1);
    assert_eq!(overview["concept_count"], 4);
    assert_eq!(overview["edge_count"], 3);
    assert_eq!(overview["root_count"], 2);
    assert_eq!(overview["leaf_count"], 1);
    assert_eq!(overview["shared_concept_count"], 1);

    let first_roots = library.json_ok(["roots", "--at", "1", "--limit", "1"])?;
    assert_eq!(first_roots["revision"], 1);
    assert_eq!(first_roots["roots"]["page"]["limit"], 1);
    assert_eq!(first_roots["roots"]["page"]["returned"], 1);
    assert_eq!(first_roots["roots"]["page"]["total"], 2);
    let cursor = first_roots["roots"]["page"]["next_cursor"]
        .as_str()
        .ok_or("first roots page had no continuation cursor")?;
    let roots_human = library.human_ok(["roots", "--at", "1", "--limit", "1"])?;
    assert!(roots_human.contains("More at revision 1"));
    assert!(roots_human.contains("--at 1 --cursor"));
    let second_roots =
        library.json_ok(["roots", "--at", "1", "--limit", "1", "--cursor", cursor])?;
    assert!(second_roots["roots"]["page"]["next_cursor"].is_null());
    let database_id = concept_id_across_pages(
        &first_roots["roots"]["items"],
        &second_roots["roots"]["items"],
        DATABASE_LABEL,
    )?;
    let concurrency_id = concept_id_across_pages(
        &first_roots["roots"]["items"],
        &second_roots["roots"]["items"],
        CONCURRENCY_LABEL,
    )?;

    let database_children = library.json_ok([
        "concept",
        "children",
        &database_id,
        "--at",
        "1",
        "--limit",
        "10",
    ])?;
    let serializable_id = concept_id(&database_children["children"]["items"], SERIALIZABLE_LABEL)?;

    let serializable = library.json_ok([
        "concept",
        "show",
        &serializable_id,
        "--at",
        "1",
        "--preview-limit",
        "1",
    ])?;
    assert_eq!(serializable["concept"]["id"], serializable_id);
    assert_eq!(serializable["concept"]["parent_count"], 2);
    assert_eq!(serializable["concept"]["child_count"], 1);
    assert_eq!(serializable["concept"]["shared"], true);
    assert_eq!(serializable["concept"]["parents"]["page"]["returned"], 1);
    assert_eq!(serializable["concept"]["parents"]["page"]["total"], 2);

    let parents = library.json_ok([
        "concept",
        "parents",
        &serializable_id,
        "--at",
        "1",
        "--limit",
        "10",
    ])?;
    assert_eq!(parents["concept"]["id"], serializable_id);
    assert_eq!(
        concept_ids(&parents["parents"]["items"])?,
        BTreeSet::from([database_id.clone(), concurrency_id.clone()])
    );

    let children = library.json_ok([
        "concept",
        "children",
        &serializable_id,
        "--at",
        "1",
        "--limit",
        "10",
    ])?;
    let locking_id = concept_id(&children["children"]["items"], LOCKING_LABEL)?;
    let evidence = library.json_ok([
        "concept",
        "evidence",
        &serializable_id,
        "--at",
        "1",
        "--limit",
        "10",
    ])?;
    assert_eq!(evidence["evidence"]["page"]["total"], 1);
    assert_eq!(
        evidence["evidence"]["items"][0]["quote"],
        "Serializable transactions behave as some serial execution."
    );

    let graph = library.json_ok([
        "graph",
        &serializable_id,
        "--at",
        "1",
        "--direction",
        "both",
        "--depth",
        "1",
        "--max-nodes",
        "10",
    ])?;
    assert_eq!(graph["revision"], 1);
    assert_eq!(graph["seed"], serializable_id);
    assert_eq!(graph["edges"].as_array().map(Vec::len), Some(3));
    assert_eq!(
        concept_ids(&graph["nodes"])?,
        BTreeSet::from([
            database_id.clone(),
            concurrency_id.clone(),
            serializable_id.clone(),
            locking_id.clone(),
        ])
    );
    let graph_human = library.human_ok([
        "graph",
        &serializable_id,
        "--at",
        "1",
        "--direction",
        "both",
        "--depth",
        "1",
        "--max-nodes",
        "10",
    ])?;
    assert!(graph_human.contains("Nodes (4)"));
    assert!(graph_human.contains("Edges (3)"));
    assert!(graph_human.contains(" -> "));
    assert!(graph_human.contains("Frontier"));
    assert!(graph_human.contains("Complete through requested depth 1"));

    let search = library.json_ok([
        "search",
        DATABASE_LABEL,
        "--at",
        "1",
        "--within",
        &serializable_id,
        "--limit",
        "10",
    ])?;
    assert_eq!(search["revision"], 1);
    assert_eq!(search["within"]["id"], serializable_id);
    assert!(
        concept_ids(&search["results"]["items"])?.contains(&locking_id),
        "descendant was not found through ancestor context: {search}"
    );
    let search_human = library.human_ok([
        "search",
        DATABASE_LABEL,
        "--at",
        "1",
        "--within",
        &serializable_id,
        "--limit",
        "1",
    ])?;
    assert!(search_human.contains("More at revision 1"));

    let remove_parent = json!({
        "summary": "Remove one of the shared concept's parents",
        "operations": [
            {
                "action": "add_evidence",
                "concept": {"id": serializable_id},
                "evidence": [{
                    "quote": "Serializable transactions behave as some serial execution."
                }]
            },
            {
                "action": "remove_parent",
                "concept": {"id": serializable_id},
                "parent": {"id": concurrency_id}
            }
        ]
    });
    library.submit("Graph source", 1, "remove-parent.json", &remove_parent)?;
    assert_eq!(library.json_ok(["change", "apply"])?["revision"], 2);
    let revision_two_parents = library.json_ok([
        "concept",
        "parents",
        &serializable_id,
        "--at",
        "2",
        "--limit",
        "10",
    ])?;
    assert_eq!(
        concept_ids(&revision_two_parents["parents"]["items"])?,
        BTreeSet::from([database_id.clone()])
    );

    let removed = library.json_ok(["diff", "1", "2"])?;
    assert_eq!(removed["entries"].as_array().map(Vec::len), Some(1));
    assert_eq!(removed["entries"][0]["kind"], "parent_removed");
    assert_eq!(removed["entries"][0]["parent"]["id"], concurrency_id);
    assert_eq!(removed["entries"][0]["child"]["id"], serializable_id);
    let recorded_removal = library.json_ok(["change", "show", "--at", "2"])?;
    assert_eq!(
        recorded_removal["resolved_operations"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(recorded_removal["effects"], removed["entries"]);
    assert_eq!(
        recorded_removal["effects"].as_array().map(Vec::len),
        Some(1)
    );
    assert!(
        library
            .human_ok(["change", "show", "--at", "2"])?
            .contains("Material effects (1)")
    );

    let reverted = library.json_ok(["revert", "2"])?;
    assert_eq!(reverted["revision"], 3);
    assert_eq!(reverted["reverted_revision"], 2);
    let restored = library.json_ok([
        "concept",
        "parents",
        &serializable_id,
        "--at",
        "3",
        "--limit",
        "10",
    ])?;
    assert_eq!(
        concept_ids(&restored["parents"]["items"])?,
        BTreeSet::from([database_id, concurrency_id])
    );
    let restored_diff = library.json_ok(["diff", "2", "3"])?;
    assert_eq!(restored_diff["entries"][0]["kind"], "parent_added");
    let recorded_revert = library.json_ok(["change", "show", "--at", "3"])?;
    assert_eq!(recorded_revert["kind"], "revert");
    assert_eq!(recorded_revert["effects"], restored_diff["entries"]);
    assert_eq!(library.json_ok(["log"])?["head_revision"], 3);
    Ok(())
}

#[test]
fn an_equal_reconciliation_is_recorded_without_a_commit() -> TestResult {
    let library = Library::initialized()?;
    seed_diamond(&library, "Graph source")?;
    let serializable_id = search_concept_id(&library, 1, SERIALIZABLE_LABEL)?;

    let recorded = library.submit(
        "Graph source",
        1,
        "equal.json",
        &json!({
            "summary": "Repeat an already represented evidence mapping",
            "operations": [{
                "action": "add_evidence",
                "concept": {"id": serializable_id},
                "evidence": [{
                    "quote": "Serializable transactions behave as some serial execution."
                }]
            }],
            "annotations": ["This mapping is already present at the base revision."]
        }),
    )?;
    assert_eq!(recorded["status"], "recorded");
    assert_eq!(recorded["base_revision"], 1);

    let stats = library.json_ok(["stats"])?;
    assert_eq!(stats["revision"], 1);
    assert_eq!(stats["commit_count"], 1);
    let current = library.json_ok(["change", "show"])?;
    assert_eq!(current["status"], "recorded");
    let archived = library.json_ok(["change", "show", "--at", "1"])?;
    assert!(archived.get("status").is_none());
    assert!(archived.get("parent_revision").is_none());
    assert!(archived.get("base_revision").is_none());
    assert!(archived.get("metadata").is_none());
    assert_eq!(archived["submitted_request"], diamond_reconciliation());
    assert_eq!(
        archived["resolved_operations"].as_array().map(Vec::len),
        Some(4)
    );
    let genesis_diff = library.json_ok(["diff", "0", "1"])?;
    assert_eq!(archived["effects"], genesis_diff["entries"]);
    Ok(())
}

#[test]
fn stale_reconciliation_is_rejected_without_partial_graph_or_history_writes() -> TestResult {
    let library = Library::initialized()?;
    library.add_work("First work", "first.txt", diamond_source())?;
    library.add_work("Second work", "second.txt", "A distinct source claim.\n")?;
    library.submit("First work", 0, "first.json", &diamond_reconciliation())?;
    library.submit(
        "Second work",
        0,
        "second.json",
        &json!({
            "summary": "Add a stale concept",
            "operations": [{
                "action": "create_concept",
                "ref": "stale",
                "label": "Stale concept",
                "parents": [],
                "evidence": [{"quote": "A distinct source claim."}]
            }]
        }),
    )?;

    assert_eq!(
        library.json_ok(["change", "apply", "--work", "First work"])?["revision"],
        1
    );
    let before_stats = library.json_ok(["stats"])?;
    let before_overview = library.json_ok(["overview"])?;
    let before_log = library.json_ok(["log"])?;

    let error = library.json_error(["change", "apply", "--work", "Second work"], "stale_change")?;
    assert!(error["error"]["message"].as_str().is_some_and(|message| {
        message.contains("revision 0") && message.contains("revision 1")
    }));

    let after_stats = library.json_ok(["stats"])?;
    for field in [
        "revision",
        "concept_count",
        "edge_count",
        "work_count",
        "evidence_count",
        "pending_reconciliation_count",
        "commit_count",
    ] {
        assert_eq!(after_stats[field], before_stats[field], "changed {field}");
    }
    assert_eq!(library.json_ok(["overview"])?, before_overview);
    assert_eq!(library.json_ok(["log"])?, before_log);
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn shake_reports_confirms_commits_preserves_ancestry_and_is_revertible() -> TestResult {
    let library = Library::initialized()?;
    library.add_work(
        "Ordered scopes",
        "ordered.txt",
        concat!(
            "Alpha is the broadest scope.\n",
            "Beta is narrower than alpha.\n",
            "Gamma is narrower than beta.\n",
            "Delta is the narrowest claim.\n"
        ),
    )?;
    library.submit(
        "Ordered scopes",
        0,
        "ordered.json",
        &complete_order_reconciliation(),
    )?;
    assert_eq!(
        library.json_ok(["change", "apply", "--work", "Ordered scopes"])?["revision"],
        1
    );
    let before_stats = library.json_ok(["stats"])?;
    assert_eq!(before_stats["edge_count"], 6);
    let before_search = library.json_ok(["search", "Alpha scope", "--at", "1"])?;
    let before_items = &before_search["results"]["items"];
    let before_ids = concept_ids(before_items)?;
    let alpha_id = concept_id(before_items, "Alpha scope")?;
    let beta_id = concept_id(before_items, "Beta scope")?;
    let gamma_id = concept_id(before_items, "Gamma scope")?;
    let delta_id = concept_id(before_items, "Delta claim")?;

    let preview = library.json_ok(["shake"])?;
    assert_eq!(preview["status"], "confirmation_required");
    assert_eq!(preview["base_revision"], 1);
    assert_eq!(preview["revision"], 1);
    assert_eq!(preview["edge_count_before"], 6);
    assert_eq!(preview["removed_edge_count"], 3);
    assert_eq!(preview["edge_count_after"], 3);
    assert_eq!(
        edge_labels(&preview["removed_edges"])?,
        BTreeSet::from([
            ("Alpha scope".to_owned(), "Gamma scope".to_owned()),
            ("Alpha scope".to_owned(), "Delta claim".to_owned()),
            ("Beta scope".to_owned(), "Delta claim".to_owned()),
        ])
    );
    assert_eq!(library.json_ok(["stats"])?["revision"], 1);

    let declined = library.human_with_input(["shake", "--quiet"], "no\n")?;
    assert!(declined.status.success());
    let declined_stdout = String::from_utf8(declined.stdout)?;
    let declined_stderr = String::from_utf8(declined.stderr)?;
    assert!(declined_stdout.contains("Shake cancelled"));
    assert!(declined_stdout.contains("3 of 6 parent edges will be removed"));
    assert!(declined_stdout.contains("Alpha scope"));
    assert!(declined_stdout.contains("Gamma scope"));
    assert!(declined_stdout.contains("Delta claim"));
    assert!(declined_stderr.ends_with("[y/N] "));
    assert_eq!(library.json_ok(["stats"])?, before_stats);

    let eof = library.human_with_input(["shake"], "")?;
    assert!(eof.status.success());
    assert!(String::from_utf8(eof.stdout)?.contains("Shake cancelled"));
    assert_eq!(library.json_ok(["stats"])?, before_stats);

    let confirmed = library.human_with_input(["shake"], "YES\n")?;
    assert!(
        confirmed.status.success(),
        "shake failed: {}",
        String::from_utf8_lossy(&confirmed.stderr)
    );
    let confirmed_stdout = String::from_utf8(confirmed.stdout)?;
    assert!(confirmed_stdout.contains("Applied shake as revision 2"));
    assert!(confirmed_stdout.contains("3 of 6 parent edges"));
    assert!(String::from_utf8(confirmed.stderr)?.ends_with("[y/N] "));

    let overview = library.json_ok(["overview"])?;
    assert_eq!(overview["revision"], 2);
    assert_eq!(overview["concept_count"], 4);
    assert_eq!(overview["edge_count"], 3);
    assert_eq!(overview["root_count"], 1);
    assert_eq!(overview["leaf_count"], 1);
    assert_eq!(overview["shared_concept_count"], 0);
    let graph = library.json_ok([
        "graph",
        alpha_id.as_str(),
        "--at",
        "2",
        "--direction",
        "children",
        "--depth",
        "3",
        "--max-nodes",
        "10",
    ])?;
    assert_eq!(
        edge_ids(&graph["edges"])?,
        BTreeSet::from([
            (alpha_id.clone(), beta_id.clone()),
            (beta_id, gamma_id.clone()),
            (gamma_id, delta_id),
        ])
    );
    let after_search = library.json_ok(["search", "Alpha scope", "--at", "2"])?;
    assert_eq!(concept_ids(&after_search["results"]["items"])?, before_ids);

    let changes = library.json_ok(["diff", "1", "2"])?;
    let entries = changes["entries"].as_array().ok_or("diff had no entries")?;
    assert_eq!(entries.len(), 3);
    assert!(
        entries
            .iter()
            .all(|entry| entry["kind"] == "parent_removed")
    );
    let recorded = library.json_ok(["change", "show", "--at", "2"])?;
    assert_eq!(recorded["kind"], "shake");
    assert_eq!(
        recorded["submitted_request"]["operation"],
        "transitive_reduction"
    );
    assert_eq!(recorded["effects"], changes["entries"]);
    assert!(recorded.get("metadata").is_none());
    assert!(
        library
            .human_ok(["change", "show", "--at", "2"])?
            .contains("Material effects (3)")
    );
    let log = library.json_ok(["log"])?;
    assert_eq!(log["commits"][0]["kind"], "shake");

    let no_op = library.human_with_input(["shake"], "")?;
    assert!(no_op.status.success());
    assert!(String::from_utf8(no_op.stdout)?.contains("already transitively reduced"));
    assert!(no_op.stderr.is_empty());

    let second = library.json_ok(["shake", "--yes"])?;
    assert_eq!(second["status"], "unchanged");
    assert_eq!(second["removed_edge_count"], 0);
    assert_eq!(second["revision"], 2);
    assert_eq!(library.json_ok(["stats"])?["commit_count"], 2);

    assert_eq!(library.json_ok(["revert", "2"])?["revision"], 3);
    assert_eq!(library.json_ok(["overview"])?["edge_count"], 6);

    let automatic = library.json_ok(["shake", "--yes"])?;
    assert_eq!(automatic["status"], "applied");
    assert_eq!(automatic["base_revision"], 3);
    assert_eq!(automatic["revision"], 4);
    assert_eq!(automatic["removed_edge_count"], 3);
    assert_eq!(automatic["removed_edges"].as_array().map(Vec::len), Some(3));

    assert_eq!(library.json_ok(["revert", "4"])?["revision"], 5);
    let quiet = library.human_with_input(["shake", "--yes", "--quiet"], "")?;
    assert!(
        quiet.status.success(),
        "quiet shake failed: {}",
        String::from_utf8_lossy(&quiet.stderr)
    );
    assert!(quiet.stdout.is_empty());
    assert!(quiet.stderr.is_empty());
    assert_eq!(library.json_ok(["overview"])?["revision"], 6);
    assert_eq!(library.json_ok(["overview"])?["edge_count"], 3);
    Ok(())
}

#[test]
fn top_level_validate_command_is_not_available() -> TestResult {
    let library = Library::initialized()?;
    library.json_error(["validate"], "invalid_command")?;
    Ok(())
}

#[test]
fn public_help_uses_graph_commands_and_legacy_tree_contract_is_rejected() -> TestResult {
    let help = Command::new(env!("CARGO_BIN_EXE_annals"))
        .arg("--help")
        .output()?;
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout)?;
    for required in [
        "work",
        "integrate",
        "change",
        "overview",
        "roots",
        "concept",
        "graph",
        "shake",
        "search",
        "lately",
        "log",
        "diff",
        "revert",
    ] {
        assert!(help.contains(required), "help omitted {required:?}\n{help}");
    }
    for forbidden in [
        "--position",
        "node move",
        "tree create",
        "tree delete",
        "ingest",
    ] {
        assert!(
            !help.contains(forbidden),
            "help exposed {forbidden:?}\n{help}"
        );
    }

    let library = Library::initialized()?;
    library.add_work(
        "Language source",
        "language.txt",
        "Exact source language.\n",
    )?;
    let legacy_requests = [
        json!({
            "summary": "Try a path selector",
            "operations": [{
                "action": "add_evidence",
                "concept": {"path": ["Old tree node"]},
                "evidence": [{"quote": "Exact source language."}]
            }]
        }),
        json!({
            "summary": "Try ordered placement",
            "operations": [{
                "action": "create_concept",
                "ref": "old",
                "label": "Old tree node",
                "parents": [],
                "before": {"id": "c1"},
                "position": 0,
                "evidence": [{"quote": "Exact source language."}]
            }]
        }),
        json!({
            "summary": "Try moving a subtree",
            "operations": [{
                "action": "move_concept",
                "concept": {"id": "c1"},
                "under": {"id": "c2"}
            }]
        }),
    ];
    for (index, request) in legacy_requests.iter().enumerate() {
        let filename = format!("legacy-{index}.json");
        let input = library.file(&filename, &serde_json::to_string_pretty(request)?)?;
        library.json_error(
            [
                OsStr::new("change"),
                OsStr::new("submit"),
                input.as_os_str(),
                OsStr::new("--work"),
                OsStr::new("Language source"),
                OsStr::new("--base"),
                OsStr::new("0"),
            ],
            "invalid_reconciliation",
        )?;
    }
    assert_eq!(
        library.json_ok(["stats"])?["pending_reconciliation_count"],
        0
    );
    Ok(())
}
