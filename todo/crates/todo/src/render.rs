use std::fmt::Write as _;

use crate::api::{Data, Failure, Success};
use serde_json::{Value, json};

use crate::error::AppError;

#[derive(Debug)]
pub(crate) struct CommandOutput {
    pub(crate) data: Value,
    pub(crate) human: String,
    pub(crate) diagnostics: String,
    pub(crate) quietable: bool,
}

impl CommandOutput {
    pub(crate) fn new(data: Value, human: impl Into<String>) -> Self {
        Self {
            data,
            human: human.into(),
            diagnostics: String::new(),
            quietable: false,
        }
    }

    #[must_use]
    pub(crate) fn mutation(mut self) -> Self {
        self.quietable = true;
        self
    }
}

pub(crate) fn success_json(data: &Value) -> Result<String, AppError> {
    serde_json::from_value::<Data>(data.clone())
        .and_then(|data| serde_json::to_string(&Success::new(data)))
        .map_err(|error| AppError::unexpected("json_serialization_failed", error.to_string()))
}

#[must_use]
pub(crate) fn error_json(error: &AppError) -> String {
    let envelope = Failure::new(error.code(), error.to_string());
    serde_json::to_string(&envelope).unwrap_or_else(|_| {
        json!({
            "ok": false,
            "error": {
                "code": "json_serialization_failed",
                "message": "unable to serialize the error response"
            }
        })
        .to_string()
    })
}

/// Render caller- or model-controlled text without terminal control sequences.
#[must_use]
pub(crate) fn terminal_text(value: &str, allow_newlines: bool) -> String {
    let mut rendered = String::with_capacity(value.len());
    for character in value.chars() {
        if character == '\n' && allow_newlines {
            rendered.push(character);
        } else if character.is_control() {
            let _ = write!(rendered, "\\u{{{:x}}}", u32::from(character));
        } else {
            rendered.push(character);
        }
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::terminal_text;

    #[test]
    fn terminal_controls_are_escaped() {
        let value = "first\nsecond\t\u{1b}[31m";
        assert_eq!(
            terminal_text(value, false),
            "first\\u{a}second\\u{9}\\u{1b}[31m"
        );
        assert_eq!(terminal_text(value, true), "first\nsecond\\u{9}\\u{1b}[31m");
    }
}
