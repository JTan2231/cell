//! Deterministic mail from one ledger snapshot and retained Milieu metadata.
use crate::jobs::Job;
use anyhow::{Context, Result};
use chrono::Local;
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
    opportunity: Option<&'a Job>,
}

impl Application<'_> {
    fn label(&self) -> String {
        self.opportunity.map_or_else(
            || format!("{} [opportunity details unavailable]", self.reference),
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

pub fn render(entries: &[Entry], opportunities: Option<&[Job]>, date: &str) -> Result<Digest> {
    let jobs: BTreeMap<_, _> = opportunities
        .unwrap_or_default()
        .iter()
        .map(|job| (job.milieu_job_id.as_str(), job))
        .collect();
    let mut applications = BTreeMap::new();
    for entry in active_entries(entries) {
        let Some(reference) = entry.application_job_id() else {
            continue;
        };
        let item = applications
            .entry(reference)
            .or_insert_with(|| Application {
                reference,
                status: None,
                notes: Vec::new(),
                opportunity: jobs.get(reference).copied(),
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
            "\nSome opportunity details were unavailable. All qualifying Clew records are included.\n",
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
    let jobs = crate::milieu_jobs();
    render(&entries, jobs.as_ref().ok().map(Vec::as_slice), &date)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::EntryReference;

    #[test]
    fn legacy_reference_reports_keep_their_content_when_context_is_missing() -> Result<()> {
        let entry = Entry {
            sequence: 1,
            id: "reported-application".into(),
            recorded_at: "2026-10-10T12:00:00Z".into(),
            kind: "record".into(),
            thread: None,
            references: vec![EntryReference {
                namespace: "milieu.job".into(),
                external_id: "retained-opportunity".into(),
                role: "application_report".into(),
            }],
            status: Some("applied".into()),
            notes: Some("Applied using the retained packet.\nAwaiting a reply.".into()),
            replaces: None,
        };
        let digest = render(&[entry], None, "2026-10-10")?;
        assert_eq!(digest.application_count, 1);
        assert!(!digest.context_available);
        assert!(
            digest
                .body
                .contains("retained-opportunity [opportunity details unavailable]")
        );
        assert!(digest.body.contains("Status: applied"));
        assert!(
            digest
                .body
                .contains("Applied using the retained packet.\nAwaiting a reply.")
        );
        assert!(
            digest
                .body
                .contains("Some opportunity details were unavailable.")
        );
        Ok(())
    }
}
