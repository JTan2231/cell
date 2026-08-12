use std::error::Error;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

struct Library {
    directory: TempDir,
    path: PathBuf,
}

impl Library {
    fn new() -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        Ok(Self { directory, path })
    }

    fn initialized() -> TestResult<Self> {
        let library = Self::new()?;
        library.json_ok(["init"])?;
        Ok(library)
    }

    fn command(&self) -> Command {
        command_for(&self.path)
    }

    fn output<I, S>(&self, arguments: I) -> io::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command().args(arguments).output()
    }

    fn json_output<I, S>(&self, arguments: I) -> io::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.command();
        command.arg("--json").args(arguments).output()
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
        let output = self.json_output(arguments)?;
        error_json(&output, exit_code, code)
    }

    fn create_tree(&self, title: &str, body: &str) -> TestResult<i64> {
        let data = self.json_ok(["tree", "create", "--title", title, "--body", body])?;
        mutation_id(&data)
    }

    fn add_node(&self, parent: i64, kind: &str, title: &str, body: &str) -> TestResult<i64> {
        let parent = parent.to_string();
        let data = self.json_ok([
            "node", "add", "--parent", &parent, "--kind", kind, "--title", title, "--body", body,
        ])?;
        mutation_id(&data)
    }

    fn show_node(&self, node_id: i64) -> TestResult<Value> {
        let node_id = node_id.to_string();
        self.json_ok(["node", "show", &node_id])
    }

    fn search(&self, query: &str) -> TestResult<Value> {
        self.json_ok(["search", query])
    }

    fn revision(&self) -> TestResult<i64> {
        self.json_ok(["stats"])?["revision"]
            .as_i64()
            .ok_or_else(|| io::Error::other("stats omitted the library revision").into())
    }

    fn json_with_stdin<I, S>(&self, arguments: I, input: &[u8]) -> TestResult<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.command();
        let mut child = command
            .arg("--json")
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("child standard input was not piped"))?;
        stdin.write_all(input)?;
        drop(stdin);
        Ok(child.wait_with_output()?)
    }
}

fn command_for(path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_annals"));
    command.arg("--library").arg(path);
    command
}

fn successful_json(output: &Output) -> TestResult<Value> {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "successful JSON wrote to stderr");
    let envelope = serde_json::from_slice::<Value>(&output.stdout)?;
    assert_eq!(envelope["ok"], true);
    Ok(envelope["data"].clone())
}

fn error_json(output: &Output, exit_code: i32, code: &str) -> TestResult<Value> {
    assert_eq!(
        output.status.code(),
        Some(exit_code),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "JSON error wrote to stdout");
    let envelope = serde_json::from_slice::<Value>(&output.stderr)?;
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], code);
    Ok(envelope)
}

fn mutation_id(data: &Value) -> TestResult<i64> {
    data["node_ids"]
        .as_array()
        .and_then(|ids| ids.first())
        .and_then(Value::as_i64)
        .ok_or_else(|| io::Error::other("mutation response omitted its node ID").into())
}

fn result_contains(data: &Value, node_id: i64) -> bool {
    data["results"].as_array().is_some_and(|results| {
        results
            .iter()
            .any(|result| result["node_id"].as_i64() == Some(node_id))
    })
}

fn results_are_kind(data: &Value, kind: &str) -> bool {
    data["results"].as_array().is_some_and(|results| {
        !results.is_empty() && results.iter().all(|result| result["kind"] == kind)
    })
}

#[test]
fn lifecycle_json_streams_and_exit_codes() -> TestResult {
    let library = Library::new()?;
    let initialized = library.json_ok(["init"])?;
    assert_eq!(initialized["library"], library.path.display().to_string());

    let stats = library.json_ok(["stats"])?;
    assert_eq!(stats["revision"], 0);
    assert_eq!(stats["node_count"], 0);
    assert_eq!(stats["index_current"], true);

    library.json_error(["init"], 4, "library_exists")?;
    library.json_error(["tree", "create"], 2, "invalid_command")?;
    library.json_error(
        [
            "tree",
            "create",
            "--title",
            "Unreadable",
            "--body-file",
            "missing.txt",
        ],
        1,
        "body_read_failed",
    )?;

    let missing = library.directory.path().join("missing.db");
    let output = command_for(&missing).arg("--json").arg("stats").output()?;
    error_json(&output, 3, "library_not_found")?;

    let mut quiet = library.command();
    let output = quiet
        .args(["--quiet", "tree", "create", "--title", "Quiet root"])
        .output()?;
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn canonical_mutations_increment_the_revision_once() -> TestResult {
    let library = Library::initialized()?;
    assert_eq!(library.revision()?, 0);

    let root = library.create_tree("Root", "root")?;
    assert_eq!(library.revision()?, 1);
    let branch = library.add_node(root, "topic", "Branch", "branch")?;
    assert_eq!(library.revision()?, 2);
    let container = library.add_node(root, "topic", "Container", "container")?;
    assert_eq!(library.revision()?, 3);

    let branch_text = branch.to_string();
    library.json_ok(["node", "edit", &branch_text, "--body", "updated"])?;
    assert_eq!(library.revision()?, 4);
    let container_text = container.to_string();
    library.json_ok(["node", "move", &branch_text, "--parent", &container_text])?;
    assert_eq!(library.revision()?, 5);
    library.json_ok(["node", "delete", &branch_text])?;
    assert_eq!(library.revision()?, 6);

    let root_text = root.to_string();
    library.json_ok(["tree", "delete", &root_text, "--yes"])?;
    assert_eq!(library.revision()?, 7);
    library.json_ok(["reindex"])?;
    assert_eq!(library.revision()?, 7);
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn tree_and_node_crud_enforce_invariants_without_partial_writes() -> TestResult {
    let library = Library::initialized()?;
    let root = library.create_tree("Root", "root overview")?;
    let branch = library.add_node(root, "topic", "Branch", "branch notes")?;
    let nested = library.add_node(branch, "topic", "Nested", "nested notes")?;
    let source = library.add_node(nested, "source", "Source", "source body")?;
    let other_root = library.create_tree("Other", "other overview")?;

    let source_text = source.to_string();
    library.json_error(
        [
            "node",
            "add",
            "--parent",
            &source_text,
            "--kind",
            "topic",
            "--title",
            "Impossible",
        ],
        4,
        "source_cannot_have_children",
    )?;
    let root_text = root.to_string();
    library.json_error(
        [
            "node",
            "add",
            "--parent",
            &root_text,
            "--kind",
            "topic",
            "--title",
            "Bad metadata",
            "--locator",
            "somewhere",
        ],
        2,
        "source_metadata_for_topic",
    )?;

    let branch_text = branch.to_string();
    let nested_text = nested.to_string();
    library.json_error(
        ["node", "move", &branch_text, "--parent", &nested_text],
        4,
        "would_create_cycle",
    )?;
    library.json_error(
        ["node", "edit", &branch_text, "--kind", "source"],
        4,
        "source_cannot_have_children",
    )?;
    library.json_error(["node", "delete", &root_text], 4, "root_delete_not_allowed")?;
    let other_root_text = other_root.to_string();
    library.json_error(
        ["node", "move", &branch_text, "--parent", &other_root_text],
        4,
        "cross_tree_move_not_supported",
    )?;

    let branch_after = library.show_node(branch)?;
    assert_eq!(branch_after["parent_id"], root);
    assert_eq!(branch_after["kind"], "topic");
    let children = library.json_ok(["node", "children", &root_text])?;
    assert_eq!(children.as_array().map(Vec::len), Some(1));
    let shown = library.json_ok(["tree", "show", &root_text, "--depth", "1"])?;
    assert_eq!(shown.as_array().map(Vec::len), Some(2));
    let trees = library.json_ok(["tree", "list"])?;
    assert_eq!(trees.as_array().map(Vec::len), Some(2));
    Ok(())
}

#[test]
fn stdin_utf8_and_noninteractive_recursive_deletion() -> TestResult {
    let library = Library::initialized()?;
    let root = library.create_tree("Input", "input root")?;
    let root_text = root.to_string();
    let input = "Unicode café 日本語 🦀\nsecond line";
    let output = library.json_with_stdin(
        [
            "node",
            "add",
            "--parent",
            &root_text,
            "--kind",
            "topic",
            "--title",
            "Piped",
            "--body-file",
            "-",
        ],
        input.as_bytes(),
    )?;
    let branch = mutation_id(&successful_json(&output)?)?;
    assert_eq!(library.show_node(branch)?["body"], input);

    let branch_text = branch.to_string();
    let leaf = library.add_node(branch, "source", "Leaf", "leaf body")?;
    library.json_error(
        ["node", "delete", &branch_text],
        4,
        "recursive_delete_required",
    )?;
    library.json_error(
        ["node", "delete", &branch_text, "--recursive"],
        4,
        "confirmation_required",
    )?;
    assert_eq!(library.show_node(leaf)?["parent_id"], branch);
    library.json_ok(["node", "delete", &branch_text, "--recursive", "--yes"])?;
    library.json_error(["node", "show", &branch_text], 3, "node_not_found")?;

    library.json_error(["tree", "delete", &root_text], 4, "confirmation_required")?;
    library.json_ok(["tree", "delete", &root_text, "--yes"])?;
    assert_eq!(
        library.json_ok(["tree", "list"])?.as_array().map(Vec::len),
        Some(0)
    );
    Ok(())
}

#[test]
fn human_output_escapes_user_controls_without_changing_json() -> TestResult {
    let library = Library::initialized()?;
    let title = "Unsafe\u{1b}[31m\nTitle";
    let body = "first line\nsecond\u{1b}[2J line";
    let root = library.create_tree(title, body)?;
    let root_text = root.to_string();
    let locator = "paper\t\u{1b}[5m";
    let source_data = library.json_ok([
        "node",
        "add",
        "--parent",
        &root_text,
        "--kind",
        "source",
        "--title",
        "Evidence",
        "--locator",
        locator,
    ])?;
    let source = mutation_id(&source_data)?;

    let root_json = library.show_node(root)?;
    assert_eq!(root_json["title"], title);
    assert_eq!(root_json["body"], body);
    let source_json = library.show_node(source)?;
    assert_eq!(source_json["source"]["locator"], locator);

    let source_text = source.to_string();
    for arguments in [
        vec!["tree", "list"],
        vec!["tree", "show", &root_text],
        vec!["node", "show", &root_text],
        vec!["node", "show", &source_text],
        vec!["search", "Unsafe"],
    ] {
        let output = library.output(arguments)?;
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout)?;
        assert!(!stdout.contains('\u{1b}'));
    }
    Ok(())
}

#[test]
fn search_supports_exact_phrase_or_prefix_scope_kind_and_no_match() -> TestResult {
    let library = Library::initialized()?;
    let root = library.create_tree("Knowledge", "architecture overview")?;
    let transactions =
        library.add_node(root, "topic", "Transactions", "atomic isolation durability")?;
    let paper = library.add_node(
        transactions,
        "source",
        "Write Skew Paper",
        "Serializable snapshot isolation permits the write skew anomaly.",
    )?;
    let distributed = library.add_node(
        root,
        "topic",
        "Distributed Systems",
        "consensus and replication",
    )?;
    let quorum = library.add_node(
        distributed,
        "source",
        "Quorum Notes",
        "Network partitions require a consensus quorum.",
    )?;
    let abi = library.add_node(
        distributed,
        "source",
        "C++ ABI",
        "The C++ application binary interface defines name mangling and object layout.",
    )?;
    assert_eq!(abi, 6, "the relevance fixture depends on stable seed IDs");

    let exact = library.search("Transactions")?;
    assert!(result_contains(&exact, transactions));
    assert!(exact.get("explanation").is_none());
    assert!(exact["results"].as_array().is_some_and(|results| {
        results
            .iter()
            .all(|result| result.get("explanation").is_none())
    }));
    let by_id = library.search(&paper.to_string())?;
    assert!(result_contains(&by_id, paper));
    let phrase = library.search("\"write skew\"")?;
    assert!(result_contains(&phrase, paper));
    let fallback = library.search("serializable absentterm")?;
    assert!(result_contains(&fallback, paper));
    let explained = library.json_ok(["search", "serializable absentterm", "--explain"])?;
    assert_eq!(explained["explanation"]["or_fallback_used"], true);
    assert!(explained["results"].as_array().is_some_and(|results| {
        results.iter().all(|result| {
            result["explanation"]["chain_group_node_id"].is_i64()
                && result["explanation"]["branch_key"].is_i64()
                && result["explanation"]["final_position"].is_u64()
        })
    }));
    let human_explain = library.output(["search", "serializable absentterm", "--explain"])?;
    assert!(human_explain.status.success());
    let human_explain = String::from_utf8(human_explain.stdout)?;
    assert!(human_explain.contains("search explain:"));
    assert!(human_explain.contains("grouping="));
    let prefix = library.search("Transact")?;
    assert!(result_contains(&prefix, transactions));
    let punctuation = library.search("C++ ABI")?;
    assert_eq!(punctuation["results"][0]["node_id"], abi);
    assert!(
        punctuation["results"][0]["match_reasons"]
            .as_array()
            .is_some_and(|reasons| reasons.iter().any(|reason| reason == "exact_title"))
    );

    let transactions_text = transactions.to_string();
    let scoped_empty = library.json_ok(["search", "consensus", "--within", &transactions_text])?;
    assert_eq!(scoped_empty["results"].as_array().map(Vec::len), Some(0));
    let distributed_text = distributed.to_string();
    let scoped = library.json_ok(["search", "consensus", "--within", &distributed_text])?;
    assert!(result_contains(&scoped, quorum) || result_contains(&scoped, distributed));
    let sources = library.json_ok(["search", "isolation", "--kind", "source"])?;
    assert!(results_are_kind(&sources, "source"));

    let no_match = library.search("wordthatdoesnotexistanywhere")?;
    assert_eq!(no_match["results"].as_array().map(Vec::len), Some(0));
    let human = library.output(["search", "wordthatdoesnotexistanywhere"])?;
    assert!(human.status.success());
    assert_eq!(String::from_utf8(human.stdout)?.trim(), "No matches");
    Ok(())
}

#[test]
fn edits_moves_and_reindex_are_immediately_visible() -> TestResult {
    let library = Library::initialized()?;
    let root = library.create_tree("Root", "overview")?;
    let left = library.add_node(root, "topic", "Left", "left branch")?;
    let right = library.add_node(root, "topic", "Right", "right branch")?;
    let source = library.add_node(left, "source", "Movable", "legacyterm")?;

    let source_text = source.to_string();
    library.json_ok(["node", "edit", &source_text, "--body", "revisedterm"])?;
    assert!(!result_contains(&library.search("legacyterm")?, source));
    assert!(result_contains(&library.search("revisedterm")?, source));

    let right_text = right.to_string();
    library.json_ok(["node", "move", &source_text, "--parent", &right_text])?;
    let left_text = left.to_string();
    let old_scope = library.json_ok(["search", "revisedterm", "--within", &left_text])?;
    let new_scope = library.json_ok(["search", "revisedterm", "--within", &right_text])?;
    assert!(!result_contains(&old_scope, source));
    assert!(result_contains(&new_scope, source));

    let root_text = root.to_string();
    library.json_ok(["node", "edit", &root_text, "--title", "Renamed Root"])?;
    let renamed = library.search("revisedterm")?;
    let result = renamed["results"]
        .as_array()
        .and_then(|results| results.iter().find(|item| item["node_id"] == source))
        .ok_or_else(|| io::Error::other("edited source disappeared from search"))?;
    assert_eq!(result["breadcrumb"][0]["title"], "Renamed Root");

    let connection = rusqlite::Connection::open(&library.path)?;
    connection.execute(
        "UPDATE index_metadata SET value = '999' WHERE key = 'indexer_version'",
        [],
    )?;
    drop(connection);
    library.json_error(["search", "revisedterm"], 5, "reindex_required")?;
    let rebuilt = library.json_ok(["reindex"])?;
    assert_eq!(rebuilt["indexed_nodes"], 4);
    assert!(result_contains(&library.search("revisedterm")?, source));
    Ok(())
}

#[test]
fn backup_stats_and_validate_report_consistent_state() -> TestResult {
    let library = Library::initialized()?;
    let root = library.create_tree("Complete", "complete root")?;
    library.add_node(root, "source", "Evidence", "evidence text")?;

    let stats = library.json_ok(["stats"])?;
    assert_eq!(stats["root_count"], 1);
    assert_eq!(stats["node_count"], 2);
    assert_eq!(stats["source_count"], 1);
    assert_eq!(stats["indexed_unit_count"], 2);
    assert_eq!(stats["index_current"], true);
    let report = library.json_ok(["validate"])?;
    assert_eq!(report["valid"], true);
    assert_eq!(report["issues"].as_array().map(Vec::len), Some(0));

    let backup_path = library.directory.path().join("backup.db");
    let backup = backup_path.as_os_str();
    let copied = library.json_ok([OsStr::new("backup"), backup])?;
    assert_eq!(copied["output"], backup_path.display().to_string());
    assert!(backup_path.exists());
    library.json_error([OsStr::new("backup"), backup], 4, "backup_exists")?;

    let output = command_for(&backup_path)
        .args(["--json", "stats"])
        .output()?;
    let backup_stats = successful_json(&output)?;
    assert_eq!(backup_stats["node_count"], 2);
    assert_eq!(backup_stats["source_count"], 1);
    Ok(())
}
