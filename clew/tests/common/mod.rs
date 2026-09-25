use serde_json::{Value, json};

pub fn snapshot(specifications: &[(&str, &str, &str)]) -> Value {
    let companies: Vec<_> = specifications
        .iter()
        .map(|(id, company, _)| {
            json!({
                "id":format!("company-{id}"),"revision":1,"name":company,
                "first_seen_at":"2026-09-25T00:00:00Z","last_seen_at":"2026-09-25T00:00:00Z",
                "relevance_reasons":[],"evidence":[]
            })
        })
        .collect();
    let jobs: Vec<_> = specifications.iter().map(|(id, _, title)| json!({
        "id":id,"revision":1,"company_id":format!("company-{id}"),
        "source_id":"fixture","source_key":format!("fixture:{id}"),"title":title,
        "url":format!("https://example.com/{id}"),
        "first_seen_at":"2026-09-25T00:00:00Z","last_seen_at":"2026-09-25T00:00:00Z",
        "availability":"presumed_closed","missing_complete_snapshots":2,
        "evidence":[],"compensation":[],"geographic_eligibility":[],"parser_version":"fixture"
    })).collect();
    json!({"schema_version":1,"snapshot_revision":1,"captured_at":"2026-09-25T00:00:00Z",
        "companies":companies,"jobs":jobs,"source_health":[],"coverage":[]})
}
