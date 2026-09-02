mod app;
mod capture;
mod cli;
mod error;
mod model;
mod projection;
mod store;

use std::ffi::OsStr;

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

/// Parse and execute the Geste command line, returning its process exit status.
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
                    eprintln!("geste: {print_error}");
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
                eprintln!("geste: {print_error}");
                return 1;
            }
            return exit_code;
        }
    };

    match app::run(&cli) {
        Ok(output) => {
            if cli.json {
                if let Ok(json) = success_json(&output.data) {
                    println!("{json}");
                } else {
                    let error = AppError::new(
                        "json_serialization_failed",
                        "unable to serialize command output",
                    );
                    eprintln!("{}", error_json(&error));
                    return error.exit_code();
                }
            } else {
                println!("{}", output.human);
            }
            0
        }
        Err(error) => {
            if cli.json {
                eprintln!("{}", error_json(&error));
            } else {
                eprintln!("geste: {error}");
            }
            error.exit_code()
        }
    }
}
