//! Read-only retained opportunity interface. All references belong to Platter.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PacketSummary {
    pub id: String,
    pub created_at: String,
    pub preparation_status: String,
    pub has_resume: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Opportunity {
    /// Opaque stable opportunity key. Regeneration preserves this reference.
    pub reference: String,
    pub cast_job_id: String,
    pub company: String,
    pub title: String,
    pub urls: Vec<String>,
    pub packets: Vec<PacketSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OpportunityList {
    pub schema_version: u32,
    /// Complete result for this query, from one retained-state transaction.
    pub items: Vec<Opportunity>,
}

pub struct Client {
    executable: PathBuf,
}

impl Client {
    #[must_use]
    pub fn new(executable: PathBuf) -> Self {
        Self { executable }
    }

    pub fn list(&self, query: Option<&str>) -> Result<OpportunityList> {
        ensure!(
            self.executable.is_absolute(),
            "Platter executable must be absolute"
        );
        let mut command = std::process::Command::new(&self.executable);
        command
            .env("CHANCERY_USAGE_INTERNAL", "1")
            .args(["opportunities", "list"]);
        if let Some(query) = query {
            command.args(["--query", query]);
        }
        let output = command
            .output()
            .context("cannot invoke Platter opportunity reader")?;
        ensure!(
            output.status.success(),
            "Platter opportunity read failed ({})",
            output.status
        );
        let result: OpportunityList = serde_json::from_slice(&output.stdout)?;
        ensure!(
            result.schema_version == 1,
            "unsupported Platter opportunity schema"
        );
        Ok(result)
    }

    pub fn show(&self, reference: &str) -> Result<Opportunity> {
        self.list(Some(reference))?
            .items
            .into_iter()
            .find(|item| item.reference == reference)
            .context("Platter opportunity reference was not found")
    }
}
