mod app;
mod cli;
mod error;
mod model;
mod registry;
mod render;

use std::ffi::OsStr;

use clap::Parser as _;

use crate::cli::Cli;

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
                    eprintln!("chancery: {print_error}");
                    return 1;
                }
                return 0;
            }
            if json_requested {
                let error = crate::error::AppError::usage(error.to_string());
                eprintln!("{}", crate::render::error_json(&error));
                return error.exit_code();
            }
            let exit_code = error.exit_code();
            if let Err(print_error) = error.print() {
                eprintln!("chancery: {print_error}");
                return 1;
            }
            return exit_code;
        }
    };

    match crate::app::run(&cli) {
        Ok(output) => {
            if cli.json {
                match crate::render::output_json(&output) {
                    Ok(json) => println!("{json}"),
                    Err(error) => {
                        eprintln!("{}", crate::render::error_json(&error));
                        return error.exit_code();
                    }
                }
            } else {
                println!("{}", output.human);
            }
            output.exit_code
        }
        Err(error) => {
            if cli.json {
                eprintln!("{}", crate::render::error_json(&error));
            } else {
                eprintln!("chancery: {error}");
            }
            error.exit_code()
        }
    }
}
