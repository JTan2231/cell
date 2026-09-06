//! Semantics owns scheduler, database and admission recovery; cell-install owns files.
#![allow(clippy::too_many_lines, clippy::missing_errors_doc)]

#[path = "semantics-install/adapter.rs"]
mod adapter;
#[path = "semantics-install/lifecycle.rs"]
mod lifecycle;

use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::ExitCode;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn fail<T>(message: &str) -> Result<T> {
    Err(message.into())
}

#[derive(Parser)]
#[command(
    name = "semantics-install",
    version,
    about = "Install, inspect and recover Semantics"
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    Install(Candidate),
    Inspect(HomeArgs),
    Verify(Candidate),
    VerifyRelease {
        release: PathBuf,
    },
    Uninstall(HomeArgs),
    /// Recover the exact private backup of an interrupted installer transaction.
    Recover {
        #[arg(long)]
        transaction: PathBuf,
        #[arg(long)]
        forward: bool,
        #[command(flatten)]
        home: HomeArgs,
    },
    #[command(hide = true)]
    Adapter {
        operation: String,
    },
}

#[derive(Args)]
struct HomeArgs {
    #[arg(long, env = "HOME")]
    home: PathBuf,
    #[arg(long)]
    clockwork: Option<PathBuf>,
    #[arg(long, default_value = "/bin/launchctl")]
    launchctl: PathBuf,
}

#[derive(Args)]
struct Candidate {
    #[arg(long)]
    binary: PathBuf,
    #[arg(long)]
    bundle: PathBuf,
    #[command(flatten)]
    home: HomeArgs,
    #[arg(long)]
    expected_current: Option<String>,
    #[arg(long, requires = "keep_maintenance")]
    final_decisions_watermark: Option<String>,
    #[arg(long)]
    keep_maintenance: bool,
}

fn run(action: Action) -> Result<Value> {
    match action {
        Action::Adapter { operation } => adapter::run(&operation),
        Action::VerifyRelease { release } => {
            Ok(json!({"ok":true,"data":lifecycle::verify_release(&release)?}))
        }
        Action::Install(args) => Ok(json!({"ok":true,"data":lifecycle::install(&args)?})),
        Action::Verify(args) => Ok(json!({"ok":true,"data":lifecycle::verify(&args)?})),
        Action::Inspect(home) => Ok(json!({"ok":true,"data":lifecycle::inspect(&home)?})),
        Action::Uninstall(home) => {
            lifecycle::uninstall(&home)?;
            Ok(json!({"ok":true}))
        }
        Action::Recover {
            transaction,
            home,
            forward,
        } => {
            lifecycle::recover(&home, &transaction, forward)?;
            Ok(json!({"ok":true}))
        }
    }
}

fn main() -> ExitCode {
    rustix::process::umask(rustix::fs::Mode::from_bits_truncate(0o077));
    let cli = Cli::parse();
    let adapter = matches!(cli.command, Action::Adapter { .. });
    match run(cli.command) {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let value = if adapter {
                json!({"schema":1,"status":"stopped","detail":error.to_string(),"data":{}})
            } else {
                json!({"ok":false,"error":{"code":"installation_failed","detail":error.to_string()}})
            };
            if adapter {
                println!("{value}");
            } else {
                eprintln!("{value}");
            }
            ExitCode::FAILURE
        }
    }
}
