use std::fmt::Write as _;
use std::io::IsTerminal;

use serde::Serialize;
use serde_json::{Value, json};

use crate::error::AppError;

pub const HIGHLIGHT_START: char = '\u{e000}';
pub const HIGHLIGHT_END: char = '\u{e001}';

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
    pub fn with_diagnostics(mut self, diagnostics: impl Into<String>) -> Self {
        self.diagnostics = diagnostics.into();
        self
    }

    #[must_use]
    pub fn mutation(mut self) -> Self {
        self.quietable = true;
        self
    }
}

#[derive(Serialize)]
struct SuccessEnvelope<'a> {
    format_version: u8,
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
    format_version: u8,
    ok: bool,
    error: ErrorBody<'a>,
}

pub fn success_json(data: &Value) -> Result<String, AppError> {
    serde_json::to_string(&SuccessEnvelope {
        format_version: 1,
        ok: true,
        data,
    })
    .map_err(|error| AppError::unexpected("json_serialization_failed", error.to_string()))
}

pub fn error_json(error: &AppError) -> String {
    let envelope = ErrorEnvelope {
        format_version: 1,
        ok: false,
        error: ErrorBody {
            code: error.code(),
            message: error.to_string(),
        },
    };
    serde_json::to_string(&envelope).unwrap_or_else(|_| {
        json!({
            "format_version": 1,
            "ok": false,
            "error": {
                "code": "json_serialization_failed",
                "message": "unable to serialize the error response"
            }
        })
        .to_string()
    })
}

#[must_use]
pub fn color_enabled(no_color: bool) -> bool {
    !no_color && std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

#[must_use]
#[allow(clippy::format_push_string)]
pub fn render_snippet(snippet: &str, color: bool) -> String {
    let mut rendered = String::with_capacity(snippet.len());
    for character in snippet.chars() {
        match character {
            HIGHLIGHT_START if color => rendered.push_str("\u{1b}[1;33m"),
            HIGHLIGHT_END if color => rendered.push_str("\u{1b}[0m"),
            HIGHLIGHT_START | HIGHLIGHT_END => {}
            other => push_terminal_character(&mut rendered, other, false),
        }
    }
    rendered
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
    use super::{HIGHLIGHT_END, HIGHLIGHT_START, render_snippet, render_terminal_text};

    #[test]
    fn strips_markers_and_escapes_controls_without_color() {
        let input = format!("a{HIGHLIGHT_START}hit{HIGHLIGHT_END}\u{7}");
        assert_eq!(render_snippet(&input, false), "ahit\\u{7}");
    }

    #[test]
    fn renders_ansi_highlights_when_enabled() {
        let input = format!("{HIGHLIGHT_START}hit{HIGHLIGHT_END}");
        assert_eq!(render_snippet(&input, true), "\u{1b}[1;33mhit\u{1b}[0m");
    }

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
