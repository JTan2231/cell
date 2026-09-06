//! Krisis-owned lifecycle; artifact and selector transactions are shared.

#[path = "krisis-install/adapter.rs"]
mod adapter;
#[path = "krisis-install/legacy.rs"]
mod legacy;
#[path = "krisis-install/lifecycle.rs"]
mod lifecycle;
#[path = "krisis-install/package.rs"]
mod package;
#[path = "krisis-install/support.rs"]
mod support;

use cell_install::{Error, Result};
use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "krisis-install",
    version,
    about = "Install and operate the owned Krisis release"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Install(Install),
    Uninstall(Control),
    Inspect(Home),
    VerifyRelease {
        release: PathBuf,
    },
    #[command(hide = true)]
    Adapter {
        operation: cell_install::adapter::Operation,
    },
}

#[derive(Clone, Args)]
struct Home {
    #[arg(long)]
    home: Option<PathBuf>,
}

#[derive(Clone, Args)]
struct Control {
    #[arg(long)]
    home: Option<PathBuf>,
    #[arg(long)]
    clockwork: Option<PathBuf>,
    #[arg(long, default_value = "/bin/launchctl")]
    launchctl: PathBuf,
}

#[derive(Clone, Args)]
struct Install {
    #[arg(long)]
    binary: PathBuf,
    /// Product source directory; retained package/install defaults to its own assets.
    #[arg(long)]
    source_root: Option<PathBuf>,
    #[arg(long)]
    home: Option<PathBuf>,
    #[arg(long)]
    codex: PathBuf,
    #[arg(long)]
    annals: PathBuf,
    #[arg(long)]
    annals_config: PathBuf,
    #[arg(long)]
    annals_library_id: String,
    #[arg(long)]
    clockwork: PathBuf,
    #[arg(long, default_value = "/bin/launchctl")]
    launchctl: PathBuf,
    #[arg(long, conflicts_with = "release_maintenance")]
    final_cutover: bool,
    #[arg(long, requires = "final_cutover")]
    keep_maintenance: bool,
    #[arg(long, conflicts_with_all = ["final_cutover", "keep_maintenance"])]
    release_maintenance: bool,
    #[arg(long)]
    expected_current: Option<String>,
}

fn home(value: Option<PathBuf>) -> Result<PathBuf> {
    value
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .ok_or_else(|| Error::new("operator home is unavailable"))
}

fn dispatch(command: Command) -> Result<Value> {
    match command {
        Command::Install(options) => lifecycle::install(&options, None),
        Command::Uninstall(options) => lifecycle::uninstall(&options),
        Command::Inspect(options) => {
            let paths = support::Paths::new(home(options.home)?)?;
            let snapshot = package::inspect(&paths)?;
            Ok(json!({"ok":true,"data":support::inspect_result(snapshot.current.as_ref())}))
        }
        Command::VerifyRelease { release } => {
            let info = package::verify(&release, support::operator_uid()?)?;
            Ok(json!({"ok":true,"data":info}))
        }
        Command::Adapter { operation } => adapter::execute(operation),
    }
}

fn main() -> std::process::ExitCode {
    cell_install::adapter::finish(dispatch(Cli::parse().command))
}
