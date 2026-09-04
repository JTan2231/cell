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
    Migrate(MigrationArgs),
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
        #[arg(long)]
        source_root: Option<PathBuf>,
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
    List,
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
    #[arg(long)]
    pub request_key: Option<String>,
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
    #[arg(long)]
    pub request_key: Option<String>,
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
        #[arg(long)]
        request_key: Option<String>,
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
    List,
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
        #[arg(long)]
        request_key: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConformanceCommand {
    Review {
        agreement: String,
        #[arg(long)]
        candidate_basis: PathBuf,
        #[arg(long)]
        source_root: Option<PathBuf>,
        #[arg(long)]
        request_key: Option<String>,
    },
    Show(IdArgs),
}

#[derive(Debug, Args)]
pub struct IdArgs {
    pub id: String,
}

#[derive(Debug, Args)]
pub struct MigrationArgs {
    #[arg(long)]
    pub backup: PathBuf,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use clap::Parser as _;

    use super::{Cli, Command, ConformanceCommand, StewardCommand};

    #[test]
    fn manifest_commands_parse_standard_input_with_an_explicit_source_root() {
        let steward = Cli::try_parse_from([
            "pratica",
            "steward",
            "register",
            "-",
            "--source-root",
            "/tmp/pratica-source-root",
        ])
        .expect("steward registration arguments");
        let Command::Steward {
            command:
                StewardCommand::Register {
                    manifest,
                    source_root,
                },
        } = steward.command
        else {
            panic!("expected steward registration");
        };
        assert_eq!(manifest, Path::new("-"));
        assert_eq!(
            source_root.as_deref(),
            Some(Path::new("/tmp/pratica-source-root"))
        );

        let conformance = Cli::try_parse_from([
            "pratica",
            "conformance",
            "review",
            "agreement-1",
            "--candidate-basis",
            "-",
            "--source-root",
            "/tmp/pratica-candidate-root",
        ])
        .expect("conformance review arguments");
        let Command::Conformance {
            command:
                ConformanceCommand::Review {
                    agreement,
                    candidate_basis,
                    source_root,
                    request_key,
                },
        } = conformance.command
        else {
            panic!("expected conformance review");
        };
        assert_eq!(agreement, "agreement-1");
        assert_eq!(candidate_basis, Path::new("-"));
        assert_eq!(
            source_root.as_deref(),
            Some(Path::new("/tmp/pratica-candidate-root"))
        );
        assert!(request_key.is_none());
    }
}
