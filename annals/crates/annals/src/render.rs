use std::fmt::Write as _;

use serde::Serialize;
use serde_json::{Value, json};

use crate::error::AppError;

#[derive(Debug)]
pub struct CommandOutput {
    pub data: Value,
    pub human: String,
    pub diagnostics: String,
    pub quietable: bool,
}

impl CommandOutput {
    pub fn new(data: Value, human: impl Into<String>) -> Self {
        Self {
            data,
            human: human.into(),
            diagnostics: String::new(),
            quietable: false,
        }
    }

    #[must_use]
    pub fn mutation(mut self) -> Self {
        self.quietable = true;
        self
    }
}

#[derive(Serialize)]
struct SuccessEnvelope<'a> {
    ok: bool,
    data: &'a Value,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: String,
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    ok: bool,
    error: ErrorBody<'a>,
}

pub fn success_json(data: &Value) -> Result<String, AppError> {
    serde_json::to_string(&SuccessEnvelope { ok: true, data })
        .map_err(|error| AppError::unexpected("json_serialization_failed", error.to_string()))
}

pub fn error_json(error: &AppError) -> String {
    let envelope = ErrorEnvelope {
        ok: false,
        error: ErrorBody {
            code: error.code(),
            message: error.to_string(),
        },
    };
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

/// Render user-controlled text without allowing terminal control sequences.
///
/// Single-line fields escape every control character. Full bodies may retain
/// line feeds while all other controls, including escape, remain visible data.
#[must_use]
pub fn render_terminal_text(text: &str, allow_newlines: bool) -> String {
    let mut rendered = String::with_capacity(text.len());
    for character in text.chars() {
        push_terminal_character(&mut rendered, character, allow_newlines);
    }
    rendered
}

fn push_terminal_character(rendered: &mut String, character: char, allow_newlines: bool) {
    if character == '\n' && allow_newlines {
        rendered.push(character);
    } else if character.is_control() {
        let _ = write!(rendered, "\\u{{{:x}}}", u32::from(character));
    } else {
        rendered.push(character);
    }
}

#[cfg(test)]
mod tests {
    use super::render_terminal_text;

    #[test]
    fn user_text_cannot_emit_terminal_controls() {
        let input = "title\nnext\t\u{1b}[31mred";
        assert_eq!(
            render_terminal_text(input, false),
            "title\\u{a}next\\u{9}\\u{1b}[31mred"
        );
        assert_eq!(
            render_terminal_text(input, true),
            "title\nnext\\u{9}\\u{1b}[31mred"
        );
    }
}
