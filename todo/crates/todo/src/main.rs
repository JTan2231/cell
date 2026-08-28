mod app;
mod cli;
mod config;
mod db;
mod email;
mod error;
mod liaison;
mod model;
mod model_runner;
mod render;
mod todo_store;
mod tool_server;

use std::ffi::OsStr;

use clap::Parser as _;

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
                    eprintln!("todo: {print_error}");
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
                eprintln!("todo: {print_error}");
                return 1;
            }
            return exit_code;
        }
    };

    let config = match config::Config::load(cli.config.as_ref()) {
        Ok(config) => config,
        Err(error) => return render_early_error(&error, cli.json),
    };
    let database = match app::database_path(cli.database.as_ref(), &config) {
        Ok(database) => database,
        Err(error) => return render_early_error(&error, cli.json),
    };
    if cli.verbose > 0 && !cli.json {
        eprintln!("todo: database {}", database.display());
    }

    match app::run(&cli, &config, &database) {
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
                eprintln!("todo: {error} (database: {})", database.display());
            }
            error.exit_code()
        }
    }
}

fn render_early_error(error: &AppError, json: bool) -> i32 {
    if json {
        eprintln!("{}", render::error_json(error));
    } else {
        eprintln!("todo: {error}");
    }
    error.exit_code()
}
