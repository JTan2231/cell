//! Typed transport for Weaver's installed local CLI. Authoring stays provider-owned.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentView {
    pub id: String,
    pub nucleus_job_id: String,
    pub direction: String,
    pub created_at: i64,
    pub markdown: Option<String>,
    pub finished_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AuthoringOutcome {
    Document(DocumentView),
    Deferred {
        id: String,
        outcome: String,
        detail: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BatchItem {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<AuthoringOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BatchOutcome {
    pub results: Vec<BatchItem>,
}

#[derive(Debug, Clone)]
pub struct Client {
    pub executable: PathBuf,
}

impl Client {
    pub fn new(executable: PathBuf) -> Result<Self> {
        ensure!(
            executable.is_absolute(),
            "Weaver executable must be absolute"
        );
        Ok(Self { executable })
    }

    async fn invoke(&self, args: &[&str]) -> Result<Value> {
        let output = tokio::process::Command::new(&self.executable)
            .arg("--json")
            .args(args)
            .env("CHANCERY_USAGE_INTERNAL", "1")
            .kill_on_drop(true)
            .output()
            .await
            .context("invoke Weaver")?;
        ensure!(
            output.status.success(),
            "Weaver failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: Value =
            serde_json::from_slice(&output.stdout).context("decode Weaver response")?;
        ensure!(
            envelope["ok"] == true,
            "Weaver returned a non-success response"
        );
        Ok(envelope["data"].clone())
    }

    pub async fn write(&self, id: &str, direction: &str) -> Result<AuthoringOutcome> {
        Ok(serde_json::from_value(
            self.invoke(&["write", "--id", id, direction]).await?,
        )?)
    }

    /// Run a batch through one installed Weaver process. Inspect every item for
    /// its document, quota deferral, or error.
    pub async fn write_many(&self, directions: &[String], jobs: usize) -> Result<BatchOutcome> {
        let jobs = jobs.to_string();
        let mut args = vec!["write-many", "--jobs", &jobs, "--"];
        args.extend(directions.iter().map(String::as_str));
        Ok(serde_json::from_value(self.invoke(&args).await?)?)
    }

    pub async fn revise(
        &self,
        id: &str,
        document: &str,
        direction: &str,
    ) -> Result<AuthoringOutcome> {
        Ok(serde_json::from_value(
            self.invoke(&["revise", document, "--request-id", id, direction])
                .await?,
        )?)
    }

    pub async fn show(&self, id: &str) -> Result<DocumentView> {
        Ok(serde_json::from_value(self.invoke(&["show", id]).await?)?)
    }

    pub async fn resume(&self, id: &str) -> Result<AuthoringOutcome> {
        Ok(serde_json::from_value(self.invoke(&["resume", id]).await?)?)
    }

    pub async fn doctor(&self) -> Result<()> {
        ensure!(
            self.invoke(&["doctor"]).await?["ready"] == true,
            "Weaver is not ready"
        );
        Ok(())
    }
}
