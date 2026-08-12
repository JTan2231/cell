use std::error::Error;
use std::io;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn run(library: &Path, arguments: &[&str]) -> TestResult<Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_annals"))
        .arg("--library")
        .arg(library)
        .arg("--json")
        .args(arguments)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(serde_json::from_slice::<Value>(&output.stdout)?["data"].clone())
}

fn create(library: &Path, text: &str) -> TestResult<i64> {
    run(library, &["tree", "create", "--text", text])?["node_ids"][0]
        .as_i64()
        .ok_or_else(|| io::Error::other("tree create omitted ID").into())
}

fn add(library: &Path, parent: i64, text: &str) -> TestResult<i64> {
    run(
        library,
        &[
            "node",
            "add",
            "--parent",
            &parent.to_string(),
            "--text",
            text,
        ],
    )?["node_ids"][0]
        .as_i64()
        .ok_or_else(|| io::Error::other("node add omitted ID").into())
}

#[test]
fn exact_concept_and_path_queries_rank_deterministically() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = directory.path().join("annals.db");
    run(&library, &["init"])?;
    let root = create(&library, "Knowledge")?;
    let databases = add(&library, root, "Databases")?;
    let database_indexes = add(&library, databases, "Indexes")?;
    let languages = add(&library, root, "Languages")?;
    let language_indexes = add(&library, languages, "Indexes")?;

    let exact_path = run(&library, &["search", "Knowledge / Databases / Indexes"])?;
    assert_eq!(exact_path["results"][0]["node_id"], database_indexes);
    let exact_text = run(&library, &["search", "Indexes"])?;
    let ids = exact_text["results"]
        .as_array()
        .ok_or("search results were not an array")?
        .iter()
        .filter_map(|result| result["node_id"].as_i64())
        .collect::<Vec<_>>();
    assert!(ids.contains(&database_indexes));
    assert!(ids.contains(&language_indexes));
    Ok(())
}

#[test]
fn scope_excludes_same_text_from_other_trees() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = directory.path().join("annals.db");
    run(&library, &["init"])?;
    let left = create(&library, "Left")?;
    let left_hit = add(&library, left, "Shared concept")?;
    let right = create(&library, "Right")?;
    let right_hit = add(&library, right, "Shared concept")?;

    let result = run(
        &library,
        &["search", "Shared concept", "--within", &left.to_string()],
    )?;
    let ids = result["results"]
        .as_array()
        .ok_or("search results were not an array")?
        .iter()
        .filter_map(|entry| entry["node_id"].as_i64())
        .collect::<Vec<_>>();
    assert!(ids.contains(&left_hit));
    assert!(!ids.contains(&right_hit));
    Ok(())
}
