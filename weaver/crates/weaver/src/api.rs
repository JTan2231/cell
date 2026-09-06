//! Weaver-owned workflow types, artifact layout and client for the existing CLI.
//! Private current.json and Nucleus requests are never part of this interface.
#![allow(clippy::missing_errors_doc)]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Blocked,
    Failed,
    Cancelled,
}

impl RunStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Blocked | Self::Failed | Self::Cancelled
        )
    }
}

pub const STAGES: [Stage; 5] = [
    Stage {
        ordinal: 1,
        name: "stories",
        directory: "01-stories",
    },
    Stage {
        ordinal: 2,
        name: "themes",
        directory: "02-themes",
    },
    Stage {
        ordinal: 3,
        name: "compose",
        directory: "03-draft",
    },
    Stage {
        ordinal: 4,
        name: "review",
        directory: "04-review",
    },
    Stage {
        ordinal: 5,
        name: "finalize",
        directory: "05-final",
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stage {
    pub ordinal: usize,
    pub name: &'static str,
    pub directory: &'static str,
}

impl Stage {
    #[must_use]
    pub fn prompt_relative(self) -> String {
        format!("workflow/narrative/{}.md", self.name)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Pass,
    Revise,
    Blocked,
}

impl Verdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Revise => "REVISE",
            Self::Blocked => "BLOCKED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub run_id: String,
    pub narrative: String,
    pub status: RunStatus,
    pub completed_stages: usize,
    pub verdict: Option<String>,
    pub active_job_id: Option<String>,
    pub detail: Option<String>,
}

impl std::fmt::Display for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Run: {}", self.run_id)?;
        writeln!(f, "Narrative: narratives/{}", self.narrative)?;
        writeln!(f, "State: {}", self.status.as_str())?;
        writeln!(f, "Completed stages: {}/5", self.completed_stages)?;
        if let Some(verdict) = &self.verdict {
            writeln!(f, "Verdict: {verdict}")?;
        }
        if let Some(job) = &self.active_job_id {
            writeln!(f, "Nucleus job: {job}")?;
        }
        if let Some(detail) = &self.detail {
            writeln!(f, "Detail: {detail}")?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

impl std::str::FromStr for Run {
    type Err = Error;
    fn from_str(text: &str) -> Result<Self, Error> {
        fn field<'a>(
            lines: &mut impl Iterator<Item = &'a str>,
            prefix: &str,
        ) -> Result<&'a str, Error> {
            lines
                .next()
                .and_then(|line| line.strip_prefix(prefix))
                .ok_or_else(|| Error(format!("invalid Weaver report: expected {prefix}")))
        }
        let mut lines = text.lines();
        let run_id = field(&mut lines, "Run: ")?.to_owned();
        let narrative = field(&mut lines, "Narrative: narratives/")?.to_owned();
        let status = match field(&mut lines, "State: ")? {
            "queued" => RunStatus::Queued,
            "running" => RunStatus::Running,
            "succeeded" => RunStatus::Succeeded,
            "blocked" => RunStatus::Blocked,
            "failed" => RunStatus::Failed,
            "cancelled" => RunStatus::Cancelled,
            value => return Err(Error(format!("unsupported Weaver state: {value}"))),
        };
        let completed_stages = field(&mut lines, "Completed stages: ")?
            .strip_suffix("/5")
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| *n <= STAGES.len())
            .ok_or_else(|| Error("invalid Weaver stage count".to_owned()))?;
        let mut run = Self {
            run_id,
            narrative,
            status,
            completed_stages,
            verdict: None,
            active_job_id: None,
            detail: None,
        };
        while let Some(line) = lines.next() {
            if let Some(verdict) = line.strip_prefix("Verdict: ") {
                run.verdict = Some(verdict.to_owned());
            } else if let Some(job) = line.strip_prefix("Nucleus job: ") {
                run.active_job_id = Some(job.to_owned());
            } else if let Some(detail) = line.strip_prefix("Detail: ") {
                run.detail = Some(
                    std::iter::once(detail)
                        .chain(lines)
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
                break;
            } else {
                return Err(Error("invalid Weaver report field".to_owned()));
            }
        }
        Ok(run)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submission {
    pub run_id: String,
    pub narrative: String,
}
impl std::fmt::Display for Submission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "weaver: submitted {}: narratives/{}",
            self.run_id, self.narrative
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cancellation {
    pub run_id: String,
}
impl std::fmt::Display for Cancellation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "weaver: cancellation requested: {}", self.run_id)
    }
}

/// Calls the chosen Weaver executable, preserving its worker process lineage.
#[derive(Debug, Clone)]
pub struct Client {
    executable: PathBuf,
    repo: Option<PathBuf>,
    state_dir: Option<PathBuf>,
}

impl Client {
    #[must_use]
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            repo: None,
            state_dir: None,
        }
    }
    #[must_use]
    pub fn with_repo(mut self, repo: impl Into<PathBuf>) -> Self {
        self.repo = Some(repo.into());
        self
    }
    #[must_use]
    pub fn with_state_dir(mut self, state_dir: impl Into<PathBuf>) -> Self {
        self.state_dir = Some(state_dir.into());
        self
    }

    fn invoke(&self, args: &[&str]) -> Result<std::process::Output, Error> {
        let mut command = std::process::Command::new(&self.executable);
        if let Some(repo) = &self.repo {
            command.arg("--repo").arg(repo);
        }
        if let Some(state_dir) = &self.state_dir {
            command.arg("--state-dir").arg(state_dir);
        }
        command
            .args(args)
            .output()
            .map_err(|e| Error(format!("cannot invoke Weaver: {e}")))
    }
    fn success(output: &std::process::Output) -> Result<(), Error> {
        if output.status.success() {
            Ok(())
        } else {
            Err(Error(format!(
                "Weaver exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }
    fn text(output: &std::process::Output) -> Result<&str, Error> {
        std::str::from_utf8(&output.stdout)
            .map_err(|e| Error(format!("invalid Weaver output: {e}")))
    }
    pub fn submit(&self, narrative: &str) -> Result<Submission, Error> {
        let output = self.invoke(&["submit", narrative])?;
        Self::success(&output)?;
        let (run_id, narrative) = Self::text(&output)?
            .trim_end()
            .strip_prefix("weaver: submitted ")
            .and_then(|value| value.split_once(": narratives/"))
            .ok_or_else(|| Error("invalid Weaver submission receipt".to_owned()))?;
        Ok(Submission {
            run_id: run_id.to_owned(),
            narrative: narrative.to_owned(),
        })
    }
    pub fn status(&self, run_id: Option<&str>) -> Result<Run, Error> {
        let mut args = vec!["status"];
        if let Some(id) = run_id {
            args.push(id);
        }
        let output = self.invoke(&args)?;
        Self::success(&output)?;
        Self::text(&output)?.parse()
    }
    /// Wait for a terminal record. Failed and blocked runs return their typed domain outcome.
    pub fn wait(&self, run_id: Option<&str>) -> Result<Run, Error> {
        let mut args = vec!["wait"];
        if let Some(id) = run_id {
            args.push(id);
        }
        let output = self.invoke(&args)?;
        let run = Self::text(&output)?.parse::<Run>();
        match run {
            Ok(run) if run.status.is_terminal() => Ok(run),
            _ => {
                Self::success(&output)?;
                Err(Error(
                    "Weaver wait did not return a terminal run".to_owned(),
                ))
            }
        }
    }
    pub fn cancel(&self, run_id: Option<&str>) -> Result<Cancellation, Error> {
        let mut args = vec!["cancel"];
        if let Some(id) = run_id {
            args.push(id);
        }
        let output = self.invoke(&args)?;
        Self::success(&output)?;
        let run_id = Self::text(&output)?
            .trim_end()
            .strip_prefix("weaver: cancellation requested: ")
            .ok_or_else(|| Error("invalid Weaver cancellation receipt".to_owned()))?;
        Ok(Cancellation {
            run_id: run_id.to_owned(),
        })
    }
    pub fn check(&self, narrative: &str) -> Result<Verdict, Error> {
        let output = self.invoke(&["check", narrative])?;
        if output.status.code() == Some(3) {
            return Ok(Verdict::Blocked);
        }
        Self::success(&output)?;
        let text = Self::text(&output)?;
        if text.starts_with("weaver: check passed (PASS): ") {
            Ok(Verdict::Pass)
        } else if text.starts_with("weaver: check passed (REVISE): ") {
            Ok(Verdict::Revise)
        } else {
            Err(Error("invalid Weaver check result".to_owned()))
        }
    }
    pub fn doctor(&self) -> Result<(), Error> {
        Self::success(&self.invoke(&["doctor"])?)
    }
    pub fn begin_maintenance(&self, wait_seconds: u64) -> Result<(), Error> {
        Self::success(&self.invoke(&[
            "maintenance",
            "begin",
            "--wait-seconds",
            &wait_seconds.to_string(),
        ])?)
    }
    pub fn end_maintenance(&self) -> Result<(), Error> {
        Self::success(&self.invoke(&["maintenance", "end"])?)
    }
}
