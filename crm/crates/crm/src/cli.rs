use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::model::Stage;

#[derive(Debug, Parser)]
#[command(
    name = "crm",
    version,
    about = "Maintain a private, stewarded relationship case library"
)]
pub struct Cli {
    #[arg(long, env = "CRM_DATABASE", global = true)]
    pub database: Option<PathBuf>,
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Maintenance {
        #[command(subcommand)]
        command: MaintenanceCommand,
    },
    Init,
    Migrate {
        #[arg(long)]
        backup: PathBuf,
    },
    Doctor,
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    Case {
        #[command(subcommand)]
        command: CaseCommand,
    },
    Search(SearchArgs),
    Tell(TellArgs),
    Update {
        #[command(subcommand)]
        command: UpdateCommand,
    },
    #[command(name = "_worker", hide = true)]
    Worker {
        #[command(subcommand)]
        command: WorkerCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    New {
        #[arg(long)]
        title: String,
        input: PathBuf,
    },
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Show {
        entry: String,
    },
    Update {
        entry: String,
        #[arg(long)]
        title: String,
        input: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum CaseCommand {
    New {
        #[arg(long)]
        title: String,
        input: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = Stage::Research)]
        stage: Stage,
    },
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Show {
        case: String,
        #[arg(long)]
        revision: Option<u64>,
    },
    History {
        case: String,
    },
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    pub query: String,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct TellArgs {
    pub case: String,
    pub input: PathBuf,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub source: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum UpdateCommand {
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Show {
        update: String,
    },
    Wait {
        update: String,
        #[arg(long, default_value_t = 1200)]
        timeout: u64,
    },
    Resume {
        update: String,
    },
    Retry {
        update: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum WorkerCommand {
    Drain,
    Resume { update: String },
}

#[derive(Debug, Subcommand)]
pub enum MaintenanceCommand {
    Hold {
        run_id: String,
    },
    Status,
    Release {
        run_id: String,
    },
    /// Recover and settle already admitted work; never retries failed work.
    Drain,
}
