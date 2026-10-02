//! Retained-material selection. These operations create ordinary edition records
//! and leave the explicit job eligibility field alone.
use crate::{
    store::{Edition, Store},
    workflow,
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub fn preview(
    root: &Path,
    day: &str,
    run_id: &str,
    packet_ids: &[String],
    brief_overrides: Option<&Path>,
) -> Result<Edition> {
    workflow::validate_day(day)?;
    let id = occurrence_identity(run_id)?;
    let store = Store::open(root)?;
    let overrides: BTreeMap<String, String> = if let Some(path) = brief_overrides {
        ensure!(path.is_absolute(), "brief overrides path must be absolute");
        serde_json::from_slice(&std::fs::read(path)?)?
    } else {
        BTreeMap::new()
    };
    for text in overrides.values() {
        crate::agent::validate_brief_text(text)?;
    }
    if let Some(edition) = store.edition(&id)? {
        ensure!(edition.day == day, "edition date is immutable");
        ensure!(
            packet_ids.is_empty() || packet_ids == edition.packet_ids,
            "edition selection is immutable"
        );
        if brief_overrides.is_some() {
            let selected = edition
                .packet_ids
                .iter()
                .map(|id| store.run(id))
                .collect::<Result<Vec<_>>>()?;
            let candidate = workflow::compose(&store, day, &selected, &overrides)?;
            ensure!(
                candidate.body == edition.body,
                "edition body is immutable; use a new occurrence ID"
            );
        }
        return Ok(edition);
    }
    let mut seen = BTreeSet::new();
    ensure!(
        packet_ids.iter().all(|id| seen.insert(id)),
        "duplicate selected packet"
    );
    let selected = if packet_ids.is_empty() {
        let mut selected = vec![];
        for run in store.list()? {
            if store.run_artifact(&run.id, "resume-pdf")?.is_some() {
                selected.push(run);
                if selected.len() == 3 {
                    break;
                }
            }
        }
        selected
    } else {
        packet_ids
            .iter()
            .map(|id| store.run(id))
            .collect::<Result<Vec<_>>>()?
    };
    let edition = workflow::compose(&store, day, &selected, &overrides)?;
    store.freeze_as(&id, &edition, false)?;
    store.edition(&id)?.context("edition disappeared")
}

pub fn send(root: &Path, day: &str, run_id: &str, executable: Option<&Path>) -> Result<Edition> {
    workflow::validate_day(day)?;
    let id = occurrence_identity(run_id)?;
    let edition = Store::open_read_only(root)?
        .edition(&id)?
        .context("preview the edition before sending")?;
    ensure!(edition.day == day, "edition date is immutable");
    workflow::send_edition(root, &id, executable)
}

pub(crate) fn occurrence_identity(run_id: &str) -> Result<String> {
    ensure!(
        !run_id.is_empty()
            && run_id.len() <= 80
            && run_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "occurrence ID must contain 1 to 80 ASCII letters, digits, hyphens or underscores"
    );
    Ok(format!("ad-hoc/{run_id}"))
}
