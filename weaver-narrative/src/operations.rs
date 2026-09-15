//! Foreground authoring, document reads, and owned installation admission.
use anyhow::{Context, Result, bail, ensure};
use nucleus_client::NucleusClient;
use serde_json::{Value, json};
use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use crate::{
    Config, agent, gate,
    store::{self, Store},
};

pub fn initialize(root: &Path, config: &Config) -> Result<Value> {
    config.validate()?;
    config.reader().start()?;
    let owner = std::env::var("CELL_DEPLOYMENT_RUN_ID").ok();
    let admission = gate(root);
    let _guard = if let Some(owner) = owner {
        admission.enter_for(&owner)?
    } else {
        admission.enter()?
    };
    let _lock = store::runner_lock(root)?;
    Store::initialize(root)?;
    store::private_directory(&root.join("agent-workspace"))?;
    let target = root.join("config.json");
    if target.symlink_metadata().is_ok() {
        store::regular(&target)?;
    }
    let mut pending = tempfile::NamedTempFile::new_in(root)?;
    pending.write_all(&serde_json::to_vec_pretty(config)?)?;
    pending.as_file().sync_all()?;
    pending.persist(target)?;
    std::fs::File::open(root)?.sync_all()?;
    Ok(json!({"initialized":true,"schema_version":1}))
}

pub async fn write(root: &Path, direction: &str, revise: Option<&str>) -> Result<Value> {
    write_with_id(root, direction, revise, None).await
}

pub async fn write_with_id(
    root: &Path,
    direction: &str,
    revise: Option<&str>,
    id: Option<&str>,
) -> Result<Value> {
    let retained = id
        .map(|id| Store::open(root, true)?.contains(id))
        .transpose()?
        .unwrap_or(false);
    let admission = gate(root);
    let _admission = if retained {
        admission.recover()?
    } else {
        admission.enter()?
    };
    let _lock = store::runner_lock(root)?;
    let config = Config::read(root)?;
    let mut store = Store::open(root, false)?;
    let existing = revise.map(|id| store.document(id)).transpose()?;
    let markdown = if let Some(existing) = &existing {
        Some(
            existing
                .markdown
                .as_deref()
                .context("the selected document has no saved Markdown")?,
        )
    } else {
        None
    };
    let request = assignment(
        &store,
        direction,
        markdown,
        &root.join("agent-workspace"),
        id,
    )?;
    let saved = store.document(request.id.as_str())?;
    if saved.finished_at.is_some() && saved.error.is_none() && saved.markdown.is_some() {
        return saved.view();
    }
    eprintln!("Weaver document {}", request.id);
    execute(&mut store, &config, &request).await
}

fn assignment(
    store: &Store,
    direction: &str,
    markdown: Option<&str>,
    cwd: &Path,
    id: Option<&str>,
) -> Result<nucleus_core::JobRequestV1> {
    if let Some(id) = id {
        ensure!(
            (1..=120).contains(&id.len())
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "invalid Weaver request ID"
        );
        if store.contains(id)? {
            let request = store.document(id)?.request;
            let input: Value = serde_json::from_str(&request.prompt)?;
            ensure!(
                input["direction"].as_str() == Some(direction)
                    && input.get("document").and_then(Value::as_str) == markdown,
                "Weaver request ID belongs to different input"
            );
            return Ok(request);
        }
    }
    let mut request = agent::request(direction, markdown, cwd)?;
    if let Some(id) = id {
        request.id = id.into();
        request.requester.id = id.into();
    }
    store.create(&request)?;
    Ok(request)
}

pub async fn resume(root: &Path, id: &str) -> Result<Value> {
    // Resolve existing work before using continuation admission.
    let request = Store::open(root, true)?.document(id)?.request;
    let _admission = gate(root).recover()?;
    let _lock = store::runner_lock(root)?;
    let mut store = Store::open(root, false)?;
    execute(&mut store, &Config::read(root)?, &request).await
}

async fn execute(
    store: &mut Store,
    config: &Config,
    request: &nucleus_core::JobRequestV1,
) -> Result<Value> {
    let client = NucleusClient::for_current_user()?;
    let result = agent::run(&client, store, config, request).await;
    if let Err(error) = &result
        && let Some(outcome) = nucleus_core::quota_condition(error.as_ref())
    {
        return Ok(json!({"id":request.id,"outcome":outcome,"detail":error.to_string()}));
    }
    result.with_context(|| {
        format!(
            "document {} retained; use weaver resume {}",
            request.id, request.id
        )
    })?;
    store.document(request.id.as_str())?.view()
}

pub async fn maintenance(root: &Path, operation: &str, owner: Option<&str>) -> Result<Value> {
    let admission = gate(root);
    match operation {
        "hold" => {
            admission.hold(owner.context("hold owner required")?)?;
        }
        "release" => {
            admission.release(owner.context("release owner required")?)?;
        }
        "status" | "drain" => {}
        _ => bail!("unsupported maintenance operation"),
    }
    let status = admission.status()?;
    let client = NucleusClient::for_current_user()?;
    let jobs =
        tokio::time::timeout(Duration::from_secs(30), agent::nonterminal_jobs(&client)).await;
    let jobs = match jobs {
        Ok(Ok(count)) => Some(count),
        _ => None,
    };
    Ok(
        json!({"maintenance":{"protocol_version":1,"holds":status.holds,
        "drained":status.drained && jobs == Some(0),"nonterminal_jobs":jobs}}),
    )
}

pub async fn doctor(root: &Path) -> Result<Value> {
    let owner = std::env::var("CELL_DEPLOYMENT_RUN_ID").ok();
    let admission = gate(root);
    let _guard = match owner.as_deref() {
        Some(owner) if !admission.status()?.holds.is_empty() => admission.enter_for(owner)?,
        _ => admission.enter()?,
    };
    let store = Store::open(root, true)?;
    let integrity: String = store
        .connection
        .query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    ensure!(integrity == "ok", "Weaver database integrity check failed");
    store.connection.prepare("SELECT id,created_at,request,markdown,pending_reply,finished_at,error FROM documents LIMIT 0")?;
    Config::read(root)?.reader().start()?;
    agent::readiness(&NucleusClient::for_current_user()?, owner.as_deref(), false).await?;
    Ok(json!({"ready":true,"schema_version":1}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_identity_replays_exact_request_and_rejects_changed_input() -> Result<()> {
        let root = tempfile::tempdir()?;
        let store = Store::initialize(root.path())?;
        let first = assignment(
            &store,
            "bullet points",
            None,
            root.path(),
            Some("platter-cell"),
        )?;
        let replay = assignment(
            &store,
            "bullet points",
            None,
            root.path(),
            Some("platter-cell"),
        )?;
        assert_eq!(first, replay);
        assert!(assignment(&store, "different", None, root.path(), Some("platter-cell")).is_err());
        assert!(
            assignment(
                &store,
                "bullet points",
                Some("revision"),
                root.path(),
                Some("platter-cell")
            )
            .is_err()
        );
        assert_eq!(
            store.list(10)?["documents"].as_array().map(Vec::len),
            Some(1)
        );
        Ok(())
    }
}
