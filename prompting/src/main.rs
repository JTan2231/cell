//! Explicit prompt import. Runtime readers never invoke this program.
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use bazaar::api::{Reader, Writer};
use cell_prompts::Selection;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Import {
    schema_version: u32,
    entries: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    content: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("prompt import failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 2 {
        return Err("usage: cell-prompts ABSOLUTE_DATABASE IMPORT_JSON".into());
    }
    let database = PathBuf::from(&arguments[0]);
    let input: Import = serde_json::from_slice(&std::fs::read(&arguments[1])?)?;
    if input.schema_version != 1 || input.entries.is_empty() {
        return Err("unsupported or empty import".into());
    }
    let mut seen = BTreeSet::new();
    for entry in &input.entries {
        let owner = entry.id.split('.').next().unwrap_or("");
        if !matches!(
            owner,
            "annals"
                | "conatus"
                | "krisis"
                | "semantics"
                | "paperboy"
                | "platter"
                | "weaver"
                | "mentor"
                | "emt"
        ) || !seen.insert(entry.id.clone())
        {
            return Err("unknown prompt owner or duplicate ID".into());
        }
    }
    let mut writer = Writer::initialize(&database)?;
    let reader = Reader::open(&database)?;
    let mut owners: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
    let mut appended = 0;
    for entry in input.entries {
        let record = match reader.get(&entry.id, None) {
            Ok(record) if record.content == entry.content => record,
            Ok(_) | Err(bazaar::api::Error::NotFound) => {
                appended += 1;
                writer.update(&entry.id, &entry.content)?
            }
            Err(error) => return Err(error.into()),
        };
        let owner = entry
            .id
            .split('.')
            .next()
            .ok_or("missing owner")?
            .to_owned();
        owners
            .entry(owner)
            .or_default()
            .insert(entry.id, record.version);
    }
    // Publish only after every component exists. Partial imports leave the old
    // selection usable; repeating an import reuses already identical records.
    for (owner, entries) in &owners {
        let id = format!("cell.prompts.{owner}");
        let content = serde_json::to_string(&Selection {
            schema_version: 1,
            entries: entries.clone(),
        })?;
        match reader.get(&id, None) {
            Ok(record) if record.content == content => {}
            Ok(_) | Err(bazaar::api::Error::NotFound) => {
                writer.update(&id, &content)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    println!(
        "Imported {} prompt IDs; appended {appended} text versions; published {} selections.",
        seen.len(),
        owners.len()
    );
    Ok(())
}
