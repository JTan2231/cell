//! Readable views of Milieu's command results. JSON remains the machine interface.
use crate::{Command, OpportunityCommand, SourceCommand};
use serde_json::Value;
use std::fmt::Write;

pub(super) fn human(command: &Command, value: &Value) -> String {
    let mut out = String::new();
    match command {
        Command::Init => {
            let _ = writeln!(
                out,
                "Initialized Milieu state: {}",
                text(&value["state_dir"])
            );
        }
        Command::Doctor => {
            out.push_str("Milieu database: ready\n");
            field(&mut out, "State directory", &value["state_dir"]);
            field(&mut out, "Database schema", &value["database_schema"]);
        }
        Command::Status => status(&mut out, value),
        Command::Export { output, .. } => export(&mut out, value, output.is_some()),
        Command::Company { .. } => company(&mut out, value),
        Command::Opportunity {
            command: OpportunityCommand::Show { .. },
        } => job(&mut out, value),
        Command::Source {
            command: SourceCommand::Add { .. },
        } => {
            let _ = writeln!(out, "Added source: {}", text(&value["id"]));
            field(&mut out, "Company", &value["company_id"]);
            field(&mut out, "URL", &value["url"]);
            field(&mut out, "Enabled", &value["enabled"]);
        }
        Command::Companies { .. } => page(&mut out, "Companies", value),
        Command::Opportunities { .. } => page(&mut out, "Opportunities", value),
        Command::Sources { .. } => page(&mut out, "Sources", value),
        Command::Search { query, .. } => {
            page(&mut out, &format!("Search results for {query}"), value);
        }
    }
    out
}

fn export(out: &mut String, value: &Value, saved: bool) {
    if saved {
        let _ = writeln!(out, "Exported JSON snapshot: {}", text(&value["exported"]));
        return;
    }
    out.push_str("Milieu records snapshot\n");
    field(out, "Snapshot revision", &value["snapshot_revision"]);
    field(out, "Captured at", &value["captured_at"]);
    let _ = writeln!(
        out,
        "Retained companies: {}\nRetained opportunities: {}\nSources: {}",
        items(&value["companies"]).len(),
        items(&value["jobs"]).len(),
        items(&value["source_health"]).len()
    );
    for item in items(&value["companies"]) {
        company(out, item);
    }
    for item in items(&value["jobs"]) {
        job(out, item);
    }
    for item in items(&value["source_health"]) {
        source(out, item);
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => if *value { "yes" } else { "no" }.into(),
        Value::Number(value) => value.to_string(),
        Value::Null => "not recorded".into(),
        Value::Array(values) => {
            if values.is_empty() {
                "none".into()
            } else {
                values.iter().map(text).collect::<Vec<_>>().join(", ")
            }
        }
        Value::Object(_) => "details below".into(),
    }
}

fn items(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn field(out: &mut String, label: &str, value: &Value) {
    let _ = writeln!(out, "{label}: {}", text(value));
}

// Query parameters and retained cursors are caller/provider-defined values.
// Use indented labels for those values rather than JSON delimiters.
fn details(out: &mut String, label: &str, value: &Value, indent: usize) {
    let padding = " ".repeat(indent);
    match value {
        Value::Object(fields) => {
            let _ = writeln!(out, "{padding}{label}:");
            for (key, value) in fields {
                details(out, &key.replace('_', " "), value, indent + 2);
            }
        }
        Value::Array(values)
            if values
                .iter()
                .any(|value| value.is_object() || value.is_array()) =>
        {
            let _ = writeln!(out, "{padding}{label}:");
            for (index, value) in values.iter().enumerate() {
                details(out, &(index + 1).to_string(), value, indent + 2);
            }
        }
        _ => {
            let _ = writeln!(out, "{padding}{label}: {}", text(value));
        }
    }
}

fn page(out: &mut String, title: &str, value: &Value) {
    let rows = items(&value["items"]);
    let title = one_line(title);
    let _ = writeln!(out, "{title} ({} shown)", rows.len());
    if rows.is_empty() {
        out.push_str("No matching retained records.\n");
    }
    for row in rows {
        match row["kind"].as_str() {
            Some("company") => {
                let _ = writeln!(
                    out,
                    "{} — {} · domain: {}",
                    row_text(&row["id"]),
                    row_text(&row["name"]),
                    row_text(&row["domain"])
                );
            }
            Some("job") => {
                let _ = writeln!(
                    out,
                    "{} — {} · {} · {} · availability: {}",
                    row_text(&row["id"]),
                    row_text(&row["title"]),
                    row_text(&row["company"]),
                    row_text(&row["location"]),
                    row_text(&row["availability"])
                );
                if !row["geographic_eligibility"].is_null()
                    && !items(&row["geographic_eligibility"]).is_empty()
                {
                    let _ = writeln!(
                        out,
                        "  Geographic eligibility: {}",
                        row_text(&row["geographic_eligibility"])
                    );
                }
            }
            _ => {
                let _ = writeln!(
                    out,
                    "{} — {} · status: {} · enabled: {}",
                    row_text(&row["id"]),
                    row_text(&row["url"]),
                    row_text(&row["status"]),
                    row_text(&row["enabled"])
                );
            }
        }
        if !row["excerpt"].is_null() {
            let _ = writeln!(out, "  Match: {}", row_text(&row["excerpt"]));
        }
    }
    if value["has_more"] == true {
        out.push_str("More results available; increase --limit to see them.\n");
    }
    field(out, "Snapshot revision", &value["snapshot_revision"]);
}

fn one_line(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                character.escape_debug().to_string()
            } else {
                character.to_string()
            }
        })
        .collect()
}

fn row_text(value: &Value) -> String {
    one_line(&text(value))
}

fn record(out: &mut String, title: &str, value: &Value, fields: &[(&str, &str)]) {
    let _ = writeln!(out, "\n{title}");
    for (label, key) in fields {
        details(out, label, &value[key], 0);
    }
}

fn company(out: &mut String, value: &Value) {
    record(
        out,
        &format!("Company: {}", text(&value["name"])),
        value,
        &[
            ("ID", "id"),
            ("Revision", "revision"),
            ("Domain", "domain"),
            ("Website", "website_url"),
            ("First seen", "first_seen_at"),
            ("Last seen", "last_seen_at"),
            ("Relevance reasons", "relevance_reasons"),
            ("Evidence", "evidence"),
        ],
    );
}

fn job(out: &mut String, value: &Value) {
    record(
        out,
        &format!("Opportunity: {}", text(&value["title"])),
        value,
        &[
            ("ID", "id"),
            ("Revision", "revision"),
            ("Company", "company_id"),
            ("Source", "source_id"),
            ("Source key", "source_key"),
            ("Posting URL", "url"),
            ("Application URL", "apply_url"),
            ("Location", "location"),
            ("Remote", "remote"),
            ("Geographic eligibility", "geographic_eligibility"),
            ("Employment type", "employment_type"),
            ("Availability", "availability"),
            ("First seen", "first_seen_at"),
            ("Last seen", "last_seen_at"),
            ("Source published at", "source_published_at"),
            ("Publication date meaning", "published_at_semantics"),
            ("Source updated at", "source_updated_at"),
            ("Source internal ID", "source_internal_id"),
            (
                "Complete scans without this opportunity",
                "missing_complete_snapshots",
            ),
            ("First missing at (Unix seconds)", "first_missing_at"),
            ("Compensation", "compensation"),
            ("Description", "description"),
            ("Evidence", "evidence"),
            ("Content fingerprint", "content_fingerprint"),
            ("Parser version", "parser_version"),
        ],
    );
}

fn source(out: &mut String, value: &Value) {
    record(
        out,
        "Source",
        value,
        &[
            ("ID", "id"),
            ("Company", "company_id"),
            ("URL", "url"),
            ("Enabled", "enabled"),
            ("Status", "status"),
            ("Last attempt", "last_attempt_at"),
            ("Last success", "last_success_at"),
            ("Next due (Unix seconds)", "next_due_at"),
            ("Discovery depth", "discovery_depth"),
            ("Cursor", "cursor"),
            ("Note", "note"),
        ],
    );
}

fn status(out: &mut String, value: &Value) {
    out.push_str("Milieu records status\n");
    for (label, key) in [
        ("Snapshot revision", "snapshot_revision"),
        ("Retained companies", "companies"),
        ("Retained opportunities", "jobs"),
        ("Sources", "sources"),
    ] {
        field(out, label, &value[key]);
    }
}
