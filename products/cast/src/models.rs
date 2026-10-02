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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CompanyDraft {
    pub name: String,
    pub domain: Option<String>,
    pub website_url: Option<String>,
    pub provider_id: Option<String>,
    pub careers_urls: Vec<String>,
    pub evidence: Vec<Evidence>,
    pub relevance_reasons: Vec<String>,
    pub jobs: Vec<JobDraft>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct JobDraft {
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
    pub description: Option<String>,
    pub is_listed: bool,
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub compensation: Vec<Compensation>,
    #[serde(default)]
    pub geographic_eligibility: Vec<String>,
    #[serde(default)]
    pub published_at_semantics: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryQuery {
    pub id: String,
    pub provider: String,
    #[serde(default)]
    pub terms: Vec<String>,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub cursor: Option<Value>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "daily")]
    pub interval_seconds: u64,
}

const fn yes() -> bool {
    true
}
const fn daily() -> u64 {
    86400
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct DiscoveryResult {
    pub companies: Vec<CompanyDraft>,
    pub next_cursor: Option<Value>,
    pub complete: bool,
    pub outcome: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct VerificationResult {
    pub company_name: Option<String>,
    pub website_url: Option<String>,
    pub careers_urls: Vec<String>,
    pub jobs: Vec<JobDraft>,
    pub next_cursor: Option<Value>,
    pub complete: bool,
    pub outcome: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Budgets {
    pub theirstack_total_credits: u64,
    pub theirstack_daily_credits: u64,
    pub brave_monthly_requests: u64,
    pub brave_daily_requests: u64,
    pub http_per_run: u64,
    pub http_daily: u64,
    pub runtime_seconds: u64,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            theirstack_total_credits: 200,
            theirstack_daily_credits: 60,
            brave_monthly_requests: 1000,
            brave_daily_requests: 30,
            http_per_run: 500,
            http_daily: 3000,
            runtime_seconds: 600,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub budgets: Budgets,
    pub queries: Vec<DiscoveryQuery>,
    pub automatic_excluded_ats: Vec<String>,
    pub careers_interval_seconds: u64,
    pub max_verifications_per_run: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            budgets: Budgets::default(),
            automatic_excluded_ats: vec!["ashby".into()],
            careers_interval_seconds: 86400,
            max_verifications_per_run: 50,
            queries: vec![
                DiscoveryQuery {
                    id: "hn-hiring".into(),
                    provider: "hn".into(),
                    terms: vec![],
                    params: Value::Null,
                    cursor: None,
                    enabled: true,
                    interval_seconds: 21600,
                },
                DiscoveryQuery {
                    id: "theirstack-backend".into(),
                    provider: "theirstack".into(),
                    terms: vec![
                        "software engineer".into(),
                        "backend engineer".into(),
                        "infrastructure engineer".into(),
                    ],
                    params: serde_json::json!({"limit":20,"posted_at_max_age_days":90}),
                    cursor: None,
                    enabled: true,
                    interval_seconds: 86400,
                },
                DiscoveryQuery {
                    id: "brave-engineering".into(),
                    provider: "brave".into(),
                    terms: vec![
                        "developer infrastructure companies careers".into(),
                        "distributed systems companies engineering careers".into(),
                        "AI infrastructure companies careers".into(),
                    ],
                    params: serde_json::json!({"max_pages":2}),
                    cursor: None,
                    enabled: true,
                    interval_seconds: 86400,
                },
            ],
        }
    }
}

impl Config {
    #[must_use]
    pub fn allows_automatic_url(&self, url: &str) -> bool {
        crate::adapters::ats_provider(url).is_none_or(|provider| {
            !self
                .automatic_excluded_ats
                .iter()
                .any(|item| item == provider)
        })
    }

    #[must_use]
    pub fn allows_automatic_posting(&self, url: &str, apply_url: Option<&str>) -> bool {
        self.allows_automatic_url(url) && apply_url.is_none_or(|url| self.allows_automatic_url(url))
    }
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
#[serde(deny_unknown_fields)]
pub struct JobSelection {
    pub schema_version: u32,
    pub job: Job,
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
