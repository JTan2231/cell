use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use iatreion::{CollectionOptions, collect, render};

#[derive(Parser)]
#[command(version, about = "Report Cell operational status without changing it")]
struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "DIRECTORY",
        help = "Directory containing installed product command selectors"
    )]
    command_dir: Option<PathBuf>,
    #[arg(long, global = true, default_value_t = 5_000)]
    deadline_ms: u64,
    #[arg(long, global = true, default_value_t = 2_000)]
    probe_timeout_ms: u64,
    #[arg(long, global = true, default_value_t = 8)]
    concurrency: usize,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Report every expected product and operational unit.
    Report(ReportArgs),
    /// Show one exact operational unit.
    Show(ShowArgs),
}

#[derive(Args)]
struct ReportArgs {
    #[arg(default_value = ".")]
    root: PathBuf,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    product: Option<String>,
}

#[derive(Args)]
struct ShowArgs {
    unit: String,
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long)]
    json: bool,
}

fn default_command_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME or --command-dir is required")?;
    Ok(PathBuf::from(home).join(".local/bin"))
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let (root, product, unit, json) = match cli.command {
        Command::Report(args) => (args.root, args.product, None, args.json),
        Command::Show(args) => (args.root, None, Some(args.unit), args.json),
    };
    let command_dir = match cli.command_dir.map_or_else(default_command_dir, Ok) {
        Ok(path) => path,
        Err(message) => return failure(&message, json),
    };
    let options = CollectionOptions {
        root,
        command_dir,
        product,
        unit,
        total_timeout: Duration::from_millis(cli.deadline_ms),
        probe_timeout: Duration::from_millis(cli.probe_timeout_ms),
        concurrency: cli.concurrency,
    };
    match collect(options).await {
        Ok(collection) => {
            let mut output = io::stdout().lock();
            let result = if json {
                serde_json::to_writer_pretty(&mut output, &collection.report)
                    .map_err(|error| error.to_string())
                    .and_then(|()| writeln!(output).map_err(|error| error.to_string()))
            } else {
                render::render(&collection.report, &mut output).map_err(|error| error.to_string())
            };
            if let Err(message) = result {
                return failure(&message, json);
            }
            ExitCode::SUCCESS
        }
        Err(message) => failure(&message, json),
    }
}

fn failure(message: &str, json: bool) -> ExitCode {
    if json {
        println!(
            "{}",
            serde_json::json!({"schema_version": 1, "error": message})
        );
    } else {
        eprintln!("iatreion: {message}");
    }
    ExitCode::from(2)
}
