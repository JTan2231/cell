//! Semantics-owned CLI types and typed access to supported repository operations.
//! Consumers import these types and keep their own policy and local projections.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub use crate::domain::{
    AccountIntake, AccountRoutingOutcome, Concept, DecisionAccountAnchor, DecisionAccountEvent,
    DecisionAnchor, DecisionEvent, Distinction, Grounding, GroundingSource, Intake, IntakeStatus,
    PathHistory, Project, ProjectDetail, ProjectStatus, Repository, RepositoryDiff, Revision,
    SemanticEffect,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionReceipt {
    pub project_id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntakeReport {
    pub annals_decision_accounts: Vec<AccountIntake>,
    pub legacy_decisions: Vec<Intake>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IntakeRecord {
    Account(AccountIntake),
    Legacy(Intake),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub ok: bool,
    pub error: ErrorBody,
}

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("unable to invoke Semantics")]
    Io(#[from] std::io::Error),
    #[error("Semantics returned invalid JSON")]
    Json(#[from] serde_json::Error),
    #[error("{code}: {message}")]
    Rejected { code: String, message: String },
    #[error("Semantics command failed without a valid error response")]
    Failed,
}

/// A CLI client. The provider owns all state access and command implementation.
#[derive(Debug, Clone)]
pub struct Client {
    binary: PathBuf,
    database: Option<PathBuf>,
}

impl Client {
    #[must_use]
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            database: None,
        }
    }

    #[must_use]
    pub fn with_database(mut self, database: impl Into<PathBuf>) -> Self {
        self.database = Some(database.into());
        self
    }

    fn json<T: DeserializeOwned>(
        &self,
        arguments: &[OsString],
        doctor: bool,
    ) -> Result<T, CliError> {
        let mut command = Command::new(&self.binary);
        if let Some(database) = &self.database {
            command.arg("--database").arg(database);
        }
        let output = command.arg("--json").args(arguments).output()?;
        if !output.status.success() && !doctor {
            return match serde_json::from_slice::<ErrorResponse>(&output.stderr) {
                Ok(response) => Err(CliError::Rejected {
                    code: response.error.code,
                    message: response.error.message,
                }),
                Err(_) => Err(CliError::Failed),
            };
        }
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    pub fn projects(&self) -> Result<Vec<Project>, CliError> {
        self.json(&["project".into(), "list".into()], false)
    }

    pub fn project(&self, id: &str) -> Result<ProjectDetail, CliError> {
        self.json(&["project".into(), "show".into(), id.into()], false)
    }

    pub fn register_project(&self, id: &str, root: &Path) -> Result<ProjectDetail, CliError> {
        self.json(
            &[
                "project".into(),
                "register".into(),
                id.into(),
                root.as_os_str().to_owned(),
            ],
            false,
        )
    }

    pub fn move_project(&self, id: &str, root: &Path) -> Result<ProjectDetail, CliError> {
        self.json(
            &[
                "project".into(),
                "move".into(),
                id.into(),
                root.as_os_str().to_owned(),
            ],
            false,
        )
    }

    pub fn pause_project(&self, id: &str) -> Result<ProjectDetail, CliError> {
        self.json(&["project".into(), "pause".into(), id.into()], false)
    }

    pub fn resume_project(&self, id: &str) -> Result<ProjectDetail, CliError> {
        self.json(&["project".into(), "resume".into(), id.into()], false)
    }

    pub fn retire_project(&self, id: &str) -> Result<ProjectDetail, CliError> {
        self.json(&["project".into(), "retire".into(), id.into()], false)
    }

    pub fn repository(&self, project: &str, revision: Option<u64>) -> Result<Repository, CliError> {
        let mut arguments = vec!["repository".into(), "show".into(), project.into()];
        append_revision(&mut arguments, revision);
        self.json(&arguments, false)
    }

    pub fn search(
        &self,
        project: &str,
        query: &str,
        revision: Option<u64>,
    ) -> Result<Vec<Concept>, CliError> {
        let mut arguments = vec![
            "repository".into(),
            "search".into(),
            project.into(),
            query.into(),
        ];
        append_revision(&mut arguments, revision);
        self.json(&arguments, false)
    }

    pub fn log(
        &self,
        project: &str,
        from: u64,
        to: Option<u64>,
    ) -> Result<Vec<Revision>, CliError> {
        let mut arguments = vec![
            "repository".into(),
            "log".into(),
            project.into(),
            "--from".into(),
            from.to_string().into(),
        ];
        if let Some(to) = to {
            arguments.extend(["--to".into(), to.to_string().into()]);
        }
        self.json(&arguments, false)
    }

    pub fn diff(&self, project: &str, from: u64, to: u64) -> Result<RepositoryDiff, CliError> {
        self.json(
            &[
                "repository".into(),
                "diff".into(),
                project.into(),
                from.to_string().into(),
                to.to_string().into(),
            ],
            false,
        )
    }

    pub fn seed(
        &self,
        project: &str,
        label: &str,
        meaning: &str,
        grounding: Option<&str>,
    ) -> Result<RevisionReceipt, CliError> {
        let mut arguments = vec![
            "repository".into(),
            "seed".into(),
            project.into(),
            "--label".into(),
            label.into(),
            "--meaning".into(),
            meaning.into(),
        ];
        if let Some(grounding) = grounding {
            arguments.extend(["--grounding".into(), grounding.into()]);
        }
        self.json(&arguments, false)
    }

    pub fn seed_markdown(&self, project: &str, path: &Path) -> Result<RevisionReceipt, CliError> {
        self.json(
            &[
                "repository".into(),
                "seed-markdown".into(),
                project.into(),
                path.as_os_str().to_owned(),
            ],
            false,
        )
    }

    pub fn intake(&self, status: Option<IntakeStatus>) -> Result<IntakeReport, CliError> {
        let mut arguments = vec!["intake".into(), "status".into()];
        if let Some(status) = status {
            arguments.extend(["--status".into(), status.as_str().into()]);
        }
        self.json(&arguments, false)
    }

    pub fn assign_intake(&self, event_id: &str, project: &str) -> Result<IntakeRecord, CliError> {
        self.json(
            &[
                "intake".into(),
                "assign".into(),
                event_id.into(),
                project.into(),
            ],
            false,
        )
    }

    pub fn retry_intake(&self, event_id: &str) -> Result<IntakeRecord, CliError> {
        self.json(&["intake".into(), "retry".into(), event_id.into()], false)
    }

    pub fn doctor(&self) -> Result<DoctorReport, CliError> {
        self.json(&["doctor".into()], true)
    }
}

fn append_revision(arguments: &mut Vec<OsString>, revision: Option<u64>) {
    if let Some(revision) = revision {
        arguments.extend(["--revision".into(), revision.to_string().into()]);
    }
}
