//! Accepted current records, projected through the existing public read model.

use serde::{Deserialize, Serialize};

/// The six-table schema for accepted current records.
///
/// Milieu installs this core when it initializes new state. Identifiers are opaque
/// strings supplied by the accepting caller. This schema creates no records.
pub const SCHEMA: &str = include_str!("current.sql");

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Company {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    /// The company that is hiring.
    pub employer_id: String,
    pub title: String,
    pub description: Option<String>,
    pub work_mode: Option<String>,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub url: String,
    /// The company that runs this board or feed, when known.
    pub operator_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Location {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct JobLocation {
    pub job_id: String,
    pub location_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct JobSource {
    pub job_id: String,
    pub source_id: String,
    pub url: String,
}

/// One caller-resolved current opportunity and its complete workplace and source links.
///
/// Compatibility values retain facts required by the existing snapshot contract.
/// Their employer, title, description, status, work mode and workplaces must agree
/// with the current record. The first accepted source and URL remain the primary
/// appearance exposed to existing consumers.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedJob {
    pub job: Job,
    pub locations: Vec<Location>,
    pub appearances: Vec<JobSource>,
    pub compatibility: crate::models::Job,
}
