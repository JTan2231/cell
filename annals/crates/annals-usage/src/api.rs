//! The existing reporting outputs, with account payload normalization unchanged.
#![allow(clippy::missing_errors_doc)]

pub use crate::budget::BudgetReport;
pub use crate::report::{
    ConsumptionReport, ConsumptionSummary, DeliveryReport, ResponseUsage, RunReport, RunSummary,
};
pub use crate::types::{ThreadTokenUsage, TokenUsageBreakdown};
use std::path::PathBuf;

#[derive(Debug)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

/// Calls an explicitly selected Annals Usage executable.
#[derive(Debug, Clone)]
pub struct Client {
    executable: PathBuf,
    config: Option<PathBuf>,
}

impl Client {
    #[must_use]
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            config: None,
        }
    }
    #[must_use]
    pub fn with_config(mut self, config: impl Into<PathBuf>) -> Self {
        self.config = Some(config.into());
        self
    }
    fn command(&self, operation: &str) -> std::process::Command {
        let mut command = std::process::Command::new(&self.executable);
        command.arg(operation);
        if let Some(config) = &self.config {
            command.arg("--config").arg(config);
        }
        command
    }
    fn read<T: serde::de::DeserializeOwned>(
        &self,
        operation: &str,
        args: &[&str],
    ) -> Result<T, Error> {
        let output = self
            .command(operation)
            .arg("--json")
            .args(args)
            .output()
            .map_err(|e| Error(format!("cannot invoke Annals Usage: {e}")))?;
        if !output.status.success() {
            return Err(Error(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|e| Error(format!("invalid Annals Usage response: {e}")))
    }
    /// Calculate a fresh report from the existing authorities.
    pub fn report(&self, limit: usize) -> Result<ConsumptionSummary, Error> {
        self.read("report", &["--limit", &limit.to_string()])
    }
    pub fn report_details(&self, limit: usize) -> Result<ConsumptionReport, Error> {
        self.read("report", &["--details", "--limit", &limit.to_string()])
    }
    pub fn budget(&self) -> Result<BudgetReport, Error> {
        self.read("budget", &[])
    }
    pub fn doctor(&self) -> Result<(), Error> {
        let output = self
            .command("doctor")
            .output()
            .map_err(|e| Error(e.to_string()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Error(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ))
        }
    }
}
