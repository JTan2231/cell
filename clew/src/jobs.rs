//! Read-only Cast handoff. Cast owns the wire types and posting identity rules.
use anyhow::{Context, Result, ensure};
use cast::models::Snapshot;
use serde::Serialize;
use std::{collections::BTreeMap, path::Path, process::Command};

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub cast_job_id: String,
    pub company: String,
    pub title: String,
    pub urls: Vec<String>,
    #[serde(skip)]
    source: cast::models::Job,
}

impl Job {
    #[must_use]
    pub fn matches(&self, query: &str) -> bool {
        if cast::normalize_url(query).is_ok() {
            return cast::adapters::job_url_matches(&self.source, query).unwrap_or(false)
                || self.urls.iter().any(|stored| {
                    match (cast::normalize_url(stored), cast::normalize_url(query)) {
                        (Ok(stored), Ok(query)) => {
                            stored.trim_end_matches('/') == query.trim_end_matches('/')
                        }
                        _ => false,
                    }
                });
        }
        let query = query.to_lowercase();
        [&self.cast_job_id, &self.company, &self.title]
            .into_iter()
            .chain(self.urls.iter())
            .any(|value| value.to_lowercase().contains(&query))
    }
}

pub fn from_snapshot(snapshot: Snapshot) -> Result<Vec<Job>> {
    ensure!(
        snapshot.schema_version == 1,
        "unsupported Cast export schema"
    );
    let companies: BTreeMap<_, _> = snapshot
        .companies
        .into_iter()
        .map(|company| (company.id, company.name))
        .collect();
    snapshot
        .jobs
        .into_iter()
        .map(|source| {
            let company = companies
                .get(&source.company_id)
                .context("Cast job has no retained company")?
                .clone();
            let mut urls = vec![source.url.clone()];
            if let Some(url) = &source.apply_url
                && !urls.contains(url)
            {
                urls.push(url.clone());
            }
            Ok(Job {
                cast_job_id: source.id.clone(),
                company,
                title: source.title.clone(),
                urls,
                source,
            })
        })
        .collect()
}

pub fn read(program: &Path) -> Result<Vec<Job>> {
    let output = Command::new(program)
        .env("CHANCERY_USAGE_INTERNAL", "1")
        .args(["export", "--json"])
        .output()
        .context("cannot read installed Cast")?;
    ensure!(
        output.status.success(),
        "Cast export failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    from_snapshot(serde_json::from_slice(&output.stdout).context("invalid Cast snapshot")?)
}
