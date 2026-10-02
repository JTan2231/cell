//! Accepted current job records, separate from the existing read model.
//!
//! The schema is not installed or opened by Cast. No existing data is migrated.

use serde::{Deserialize, Serialize};

/// The six-table schema for accepted current records.
///
/// Callers must select a database and execute this SQL explicitly. Identifiers
/// are opaque strings supplied by the caller. This schema creates no records.
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
