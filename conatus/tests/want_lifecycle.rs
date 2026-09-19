use anyhow::{Context as _, Result};
use conatus::{
    Config,
    store::{Store, WantState},
};
use serde_json::{Value, json};
use std::process::{Command, Output};

fn data(output: &Output) -> Result<Value> {
    anyhow::ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice::<Value>(&output.stdout)?["data"].clone())
}

#[test]
fn existing_schema_one_wants_default_active_without_migration_or_history() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let connection = rusqlite::Connection::open(temporary.path().join("conatus.db"))?;
    connection.execute_batch(
        "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE records (
            id TEXT PRIMARY KEY, kind TEXT NOT NULL, source TEXT NOT NULL,
            wording TEXT NOT NULL, source_data TEXT NOT NULL, work_name TEXT NOT NULL UNIQUE,
            document TEXT NOT NULL, captured_at INTEGER NOT NULL, queued_at INTEGER,
            receipt TEXT, error TEXT
         );
         INSERT INTO records VALUES ('want-existing', 'want', 'source', 'exact wording',
            '{}', 'want-existing', 'original document', 123, NULL, NULL, NULL);
         PRAGMA user_version = 1;",
    )?;
    let mut store = Store::open(temporary.path())?;
    let before = store.record("want-existing")?;
    assert_eq!(before.state, Some(WantState::Active));
    store.set_want_state(&before.id, WantState::Archived)?;
    assert_eq!(store.list("want", 20)?["items"], json!([]));
    assert_eq!(store.list_wants(20, None)?["items"][0]["state"], "archived");
    store.set_want_state(&before.id, WantState::Active)?;
    assert_eq!(
        serde_json::to_value(store.record(&before.id)?)?,
        serde_json::to_value(&before)?
    );
    assert_eq!(store.record(&before.id)?.document, before.document);
    assert_eq!(
        connection.query_row("SELECT COUNT(*) FROM settings", [], |row| row
            .get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?,
        1
    );
    Ok(())
}

#[test]
fn lifecycle_is_local_durable_explicit_and_preserves_sources() -> Result<()> {
    let (temporary, store) = fixture()?;
    let root = temporary.path().canonicalize()?;
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_conatus"))
            .arg("--state-dir")
            .arg(&root)
            .args(args)
            .env("HOME", &root)
            .env("CHANCERY_USAGE_DISABLED", "1")
            .output()
    };
    let source = "conversation exact reference";
    let wording = "  Keep the wording.\nIncluding whitespace.\n";
    let capture = data(&invoke(&["want", "add", wording, "--source", source])?)?;
    let id = capture["record"]["id"].as_str().context("want ID")?;
    let original = store.record(id)?;
    assert_eq!(original.state, Some(WantState::Active));
    assert_eq!(original.wording, wording);
    assert_eq!(
        data(&invoke(&["status"])?)?["local"]["wants"],
        json!({"active":1,"archived":0})
    );

    let archived = data(&invoke(&["want", "archive", id])?)?;
    assert_eq!(archived["changed"], true);
    assert_eq!(archived["record"]["state"], "archived");
    assert_eq!(data(&invoke(&["want", "archive", id])?)?["changed"], false);
    assert_eq!(
        Store::open(&root)?.record(id)?.state,
        Some(WantState::Archived)
    );
    assert_eq!(data(&invoke(&["want", "list"])?)?["items"], json!([]));
    assert_eq!(
        data(&invoke(&["want", "list", "--archived"])?)?["items"][0]["id"],
        id
    );
    assert_eq!(
        data(&invoke(&["want", "show", id])?)?["record"],
        archived["record"]
    );
    assert_eq!(
        data(&invoke(&["status"])?)?["local"]["wants"],
        json!({"active":0,"archived":1})
    );

    // Repeated intake cannot edit an archived source or reset its lifecycle.
    let mut replacement = original.clone();
    replacement.wording = "changed".into();
    replacement.document = "changed outgoing bytes".into();
    replacement.source = "changed reference".into();
    store.capture(&replacement)?;
    let mut retained = serde_json::to_value(store.record(id)?)?;
    retained["state"] = json!("active");
    assert_eq!(retained, capture["record"]);
    assert_eq!(store.record(id)?.document, original.document);
    assert_eq!(store.pending()?[0].id, id);
    assert_eq!(store.pending()?[0].document, original.document);
    store.queued(id, &json!({"job_id":"receipt"}))?;
    assert_eq!(store.record(id)?.state, Some(WantState::Archived));
    assert_eq!(store.record(id)?.document, original.document);
    assert_eq!(store.setting("cursor")?.as_deref(), Some("retained-cursor"));

    assert_eq!(data(&invoke(&["want", "unarchive", id])?)?["changed"], true);
    assert_eq!(
        data(&invoke(&["want", "unarchive", id])?)?["changed"],
        false
    );
    assert_eq!(store.record(id)?.state, Some(WantState::Active));
    assert_eq!(store.record(id)?.wording, wording);
    assert_eq!(store.record(id)?.document, original.document);
    assert_eq!(store.setting(&format!("archived-want/{id}"))?, None);
    assert_eq!(
        data(&invoke(&["status"])?)?["local"]["wants"],
        json!({"active":1,"archived":0})
    );
    Ok(())
}

fn fixture() -> Result<(tempfile::TempDir, Store)> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let mut store = Store::create(&root)?;
    store.configure(
        &Config {
            annals: root.join("absent-annals"),
            annals_state_dir: None,
            library: "conatus".into(),
            library_id: "0123456789abcdef0123456789abcdef".into(),
            decisions_config: root.join("decisions.toml"),
            decisions_library_id: "fedcba9876543210fedcba9876543210".into(),
        },
        "retained-cursor",
    )?;
    Ok((temporary, store))
}

#[test]
fn list_selection_and_lifecycle_admission_are_explicit() -> Result<()> {
    let (temporary, store) = fixture()?;
    let root = temporary.path().canonicalize()?;
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_conatus"))
            .arg("--state-dir")
            .arg(&root)
            .args(args)
            .env("HOME", &root)
            .env("CHANCERY_USAGE_DISABLED", "1")
            .output()
    };
    let source = "fixture source";
    let capture = data(&invoke(&[
        "want",
        "add",
        "Archived want",
        "--source",
        source,
    ])?)?;
    let id = capture["record"]["id"].as_str().context("want ID")?;
    let original = store.record(id)?;
    data(&invoke(&["want", "archive", id])?)?;
    // Filtering precedes limits, including the has_more indication.
    for n in 0..2 {
        data(&invoke(&[
            "want",
            "add",
            &format!("active {n}"),
            "--source",
            source,
        ])?)?;
    }
    assert_eq!(
        data(&invoke(&["want", "list", "--limit", "1"])?)?["has_more"],
        true
    );
    let only_archived = data(&invoke(&["want", "list", "--archived", "--limit", "1"])?)?;
    assert_eq!(only_archived["items"][0]["id"], id);
    assert_eq!(only_archived["has_more"], false);
    assert_eq!(
        data(&invoke(&["want", "list", "--all"])?)?["items"]
            .as_array()
            .context("items")?
            .len(),
        3
    );
    assert!(
        !invoke(&["want", "list", "--all", "--archived"])?
            .status
            .success()
    );

    // The commands accept only known wants and participate in maintenance admission.
    let mut decision = original.clone();
    decision.id = "decision-fixture".into();
    decision.work_name = decision.id.clone();
    decision.kind = "decision".into();
    decision.state = None;
    store.capture(&decision)?;
    for command in ["archive", "unarchive"] {
        assert!(!invoke(&["want", command, "unknown"])?.status.success());
        assert!(!invoke(&["want", command, &decision.id])?.status.success());
    }
    let decisions = data(&invoke(&["decision", "list"])?)?;
    assert_eq!(decisions["items"][0]["id"], decision.id);
    assert!(decisions["items"][0].get("state").is_none());
    conatus::gate(&root).hold("fixture")?;
    assert!(!invoke(&["want", "archive", id])?.status.success());
    assert!(!invoke(&["want", "unarchive", id])?.status.success());
    assert!(invoke(&["want", "list", "--all"])?.status.success());
    conatus::gate(&root).release("fixture")?;

    Ok(())
}
