//! Annals-owned public data views, library reads, and typed command transport.
pub mod api;

mod app;
mod catalog;
mod change;
mod cli;
mod client;
mod config;
mod corpus;
mod db;
mod decision_feed;
mod error;
mod graph;
mod inbox;
mod inbox_retry_store;
mod index;
mod ingestion;
mod instructions;
mod liaison;
mod maintenance;
mod model;
mod model_runner;
mod reconciliation_draft;
mod render;
mod resolver;
mod revision_store;
mod tool_server;

use std::ffi::OsStr;

use crate::cli::Cli;
use crate::error::AppError;

/// Run the installed command interface and return its process exit code.
#[must_use]
pub fn run_cli() -> i32 {
    let json_requested = std::env::args_os().any(|argument| argument == OsStr::new("--json"));
    chancery_usage::cli::register_if_requested::<Cli>("annals", "");
    let (cli, command_id) =
        match chancery_usage::cli::parse_command_from::<Cli>(std::env::args_os(), "")
            .and_then(|(cli, command)| cli.resolve_named_command(command))
        {
            Ok(cli) => cli,
            Err(error) => {
                if matches!(
                    error.kind(),
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
                ) {
                    if let Err(print_error) = error.print() {
                        eprintln!("annals: {print_error}");
                        return 1;
                    }
                    return 0;
                }
                if json_requested {
                    let error = AppError::invalid("invalid_command", error.to_string());
                    eprintln!("{}", render::error_json(&error));
                    return error.exit_code();
                }
                let exit_code = error.exit_code();
                if let Err(print_error) = error.print() {
                    eprintln!("annals: {print_error}");
                    return 1;
                }
                return exit_code;
            }
        };
    if let Some(command_id) = command_id {
        chancery_usage::observe("annals", &command_id);
    }
    match app::execute(&cli) {
        Ok(output) => {
            if cli.json {
                match render::success_json(&output.data) {
                    Ok(json) => {
                        if !output.diagnostics.is_empty() {
                            eprintln!("{}", output.diagnostics);
                        }
                        println!("{json}");
                    }
                    Err(error) => {
                        eprintln!("{}", render::error_json(&error));
                        return error.exit_code();
                    }
                }
            } else {
                if !output.diagnostics.is_empty() {
                    eprintln!("{}", output.diagnostics);
                }
                if !(output.human.is_empty() || cli.quiet && output.quietable) {
                    println!("{}", output.human);
                }
            }
            0
        }
        Err(error) => {
            if matches!(error.code(), "quota_deferred" | "quota_exhausted") {
                println!(
                    "{}",
                    serde_json::json!({"ok":true,"data":{"outcome":error.code()}})
                );
                return 0;
            }
            if cli.json {
                eprintln!("{}", render::error_json(&error));
            } else {
                eprintln!("annals: {error}");
            }
            error.exit_code()
        }
    }
}
