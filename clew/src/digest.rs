//! Deterministic mail from one ledger snapshot and retained Platter metadata.
use anyhow::{Context, Result};
use chrono::Local;
use platter::api::Opportunity;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

use crate::store::{Entry, Store, active_entries};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Digest {
    pub subject: String,
    pub body: String,
    pub application_count: usize,
    pub context_available: bool,
    pub ledger_sequence: Option<i64>,
}

struct Application<'a> {
    reference: &'a str,
    status: Option<&'a str>,
    notes: Vec<&'a str>,
    opportunity: Option<&'a Opportunity>,
}

impl Application<'_> {
    fn label(&self) -> String {
        self.opportunity.map_or_else(
            || format!("{} [job details unavailable]", self.reference),
            |job| format!("{} — {}", job.company, job.title),
        )
    }

    fn sort_key(&self) -> (String, String, &str) {
        self.opportunity.map_or_else(
            || (self.reference.to_lowercase(), String::new(), self.reference),
            |job| {
                (
                    job.company.to_lowercase(),
                    job.title.to_lowercase(),
                    self.reference,
                )
            },
        )
    }
}

pub fn render(
    entries: &[Entry],
    opportunities: Option<&[Opportunity]>,
    date: &str,
) -> Result<Digest> {
    let jobs: BTreeMap<_, _> = opportunities
        .unwrap_or_default()
        .iter()
        .map(|job| (job.reference.as_str(), job))
        .collect();
    let mut applications = BTreeMap::new();
    for entry in active_entries(entries) {
        let item = applications
            .entry(entry.platter_job_ref.as_str())
            .or_insert_with(|| Application {
                reference: &entry.platter_job_ref,
                status: None,
                notes: Vec::new(),
                opportunity: jobs.get(entry.platter_job_ref.as_str()).copied(),
            });
        if let Some(status) = &entry.status {
            item.status = Some(status);
        }
        if let Some(notes) = &entry.notes {
            item.notes.push(notes.as_str());
        }
    }
    let mut applications: Vec<_> = applications
        .into_values()
        .filter(|item| {
            !item
                .status
                .is_some_and(|status| status.trim().eq_ignore_ascii_case("rejected"))
        })
        .collect();
    applications.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    let mut body = String::new();
    let mut counts = BTreeMap::<&str, usize>::new();
    for item in &applications {
        *counts
            .entry(item.status.unwrap_or("No status recorded"))
            .or_default() += 1;
    }
    if applications.is_empty() {
        body.push_str("No applications to show.\n");
    } else {
        writeln!(body, "{} applications", applications.len())?;
        for (status, count) in counts {
            writeln!(body, "{count} {status}")?;
        }
        for item in &applications {
            writeln!(
                body,
                "\n{}\nStatus: {}",
                item.label(),
                item.status.unwrap_or("No status recorded")
            )?;
            if let Some(job) = item.opportunity {
                for url in &job.urls {
                    writeln!(body, "{url}")?;
                }
            }
        }
        if applications.iter().any(|item| !item.notes.is_empty()) {
            body.push_str("\nSaved notes\n");
            for item in applications.iter().filter(|item| !item.notes.is_empty()) {
                writeln!(body, "\n{}", item.label())?;
                for note in &item.notes {
                    writeln!(body, "\n{note}")?;
                }
            }
        }
    }
    let context_available = applications.iter().all(|item| item.opportunity.is_some());
    if !context_available {
        body.push_str(
            "\nSome job details were unavailable. All qualifying Clew records are included.\n",
        );
    }
    Ok(Digest {
        subject: format!("Clew — {date}"),
        body,
        application_count: applications.len(),
        context_available,
        ledger_sequence: entries.last().map(|entry| entry.sequence),
    })
}

pub(crate) fn prepare(root: &Path) -> Result<Digest> {
    let entries = Store::open(root, false)?.entries()?;
    let date = Local::now().format("%Y-%m-%d").to_string();
    let without_context = render(&entries, None, &date)?;
    if without_context.application_count == 0 {
        return Ok(without_context);
    }
    let jobs = crate::platter_client()?.list(None);
    render(
        &entries,
        jobs.as_ref().ok().map(|list| list.items.as_slice()),
        &date,
    )
}

pub fn preview(root: &Path, occurrence: Option<&str>) -> Result<Value> {
    let digest = match occurrence {
        Some(id) => {
            crate::delivery::retained(root, id)?
                .context("unknown email occurrence")?
                .digest
        }
        None => prepare(root)?,
    };
    Ok(json!({"from":email::api::sender(),"to":email::api::recipient(),"digest":digest}))
}

pub use crate::delivery::send;
