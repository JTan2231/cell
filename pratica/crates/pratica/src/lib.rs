#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

mod agent_contracts;
mod app;
mod cli;
mod error;
mod model;
mod nucleus;
mod source_catalog;
mod store;

use std::ffi::OsStr;
use std::io::Write as _;

use clap::Parser as _;
use serde::Serialize;

use crate::cli::Cli;
use crate::error::AppError;

pub const OUTPUT_SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct SuccessEnvelope<'a> {
    schema_version: u32,
    ok: bool,
    data: &'a serde_json::Value,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    schema_version: u32,
    ok: bool,
    error: ErrorBody<'a>,
}

fn success_json(data: &serde_json::Value) -> Result<String, serde_json::Error> {
    serde_json::to_string(&SuccessEnvelope {
        schema_version: OUTPUT_SCHEMA_VERSION,
        ok: true,
        data,
    })
}

#[must_use]
fn error_json(error: &AppError) -> String {
    serde_json::to_string(&ErrorEnvelope {
        schema_version: OUTPUT_SCHEMA_VERSION,
        ok: false,
        error: ErrorBody {
            code: error.code(),
            message: error.message(),
        },
    })
    .unwrap_or_else(|_| {
        "{\"schema_version\":1,\"ok\":false,\"error\":{\"code\":\"json_serialization_failed\",\"message\":\"unable to serialize error\"}}".to_owned()
    })
}

/// Parse and execute the Pratica command line, returning its process exit status.
#[must_use]
pub fn main_entry() -> i32 {
    let json_requested = std::env::args_os().any(|argument| argument == OsStr::new("--json"));
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                if let Err(print_error) = error.print() {
                    eprintln!("pratica: {print_error}");
                    return 1;
                }
                return 0;
            }
            if json_requested {
                let app_error = AppError::usage("usage_error", "invalid command-line arguments");
                eprintln!("{}", error_json(&app_error));
                return app_error.exit_code();
            }
            let exit_code = error.exit_code();
            if let Err(print_error) = error.print() {
                eprintln!("pratica: {print_error}");
                return 1;
            }
            return exit_code;
        }
    };

    match app::run(&cli) {
        Ok(output) => {
            if cli.json {
                match success_json(&output.data) {
                    Ok(json) => println!("{json}"),
                    Err(error) => {
                        let error = AppError::new(
                            "json_serialization_failed",
                            format!("unable to serialize command output: {error}"),
                        );
                        eprintln!("{}", error_json(&error));
                        return error.exit_code();
                    }
                }
            } else {
                let write_result = match output.human {
                    app::HumanOutput::Text(text) => writeln!(std::io::stdout().lock(), "{text}"),
                    app::HumanOutput::Exact(bytes) => std::io::stdout().lock().write_all(&bytes),
                };
                if let Err(error) = write_result {
                    let error = AppError::new(
                        "stdout_write_failed",
                        format!("unable to write command output: {error}"),
                    );
                    eprintln!("pratica: {error}");
                    return error.exit_code();
                }
            }
            0
        }
        Err(error) => {
            if cli.json {
                eprintln!("{}", error_json(&error));
            } else {
                eprintln!("pratica: {error}");
            }
            error.exit_code()
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{error_json, success_json};
    use crate::error::AppError;

    #[test]
    fn machine_envelopes_keep_the_versioned_success_and_error_shape()
    -> Result<(), Box<dyn std::error::Error>> {
        let success: serde_json::Value =
            serde_json::from_str(&success_json(&json!({"type": "probe"}))?)?;
        assert_eq!(
            success,
            json!({
                "schema_version": 1,
                "ok": true,
                "data": {"type": "probe"}
            })
        );

        let error: serde_json::Value = serde_json::from_str(&error_json(&AppError::usage(
            "probe_failed",
            "the probe failed",
        )))?;
        assert_eq!(
            error,
            json!({
                "schema_version": 1,
                "ok": false,
                "error": {"code": "probe_failed", "message": "the probe failed"}
            })
        );
        Ok(())
    }
}
