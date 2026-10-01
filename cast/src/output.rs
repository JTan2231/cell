//! Readable views of Cast's command results. JSON remains the machine interface.
use crate::{Command, ConfigCommand, JobCommand, SourceCommand};
use serde_json::Value;
use std::fmt::Write;

pub(super) fn human(command: &Command, value: &Value) -> String {
    let mut out = String::new();
    match command {
        Command::Init => {
            let _ = writeln!(out, "Initialized Cast state: {}", text(&value["state_dir"]));
        }
        Command::Doctor => {
            out.push_str("Cast database: ready\n");
            field(&mut out, "State directory", &value["state_dir"]);
            field(
                &mut out,
                "TheirStack credential present",
                &value["credentials"]["theirstack"],
            );
            field(
                &mut out,
                "Brave credential present",
                &value["credentials"]["brave"],
            );
            config(&mut out, &value["config"]);
        }
        Command::Config {
            command: ConfigCommand::Show,
        } => config(&mut out, value),
        Command::Config {
            command: ConfigCommand::Set { .. },
        } => {
            out.push_str("Saved collection configuration.\n");
        }
        Command::State { .. } => {
            out.push_str("Reconciled employer ownership.\n");
            for (label, key) in [
                ("Moved sources", "moved_sources"),
                ("Moved jobs", "moved_jobs"),
                ("Quarantined jobs", "quarantined_jobs"),
                ("Renamed candidates", "renamed_candidates"),
                ("Cleared shared identities", "cleared_shared_identities"),
            ] {
                field(&mut out, label, &value[key]);
            }
        }
        Command::Status => status(&mut out, value),
        Command::Run { .. }
        | Command::Job {
            command: JobCommand::Refresh { .. },
        } => {
            let _ = writeln!(
                out,
                "Collection run {}: {}",
                text(&value["run_id"]),
                text(&value["status"])
            );
            details(&mut out, "Summary", &value["summary"], 0);
            status(&mut out, &value["state"]);
        }
        Command::Export {
            output: Some(_), ..
        } => {
            let _ = writeln!(out, "Exported JSON snapshot: {}", text(&value["exported"]));
        }
        Command::Export { output: None, .. } => {
            out.push_str("Cast discovery snapshot\n");
            field(&mut out, "Snapshot revision", &value["snapshot_revision"]);
            field(&mut out, "Captured at", &value["captured_at"]);
            let _ = writeln!(
                out,
                "Retained companies: {}\nRetained jobs: {}\nSources: {}",
                items(&value["companies"]).len(),
                items(&value["jobs"]).len(),
                items(&value["source_health"]).len()
            );
            for item in items(&value["companies"]) {
                company(&mut out, item);
            }
            for item in items(&value["jobs"]) {
                job(&mut out, item);
            }
            for item in items(&value["source_health"]) {
                source(&mut out, item);
            }
            details(&mut out, "Query coverage", &value["coverage"], 0);
        }
        Command::Company { .. } => company(&mut out, value),
        Command::Job {
            command: JobCommand::Show { .. },
        } => job(&mut out, value),
        Command::Job {
            command: JobCommand::Collect { .. },
        } => {
            let job = &value["job"];
            let _ = writeln!(
                out,
                "Collected job: {} ({})",
                text(&job["title"]),
                text(&job["id"])
            );
            field(&mut out, "URL", &job["url"]);
        }
        Command::Source {
            command: SourceCommand::Add { .. },
        } => {
            let _ = writeln!(out, "Added source: {}", text(&value["id"]));
            field(&mut out, "Company", &value["company_id"]);
            field(&mut out, "URL", &value["url"]);
            field(&mut out, "Enabled", &value["enabled"]);
        }
        Command::Source {
            command: SourceCommand::Disable { .. },
        } => {
            let _ = writeln!(out, "Disabled source: {}", text(&value["id"]));
        }
        Command::Companies { .. } => page(&mut out, "Companies", value),
        Command::Jobs { .. } => page(&mut out, "Jobs", value),
        Command::Sources { .. } => page(&mut out, "Sources", value),
        Command::Unresolved { .. } => page(&mut out, "Unresolved records", value),
        Command::Search { query, .. } => {
            page(&mut out, &format!("Search results for {query}"), value)
        }
    }
    out
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
        &format!("Job: {}", text(&value["title"])),
        value,
        &[
            ("ID", "id"),
            ("Revision", "revision"),
            ("Company", "company_id"),
            ("Source", "source_id"),
            ("Source key", "source_key"),
            ("URL", "url"),
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
                "Complete scans without this job",
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

fn budgets(out: &mut String, value: &Value) {
    for (label, key) in [
        (
            "TheirStack lifetime cap (credits)",
            "theirstack_total_credits",
        ),
        ("TheirStack daily cap (credits)", "theirstack_daily_credits"),
        ("Brave monthly cap (requests)", "brave_monthly_requests"),
        ("Brave daily cap (requests)", "brave_daily_requests"),
        ("HTTP cap per run (requests)", "http_per_run"),
        ("HTTP daily cap (requests)", "http_daily"),
        ("Runtime cap per run (seconds)", "runtime_seconds"),
    ] {
        field(out, label, &value[key]);
    }
}

fn config(out: &mut String, value: &Value) {
    out.push_str("Collection configuration\n");
    budgets(out, &value["budgets"]);
    field(
        out,
        "Excluded ATS providers for ordinary collection",
        &value["automatic_excluded_ats"],
    );
    field(
        out,
        "Careers interval (seconds)",
        &value["careers_interval_seconds"],
    );
    field(
        out,
        "Targeted adapter-page cap per run",
        &value["max_verifications_per_run"],
    );
    for query in items(&value["queries"]) {
        record(
            out,
            "Discovery query",
            query,
            &[
                ("ID", "id"),
                ("Provider", "provider"),
                ("Terms", "terms"),
                ("Enabled", "enabled"),
                ("Interval (seconds)", "interval_seconds"),
                ("Parameters", "params"),
                ("Cursor", "cursor"),
            ],
        );
    }
}

fn status(out: &mut String, value: &Value) {
    out.push_str("Cast collection status\n");
    for (label, key) in [
        ("Snapshot revision", "snapshot_revision"),
        ("Retained companies", "companies"),
        ("Retained jobs", "jobs"),
        ("Sources", "sources"),
        ("HTTP requests today (UTC)", "http_requests_today"),
    ] {
        field(out, label, &value[key]);
    }
    budgets(out, &value["budgets"]);
    for (provider, unit) in [
        ("theirstack", "credits"),
        ("brave", "requests"),
        ("http", "requests"),
        ("hn", "requests"),
    ] {
        let usage = &value["usage"][provider];
        let _ = writeln!(
            out,
            "{provider} usage: {} {unit} today (UTC); {} {unit} retained total",
            text(&usage["daily_units"]),
            text(&usage["total_units"])
        );
    }
    let last = &value["last_run"];
    if last.is_null() {
        out.push_str("Last collection run: none\n");
    } else {
        record(
            out,
            "Last collection run",
            last,
            &[
                ("ID", "id"),
                ("Status", "status"),
                ("Started", "started_at"),
                ("Finished", "finished_at"),
            ],
        );
        if let Some(note) = last["note"].as_str() {
            if let Ok(summary) = serde_json::from_str::<Value>(note) {
                details(out, "Summary", &summary, 0);
            } else {
                field(out, "Note", &last["note"]);
            }
        }
    }
    details(out, "Sources needing attention", &value["source_health"], 0);
    details(out, "Queries needing attention", &value["coverage"], 0);
}
