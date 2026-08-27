use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nucleus_daemon::{DaemonError, ServeConfig, resolve_codex, serve, standard_paths};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "nucleusd",
    version,
    about = "Local Nucleus agent-job coordinator"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Serve the v1 HTTP API on a per-user Unix socket.
    Serve {
        /// Unix-domain socket path.
        #[arg(long, env = "NUCLEUS_SOCKET")]
        socket: Option<PathBuf>,
        /// Durable database path.
        #[arg(long, env = "NUCLEUS_DATABASE")]
        database: Option<PathBuf>,
        /// Exact Codex executable used for inspection and execution.
        #[arg(long, env = "NUCLEUS_CODEX")]
        codex: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("nucleus=info")),
        )
        .init();
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nucleusd: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), DaemonError> {
    match cli.command {
        Command::Serve {
            socket,
            database,
            codex,
        } => {
            let standard = standard_paths()?;
            serve(ServeConfig {
                socket: socket.unwrap_or(standard.socket),
                database: database.unwrap_or(standard.database),
                codex: resolve_codex(codex)?,
            })
            .await
        }
    }
}
