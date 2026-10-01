use std::fmt::Write as _;

use serde_json::Value;

use super::{Command, EmailCommand, InstructionsCommand, ReadCommand, WantCommand};

pub(super) fn render(command: &Command, data: &Value) -> String {
    let mut output = String::new();
    match command {
        Command::Email(EmailCommand::Preview { .. }) => {
            writeln!(
                output,
                "From: {}\nTo: {}\nSubject: {}\n\n{}",
                text(&data["from"]),
                text(&data["to"]),
                text(&data["digest"]["subject"]),
                text(&data["digest"]["body"])
            )
            .expect("write to string");
        }
        Command::Want(WantCommand::List { .. }) | Command::Decision(ReadCommand::List { .. }) => {
            list(&mut output, data)
        }
        Command::Want(WantCommand::Add(_)) => {
            output.push_str("Want captured locally.\n");
            details(&mut output, "Record", &data["record"], 0);
        }
        Command::Want(WantCommand::Archive { .. } | WantCommand::Unarchive { .. }) => {
            let state = text(&data["record"]["state"]);
            writeln!(
                output,
                "Want {}: {state}{}.",
                text(&data["record"]["id"]),
                if data["changed"] == false {
                    " (unchanged)"
                } else {
                    ""
                }
            )
            .expect("write to string");
            details(&mut output, "Record", &data["record"], 0);
        }
        Command::Email(EmailCommand::Send { .. }) => {
            if let Some(reason) = data["skipped"].as_str() {
                writeln!(output, "Email skipped: {}.", reason.replace('_', " "))
                    .expect("write to string");
            } else {
                output.push_str("Email accepted by provider.\n");
                details(&mut output, "Receipt", data, 0);
            }
        }
        _ => {
            let heading = match command {
                Command::Init { .. } => "Conatus initialization",
                Command::Want(WantCommand::Show { .. }) => "Captured want",
                Command::Decision(ReadCommand::Show { .. }) => "Accepted decision document",
                Command::Update => "Update results",
                Command::Status => "Conatus status",
                Command::Config => "Conatus configuration",
                Command::Graph => "Annals concept graph",
                Command::History { .. } => "Annals corpus history",
                Command::Instructions(InstructionsCommand::Show) => "Selected library instructions",
                Command::Instructions(InstructionsCommand::Set { .. }) => "Instruction selection",
                Command::Retry { .. } => "Annals retry receipt",
                Command::Reexamine { .. } => "Annals re-examination receipt",
                Command::Pause | Command::Resume => "Update admission",
                Command::Maintenance { .. } => "Product maintenance",
                _ => unreachable!("other commands have dedicated text output"),
            };
            details(&mut output, heading, data, 0);
        }
    }
    output
}

fn list(output: &mut String, data: &Value) {
    let items = data["items"].as_array().map_or(&[][..], Vec::as_slice);
    writeln!(
        output,
        "{} records shown: {}",
        row_text(&data["kind"]),
        items.len()
    )
    .expect("write to string");
    if items.is_empty() {
        output.push_str("No matching records.\n");
    }
    for item in items {
        let wording = text(&item["wording"]);
        let first_line = wording.lines().next().unwrap_or_default();
        let state = item["state"]
            .as_str()
            .map_or(String::new(), |state| format!(" | {}", one_line(state)));
        writeln!(
            output,
            "{}{} | {}{}",
            row_text(&item["id"]),
            state,
            one_line(&first_line.chars().take(100).collect::<String>()),
            if wording.contains('\n') || first_line.chars().count() > 100 {
                " …"
            } else {
                ""
            }
        )
        .expect("write to string");
    }
    if data["has_more"] == true {
        output.push_str("More records are available. Increase --limit to read more.\n");
    }
}

// Annals observations keep their labels and all returned completeness and error
// fields. Source strings stay unchanged, including complete documents and quotes.
fn details(output: &mut String, label: &str, value: &Value, indent: usize) {
    let prefix = " ".repeat(indent);
    match value {
        Value::Object(fields) => {
            writeln!(output, "{prefix}{label}:").expect("write to string");
            if fields.is_empty() {
                writeln!(output, "{prefix}  None").expect("write to string");
            }
            for (key, value) in fields {
                details(output, &field_label(key), value, indent + 2);
            }
        }
        Value::Array(items) => {
            writeln!(output, "{prefix}{label}: {} items", items.len()).expect("write to string");
            for (index, item) in items.iter().enumerate() {
                details(output, &format!("Item {}", index + 1), item, indent + 2);
            }
        }
        Value::String(content) if content.contains('\n') => {
            writeln!(output, "{prefix}{label}:\n{content}").expect("write to string");
        }
        _ => {
            writeln!(output, "{prefix}{label}: {}", text(value)).expect("write to string");
        }
    }
}

fn field_label(key: &str) -> String {
    match key {
        "captured_at" => "Captured at (UTC Unix seconds)".to_owned(),
        "queued_at" => "Queued at (UTC Unix seconds)".to_owned(),
        "started_at" => "Started at (UTC Unix seconds)".to_owned(),
        "finished_at" => "Finished at (UTC Unix seconds)".to_owned(),
        _ => {
            let words = key.replace('_', " ");
            let mut chars = words.chars();
            chars.next().map_or_else(String::new, |first| {
                format!("{}{}", first.to_uppercase(), chars.as_str())
            })
        }
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(value) => if *value { "Yes" } else { "No" }.to_owned(),
        Value::Number(value) => value.to_string(),
        _ => "None".to_owned(),
    }
}

fn row_text(value: &Value) -> String {
    one_line(&text(value))
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
