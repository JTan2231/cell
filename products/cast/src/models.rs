use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Evidence {
    pub source_url: String,
    pub kind: String,
    pub note: String,
    #[serde(default)]
    pub observed_at: Option<String>,
    #[serde(default)]
    pub parser_version: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Compensation {
    pub currency: Option<String>,
    pub period: Option<String>,
    pub component: String,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Company {
    pub id: String,
    pub revision: u64,
    pub name: String,
    pub domain: Option<String>,
    pub website_url: Option<String>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub relevance_reasons: Vec<String>,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Job {
    pub id: String,
    pub revision: u64,
    pub company_id: String,
    pub source_id: String,
    pub source_key: String,
    pub title: String,
    pub url: String,
    pub apply_url: Option<String>,
    pub location: Option<String>,
    pub remote: Option<bool>,
    pub employment_type: Option<String>,
    pub source_published_at: Option<String>,
    pub source_updated_at: Option<String>,
    pub source_internal_id: Option<String>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub availability: String,
    pub missing_complete_snapshots: u64,
    pub first_missing_at: Option<i64>,
    pub description: Option<String>,
    pub evidence: Vec<Evidence>,
    pub compensation: Vec<Compensation>,
    pub geographic_eligibility: Vec<String>,
    pub published_at_semantics: Option<String>,
    pub content_fingerprint: Option<String>,
    pub parser_version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Source {
    #[serde(default)]
    pub discovery_depth: u8,
    pub id: String,
    pub company_id: String,
    pub url: String,
    pub enabled: bool,
    pub status: String,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub next_due_at: i64,
    pub cursor: Option<Value>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Coverage {
    pub query_id: String,
    pub provider: String,
    pub status: String,
    pub last_attempt_at: Option<String>,
    pub last_complete_at: Option<String>,
    pub next_due_at: i64,
    pub cursor: Option<Value>,
    pub note: Option<String>,
    pub high_watermark: Option<String>,
    pub query_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Snapshot {
    pub schema_version: u32,
    pub snapshot_revision: u64,
    pub captured_at: String,
    pub companies: Vec<Company>,
    pub jobs: Vec<Job>,
    pub source_health: Vec<Source>,
    pub coverage: Vec<Coverage>,
}
