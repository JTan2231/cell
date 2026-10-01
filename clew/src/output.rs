use std::fmt::Write as _;

use serde_json::Value;

use super::{Command, EmailCommand};

pub(super) fn render(command: &Command, data: &Value) -> String {
    let mut output = String::new();
    match command {
        Command::Init => {
            output.push_str("Ledger initialized.\n");
            field(&mut output, "Schema", &data["schema_version"]);
        }
        Command::Doctor => {
            output.push_str("Ledger integrity: OK\n");
            field(&mut output, "Schema", &data["schema_version"]);
            field(&mut output, "Retained entries", &data["entries"]);
        }
        Command::Find { .. } => candidates(&mut output, data),
        Command::Search { .. } => {
            output.push_str("Ledger matches\n");
            let entries = data.as_array().map_or(&[][..], Vec::as_slice);
            if entries.is_empty() {
                output.push_str("No matching entries.\n");
            }
            for item in entries {
                entry_row(&mut output, item);
            }
        }
        Command::List => {
            output.push_str("Tracked applications\n");
            let applications = data.as_array().map_or(&[][..], Vec::as_slice);
            if applications.is_empty() {
                output.push_str("No tracked applications.\n");
            }
            for application in applications {
                let _ = writeln!(
                    output,
                    "{} | Status: {} | Latest entry: {}",
                    row_text(&application["cast_job_id"]),
                    row_text(&application["status"]),
                    row_text(&application["latest_entry"]["id"])
                );
            }
        }
        Command::Entry { .. } | Command::Record { .. } | Command::Retract { .. } => {
            entry(&mut output, data);
        }
        Command::Thread { .. } => {
            field(&mut output, "Thread", &data["thread"]["name"]);
            field(&mut output, "Thread ID", &data["thread"]["id"]);
            field(&mut output, "Current status", &data["status"]);
            field(&mut output, "Status entry ID", &data["status_entry_id"]);
            history(&mut output, &data["history"]);
        }
        Command::Show { .. } => {
            field(&mut output, "Cast job", &data["cast_job_id"]);
            field(&mut output, "Current status", &data["current"]["status"]);
            field(
                &mut output,
                "Status entry ID",
                &data["current"]["status_entry_id"],
            );
            history(&mut output, &data["history"]);
        }
        Command::Email(EmailCommand::Preview { .. }) => {
            let _ = writeln!(
                output,
                "From: {}\nTo: {}\nSubject: {}\n\n{}",
                scalar(&data["from"]),
                scalar(&data["to"]),
                scalar(&data["digest"]["subject"]),
                scalar(&data["digest"]["body"])
            );
        }
        Command::Email(EmailCommand::Send { .. }) => {
            if let Some(reason) = data["skipped"].as_str() {
                let _ = writeln!(output, "Email skipped: {}.", reason.replace('_', " "));
            } else {
                output.push_str("Email accepted by provider.\n");
                for (label, key) in [
                    ("Occurrence", "occurrence"),
                    ("Acceptance ID", "accepted_id"),
                    ("Already accepted", "already_accepted"),
                    ("Applications", "application_count"),
                ] {
                    if let Some(value) = data.get(key) {
                        field(&mut output, label, value);
                    }
                }
            }
        }
    }
    output
}

fn candidates(output: &mut String, data: &Value) {
    output.push_str("Retained job candidates\n");
    let candidates = data["candidates"].as_array().map_or(&[][..], Vec::as_slice);
    if candidates.is_empty() {
        output.push_str("No matching retained jobs.\n");
    }
    for candidate in candidates {
        let _ = writeln!(
            output,
            "{} | {} | {} | Tracked: {}",
            row_text(&candidate["cast_job_id"]),
            row_text(&candidate["company"]),
            row_text(&candidate["title"]),
            row_text(&candidate["tracked"])
        );
        for url in candidate["urls"].as_array().into_iter().flatten() {
            let _ = writeln!(output, "  URL: {}", row_text(url));
        }
    }
    for reference in data["retained_references_without_cast_record"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let _ = writeln!(
            output,
            "Retained reference without Cast record: {}",
            row_text(reference)
        );
    }
    field(output, "Complete", &data["complete"]);
}

fn entry_row(output: &mut String, entry: &Value) {
    let notes = scalar(&entry["notes"]);
    let preview = notes.lines().next().unwrap_or_default();
    let references = entry["references"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|reference| {
            format!(
                "{} {} ({})",
                row_text(&reference["namespace"]),
                row_text(&reference["external_id"]),
                row_text(&reference["role"])
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(
        output,
        "{} | {} | Status: {} | Thread: {} | Superseded by: {} | References: {} | {}{}",
        row_text(&entry["id"]),
        row_text(&entry["kind"]),
        row_text(&entry["status"]),
        row_text(&entry["thread"]["name"]),
        row_text(&entry["superseded_by"]),
        if references.is_empty() {
            "None"
        } else {
            &references
        },
        one_line(&preview.chars().take(100).collect::<String>()),
        if notes.contains('\n') || preview.chars().count() > 100 {
            " …"
        } else {
            ""
        }
    );
}

fn history(output: &mut String, history: &Value) {
    output.push_str("\nHistory\n");
    let entries = history.as_array().map_or(&[][..], Vec::as_slice);
    if entries.is_empty() {
        output.push_str("No retained entries.\n");
    }
    for item in entries {
        entry(output, item);
        output.push('\n');
    }
}

fn entry(output: &mut String, entry: &Value) {
    for (label, key) in [
        ("Entry", "id"),
        ("Sequence", "sequence"),
        ("Recorded at (UTC)", "recorded_at"),
        ("Kind", "kind"),
        ("Status", "status"),
        ("Replaces", "replaces"),
        ("Superseded by", "superseded_by"),
    ] {
        if let Some(value) = entry.get(key) {
            field(output, label, value);
        }
    }
    field(output, "Thread", &entry["thread"]["name"]);
    field(output, "Thread ID", &entry["thread"]["id"]);
    for reference in entry["references"].as_array().into_iter().flatten() {
        let _ = writeln!(
            output,
            "Reference: {} {} ({})",
            scalar(&reference["namespace"]),
            scalar(&reference["external_id"]),
            scalar(&reference["role"])
        );
    }
    if let Some(notes) = entry["notes"].as_str() {
        let _ = writeln!(output, "Notes:\n{notes}");
    } else {
        output.push_str("Notes: None\n");
    }
}

fn field(output: &mut String, label: &str, value: &Value) {
    let _ = writeln!(output, "{label}: {}", scalar(value));
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(value) => if *value { "Yes" } else { "No" }.to_owned(),
        Value::Number(value) => value.to_string(),
        _ => "None".to_owned(),
    }
}

fn row_text(value: &Value) -> String {
    one_line(&scalar(value))
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
