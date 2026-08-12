mod app;
mod cli;
mod db;
mod error;
mod index;
mod ingest;
mod model;
mod render;
mod search;
mod tree;
mod validate;

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
    let path = app::library_path(cli.library.as_ref());
    if cli.verbose > 0 && !cli.json {
        eprintln!("annals: library {}", path.display());
    }
    match app::run(&cli, &path) {
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
