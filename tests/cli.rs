use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

struct Library {
    directory: TempDir,
    path: PathBuf,
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
    assert_no_storage_selectors(&data);
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

fn assert_no_storage_selectors(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                assert!(
                    key != "id"
                        && !key.ends_with("_id")
                        && !key.ends_with("_ids")
                        && !matches!(key.as_str(), "position" | "start_byte" | "end_byte"),
                    "public JSON exposed storage field {key:?}: {value}"
                );
                assert_no_storage_selectors(child);
            }
        }
        Value::Array(array) => {
            for child in array {
                assert_no_storage_selectors(child);
            }
        }
        _ => {}
    }
}

fn initial_reconciliation() -> Value {
    json!({
        "summary": "Integrate serializable execution and predicate locking",
        "operations": [
            {
                "action": "create_concept",
                "label": "Database systems",
                "evidence": [{
                    "quote": "Serializable transactions behave as some serial execution."
                }]
            },
            {
                "action": "create_concept",
                "label": "Serializable execution",
                "under": {"new": "Database systems"},
                "evidence": [{
                    "quote": "Serializable transactions behave as some serial execution."
                }]
            },
            {
                "action": "create_concept",
                "label": "Predicate locking",
                "under": {"new": "Serializable execution"},
                "evidence": [{
                    "quote": "Predicate locks prevent phantom inserts."
                }]
            }
        ],
        "annotations": [
            "The source reports these claims; external implementation was not evaluated."
        ]
    })
}

#[test]
fn work_add_and_show_retain_exact_source_without_changing_corpus() -> TestResult {
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

    let stats = library.json_ok(["stats"])?;
    assert_eq!(stats["revision"], 0);
    assert_eq!(stats["work_count"], 1);
    assert_eq!(stats["concept_count"], 0);
    assert_eq!(stats["commit_count"], 0);
    Ok(())
}

#[test]
fn one_reconciliation_moves_from_submission_through_history_and_revert() -> TestResult {
    let library = Library::initialized()?;
    let text = concat!(
        "Serializable transactions behave as some serial execution.\n",
        "Predicate locks prevent phantom inserts.\n"
    );
    library.add_work("Serializable execution", "serializable.txt", text)?;

    let submitted = library.submit(
        "Serializable execution",
        0,
        "reconciliation.json",
        &initial_reconciliation(),
    )?;
    assert_eq!(submitted["work"], "Serializable execution");
    assert_eq!(submitted["base_revision"], 0);
    assert_eq!(submitted["status"], "pending");
    assert_eq!(submitted["operation_count"], 3);
    assert_eq!(library.json_ok(["stats"])?["revision"], 0);

    let shown_reconciliation = library.json_ok(["change", "show"])?;
    assert_eq!(
        shown_reconciliation["reconciliation"]["operations"][1]["under"]["new"],
        "Database systems"
    );
    assert_eq!(
        shown_reconciliation["annotations"],
        json!(["The source reports these claims; external implementation was not evaluated."])
    );
    let quiet_show = Command::new(env!("CARGO_BIN_EXE_annals"))
        .arg("--library")
        .arg(&library.path)
        .arg("--quiet")
        .args(["change", "show"])
        .output()?;
    assert!(quiet_show.status.success());
    assert!(quiet_show.stderr.is_empty());
    assert!(String::from_utf8(quiet_show.stdout)?.contains("Pending reconciliation"));
    let validated = library.json_ok(["change", "validate"])?;
    assert_eq!(validated["status"], "valid");
    assert_eq!(validated["operations"].as_array().map(Vec::len), Some(3));

    let applied = library.json_ok(["change", "apply"])?;
    assert_eq!(applied["revision"], 1);
    assert_eq!(applied["status"], "applied");

    let genesis = library.json_ok(["show", "--at", "0"])?;
    assert_eq!(genesis["revision"], 0);
    assert_eq!(genesis["concepts"], json!([]));
    let revision_one = library.json_ok(["show", "--at", "1"])?;
    assert_eq!(revision_one["revision"], 1);
    assert_eq!(revision_one["concepts"].as_array().map(Vec::len), Some(3));
    assert_eq!(
        revision_one["concepts"][2]["path"],
        json!([
            "Database systems",
            "Serializable execution",
            "Predicate locking"
        ])
    );
    assert_eq!(
        revision_one["concepts"][2]["evidence"][0]["quote"],
        "Predicate locks prevent phantom inserts."
    );

    let log = library.json_ok(["log"])?;
    assert_eq!(log["head_revision"], 1);
    assert_eq!(log["commits"][0]["revision"], 1);
    assert_eq!(log["commits"][0]["parent_revision"], 0);
    assert_eq!(
        log["commits"][0]["summary"],
        initial_reconciliation()["summary"]
    );

    let diff = library.json_ok(["diff", "0", "1"])?;
    assert_eq!(diff["from_revision"], 0);
    assert_eq!(diff["to_revision"], 1);
    assert!(diff["entries"].as_array().is_some_and(|entries| {
        entries.iter().any(|entry| {
            entry["kind"] == "created"
                && entry["after"]
                    == json!([
                        "Database systems",
                        "Serializable execution",
                        "Predicate locking"
                    ])
        })
    }));
    assert_eq!(library.json_ok(["validate"])?["valid"], true);
    let reverted = library.json_ok(["revert", "1"])?;
    assert_eq!(reverted["revision"], 2);
    assert_eq!(reverted["reverted_revision"], 1);
    assert_eq!(
        library.json_ok(["show", "--at", "2"])?["concepts"],
        json!([])
    );
    assert_eq!(
        library.json_ok(["show", "--at", "1"])?["concepts"],
        revision_one["concepts"]
    );
    let log = library.json_ok(["log"])?;
    assert_eq!(log["head_revision"], 2);
    assert_eq!(log["commits"].as_array().map(Vec::len), Some(2));
    assert_eq!(library.json_ok(["validate"])?["valid"], true);
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn applied_reconciliation_remains_inspectable_after_an_equal_reconciliation() -> TestResult {
    let library = Library::initialized()?;
    library.add_work(
        "Serializable execution",
        "serializable.txt",
        "Serializable transactions behave as some serial execution.\nPredicate locks prevent phantom inserts.\n",
    )?;
    library.submit(
        "Serializable execution",
        0,
        "initial-reconciliation.json",
        &initial_reconciliation(),
    )?;
    library.json_ok(["change", "apply"])?;
    let recorded = library.submit(
        "Serializable execution",
        1,
        "later-reconciliation.json",
        &json!({
            "summary": "Associate the work with its represented predicate-locking concept",
            "operations": [{
                "action": "add_evidence",
                "concept": {
                    "path": [
                        "Database systems",
                        "Serializable execution",
                        "Predicate locking"
                    ]
                },
                "evidence": [{"quote": "Predicate locks prevent phantom inserts."}]
            }],
            "annotations": ["This relationship was already materialized at the base revision."]
        }),
    )?;
    assert_eq!(recorded["status"], "recorded");
    assert_eq!(recorded["base_revision"], 1);
    assert_eq!(recorded["annotations"].as_array().map(Vec::len), Some(1));
    let stats = library.json_ok(["stats"])?;
    assert_eq!(stats["revision"], 1);
    assert_eq!(stats["commit_count"], 1);

    let shown = library.json_ok(["change", "show"])?;
    assert_eq!(shown["status"], "recorded");
    assert_eq!(shown["reconciliation"], recorded["reconciliation"]);
    let archived = library.json_ok(["change", "show", "--at", "1"])?;
    assert_eq!(archived["revision"], 1);
    assert_eq!(archived["status"], "applied");
    assert_eq!(archived["parent_revision"], 0);
    assert_eq!(archived["base_revision"], 0);
    assert_eq!(archived["kind"], "change");
    assert_eq!(archived["submitted_request"], initial_reconciliation());
    assert_eq!(
        archived["resolved_operations"][2]["path"],
        json!([
            "Database systems",
            "Serializable execution",
            "Predicate locking"
        ])
    );
    assert_eq!(archived["metadata"]["reconciliation_actor"], "human");

    let historical_human = Command::new(env!("CARGO_BIN_EXE_annals"))
        .arg("--library")
        .arg(&library.path)
        .args(["change", "show", "--at", "1"])
        .output()?;
    assert!(historical_human.status.success());
    assert!(historical_human.stderr.is_empty());
    let historical_human = String::from_utf8(historical_human.stdout)?;
    assert!(historical_human.contains("Applied change at revision 1"));
    assert!(historical_human.contains("Submitted operations (3)"));
    assert!(historical_human.contains("Resolved operations (3)"));
    assert!(historical_human.contains("Metadata:"));

    let diff_human = Command::new(env!("CARGO_BIN_EXE_annals"))
        .arg("--library")
        .arg(&library.path)
        .args(["diff", "0", "1"])
        .output()?;
    assert!(diff_human.status.success());
    assert!(diff_human.stderr.is_empty());
    let diff_human = String::from_utf8(diff_human.stdout)?;
    assert!(diff_human.contains("Created: “Database systems”"));
    assert!(diff_human.contains("Evidence added:"));
    assert!(!diff_human.contains("Created: - ->"));

    library.json_ok(["revert", "1"])?;
    let archived_revert = library.json_ok(["change", "show", "--at", "2"])?;
    assert_eq!(archived_revert["kind"], "revert");
    assert_eq!(archived_revert["status"], "applied");
    assert_eq!(archived_revert["submitted_request"]["revert_revision"], 1);
    assert!(
        archived_revert["resolved_operations"]
            .as_array()
            .is_some_and(|operations| !operations.is_empty())
    );
    let revert_human = Command::new(env!("CARGO_BIN_EXE_annals"))
        .arg("--library")
        .arg(&library.path)
        .args(["change", "show", "--at", "2"])
        .output()?;
    assert!(revert_human.status.success());
    let revert_human = String::from_utf8(revert_human.stdout)?;
    assert!(revert_human.contains("Applied revert at revision 2"));
    assert!(revert_human.contains("Submitted request: revert revision 1"));
    assert!(revert_human.contains("Resolved transition"));
    Ok(())
}

#[test]
fn stale_reconciliation_is_rejected_without_partial_corpus_or_history_writes() -> TestResult {
    let library = Library::initialized()?;
    let source = "Serializable transactions behave as some serial execution.\nPredicate locks prevent phantom inserts.\n";
    library.add_work("First work", "first.txt", source)?;
    library.add_work("Second work", "second.txt", "A distinct source claim.\n")?;
    library.submit(
        "First work",
        0,
        "first-reconciliation.json",
        &initial_reconciliation(),
    )?;
    library.submit(
        "Second work",
        0,
        "second-reconciliation.json",
        &json!({
            "summary": "Add a stale concept",
            "operations": [{
                "action": "create_concept",
                "label": "Stale concept",
                "evidence": [{"quote": "A distinct source claim."}]
            }]
        }),
    )?;

    let first = library.json_ok(["change", "apply", "--work", "First work"])?;
    assert_eq!(first["revision"], 1);
    let before = library.json_ok(["stats"])?;
    let before_corpus = library.json_ok(["show"])?;
    let before_log = library.json_ok(["log"])?;

    let error = library.json_error(["change", "apply", "--work", "Second work"], "stale_change")?;
    assert!(error["error"]["message"].as_str().is_some_and(|message| {
        message.contains("revision 0") && message.contains("revision 1")
    }));

    let after = library.json_ok(["stats"])?;
    assert_eq!(after["revision"], before["revision"]);
    assert_eq!(after["concept_count"], before["concept_count"]);
    assert_eq!(after["evidence_count"], before["evidence_count"]);
    assert_eq!(after["commit_count"], before["commit_count"]);
    assert_eq!(library.json_ok(["show"])?, before_corpus);
    assert_eq!(library.json_ok(["log"])?, before_log);
    assert_eq!(library.json_ok(["validate"])?["valid"], true);
    Ok(())
}

#[test]
fn public_help_and_request_contract_exclude_storage_selectors() -> TestResult {
    let help = Command::new(env!("CARGO_BIN_EXE_annals"))
        .arg("--help")
        .output()?;
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout)?;
    for forbidden in [
        "NODE_ID",
        "ROOT_NODE_ID",
        "--position",
        "node add",
        "node edit",
        "node move",
        "node delete",
        "tree create",
        "tree delete",
        "ingest",
    ] {
        assert!(
            !help.contains(forbidden),
            "help exposed {forbidden:?}\n{help}"
        );
    }
    for required in [
        "work",
        "integrate",
        "change",
        "show",
        "log",
        "diff",
        "revert",
    ] {
        assert!(help.contains(required), "help omitted {required:?}\n{help}");
    }

    let library = Library::initialized()?;
    library.add_work(
        "Language source",
        "language.txt",
        "Exact source language.\n",
    )?;
    let invalid = library.file(
        "opaque.json",
        r#"{
            "summary": "Try an opaque selector",
            "operations": [{
                "action": "add_evidence",
                "concept": {"node_id": 41},
                "evidence": [{"quote": "Exact source language."}]
            }]
        }"#,
    )?;
    library.json_error(
        [
            OsStr::new("change"),
            OsStr::new("submit"),
            invalid.as_os_str(),
            OsStr::new("--work"),
            OsStr::new("Language source"),
            OsStr::new("--base"),
            OsStr::new("0"),
        ],
        "invalid_reconciliation",
    )?;
    assert_eq!(
        library.json_ok(["stats"])?["pending_reconciliation_count"],
        0
    );
    Ok(())
}
