use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "pratica",
    version,
    about = "Broker exact, evidence-bounded integration agreements"
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
    Steward {
        #[command(subcommand)]
        command: StewardCommand,
    },
    Integration {
        #[command(subcommand)]
        command: IntegrationCommand,
    },
    Track {
        #[command(subcommand)]
        command: TrackCommand,
    },
    Negotiation {
        #[command(subcommand)]
        command: NegotiationCommand,
    },
    Attempt {
        #[command(subcommand)]
        command: AttemptCommand,
    },
    Agreement {
        #[command(subcommand)]
        command: AgreementCommand,
    },
    Conformance {
        #[command(subcommand)]
        command: ConformanceCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum StewardCommand {
    Register {
        manifest: PathBuf,
    },
    List,
    Show {
        scope: String,
        #[arg(long)]
        version: Option<u32>,
    },
    Respond {
        negotiation: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum IntegrationCommand {
    Open(IntegrationOpenArgs),
    Status(IdArgs),
    Review(IdArgs),
    Report(IdArgs),
}

#[derive(Debug, Args)]
pub struct IntegrationOpenArgs {
    #[arg(long)]
    pub entrant: String,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub context: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum TrackCommand {
    Open(TrackOpenArgs),
    Retire {
        track: String,
        #[arg(long)]
        reason: String,
    },
}

#[derive(Debug, Args)]
pub struct TrackOpenArgs {
    pub integration: String,
    #[arg(long)]
    pub steward: String,
    #[arg(long)]
    pub steward_version: Option<u32>,
    #[arg(long)]
    pub terms: PathBuf,
}

#[derive(Debug, Subcommand)]
pub enum NegotiationCommand {
    Show(IdArgs),
    History(IdArgs),
    Propose {
        negotiation: String,
        #[arg(long)]
        base: String,
        #[arg(long)]
        terms: PathBuf,
    },
    Assent {
        negotiation: String,
        #[arg(long)]
        offer: String,
    },
    Withdraw {
        negotiation: String,
        #[arg(long)]
        offer: String,
    },
    Cancel {
        negotiation: String,
        #[arg(long)]
        reason: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum AttemptCommand {
    Show(IdArgs),
    Retry(IdArgs),
}

#[derive(Debug, Subcommand)]
pub enum AgreementCommand {
    Show(IdArgs),
    Export {
        agreement: String,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Verify(IdArgs),
    Amend {
        agreement: String,
        #[arg(long)]
        terms: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConformanceCommand {
    Review {
        agreement: String,
        #[arg(long)]
        candidate_basis: PathBuf,
    },
    Show(IdArgs),
}

#[derive(Debug, Args)]
pub struct IdArgs {
    pub id: String,
}
