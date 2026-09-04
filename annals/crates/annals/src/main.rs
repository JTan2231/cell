mod app;
mod change;
mod cli;
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
mod liaison;
mod model;
mod model_runner;
mod reconciliation_draft;
mod render;
mod resolver;
mod revision_store;
mod tool_server;

use std::ffi::OsStr;

use clap::Parser;

use crate::cli::Cli;
use crate::error::AppError;

fn main() {
    let exit_code = run_main();
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

fn run_main() -> i32 {
    let json_requested = std::env::args_os().any(|argument| argument == OsStr::new("--json"));
    let cli = match Cli::try_parse() {
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
    let config = match config::Config::load(cli.config.as_ref()) {
        Ok(config) => config,
        Err(error) => {
            if cli.json {
                eprintln!("{}", render::error_json(&error));
            } else {
                eprintln!("annals: {error}");
            }
            return error.exit_code();
        }
    };
    let path = match app::selected_library_path(&cli, &config) {
        Ok(path) => path,
        Err(error) => {
            if cli.json {
                eprintln!("{}", render::error_json(&error));
            } else {
                eprintln!("annals: {error}");
            }
            return error.exit_code();
        }
    };
    if cli.verbose > 0 && !cli.json {
        eprintln!("annals: library {}", path.display());
    }
    match app::run(&cli, &config, &path) {
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
            if cli.json {
                eprintln!("{}", render::error_json(&error));
            } else {
                eprintln!("annals: {error} (library: {})", path.display());
            }
            error.exit_code()
        }
    }
}
