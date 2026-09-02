use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "geste",
    version,
    about = "Maintain and search a manual local episode casebook"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub database: Option<PathBuf>,
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Init,
    Doctor,
    Search(SearchArgs),
    Episode {
        #[command(subcommand)]
        command: EpisodeCommand,
    },
    Report(ReadArgs),
    Graph(ReadArgs),
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    pub query: String,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Debug, Subcommand)]
pub enum EpisodeCommand {
    Create {
        input: PathBuf,
    },
    Revise {
        episode: String,
        input: PathBuf,
        #[arg(long, required = true)]
        base: u32,
    },
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Show(ReadArgs),
}

#[derive(Debug, Args)]
pub struct ReadArgs {
    pub episode: String,
    #[arg(long)]
    pub at: Option<u32>,
}
