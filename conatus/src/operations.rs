use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use crate::store::{Record, Store, private_directory, runner_lock};
use crate::{Config, LIBRARIAN_INSTRUCTIONS, now};

pub fn initialize(
    root: &Path,
    annals: &Path,
    decisions_config: &Path,
    annals_state_dir: Option<&Path>,
    library_name: &str,
) -> Result<Value> {
    let _lock = runner_lock(root)?;
    let annals = std::fs::canonicalize(annals).context("Annals executable is unavailable")?;
    let decisions_config = std::fs::canonicalize(decisions_config)
        .context("the explicitly selected decisions configuration is unavailable")?;
    let annals_state_dir = Some(match annals_state_dir {
        Some(path) => absolute_path(path)?,
        None => {
            if let Some(path) = std::env::var_os("ANNALS_STATE_DIR") {
                absolute_path(Path::new(&path))?
            } else {
                let home = std::env::var_os("HOME").context("HOME is required")?;
                absolute_path(&PathBuf::from(home).join("Library/Application Support/Annals"))?
            }
        }
    });
    let mut store = Store::create(root)?;
    if store.setting("config")?.is_some() {
        let mut existing = store.config()?;
        ensure!(
            existing.decisions_config == decisions_config
                && existing.annals_state_dir == annals_state_dir
                && existing.library == library_name,
            "Conatus is already initialized with different library selections"
        );
        let rebound = existing.annals != annals;
        if rebound {
            existing.annals = annals;
            let watermark = existing.feed().watermark()?;
            ensure!(
                watermark.library_id == existing.decisions_library_id,
                "the decisions library does not match the configured library identity"
            );
            existing.library().current_instructions()?;
            store.set("config", &serde_json::to_string(&existing)?)?;
        }
        return Ok(json!({
            "initialized":false,"rebound":rebound,
            "config":existing,"cursor":store.setting("cursor")?
        }));
    }
    let feed = annals_api::Client::new(&annals, &decisions_config);
    let watermark = feed.watermark()?;
    let catalog = crate::annals::Annals::new(
        annals.clone(),
        annals_state_dir.clone(),
        library_name.to_owned(),
        None,
    );
    let library_id = catalog.create()?;
    ensure!(
        library_id != watermark.library_id,
        "Conatus requires a separate general library"
    );
    let config = Config {
        annals,
        annals_state_dir,
        library: library_name.to_owned(),
        library_id,
        decisions_config,
        decisions_library_id: watermark.library_id,
    };
    let instructions = config.library().instructions(LIBRARIAN_INSTRUCTIONS)?;
    store.configure(&config, &watermark.watermark)?;
    Ok(json!({
        "initialized":true,"config":config,"cursor":watermark.watermark,
        "instructions":instructions,"baseline":"new accepted decisions after this watermark",
        "scheduled":false
    }))
}

pub fn capture_want(root: &Path, wording: &str, source: &str) -> Result<Value> {
    ensure!(!wording.trim().is_empty(), "want wording must not be blank");
    ensure!(!source.trim().is_empty(), "a source reference is required");
    let store = Store::open(root)?;
    let _config = store.config()?;
    let id = format!("want-{}", uuid::Uuid::now_v7());
    let record = Record {
        document: format!(
            "# Conatus captured want\n\nRecord: {id}\nSource: {source}\n\n## Source wording\n\n{wording}"
        ),
        work_name: id.clone(),
        id,
        kind: "want".to_owned(),
        source: source.to_owned(),
        wording: wording.to_owned(),
        source_data: json!({"source":source}),
        captured_at: now()?,
        queued_at: None,
        receipt: None,
        error: None,
    };
    store.capture(&record)?;
    Ok(json!({"captured":true,"record":record}))
}

pub fn update(root: &Path) -> Result<Value> {
    let _lock = runner_lock(root)?;
    let mut store = Store::open(root)?;
    let config = store.config()?;
    if store.setting("paused")?.as_deref() == Some("true") {
        return Ok(json!({"paused":true,"updated":false}));
    }
    let started_at = now()?;
    let mut errors = Vec::new();
    let feed_events_read = match consume_feed(&mut store, &config) {
        Ok(count) => Some(count),
        Err(error) => {
            errors.push(json!({"operation":"decision_feed","error":error.to_string()}));
            None
        }
    };
    let library = config.library();
    let outbox = root.join("outbox");
    private_directory(&outbox)?;
    let mut queued_records = 0_u64;
    if errors.is_empty() {
        for record in store.pending()? {
            let path = outbox.join(format!("{}.md", record.work_name));
            let result =
                write_document(&path, &record.document).and_then(|()| library.enqueue(&path));
            match result {
                Ok(receipt) => {
                    store.queued(&record.id, &receipt)?;
                    queued_records += 1;
                }
                Err(error) => {
                    store.failed_handoff(&record.id, &error.to_string())?;
                    errors.push(
                        json!({"operation":"enqueue","record_id":record.id,"error":error.to_string()}),
                    );
                    break;
                }
            }
        }
    }
    // Do not admit successor work after this activation encounters an abend.
    let inbox_run = if errors.is_empty() {
        match library.run() {
            Ok(result) => Some(result),
            Err(error) => {
                errors.push(json!({"operation":"inbox_run","error":error.to_string()}));
                None
            }
        }
    } else {
        None
    };
    let report = json!({
        "started_at":started_at,"finished_at":now()?,
        "feed_events_read":feed_events_read,"queued_records":queued_records,
        "cursor":store.setting("cursor")?,"inbox_run":inbox_run,"errors":errors
    });
    store.set("last_update", &serde_json::to_string(&report)?)?;
    ensure!(
        errors.is_empty(),
        "update incomplete; conatus status retains the operation results: {errors:?}"
    );
    Ok(report)
}

fn consume_feed(store: &mut Store, config: &Config) -> Result<usize> {
    let feed = config.feed();
    let watermark = feed.watermark()?;
    ensure!(
        watermark.library_id == config.decisions_library_id,
        "the decision feed library differs from the configured identity"
    );
    let mut cursor = store
        .setting("cursor")?
        .context("decision feed cursor is missing")?;
    let mut count = 0;
    loop {
        let page = feed.read_page(&cursor, &watermark.watermark, 100)?;
        ensure!(
            page.library_id == config.decisions_library_id,
            "decision feed library changed"
        );
        ensure!(
            page.request_cursor == cursor,
            "decision feed returned a different request cursor"
        );
        if page.events.is_empty() {
            break;
        }
        ensure!(
            page.next_cursor != cursor,
            "decision feed made no cursor progress"
        );
        let captured_at = now()?;
        let records = page
            .events
            .iter()
            .map(|event| decision_record(&page.library_id, event, captured_at))
            .collect::<Result<Vec<_>>>()?;
        store.accept_page(&records, &page.next_cursor)?;
        count += records.len();
        cursor = page.next_cursor;
    }
    store.set("last_feed_read_at", &now()?.to_string())?;
    Ok(count)
}

fn decision_record(
    library_id: &str,
    event: &annals_api::AcceptedDocumentEvent,
    captured_at: i64,
) -> Result<Record> {
    let id = format!(
        "decision-{:x}",
        Sha256::digest(format!("{library_id}:{}", event.event_id))
    );
    let source = format!(
        "annals:{library_id}/event/{}/document/{}",
        event.event_id, event.document_id
    );
    let document = event.document.clone();
    Ok(Record {
        work_name: id.clone(),
        id,
        kind: "decision".to_owned(),
        source,
        wording: event.document.clone(),
        source_data: serde_json::to_value(event)?,
        document,
        captured_at,
        queued_at: None,
        receipt: None,
        error: None,
    })
}

pub fn show(root: &Path, id: &str, kind: &str) -> Result<Value> {
    let store = Store::open(root)?;
    let record = store.record(id)?;
    ensure!(record.kind == kind, "record is not a {kind}");
    let library = store.config()?.library();
    let associations = library.associations(&record.work_name);
    let related_records = match &associations {
        Ok(associations) => related_records(&store, id, associations)?,
        Err(_) => Vec::new(),
    };
    Ok(json!({
        "record":record,
        "retention":observation(library.work(&record.work_name)),
        "interpretation":observation(library.examination(&record.work_name)),
        "associations":observation(associations),
        "related_records":related_records
    }))
}

fn related_records(store: &Store, selected_id: &str, associations: &Value) -> Result<Vec<Value>> {
    let mut records: BTreeMap<(String, String), Value> = BTreeMap::new();
    for concept in associations["grounded_concepts"]
        .as_array()
        .into_iter()
        .flatten()
    {
        for direction in ["serves", "served_by"] {
            for relation in concept[direction].as_array().into_iter().flatten() {
                for work in relation["source_works"].as_array().into_iter().flatten() {
                    let Some(work) = work.as_str() else { continue };
                    let Some(record) = store.record_for_work(work)? else {
                        continue;
                    };
                    if record.id == selected_id {
                        continue;
                    }
                    let key = (direction.to_owned(), record.id.clone());
                    let hops = relation["shortest_returned_path_hops"]
                        .as_u64()
                        .unwrap_or(u64::MAX);
                    if records.get(&key).is_some_and(|previous| {
                        previous["shortest_returned_path_hops"]
                            .as_u64()
                            .unwrap_or(u64::MAX)
                            <= hops
                    }) {
                        continue;
                    }
                    records.insert(
                        key,
                        json!({
                            "record_id":record.id,"kind":record.kind,
                            "wording":record.wording,"source":record.source,
                            "direction":direction,"concept_id":relation["concept_id"],
                            "relationship":relation["relationship"],
                            "shortest_returned_path_hops":relation["shortest_returned_path_hops"]
                        }),
                    );
                }
            }
        }
    }
    Ok(records.into_values().collect())
}

pub fn status(root: &Path) -> Result<Value> {
    let store = Store::open(root)?;
    let config = store.config()?;
    Ok(
        json!({"config":config,"local":store.status()?,"annals_inbox":observation(config.library().status())}),
    )
}

pub fn instructions(root: &Path, content: Option<&str>) -> Result<Value> {
    let store = Store::open(root)?;
    let library = store.config()?.library();
    if let Some(content) = content {
        let _lock = runner_lock(root)?;
        library.instructions(content)
    } else {
        library.current_instructions()
    }
}

pub fn retry(root: &Path, from: &str, through: &str) -> Result<Value> {
    let _lock = runner_lock(root)?;
    Store::open(root)?.config()?.library().retry(from, through)
}

pub fn reexamine(root: &Path, id: &str) -> Result<Value> {
    let _lock = runner_lock(root)?;
    let store = Store::open(root)?;
    let record = store.record(id)?;
    store.config()?.library().reexamine(&record.work_name)
}

pub fn pause(root: &Path, paused: bool) -> Result<Value> {
    let store = Store::open(root)?;
    let _config = store.config()?;
    store.set("paused", if paused { "true" } else { "false" })?;
    Ok(json!({"paused":paused,"scope":"subsequent Conatus update activations"}))
}

fn observation(result: Result<Value>) -> Value {
    match result {
        Ok(data) => json!({"available":true,"data":data}),
        Err(error) => json!({"available":false,"error":error.to_string()}),
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    ensure!(
        path.is_absolute(),
        "Annals state directory must be absolute"
    );
    Ok(path.to_owned())
}

fn write_document(path: &Path, document: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(document.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn rebind_preserves_both_libraries_cursor_pause_and_captured_wording() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().canonicalize()?;
        let old = root.join("old-annals");
        let new = root.join("new-annals");
        let decisions = root.join("decisions.toml");
        std::fs::write(&old, "#!/bin/sh\nexit 1\n")?;
        std::fs::write(&decisions, "fixture")?;
        std::fs::write(
            &new,
            r#"#!/bin/sh
case "$*" in
  *decision-feed*) printf '%s\n' '{"ok":true,"data":{"contract_version":2,"library_id":"fedcba9876543210fedcba9876543210","watermark":"new-watermark"}}' ;;
  *instructions*) printf '%s\n' '{"ok":true,"data":{"library_id":"0123456789abcdef0123456789abcdef","revision":1,"content":"fixture","sha256":"fixture","recorded_at":"2026-09-09T00:00:00Z"}}' ;;
  *) exit 1 ;;
esac
"#,
        )?;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o700))?;
        let mut store = Store::create(&root)?;
        let config = Config {
            annals: old,
            annals_state_dir: Some(root.join("annals-state")),
            library: "conatus".into(),
            library_id: "0123456789abcdef0123456789abcdef".into(),
            decisions_config: decisions.clone(),
            decisions_library_id: "fedcba9876543210fedcba9876543210".into(),
        };
        store.configure(&config, "retained-cursor")?;
        capture_want(&root, "Keep these exact words.\n", "synthetic")?;
        pause(&root, true)?;
        let output = initialize(
            &root,
            &new,
            &decisions,
            config.annals_state_dir.as_deref(),
            "conatus",
        )?;
        assert_eq!(output["rebound"], true);
        assert_eq!(store.setting("cursor")?.as_deref(), Some("retained-cursor"));
        assert_eq!(store.setting("paused")?.as_deref(), Some("true"));
        assert_eq!(store.config()?.library_id, config.library_id);
        assert_eq!(
            store.config()?.decisions_library_id,
            config.decisions_library_id
        );
        assert_eq!(store.pending()?[0].wording, "Keep these exact words.\n");
        assert_eq!(
            initialize(
                &root,
                &new,
                &decisions,
                config.annals_state_dir.as_deref(),
                "conatus"
            )?["rebound"],
            false
        );
        Ok(())
    }

    #[test]
    fn feed_failure_retains_intake_without_starting_handoffs_or_processing() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path();
        let annals = root.join("annals");
        std::fs::write(
            &annals,
            "#!/bin/sh\nprintf '%s\\n' invoked >>\"${0%/*}/calls\"\nexit 1\n",
        )?;
        std::fs::set_permissions(&annals, std::fs::Permissions::from_mode(0o700))?;
        let mut store = Store::create(root)?;
        store.configure(
            &Config {
                annals,
                annals_state_dir: Some(root.join("annals-state")),
                library: "conatus".to_owned(),
                library_id: "0123456789abcdef0123456789abcdef".to_owned(),
                decisions_config: root.join("decisions.toml"),
                decisions_library_id: "fedcba9876543210fedcba9876543210".to_owned(),
            },
            "cursor-0",
        )?;
        capture_want(root, "Keep the captured wording.", "synthetic-test")?;

        assert!(update(root).is_err());
        assert_eq!(std::fs::read_to_string(root.join("calls"))?, "invoked\n");
        assert_eq!(store.pending()?.len(), 1);
        let report: Value =
            serde_json::from_str(&store.setting("last_update")?.context("report")?)?;
        assert_eq!(report["errors"][0]["operation"], "decision_feed");
        assert_eq!(report["queued_records"], 0);
        assert!(report["inbox_run"].is_null());
        Ok(())
    }
}
