use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use rusqlite::Connection;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct State {
    _temp: TempDir,
    database: PathBuf,
}

impl State {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let state = temp.path().join("state");
        fs::create_dir(&state)?;
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700))?;
        Ok(Self {
            database: state.join("geste.db"),
            _temp: temp,
        })
    }

    fn init(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let output = geste(&self.database, &["init"], None)?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        success_data(&output)
    }
}

#[test]
fn create_and_revise_accept_exact_bounded_stdin_snapshots() -> TestResult {
    let state = State::new()?;
    state.init()?;
    let first = capture("Initial title", "contract gate", "first outcome");
    let first_bytes = serde_json::to_vec(&first)?;
    let created = geste(
        &state.database,
        &["episode", "create", "-"],
        Some(&first_bytes),
    )?;
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let created = success_data(&created)?;
    assert_eq!(created["type"], "episode_created");
    assert_eq!(created["episode"]["episode"], "e1");
    assert_eq!(
        created["episode"]["submitted_sha256"],
        format!("{:x}", Sha256::digest(&first_bytes))
    );

    let mut second = capture("Revised title", "new shape", "second outcome");
    second["actions"] = json!([]);
    second["lessons"] = json!([]);
    let second_bytes = serde_json::to_vec_pretty(&second)?;
    let revised = geste(
        &state.database,
        &["episode", "revise", "e1", "-", "--base", "1"],
        Some(&second_bytes),
    )?;
    assert!(
        revised.status.success(),
        "{}",
        String::from_utf8_lossy(&revised.stderr)
    );
    let revised = success_data(&revised)?;
    assert_eq!(revised["episode"]["revision"], 2);
    assert_eq!(
        revised["episode"]["submitted_sha256"],
        format!("{:x}", Sha256::digest(&second_bytes))
    );
    assert_eq!(revised["episode"]["actions"], json!([]));
    assert_eq!(revised["episode"]["lessons"], json!([]));

    let historical = success_data(&geste(
        &state.database,
        &["episode", "show", "e1", "--at", "1"],
        None,
    )?)?;
    assert_eq!(historical["episode"]["revision"], 1);
    assert_eq!(historical["episode"]["title"], "Initial title");
    assert_eq!(
        historical["episode"]["actions"],
        json!(["Reviewed the boundary"])
    );

    let oversized = vec![b' '; 256 * 1024 + 1];
    let rejected = geste(
        &state.database,
        &["episode", "create", "-"],
        Some(&oversized),
    )?;
    assert!(!rejected.status.success());
    assert_eq!(error_code(&rejected)?, "input_too_large");

    let oversized_path = state
        .database
        .parent()
        .ok_or_else(|| test_error("database fixture must have a parent"))?
        .join("oversized.json");
    fs::write(&oversized_path, &oversized)?;
    let oversized_argument = oversized_path
        .to_str()
        .ok_or_else(|| test_error("test input path must be UTF-8"))?;
    let rejected = geste(
        &state.database,
        &["episode", "create", oversized_argument],
        None,
    )?;
    assert_eq!(error_code(&rejected)?, "input_too_large");

    let symlink_path = oversized_path.with_file_name("capture-link.json");
    symlink(&oversized_path, &symlink_path)?;
    let symlink_argument = symlink_path
        .to_str()
        .ok_or_else(|| test_error("test symlink path must be UTF-8"))?;
    let rejected = geste(
        &state.database,
        &["episode", "create", symlink_argument],
        None,
    )?;
    assert_eq!(error_code(&rejected)?, "capture_input_not_regular");
    Ok(())
}

#[test]
fn settlement_grounding_and_strict_json_fail_closed() -> TestResult {
    let state = State::new()?;
    state.init()?;

    let mut unknown = capture("Unknown field", "shape", "outcome");
    unknown["extra"] = json!(true);
    let rejected = create_json(&state.database, &unknown)?;
    assert_eq!(error_code(&rejected)?, "invalid_capture_json");

    let mut missing_gap = capture("Missing gap member", "shape", "outcome");
    missing_gap["settlements"][0]
        .as_object_mut()
        .ok_or_else(|| test_error("settlement fixture must be an object"))?
        .remove("gap");
    let rejected = create_json(&state.database, &missing_gap)?;
    assert_eq!(error_code(&rejected)?, "invalid_capture_json");

    let mut missing_revision = capture("Missing revision member", "shape", "outcome");
    missing_revision["sources"][0]
        .as_object_mut()
        .ok_or_else(|| test_error("source fixture must be an object"))?
        .remove("revision");
    let rejected = create_json(&state.database, &missing_revision)?;
    assert_eq!(error_code(&rejected)?, "invalid_capture_json");

    let mut missing_digest = capture("Missing digest member", "shape", "outcome");
    missing_digest["sources"][0]
        .as_object_mut()
        .ok_or_else(|| test_error("source fixture must be an object"))?
        .remove("digest");
    let rejected = create_json(&state.database, &missing_digest)?;
    assert_eq!(error_code(&rejected)?, "invalid_capture_json");

    let mut verified = capture("Verified", "shape", "outcome");
    verified["settlements"] = json!([{
        "id": "choice",
        "statement": "This became an enacted choice.",
        "status": "verified",
        "gap": null
    }]);
    verified["sources"][0]["supports"] = json!(["settlement:choice"]);
    let rejected = create_json(&state.database, &verified)?;
    assert_eq!(error_code(&rejected)?, "verified_settlement_not_grounded");

    verified["sources"][0]["system"] = json!("decisions");
    verified["sources"][0]["kind"] = json!("lifecycle_event");
    verified["sources"][0]["role"] = json!("authority");
    let accepted = create_json(&state.database, &verified)?;
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let verified_graph = success_data(&geste(&state.database, &["graph", "e1"], None)?)?;
    let verified_nodes = verified_graph["nodes"]
        .as_array()
        .ok_or_else(|| test_error("verified graph nodes must be an array"))?;
    assert!(verified_nodes.iter().any(|node| {
        node["id"] == "claim:settlement:choice"
            && node["origin"] == "geste_structured_verified_settlement"
    }));
    assert!(
        verified_nodes
            .iter()
            .all(|node| node["origin"] != "decisions_grounded_settlement")
    );

    verified["settlements"][0]["gap"] = json!("gap:1");
    let rejected = create_json(&state.database, &verified)?;
    assert_eq!(error_code(&rejected)?, "verified_settlement_gap_forbidden");

    let mut unverified = capture("Unverified", "shape", "outcome");
    unverified["settlements"] = json!([{
        "id": "choice",
        "statement": "This may be a choice.",
        "status": "unverified",
        "gap": null
    }]);
    let rejected = create_json(&state.database, &unverified)?;
    assert_eq!(error_code(&rejected)?, "unverified_settlement_gap_required");

    let mut mutable_git = capture("Mutable Git anchor", "shape", "outcome");
    mutable_git["sources"][0]["system"] = json!("git");
    mutable_git["sources"][0]["kind"] = json!("commit");
    mutable_git["sources"][0]["revision"] = json!("main");
    let rejected = create_json(&state.database, &mutable_git)?;
    assert_eq!(error_code(&rejected)?, "invalid_git_revision");

    mutable_git["sources"][0]["revision"] = Value::Null;
    assert!(create_json(&state.database, &mutable_git)?.status.success());

    mutable_git["sources"][0]["revision"] = json!("a".repeat(40));
    assert!(create_json(&state.database, &mutable_git)?.status.success());
    Ok(())
}

#[test]
fn stale_revisions_and_invalid_relationships_are_atomic() -> TestResult {
    let state = State::new()?;
    state.init()?;

    let mut invalid = capture("Invalid relation", "shape", "outcome");
    invalid["related_episodes"] = json!([{
        "episode": "e9",
        "revision": 1,
        "relation": "builds_on"
    }]);
    let rejected = create_json(&state.database, &invalid)?;
    assert_eq!(error_code(&rejected)?, "related_episode_not_found");

    let first = create_json(&state.database, &capture("First", "shape", "outcome"))?;
    assert!(first.status.success());
    assert_eq!(success_data(&first)?["episode"]["episode"], "e1");

    let revision = serde_json::to_vec(&capture("Second", "new shape", "outcome"))?;
    let accepted = geste(
        &state.database,
        &["episode", "revise", "e1", "-", "--base", "1"],
        Some(&revision),
    )?;
    assert!(accepted.status.success());
    let stale_write = geste(
        &state.database,
        &["episode", "revise", "e1", "-", "--base", "1"],
        Some(&revision),
    )?;
    assert!(!stale_write.status.success());
    assert_eq!(error_code(&stale_write)?, "stale_revision");

    let missing = geste(
        &state.database,
        &["episode", "show", "e1", "--at", "3"],
        None,
    )?;
    assert_eq!(error_code(&missing)?, "revision_not_found");
    Ok(())
}

#[test]
fn search_uses_only_heads_fixed_weights_all_terms_and_numeric_ties() -> TestResult {
    let state = State::new()?;
    state.init()?;

    let mut first = capture("Alpha request", "contract gate", "done");
    first["tags"] = json!(["process"]);
    assert!(create_json(&state.database, &first)?.status.success());

    let mut second = capture("Beta request", "process contract", "done");
    second["tags"] = json!(["process"]);
    assert!(create_json(&state.database, &second)?.status.success());

    let results = success_data(&geste(
        &state.database,
        &["search", "process contract"],
        None,
    )?)?;
    assert_eq!(
        results["results"]
            .as_array()
            .ok_or_else(|| test_error("search results must be an array"))?
            .len(),
        2
    );
    assert_eq!(results["results"][0]["episode"], "e1");
    assert_eq!(results["results"][0]["score"], 14);
    assert_eq!(results["results"][1]["episode"], "e2");
    assert_eq!(results["results"][1]["score"], 14);

    let mut replacement = capture("Alpha request revised", "unrelated boundary", "done");
    replacement["tags"] = json!(["different"]);
    replacement["applicability"] = json!("Use only for an unrelated boundary.");
    let bytes = serde_json::to_vec(&replacement)?;
    assert!(
        geste(
            &state.database,
            &["episode", "revise", "e1", "-", "--base", "1"],
            Some(&bytes),
        )?
        .status
        .success()
    );
    let results = success_data(&geste(
        &state.database,
        &["search", "process contract"],
        None,
    )?)?;
    assert_eq!(
        results["results"]
            .as_array()
            .ok_or_else(|| test_error("search results must be an array"))?
            .len(),
        1
    );
    assert_eq!(results["results"][0]["episode"], "e2");

    let mut failed = capture("Gamma request", "unrelated shape", "No result");
    failed["outcome"]["status"] = json!("failed");
    failed["tags"] = json!(["different"]);
    failed["applicability"] = json!("Use elsewhere.");
    assert!(create_json(&state.database, &failed)?.status.success());
    let results = success_data(&geste(&state.database, &["search", "failed"], None)?)?;
    assert_eq!(results["results"][0]["episode"], "e3");
    assert_eq!(results["results"][0]["score"], 2);
    assert_eq!(results["results"][0]["matched_fields"], json!(["outcome"]));

    let duplicate = geste(&state.database, &["search", "Term ＴＥＲＭ"], None)?;
    assert_eq!(error_code(&duplicate)?, "duplicate_query_term");
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn report_graph_doctor_and_permissions_expose_exact_boundaries() -> TestResult {
    let state = State::new()?;
    state.init()?;
    assert!(
        create_json(&state.database, &capture("Case", "shape", "outcome"))?
            .status
            .success()
    );

    let report = success_data(&geste(&state.database, &["report", "e1"], None)?)?;
    assert_eq!(report["type"], "episode_report");
    assert!(
        report["source_boundary"]
            .as_str()
            .ok_or_else(|| test_error("source boundary must be a string"))?
            .contains("manual assertions")
    );
    assert_eq!(
        report["warnings"]
            .as_array()
            .ok_or_else(|| test_error("report warnings must be an array"))?
            .len(),
        1
    );

    let graph = success_data(&geste(&state.database, &["graph", "e1"], None)?)?;
    assert_eq!(graph["type"], "episode_graph");
    assert!(
        graph["interpretation_label"]
            .as_str()
            .ok_or_else(|| test_error("graph interpretation label must be a string"))?
            .contains("Geste-authored interpretation")
    );
    assert!(
        graph["source_boundary"]
            .as_str()
            .ok_or_else(|| test_error("graph source boundary must be a string"))?
            .contains("manual assertions")
    );
    let graph_nodes = graph["nodes"]
        .as_array()
        .ok_or_else(|| test_error("graph nodes must be an array"))?;
    assert!(
        graph_nodes
            .iter()
            .any(|node| { node["kind"] == "source" && node["origin"] == "manual_upstream_anchor" })
    );
    let source_node = graph_nodes
        .iter()
        .find(|node| node["id"] == "source:context")
        .ok_or_else(|| test_error("graph must contain structured source node"))?;
    assert_eq!(source_node["source"]["system"], "conversations");
    assert_eq!(source_node["source"]["kind"], "thread");
    assert_eq!(source_node["source"]["reference"], "thread-1");
    assert_eq!(source_node["source"]["revision"], Value::Null);
    assert_eq!(source_node["source"]["digest"], Value::Null);
    assert_eq!(source_node["source"]["observed_at"], "2026-09-02T17:00:00Z");
    assert_eq!(source_node["source"]["role"], "context");
    let graph_edges = graph["edges"]
        .as_array()
        .ok_or_else(|| test_error("graph edges must be an array"))?;
    assert!(graph_edges.iter().any(|edge| {
        edge["kind"] == "support" && edge["from"] == "source:context" && edge["to"] == "claim:shape"
    }));
    assert!(graph_edges.iter().any(|edge| {
        edge["kind"] == "structural"
            && edge["from"] == "claim:settlement:candidate"
            && edge["to"] == "claim:gap:1"
            && edge["label"] == "unverified_gap"
    }));

    let doctor = success_data(&geste(&state.database, &["doctor"], None)?)?;
    assert_eq!(doctor["foreign_keys"], "ok");
    assert_eq!(doctor["integrity"], "ok");
    assert_eq!(doctor["permissions"], "private");

    let sidecar = PathBuf::from(format!("{}-wal", state.database.display()));
    fs::write(&sidecar, b"not sqlite")?;
    fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o644))?;
    let rejected = geste(&state.database, &["doctor"], None)?;
    assert_eq!(error_code(&rejected)?, "unsafe_permissions");
    fs::remove_file(&sidecar)?;

    fs::set_permissions(&state.database, fs::Permissions::from_mode(0o644))?;
    let rejected = geste(&state.database, &["doctor"], None)?;
    assert_eq!(error_code(&rejected)?, "unsafe_permissions");
    fs::set_permissions(&state.database, fs::Permissions::from_mode(0o600))?;

    let state_directory = state
        .database
        .parent()
        .ok_or_else(|| test_error("database fixture must have a parent"))?;
    fs::set_permissions(state_directory, fs::Permissions::from_mode(0o755))?;
    let rejected = geste(&state.database, &["doctor"], None)?;
    assert_eq!(error_code(&rejected)?, "unsafe_permissions");

    fs::set_permissions(state_directory, fs::Permissions::from_mode(0o700))?;
    let mut related = capture("Related case", "shape", "outcome");
    related["related_episodes"] = json!([
        {"episode": "e1", "revision": 1, "relation": "builds_on"},
        {"episode": "e1", "revision": 1, "relation": "similar_to"}
    ]);
    assert!(create_json(&state.database, &related)?.status.success());
    let related_graph = success_data(&geste(&state.database, &["graph", "e2"], None)?)?;
    let related_nodes = related_graph["nodes"]
        .as_array()
        .ok_or_else(|| test_error("related graph nodes must be an array"))?;
    assert_eq!(
        related_nodes
            .iter()
            .filter(|node| node["id"] == "related:e1@1")
            .count(),
        1
    );
    let related_edges = related_graph["edges"]
        .as_array()
        .ok_or_else(|| test_error("related graph edges must be an array"))?;
    assert_eq!(
        related_edges
            .iter()
            .filter(|edge| { edge["kind"] == "episode_relation" && edge["to"] == "related:e1@1" })
            .count(),
        2
    );
    Ok(())
}

#[test]
fn selected_database_paths_are_isolated() -> TestResult {
    let first = State::new()?;
    let second = State::new()?;
    first.init()?;
    second.init()?;
    assert!(
        create_json(&first.database, &capture("Only first", "shape", "outcome"))?
            .status
            .success()
    );

    let first_list = success_data(&geste(&first.database, &["episode", "list"], None)?)?;
    let second_list = success_data(&geste(&second.database, &["episode", "list"], None)?)?;
    assert_eq!(
        first_list["episodes"]
            .as_array()
            .ok_or_else(|| test_error("first list must be an array"))?
            .len(),
        1
    );
    assert_eq!(
        second_list["episodes"]
            .as_array()
            .ok_or_else(|| test_error("second list must be an array"))?
            .len(),
        0
    );
    Ok(())
}

#[test]
fn init_refuses_non_geste_and_unsupported_schema() -> TestResult {
    let non_geste = State::new()?;
    let connection = Connection::open(&non_geste.database)?;
    connection.execute_batch("CREATE TABLE unrelated(value TEXT); PRAGMA user_version=1;")?;
    drop(connection);
    fs::set_permissions(&non_geste.database, fs::Permissions::from_mode(0o600))?;
    let rejected = geste(&non_geste.database, &["init"], None)?;
    assert_eq!(error_code(&rejected)?, "not_geste_database");

    let unsupported = State::new()?;
    unsupported.init()?;
    let connection = Connection::open(&unsupported.database)?;
    connection.execute_batch("PRAGMA user_version=2;")?;
    drop(connection);
    let rejected = geste(&unsupported.database, &["doctor"], None)?;
    assert_eq!(error_code(&rejected)?, "unsupported_schema");

    let incomplete = State::new()?;
    let connection = Connection::open(&incomplete.database)?;
    connection.execute_batch(
        "CREATE TABLE geste_meta(
            marker TEXT PRIMARY KEY,
            schema_version INTEGER NOT NULL
         );
         INSERT INTO geste_meta(marker, schema_version) VALUES('geste', 1);
         PRAGMA user_version=1;",
    )?;
    drop(connection);
    fs::set_permissions(&incomplete.database, fs::Permissions::from_mode(0o600))?;
    let rejected = geste(&incomplete.database, &["init"], None)?;
    assert_eq!(error_code(&rejected)?, "database_schema_incomplete");

    let damaged = State::new()?;
    damaged.init()?;
    let connection = Connection::open(&damaged.database)?;
    connection.execute_batch("DROP TRIGGER actions_no_update;")?;
    drop(connection);
    let rejected = geste(&damaged.database, &["doctor"], None)?;
    assert_eq!(error_code(&rejected)?, "database_schema_incomplete");

    let extended = State::new()?;
    extended.init()?;
    let connection = Connection::open(&extended.database)?;
    connection.execute_batch("CREATE TABLE unrelated_extra(value TEXT);")?;
    drop(connection);
    assert!(
        geste(&extended.database, &["doctor"], None)?
            .status
            .success()
    );
    Ok(())
}

#[test]
fn init_sets_database_mode_after_a_restrictive_umask() -> TestResult {
    let state = State::new()?;
    let output = Command::new("sh")
        .arg("-c")
        .arg("umask 777; exec \"$1\" --database \"$2\" --json init")
        .arg("geste-restrictive-umask")
        .arg(env!("CARGO_BIN_EXE_geste"))
        .arg(&state.database)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data = success_data(&output)?;
    assert_eq!(data["type"], "init");
    assert_eq!(
        fs::metadata(&state.database)?.permissions().mode() & 0o777,
        0o600
    );
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn sql_history_tables_refuse_updates_and_deletes() -> TestResult {
    let state = State::new()?;
    state.init()?;
    assert!(
        create_json(&state.database, &capture("First", "shape", "outcome"))?
            .status
            .success()
    );
    let mut second = capture("Second", "shape", "outcome");
    second["related_episodes"] = json!([{
        "episode": "e1",
        "revision": 1,
        "relation": "builds_on"
    }]);
    assert!(create_json(&state.database, &second)?.status.success());

    let connection = Connection::open(&state.database)?;
    for (table, column) in [
        ("episodes", "created_at"),
        ("episode_revisions", "title"),
        ("revision_seals", "sealed_at"),
        ("actions", "value"),
        ("lessons", "value"),
        ("gaps", "value"),
        ("settlements", "statement"),
        ("tags", "value"),
        ("sources", "label"),
        ("source_supports", "target"),
        ("related_episodes", "relation"),
    ] {
        let Err(update) =
            connection.execute(&format!("UPDATE {table} SET {column} = {column}"), [])
        else {
            return Err(test_error("history update unexpectedly succeeded").into());
        };
        assert!(
            update.to_string().contains("immutable_history"),
            "{table}: {update}"
        );
        let Err(delete) = connection.execute(&format!("DELETE FROM {table}"), []) else {
            return Err(test_error("history delete unexpectedly succeeded").into());
        };
        assert!(
            delete.to_string().contains("immutable_history"),
            "{table}: {delete}"
        );
    }
    for (table, statement) in [
        (
            "actions",
            "INSERT INTO actions(episode_id, revision, ordinal, value)
             VALUES(1, 1, 99, 'late')",
        ),
        (
            "lessons",
            "INSERT INTO lessons(episode_id, revision, ordinal, value)
             VALUES(1, 1, 99, 'late')",
        ),
        (
            "gaps",
            "INSERT INTO gaps(episode_id, revision, ordinal, value)
             VALUES(1, 1, 99, 'late')",
        ),
        (
            "settlements",
            "INSERT INTO settlements(
                episode_id, revision, settlement_id, statement, status, gap_ordinal
             ) VALUES(1, 1, 'late', 'late', 'unverified', 1)",
        ),
        (
            "tags",
            "INSERT INTO tags(episode_id, revision, ordinal, value, normalized)
             VALUES(1, 1, 99, 'late', 'late')",
        ),
        (
            "sources",
            "INSERT INTO sources(
                episode_id, revision, source_id, system, kind, reference,
                source_revision, digest, observed_at, role, label
             ) VALUES(
                1, 1, 'late', 'other', 'record', 'late',
                NULL, NULL, '2026-09-02T17:00:00Z', 'context', 'late'
             )",
        ),
        (
            "source_supports",
            "INSERT INTO source_supports(episode_id, revision, source_id, target)
             VALUES(1, 1, 'context', 'response')",
        ),
        (
            "related_episodes",
            "INSERT INTO related_episodes(
                episode_id, revision, ordinal, related_episode_id,
                related_revision, relation
             ) VALUES(1, 1, 99, 1, 1, 'similar_to')",
        ),
    ] {
        let Err(error) = connection.execute(statement, []) else {
            return Err(test_error("sealed child insert unexpectedly succeeded").into());
        };
        assert!(
            error.to_string().contains("sealed_revision"),
            "{table}: {error}"
        );
    }
    Ok(())
}

#[test]
fn reads_reject_a_committed_unsealed_revision() -> TestResult {
    let state = State::new()?;
    state.init()?;
    assert!(
        create_json(&state.database, &capture("First", "shape", "outcome"))?
            .status
            .success()
    );
    let connection = Connection::open(&state.database)?;
    connection.execute_batch(
        "INSERT INTO episode_revisions(
            episode_id, revision, submitted_sha256, recorded_at, title, shape,
            basis_cutoff_at, recorded_by, situation, response, outcome_status,
            outcome_summary, applicability
         )
         SELECT
            episode_id, 2, submitted_sha256, recorded_at, 'Unsealed', shape,
            basis_cutoff_at, recorded_by, situation, response, outcome_status,
            outcome_summary, applicability
         FROM episode_revisions
         WHERE episode_id = 1 AND revision = 1;",
    )?;
    drop(connection);
    let rejected = geste(&state.database, &["episode", "show", "e1"], None)?;
    assert_eq!(error_code(&rejected)?, "unsealed_revision");
    Ok(())
}

fn capture(title: &str, shape: &str, outcome: &str) -> Value {
    json!({
        "schema_version": 1,
        "title": title,
        "shape": shape,
        "basis_cutoff_at": "2026-09-02T18:00:00Z",
        "recorded_by": "codex",
        "situation": "A bounded situation",
        "response": "A bounded response",
        "outcome": {
            "status": "solved",
            "summary": outcome
        },
        "applicability": "Use for a structurally similar request after checking current contracts.",
        "actions": ["Reviewed the boundary"],
        "lessons": ["Keep authority explicit"],
        "settlements": [{
            "id": "candidate",
            "statement": "This is not yet an enacted settlement.",
            "status": "unverified",
            "gap": "gap:1"
        }],
        "tags": ["process"],
        "gaps": ["No Decisions lifecycle authority was available."],
        "sources": [{
            "id": "context",
            "system": "conversations",
            "kind": "thread",
            "reference": "thread-1",
            "revision": null,
            "digest": null,
            "observed_at": "2026-09-02T17:00:00Z",
            "role": "context",
            "label": "Originating request",
            "supports": ["shape", "situation"]
        }],
        "related_episodes": []
    })
}

fn create_json(database: &Path, value: &Value) -> Result<Output, Box<dyn std::error::Error>> {
    geste(
        database,
        &["episode", "create", "-"],
        Some(&serde_json::to_vec(value)?),
    )
}

fn geste(
    database: &Path,
    args: &[&str],
    stdin: Option<&[u8]>,
) -> Result<Output, Box<dyn std::error::Error>> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_geste"));
    command
        .arg("--database")
        .arg(database)
        .arg("--json")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    let mut child = command.spawn()?;
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .ok_or_else(|| test_error("stdin pipe must exist"))?
            .write_all(input)?;
    }
    Ok(child.wait_with_output()?)
}

fn success_data(output: &Output) -> Result<Value, Box<dyn std::error::Error>> {
    let value: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["ok"], true);
    Ok(value["data"].clone())
}

fn error_code(output: &Output) -> Result<String, Box<dyn std::error::Error>> {
    let value: Value = serde_json::from_slice(&output.stderr)?;
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["ok"], false);
    Ok(value["error"]["code"]
        .as_str()
        .ok_or_else(|| test_error("error code must be a string"))?
        .to_owned())
}

fn test_error(message: &str) -> std::io::Error {
    std::io::Error::other(message)
}
