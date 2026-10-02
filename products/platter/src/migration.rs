//! One-way import of schema-one file-backed state. Only explicitly owned files
//! enter the cleanup manifest, which survives an interrupted cutover.
use crate::{
    Config,
    agent::{StageResult, import_execution},
    resume::ResumeTemplate,
    store::{Edition, PacketRecord, SCHEMA_VERSION, Store, digest, regular_file},
};
use anyhow::{Context, Result, ensure};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Deserialize, serde::Serialize)]
struct OwnedFile {
    path: PathBuf,
    sha256: String,
}

pub fn migrate(root: &Path) -> Result<()> {
    let store = Store::control(root)?;
    if store.version()? == 1 {
        import(&store)?;
    } else if matches!(store.version()?, 2..=6) {
        upgrade_editions(&store)?;
    }
    ensure!(
        store.version()? == SCHEMA_VERSION,
        "unsupported migration schema"
    );
    cleanup(&store)
}

fn capture(path: &Path, owned: &mut BTreeMap<PathBuf, String>) -> Result<Vec<u8>> {
    ensure!(path.is_absolute(), "legacy content path must be absolute");
    regular_file(path)?;
    let bytes = std::fs::read(path)?;
    owned.insert(path.to_owned(), digest(&bytes));
    Ok(bytes)
}

fn upgrade_editions(store: &Store) -> Result<()> {
    let tx = store.connection.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS edition_packets(
        edition_id TEXT NOT NULL REFERENCES editions(id),
        position INTEGER NOT NULL CHECK(position >= 0),
        run_id TEXT NOT NULL REFERENCES runs(id),
        PRIMARY KEY(edition_id,position), UNIQUE(edition_id,run_id));
        INSERT OR IGNORE INTO edition_packets
        SELECT ea.edition_id,ea.position,a.run_id FROM edition_attachments ea
        JOIN artifacts a ON a.id=ea.artifact_id WHERE a.run_id IS NOT NULL;
        CREATE TRIGGER IF NOT EXISTS immutable_edition_packet BEFORE UPDATE ON edition_packets
        BEGIN SELECT RAISE(ABORT,'edition packets are immutable'); END;",
    )?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}
fn optional(path: &Path, owned: &mut BTreeMap<PathBuf, String>) -> Result<Option<Vec<u8>>> {
    if path.try_exists()? {
        Ok(Some(capture(path, owned)?))
    } else {
        Ok(None)
    }
}

#[allow(clippy::too_many_lines)] // Keep the ordered import and its transaction together.
fn import(store: &Store) -> Result<()> {
    let root = store.root();
    let mut owned = BTreeMap::new();
    let settings: Config =
        serde_json::from_slice(&capture(&root.join("config.json"), &mut owned)?)?;
    settings.validate()?;
    let template: ResumeTemplate =
        serde_json::from_slice(&capture(&settings.original_resume, &mut owned)?)?;
    template.validate()?;
    let mut statement = store.connection.prepare(
        "SELECT id,opportunity,job_id,company,title,status,directory FROM packets ORDER BY id",
    )?;
    let packets = statement
        .query_map([], |r| {
            Ok(PacketRecord {
                id: r.get(0)?,
                opportunity: r.get(1)?,
                job_id: r.get(2)?,
                company: r.get(3)?,
                title: r.get(4)?,
                status: r.get(5)?,
                directory: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    let mut statement = store
        .connection
        .prepare("SELECT day,json FROM editions ORDER BY day")?;
    let editions = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    let tx = store.connection.unchecked_transaction()?;
    tx.execute_batch("ALTER TABLE packets RENAME TO legacy_packets; ALTER TABLE editions RENAME TO legacy_editions;")?;
    store.create_schema()?;
    let template_artifact = store.put_artifact(
        None,
        "template",
        "original-resume.tex",
        "application/x-tex",
        template.source.as_bytes(),
    )?;
    store.set_setting("config", &settings)?;
    store.set_setting("template", &template_artifact.id)?;
    store.set_setting("template_origin", &template.source_path)?;
    let mut templates = BTreeMap::from([(template.source_sha256.clone(), template_artifact.id)]);
    for record in packets {
        let directory = PathBuf::from(&record.directory);
        let mut inputs: Value =
            serde_json::from_slice(&capture(&directory.join("inputs.json"), &mut owned)?)?;
        let captured_template: ResumeTemplate = serde_json::from_value(
            inputs
                .get("template")
                .context("captured template missing")?
                .clone(),
        )?;
        captured_template.validate()?;
        let template_id = if let Some(id) = templates.get(&captured_template.source_sha256) {
            id.clone()
        } else {
            let artifact = store.put_artifact(
                None,
                "template",
                "original-resume.tex",
                "application/x-tex",
                captured_template.source.as_bytes(),
            )?;
            templates.insert(captured_template.source_sha256.clone(), artifact.id.clone());
            artifact.id
        };
        inputs
            .as_object_mut()
            .context("invalid captured inputs")?
            .remove("template");
        inputs["template_artifact"] = json!(template_id);
        let eligible = !matches!(
            record.status.as_str(),
            "reserved" | "sent" | "declined" | "stale"
        );
        let status = if matches!(record.status.as_str(), "reserved" | "sent") {
            "ready"
        } else {
            &record.status
        };
        tx.execute(
            "INSERT INTO jobs VALUES(?1,?2,?3,?4,?5)",
            params![
                record.opportunity,
                record.job_id,
                record.company,
                record.title,
                eligible
            ],
        )?;
        tx.execute(
            "INSERT INTO runs(id,opportunity,status,inputs) VALUES(?1,?2,?3,?4)",
            params![
                record.id,
                record.opportunity,
                status,
                serde_json::to_string(&inputs)?
            ],
        )?;
        let mut executions = serde_json::Map::new();
        for stage in ["brief", "resume"] {
            if let Some(bytes) =
                optional(&directory.join(format!("{stage}-stage.json")), &mut owned)?
            {
                let value: Value = serde_json::from_slice(&bytes)?;
                executions.insert(stage.into(), import_execution(&value)?);
                if let Some(accepted) = value.get("accepted").filter(|v| !v.is_null()) {
                    match serde_json::from_value::<StageResult>(accepted.clone())? {
                        StageResult::Brief(brief) => {
                            store.put_content(&record.id, "brief", &brief)?;
                        }
                        StageResult::Resume(resume) => {
                            store.put_content(&record.id, "resume-content", &resume)?;
                        }
                        StageResult::Review(_) | StageResult::Draft(_) => {
                            anyhow::bail!(
                                "legacy execution cannot contain a review or combined draft"
                            )
                        }
                    }
                }
            }
            optional(&directory.join(format!("{stage}-stage.lock")), &mut owned)?;
        }
        tx.execute(
            "UPDATE runs SET executions=?2 WHERE id=?1",
            params![record.id, serde_json::to_string(&executions)?],
        )?;
        for (file, kind) in [
            ("brief.json", "brief"),
            ("resume-content.json", "resume-content"),
        ] {
            if let Some(bytes) = optional(&directory.join(file), &mut owned)? {
                let value: Value = serde_json::from_slice(&bytes)?;
                if let Some(existing) = store.run_artifact(&record.id, kind)? {
                    ensure!(
                        serde_json::from_slice::<Value>(&existing.content)? == value,
                        "legacy accepted content conflicts with stage output"
                    );
                } else {
                    store.put_content(&record.id, kind, &value)?;
                }
            }
        }
        if let Some(bytes) = optional(&directory.join("artifacts.json"), &mut owned)? {
            let value: Value = serde_json::from_slice(&bytes)?;
            ensure!(
                value["pages"].as_u64() == Some(1),
                "legacy resume has no one-page acceptance"
            );
            for (field, kind, media) in [
                ("resume_pdf", "resume-pdf", "application/pdf"),
                ("resume_source", "resume-source", "application/x-tex"),
            ] {
                let path = Path::new(
                    value[field]
                        .as_str()
                        .context("legacy artifact path missing")?,
                );
                let content = capture(path, &mut owned)?;
                if kind == "resume-pdf" {
                    ensure!(content.starts_with(b"%PDF-"), "legacy PDF is invalid");
                } else {
                    captured_template.validate_fixed_content(std::str::from_utf8(&content)?)?;
                }
                store.put_artifact(Some(&record.id), kind, filename(path)?, media, &content)?;
            }
        }
        if status == "ready" {
            ensure!(
                store.run_artifact(&record.id, "brief")?.is_some()
                    && store.run_artifact(&record.id, "resume-content")?.is_some()
                    && store.run_artifact(&record.id, "resume-pdf")?.is_some(),
                "ready legacy packet is incomplete"
            );
        }
    }
    for (id, value) in editions {
        let edition: Edition = serde_json::from_str(&value)?;
        optional(
            &root.join("editions").join(&id).join("edition.json"),
            &mut owned,
        )?;
        import_edition(store, &id, edition, &mut owned)?;
    }
    let occurrences = root.join("ad-hoc");
    if occurrences.try_exists()? {
        ensure!(
            !std::fs::symlink_metadata(&occurrences)?
                .file_type()
                .is_symlink(),
            "legacy occurrence directory must not be symbolic"
        );
        for entry in std::fs::read_dir(occurrences)? {
            let entry = entry?;
            ensure!(
                entry.file_type()?.is_dir() && !entry.file_type()?.is_symlink(),
                "unrecognized legacy occurrence entry"
            );
            let value: Value = serde_json::from_slice(&capture(
                &entry.path().join("occurrence.json"),
                &mut owned,
            )?)?;
            ensure!(
                value["version"] == 1,
                "unsupported legacy occurrence version"
            );
            let run_id = value["run_id"]
                .as_str()
                .context("legacy occurrence ID missing")?;
            ensure!(
                entry.file_name().to_str() == Some(run_id),
                "legacy occurrence identity mismatch"
            );
            let edition: Edition = serde_json::from_value(value["edition"].clone())?;
            let hash = digest(&serde_json::to_vec(
                &json!({"day":edition.day,"subject":edition.subject,"body":edition.body,"packet_ids":edition.packet_ids,"attachments":edition.attachments,"attachment_sha256":edition.attachment_sha256,"idempotency_key":edition.idempotency_key}),
            )?);
            ensure!(
                value["payload_sha256"].as_str() == Some(&hash),
                "legacy occurrence payload changed"
            );
            import_edition(store, &format!("ad-hoc/{run_id}"), edition, &mut owned)?;
        }
    }
    // The predecessor runner must already be drained by the caller.
    optional(&root.join("runner.lock"), &mut owned)?;
    retain_unmapped(store, root, &mut owned)?;
    let cleanup: Vec<_> = owned
        .into_iter()
        .map(|(path, sha256)| OwnedFile { path, sha256 })
        .collect();
    store.set_setting("migration_cleanup", &cleanup)?;
    tx.execute_batch("DROP TABLE legacy_packets; DROP TABLE legacy_editions;")?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}

fn import_edition(
    store: &Store,
    id: &str,
    mut edition: Edition,
    owned: &mut BTreeMap<PathBuf, String>,
) -> Result<()> {
    ensure!(
        (1..=3).contains(&edition.attachments.len())
            && edition.attachments.len() == edition.attachment_sha256.len()
            && edition.attachments.len() == edition.packet_ids.len(),
        "legacy edition has incomplete attachment identity"
    );
    let mut artifacts = vec![];
    for (index, path) in edition.attachments.iter().enumerate() {
        let path = Path::new(path);
        let bytes = capture(path, owned)?;
        ensure!(
            digest(&bytes) == edition.attachment_sha256[index],
            "legacy frozen attachment changed"
        );
        let artifact = store.put_artifact(
            Some(&edition.packet_ids[index]),
            &format!("frozen/{id}/{index}"),
            filename(path)?,
            "application/pdf",
            &bytes,
        )?;
        artifacts.push(artifact.id);
    }
    edition.attachments = artifacts;
    store.connection.execute("INSERT INTO editions(id,day,status,subject,body,idempotency_key,receipt) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,edition.day,edition.status,edition.subject,edition.body,edition.idempotency_key,edition.receipt])?;
    store.insert_edition_packets(id, &edition.packet_ids)?;
    for (index, artifact) in edition.attachments.iter().enumerate() {
        store.connection.execute(
            "INSERT INTO edition_attachments VALUES(?1,?2,?3)",
            params![id, i64::try_from(index)?, artifact],
        )?;
    }
    Ok(())
}
fn retain_unmapped(
    store: &Store,
    directory: &Path,
    owned: &mut BTreeMap<PathBuf, String>,
) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if directory == store.root()
            && matches!(
                entry.file_name().to_str(),
                Some(
                    "packets.sqlite3"
                        | "packets.sqlite3-journal"
                        | "packets.sqlite3-wal"
                        | "packets.sqlite3-shm"
                        | "backups"
                )
            )
        {
            continue;
        }
        ensure!(
            !entry.file_type()?.is_symlink(),
            "unrecognized symbolic legacy state; migration stopped"
        );
        if entry.file_type()?.is_dir() {
            retain_unmapped(store, &path, owned)?;
        } else if !owned.contains_key(&path) {
            let content = capture(&path, owned)?;
            store.put_artifact(
                None,
                "legacy-retained",
                filename(&path)?,
                "application/octet-stream",
                &content,
            )?;
        }
    }
    Ok(())
}

fn filename(path: &Path) -> Result<&str> {
    path.file_name()
        .and_then(|s| s.to_str())
        .context("legacy filename is not UTF-8")
}

fn cleanup(store: &Store) -> Result<()> {
    let files: Vec<OwnedFile> = store
        .setting::<Vec<OwnedFile>>("migration_cleanup")?
        .unwrap_or_default()
        .into_iter()
        .filter(|file| !file.path.starts_with(store.root().join("backups")))
        .collect();
    // Compare every remaining file before deleting any; retained paths are a
    // migration manifest, never runtime content dependencies.
    for file in &files {
        if file.path.try_exists()? {
            regular_file(&file.path)?;
            ensure!(
                digest(&std::fs::read(&file.path)?) == file.sha256,
                "legacy cleanup file changed; preserve it for recovery"
            );
        }
    }
    for file in &files {
        if file.path.try_exists()? {
            std::fs::remove_file(&file.path)?;
        }
    }
    store.connection.execute(
        "DELETE FROM settings WHERE key IN ('migration_cleanup','migration_backup')",
        [],
    )?;
    Ok(())
}
