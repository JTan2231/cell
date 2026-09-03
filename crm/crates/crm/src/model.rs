use std::fmt;
use std::str::FromStr;

use clap::ValueEnum;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Research,
    Warranted,
    Contacted,
    Connected,
    Helped,
    Closed,
}

impl Stage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Research => "research",
            Self::Warranted => "warranted",
            Self::Contacted => "contacted",
            Self::Connected => "connected",
            Self::Helped => "helped",
            Self::Closed => "closed",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Stage {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "research" => Ok(Self::Research),
            "warranted" => Ok(Self::Warranted),
            "contacted" => Ok(Self::Contacted),
            "connected" => Ok(Self::Connected),
            "helped" => Ok(Self::Helped),
            "closed" => Ok(Self::Closed),
            _ => Err(Error::domain(
                "stage_invalid",
                format!("unknown case stage {value:?}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct CaseRevision {
    pub case_id: String,
    pub title: String,
    pub revision: u64,
    pub markdown: String,
    pub markdown_sha256: String,
    pub stage: Stage,
    pub advisory: Option<String>,
    pub attention: bool,
    pub summary: String,
    pub source_update_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct CaseListItem {
    pub case_id: String,
    pub title: String,
    pub revision: u64,
    pub stage: Stage,
    pub advisory: Option<String>,
    pub attention: bool,
    pub summary: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub case_id: String,
    pub title: String,
    pub revision: u64,
    pub stage: Stage,
    pub advisory: Option<String>,
    pub attention: bool,
    pub summary: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Delivery {
    pub id: String,
    pub case_id: String,
    pub label: String,
    pub body: String,
    pub body_sha256: String,
    pub source: Option<String>,
    pub received_at: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    Queued,
    Running,
    Applied,
    Failed,
    Lost,
}

impl UpdateStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Applied => "applied",
            Self::Failed => "failed",
            Self::Lost => "lost",
        }
    }
}

impl FromStr for UpdateStatus {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "applied" => Ok(Self::Applied),
            "failed" => Ok(Self::Failed),
            "lost" => Ok(Self::Lost),
            _ => Err(Error::domain(
                "update_status_invalid",
                format!("unknown update status {value:?}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct StewardUpdate {
    pub id: String,
    pub case_id: String,
    pub delivery_id: String,
    pub status: UpdateStatus,
    pub base_revision: Option<u64>,
    pub requester_id: String,
    pub job_id: String,
    pub admitted: bool,
    pub applied_revision: Option<u64>,
    pub result_posted: bool,
    pub runtime_state: Option<String>,
    pub runtime_detail: Option<String>,
    pub retry_of: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl StewardUpdate {
    pub const fn is_settled(&self) -> bool {
        match self.status {
            UpdateStatus::Applied => self.runtime_state.is_some(),
            UpdateStatus::Failed | UpdateStatus::Lost => true,
            UpdateStatus::Queued | UpdateStatus::Running => false,
        }
    }

    pub const fn needs_worker(&self) -> bool {
        matches!(self.status, UpdateStatus::Queued | UpdateStatus::Running)
            || matches!(self.status, UpdateStatus::Applied) && !self.is_settled()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionProposal {
    pub base_revision: u64,
    pub document_markdown: String,
    pub stage: Stage,
    #[serde(deserialize_with = "deserialize_advisory")]
    pub advisory: Option<String>,
    pub summary: String,
}

fn deserialize_advisory<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Correlation {
    pub update_id: String,
    pub requester_id: String,
    pub job_id: String,
    pub request_json: String,
    pub request_sha256: String,
    pub tool_after: u64,
    pub admitted: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MailboxReceipt {
    pub arguments_sha256: String,
    pub result_json: String,
    pub result_sha256: String,
    pub is_error: bool,
    pub committed_revision: Option<u64>,
}
