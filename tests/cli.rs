use std::error::Error;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

    fn json_error<I, S>(&self, arguments: I, exit: i32, code: &str) -> TestResult
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.output(arguments)?;
        assert_eq!(output.status.code(), Some(exit));
        assert!(output.stdout.is_empty());
        let envelope = serde_json::from_slice::<Value>(&output.stderr)?;
        assert_eq!(envelope["ok"], false);
        assert_eq!(envelope["error"]["code"], code);
        Ok(())
    }

    fn create_tree(&self, text: &str) -> TestResult<i64> {
        mutation_id(&self.json_ok(["tree", "create", "--text", text])?)
    }

    fn add_node(&self, parent: i64, text: &str) -> TestResult<i64> {
        mutation_id(&self.json_ok([
            "node",
            "add",
            "--parent",
            &parent.to_string(),
            "--text",
            text,
        ])?)
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
    Ok(envelope["data"].clone())
}

fn mutation_id(data: &Value) -> TestResult<i64> {
    data["node_ids"]
        .as_array()
        .and_then(|ids| ids.first())
        .and_then(Value::as_i64)
        .ok_or_else(|| io::Error::other("mutation omitted its node ID").into())
}

#[test]
fn lifecycle_and_homogeneous_crud() -> TestResult {
    let library = Library::initialized()?;
    let root = library.create_tree("Database concurrency")?;
    let branch = library.add_node(root, "Write skew")?;
    let leaf = library.add_node(branch, "Serializable snapshot isolation")?;

    let shown = library.json_ok(["tree", "show", &root.to_string()])?;
    assert_eq!(shown.as_array().map(Vec::len), Some(3));
    assert_eq!(shown[2]["node"]["text"], "Serializable snapshot isolation");

    library.json_ok([
        "node",
        "edit",
        &leaf.to_string(),
        "--text",
        "Snapshot isolation anomaly",
    ])?;
    let node = library.json_ok(["node", "show", &leaf.to_string()])?;
    assert_eq!(node["text"], "Snapshot isolation anomaly");

    let stats = library.json_ok(["stats"])?;
    assert_eq!(stats["revision"], 4);
    assert_eq!(stats["node_count"], 3);
    assert_eq!(stats["index_current"], true);
    assert_eq!(library.json_ok(["validate"])?["valid"], true);
    Ok(())
}

#[test]
fn search_supports_exact_text_path_scope_and_prefix() -> TestResult {
    let library = Library::initialized()?;
    let root = library.create_tree("Databases")?;
    let transactions = library.add_node(root, "Transactions")?;
    let skew = library.add_node(transactions, "Write skew anomaly")?;
    let other = library.create_tree("Languages")?;
    library.add_node(other, "Write syntax")?;

    let exact = library.json_ok(["search", "Write skew anomaly"])?;
    assert_eq!(exact["results"][0]["node_id"], skew);
    assert_eq!(exact["results"][0]["match_reasons"][0], "exact_text");

    let scoped = library.json_ok(["search", "write", "--within", &root.to_string()])?;
    assert!(scoped["results"].as_array().is_some_and(|results| {
        results
            .iter()
            .all(|result| result["breadcrumb"][0]["node_id"] == root)
    }));
    let prefix = library.json_ok(["search", "anom"])?;
    assert_eq!(prefix["results"][0]["node_id"], skew);
    Ok(())
}

#[test]
fn tree_invariants_and_confirmation_are_enforced() -> TestResult {
    let library = Library::initialized()?;
    let root = library.create_tree("Root")?;
    let branch = library.add_node(root, "Branch")?;
    let leaf = library.add_node(branch, "Leaf")?;
    let other = library.create_tree("Other")?;

    library.json_error(
        [
            "node",
            "move",
            &branch.to_string(),
            "--parent",
            &leaf.to_string(),
        ],
        4,
        "would_create_cycle",
    )?;
    library.json_error(
        [
            "node",
            "move",
            &branch.to_string(),
            "--parent",
            &other.to_string(),
        ],
        4,
        "cross_tree_move_not_supported",
    )?;
    library.json_error(
        ["node", "delete", &branch.to_string()],
        4,
        "recursive_delete_required",
    )?;
    library.json_error(
        ["node", "delete", &branch.to_string(), "--recursive"],
        4,
        "confirmation_required",
    )?;
    library.json_ok([
        "node",
        "delete",
        &branch.to_string(),
        "--recursive",
        "--yes",
    ])?;
    Ok(())
}

#[test]
fn human_output_escapes_terminal_control_characters() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("annals.db");
    let mut plain = Command::new(env!("CARGO_BIN_EXE_annals"));
    plain.arg("--library").arg(&path);
    assert!(plain.arg("init").output()?.status.success());
    let output = Command::new(env!("CARGO_BIN_EXE_annals"))
        .arg("--library")
        .arg(&path)
        .args(["tree", "create", "--text", "safe\u{1b}[31m"])
        .output()?;
    assert!(output.status.success());
    let list = Command::new(env!("CARGO_BIN_EXE_annals"))
        .arg("--library")
        .arg(&path)
        .args(["tree", "list"])
        .output()?;
    let stdout = String::from_utf8(list.stdout)?;
    assert!(!stdout.contains('\u{1b}'));
    assert!(stdout.contains("\\u{1b}"));
    Ok(())
}
