use std::process::Command;

use bazaar::api::Reader;
use cell_prompts::Prompts;
use serde_json::Value;

#[test]
fn migration_preserves_all_text_and_repeated_imports_add_no_versions()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("private/bazaar.sqlite3");
    let seed = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("seed.json");
    for _ in 0..2 {
        let output = Command::new(env!("CARGO_BIN_EXE_cell-prompts"))
            .arg(&database)
            .arg(&seed)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let reader = Reader::open(&database)?;
    let input: Value = serde_json::from_slice(&std::fs::read(seed)?)?;
    let entries = input["entries"].as_array().ok_or("seed entries missing")?;
    assert_eq!(entries.len(), 176);
    for entry in entries {
        let id = entry["id"].as_str().ok_or("seed ID missing")?;
        let content = entry["content"].as_str().ok_or("seed content missing")?;
        assert_eq!(reader.get(id, Some(1))?.content, content);
        assert_eq!(reader.history(id)?, vec![1]);
    }
    for owner in [
        "annals",
        "conatus",
        "krisis",
        "semantics",
        "paperboy",
        "platter",
        "weaver",
        "mentor",
        "emt",
    ] {
        let prompts = Prompts::open(&database, owner, None)?;
        assert_eq!(prompts.selection.version, 1);
        assert_eq!(reader.history(&prompts.selection.id)?, vec![1]);
        for entry in entries.iter().filter(|entry| {
            entry["id"]
                .as_str()
                .is_some_and(|id| id.starts_with(&format!("{owner}.")))
        }) {
            let id = entry["id"].as_str().ok_or("seed ID missing")?;
            assert_eq!(prompts.text(id)?, entry["content"]);
        }
    }
    Ok(())
}
