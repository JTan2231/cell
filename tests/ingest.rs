use std::error::Error;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use rusqlite::Connection;
use serde_json::{Value, json};
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

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_annals"));
        command.arg("--library").arg(&self.path);
        command
    }

    fn json_ok<I, S>(&self, arguments: I) -> TestResult<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.command().arg("--json").args(arguments).output()?;
        successful_json(&output)
    }

    fn create_tree(&self, title: &str, body: &str) -> TestResult<i64> {
        let data = self.json_ok(["tree", "create", "--title", title, "--body", body])?;
        required_i64(&data["node_ids"][0], "tree create node ID")
    }

    fn ingest_output(&self, document: &Value) -> TestResult<Output> {
        let input = serde_json::to_vec(document)?;
        let mut child = self
            .command()
            .args(["--json", "ingest", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("child standard input was not piped"))?;
        stdin.write_all(&input)?;
        drop(stdin);
        Ok(child.wait_with_output()?)
    }

    fn ingest_ok(&self, document: &Value) -> TestResult<Value> {
        successful_json(&self.ingest_output(document)?)
    }

    fn ingest_error(&self, document: &Value, exit_code: i32, code: &str) -> TestResult<Value> {
        error_json(&self.ingest_output(document)?, exit_code, code)
    }
}

fn successful_json(output: &Output) -> TestResult<Value> {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "successful JSON wrote to stderr");
    let envelope = serde_json::from_slice::<Value>(&output.stdout)?;
    assert_eq!(envelope.as_object().map(serde_json::Map::len), Some(2));
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
    assert_eq!(envelope.as_object().map(serde_json::Map::len), Some(2));
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], code);
    Ok(envelope)
}

fn required_i64(value: &Value, description: &str) -> TestResult<i64> {
    value
        .as_i64()
        .ok_or_else(|| io::Error::other(format!("response omitted {description}")).into())
}

fn topic(title: &str, body: &str) -> Value {
    json!({
        "kind": "topic",
        "title": title,
        "body": body,
        "source": null
    })
}

fn source(title: &str, body: &str) -> Value {
    json!({
        "kind": "source",
        "title": title,
        "body": body,
        "source": {
            "locator": null,
            "media_type": null,
            "checksum": null,
            "captured_at": null
        }
    })
}

fn empty_plan(root: i64, revision: i64) -> Value {
    json!({
        "tree_root_id": root,
        "base_revision": revision,
        "create_nodes": [],
        "replace_nodes": [],
        "delete_subtrees": [],
        "child_orders": []
    })
}

#[derive(Debug)]
struct Seed {
    root: i64,
    alpha: i64,
    beta: i64,
    beta_child: i64,
    doomed: i64,
    doomed_child: i64,
    untouched: i64,
    untouched_first: i64,
    untouched_second: i64,
    movable: i64,
    keeper: i64,
    revision: i64,
}

fn seeded_library() -> TestResult<(Library, Seed)> {
    let library = Library::initialized()?;
    let root = library.create_tree("Root", "root overview")?;
    let setup = json!({
        "tree_root_id": root,
        "base_revision": 1,
        "create_nodes": [
            {"ref": "alpha", "node": topic("Alpha", "alpha branch")},
            {"ref": "beta", "node": topic("Beta", "beta branch")},
            {"ref": "beta_child", "node": source("Beta source", "beta source")},
            {"ref": "doomed", "node": topic("Doomed", "delete this branch")},
            {"ref": "doomed_child", "node": source("Doomed source", "doomedneedle")},
            {"ref": "untouched", "node": topic("Untouched", "untouched branch")},
            {"ref": "untouched_first", "node": source("First", "first source")},
            {"ref": "untouched_second", "node": source("Second", "second source")},
            {"ref": "movable", "node": source("Movable", "legacyneedle")},
            {"ref": "keeper", "node": source("Keeper", "keeper source")}
        ],
        "replace_nodes": [],
        "delete_subtrees": [],
        "child_orders": [
            {"parent": root, "children": ["alpha", "beta", "doomed", "untouched"]},
            {"parent": "alpha", "children": ["movable", "keeper"]},
            {"parent": "beta", "children": ["beta_child"]},
            {"parent": "doomed", "children": ["doomed_child"]},
            {"parent": "untouched", "children": ["untouched_first", "untouched_second"]}
        ]
    });
    let output = library.ingest_ok(&setup)?;
    assert_eq!(output["previous_revision"], 1);
    assert_eq!(output["new_revision"], 2);
    let created = &output["created"];
    let seed = Seed {
        root,
        alpha: required_i64(&created["alpha"], "alpha ID")?,
        beta: required_i64(&created["beta"], "beta ID")?,
        beta_child: required_i64(&created["beta_child"], "beta child ID")?,
        doomed: required_i64(&created["doomed"], "doomed ID")?,
        doomed_child: required_i64(&created["doomed_child"], "doomed child ID")?,
        untouched: required_i64(&created["untouched"], "untouched ID")?,
        untouched_first: required_i64(&created["untouched_first"], "first untouched ID")?,
        untouched_second: required_i64(&created["untouched_second"], "second untouched ID")?,
        movable: required_i64(&created["movable"], "movable ID")?,
        keeper: required_i64(&created["keeper"], "keeper ID")?,
        revision: 2,
    };

    let connection = Connection::open(&library.path)?;
    connection.execute(
        "UPDATE nodes SET position = 17 WHERE id = ?1",
        [seed.untouched_first],
    )?;
    connection.execute(
        "UPDATE nodes SET position = 8192 WHERE id = ?1",
        [seed.untouched_second],
    )?;
    drop(connection);
    Ok((library, seed))
}

fn child_positions(path: &Path, parent: i64) -> TestResult<Vec<(i64, i64)>> {
    let connection = Connection::open(path)?;
    let mut statement = connection
        .prepare("SELECT id, position FROM nodes WHERE parent_id = ?1 ORDER BY position, id")?;
    let rows = statement.query_map([parent], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn tree_ids(data: &Value) -> TestResult<Vec<i64>> {
    data.as_array()
        .ok_or_else(|| io::Error::other("tree output was not an array").into())
        .and_then(|entries| {
            entries
                .iter()
                .map(|entry| required_i64(&entry["node"]["id"], "tree node ID"))
                .collect()
        })
}

fn result_contains(data: &Value, node_id: i64) -> bool {
    data["results"].as_array().is_some_and(|results| {
        results
            .iter()
            .any(|result| result["node_id"].as_i64() == Some(node_id))
    })
}

#[derive(Debug, PartialEq, Eq)]
struct NodeState {
    id: i64,
    parent_id: Option<i64>,
    kind: String,
    title: String,
    body: String,
    position: i64,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, PartialEq, Eq)]
struct SourceState {
    node_id: i64,
    locator: Option<String>,
    media_type: Option<String>,
    checksum: Option<String>,
    captured_at: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct SearchState {
    id: i64,
    node_id: i64,
    unit_no: i64,
    title: String,
    breadcrumb: String,
    text: String,
    content_hash: String,
}

#[derive(Debug, PartialEq, Eq)]
struct DatabaseState {
    revision: i64,
    nodes: Vec<NodeState>,
    sources: Vec<SourceState>,
    search_units: Vec<SearchState>,
}

fn database_state(path: &Path) -> TestResult<DatabaseState> {
    let connection = Connection::open(path)?;
    let revision = connection.query_row(
        "SELECT revision FROM library_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    let nodes = {
        let mut statement = connection.prepare(
            "SELECT id, parent_id, kind, title, body, position, created_at, updated_at \
             FROM nodes ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(NodeState {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                kind: row.get(2)?,
                title: row.get(3)?,
                body: row.get(4)?,
                position: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let sources = {
        let mut statement = connection.prepare(
            "SELECT node_id, locator, media_type, checksum, captured_at FROM sources \
             ORDER BY node_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(SourceState {
                node_id: row.get(0)?,
                locator: row.get(1)?,
                media_type: row.get(2)?,
                checksum: row.get(3)?,
                captured_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let search_units = {
        let mut statement = connection.prepare(
            "SELECT id, node_id, unit_no, title, breadcrumb, text, content_hash \
             FROM search_units ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(SearchState {
                id: row.get(0)?,
                node_id: row.get(1)?,
                unit_no: row.get(2)?,
                title: row.get(3)?,
                breadcrumb: row.get(4)?,
                text: row.get(5)?,
                content_hash: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    Ok(DatabaseState {
        revision,
        nodes,
        sources,
        search_units,
    })
}

#[test]
fn newly_created_siblings_can_be_declared_in_any_order() -> TestResult {
    let library = Library::initialized()?;
    let root = library.create_tree("Root", "root overview")?;
    let plan = json!({
        "tree_root_id": root,
        "base_revision": 1,
        "create_nodes": [
            {"ref": "first", "node": topic("First", "first")},
            {"ref": "second", "node": topic("Second", "second")},
            {"ref": "third", "node": topic("Third", "third")}
        ],
        "replace_nodes": [],
        "delete_subtrees": [],
        "child_orders": [
            {"parent": root, "children": ["third", "second", "first"]}
        ]
    });

    let receipt = library.ingest_ok(&plan)?;
    let created = &receipt["created"];
    assert_eq!(
        child_positions(&library.path, root)?,
        [
            (required_i64(&created["third"], "third ID")?, 0),
            (required_i64(&created["second"], "second ID")?, 1024),
            (required_i64(&created["first"], "first ID")?, 2048),
        ]
    );
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn ingestion_applies_only_the_declared_tree_and_returns_an_exact_receipt() -> TestResult {
    let (library, seed) = seeded_library()?;
    let untouched_before = child_positions(&library.path, seed.untouched)?;
    assert_eq!(
        untouched_before,
        [(seed.untouched_first, 17), (seed.untouched_second, 8192)]
    );

    let plan = json!({
        "tree_root_id": seed.root,
        "base_revision": seed.revision,
        "create_nodes": [
            {"ref": "auth", "node": topic("Authentication", "authentication overview")}
        ],
        "replace_nodes": [
            {
                "id": seed.movable,
                "node": {
                    "kind": "source",
                    "title": "OAuth specification",
                    "body": "replacementneedle",
                    "source": {
                        "locator": "https://example.test/oauth",
                        "media_type": "text/html",
                        "checksum": null,
                        "captured_at": null
                    }
                }
            }
        ],
        "delete_subtrees": [
            {
                "root_id": seed.doomed,
                "expected_node_ids": [seed.doomed, seed.doomed_child]
            }
        ],
        "child_orders": [
            {
                "parent": seed.root,
                "children": [seed.beta, "auth", seed.alpha, seed.untouched]
            },
            {"parent": seed.alpha, "children": [seed.keeper]},
            {"parent": "auth", "children": [seed.movable]}
        ]
    });
    let receipt = library.ingest_ok(&plan)?;
    let auth = required_i64(&receipt["created"]["auth"], "created auth ID")?;

    assert_eq!(receipt["previous_revision"], seed.revision);
    assert_eq!(receipt["new_revision"], seed.revision + 1);
    assert_eq!(receipt["created"], json!({"auth": auth}));
    assert_eq!(receipt["replaced_node_ids"], json!([seed.movable]));
    assert_eq!(
        receipt["moved"],
        json!([{
            "node_id": seed.movable,
            "from_parent": seed.alpha,
            "to_parent": auth
        }])
    );
    assert_eq!(
        receipt["deleted_node_ids"],
        json!([seed.doomed, seed.doomed_child])
    );
    assert_eq!(
        receipt["final_child_orders"],
        json!([
            {
                "parent": seed.root,
                "children": [seed.beta, auth, seed.alpha, seed.untouched]
            },
            {"parent": seed.alpha, "children": [seed.keeper]},
            {"parent": auth, "children": [seed.movable]}
        ])
    );

    let root = seed.root.to_string();
    let shown = library.json_ok(["tree", "show", &root])?;
    assert_eq!(
        tree_ids(&shown)?,
        [
            seed.root,
            seed.beta,
            seed.beta_child,
            auth,
            seed.movable,
            seed.alpha,
            seed.keeper,
            seed.untouched,
            seed.untouched_first,
            seed.untouched_second
        ]
    );
    assert_eq!(
        child_positions(&library.path, seed.untouched)?,
        untouched_before,
        "an unmentioned parent's stored order changed"
    );

    let movable = seed.movable.to_string();
    let replaced = library.json_ok(["node", "show", &movable])?;
    assert_eq!(replaced["parent_id"], auth);
    assert_eq!(replaced["kind"], "source");
    assert_eq!(replaced["title"], "OAuth specification");
    assert_eq!(replaced["body"], "replacementneedle");
    assert_eq!(
        replaced["source"],
        json!({
            "node_id": seed.movable,
            "locator": "https://example.test/oauth",
            "media_type": "text/html",
            "checksum": null,
            "captured_at": null
        })
    );

    let new_search = library.json_ok(["search", "replacementneedle"])?;
    assert!(result_contains(&new_search, seed.movable));
    let matching_result = new_search["results"]
        .as_array()
        .and_then(|results| {
            results
                .iter()
                .find(|result| result["node_id"].as_i64() == Some(seed.movable))
        })
        .ok_or_else(|| io::Error::other("replacement search omitted the replaced node"))?;
    let breadcrumb_ids = matching_result["breadcrumb"]
        .as_array()
        .ok_or_else(|| io::Error::other("search result omitted its breadcrumb"))?
        .iter()
        .map(|item| required_i64(&item["node_id"], "breadcrumb node ID"))
        .collect::<TestResult<Vec<_>>>()?;
    assert_eq!(breadcrumb_ids, [seed.root, auth, seed.movable]);
    assert!(!result_contains(
        &library.json_ok(["search", "legacyneedle"])?,
        seed.movable
    ));

    let stats = library.json_ok(["stats"])?;
    assert_eq!(stats["revision"], seed.revision + 1);
    assert_eq!(stats["node_count"], 10);
    assert_eq!(library.json_ok(["validate"])?["valid"], true);
    Ok(())
}

#[test]
fn rejected_ingestions_leave_canonical_and_derived_state_unchanged() -> TestResult {
    let (library, seed) = seeded_library()?;
    let before = database_state(&library.path)?;

    let mut stale = empty_plan(seed.root, seed.revision - 1);
    stale["replace_nodes"] = json!([{
        "id": seed.alpha,
        "node": topic("Changed alpha", "this must not be stored")
    }]);
    library.ingest_error(&stale, 4, "stale_revision")?;
    assert_eq!(database_state(&library.path)?, before);

    let mut incomplete = empty_plan(seed.root, seed.revision);
    incomplete["child_orders"] = json!([{
        "parent": seed.beta,
        "children": [seed.beta_child, seed.movable]
    }]);
    library.ingest_error(&incomplete, 4, "incomplete_child_order")?;
    assert_eq!(database_state(&library.path)?, before);

    let mut wrong_subtree = empty_plan(seed.root, seed.revision);
    wrong_subtree["delete_subtrees"] = json!([{
        "root_id": seed.doomed,
        "expected_node_ids": [seed.doomed]
    }]);
    wrong_subtree["child_orders"] = json!([{
        "parent": seed.root,
        "children": [seed.alpha, seed.beta, seed.untouched]
    }]);
    library.ingest_error(&wrong_subtree, 4, "subtree_changed")?;
    assert_eq!(database_state(&library.path)?, before);

    let mut source_parent = empty_plan(seed.root, seed.revision);
    source_parent["child_orders"] = json!([
        {
            "parent": seed.alpha,
            "children": [seed.movable]
        },
        {
            "parent": seed.movable,
            "children": [seed.keeper]
        }
    ]);
    library.ingest_error(&source_parent, 4, "source_cannot_have_children")?;
    assert_eq!(database_state(&library.path)?, before);

    let mut cycle = empty_plan(seed.root, seed.revision);
    cycle["child_orders"] = json!([
        {
            "parent": seed.root,
            "children": [seed.beta, seed.doomed, seed.untouched]
        },
        {
            "parent": seed.alpha,
            "children": [seed.alpha, seed.movable, seed.keeper]
        }
    ]);
    library.ingest_error(&cycle, 4, "would_create_cycle")?;
    assert_eq!(database_state(&library.path)?, before);

    let mut unknown = empty_plan(seed.root, seed.revision);
    unknown["unexpected"] = json!(true);
    library.ingest_error(&unknown, 2, "invalid_ingestion")?;
    assert_eq!(database_state(&library.path)?, before);

    assert_eq!(library.json_ok(["validate"])?["valid"], true);
    Ok(())
}

#[test]
fn indexing_failure_rolls_back_the_entire_ingestion() -> TestResult {
    let (library, seed) = seeded_library()?;
    let before = database_state(&library.path)?;
    let connection = Connection::open(&library.path)?;
    connection.execute_batch(
        "CREATE TRIGGER force_ingestion_index_failure \
         BEFORE INSERT ON search_units \
         BEGIN SELECT RAISE(ABORT, 'forced ingestion index failure'); END;",
    )?;
    drop(connection);

    let mut replacement = empty_plan(seed.root, seed.revision);
    replacement["replace_nodes"] = json!([{
        "id": seed.movable,
        "node": source("Replacement that must roll back", "rollbackneedle")
    }]);
    library.ingest_error(&replacement, 5, "index_insert_failed")?;
    assert_eq!(database_state(&library.path)?, before);

    let connection = Connection::open(&library.path)?;
    connection.execute_batch("DROP TRIGGER force_ingestion_index_failure;")?;
    drop(connection);
    assert_eq!(library.json_ok(["validate"])?["valid"], true);
    Ok(())
}
