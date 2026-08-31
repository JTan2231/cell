use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::model::{EntryKind, Mode};

/// Inspect installed local capability and operation contracts.
#[derive(Debug, Parser)]
#[command(
    name = "chancery",
    version,
    about = "Discover installed local capabilities and adaptive operations",
    long_about = "Read and validate installed, versioned provider contracts. Chancery never executes a documented interface, probes readiness, contacts a model, or changes provider state.",
    arg_required_else_help = true
)]
pub(crate) struct Cli {
    /// Provider registry directory. Overrides `CHANCERY_REGISTRY`.
    #[arg(
        long,
        global = true,
        env = "CHANCERY_REGISTRY",
        value_name = "ABSOLUTE_PATH"
    )]
    pub(crate) registry: Option<PathBuf>,

    /// Emit one versioned JSON document.
    #[arg(long, global = true)]
    pub(crate) json: bool,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// List installed contracts.
    List(ListArgs),
    /// Show one complete installed contract.
    Show(ShowArgs),
    /// Validate the installed provider registry and dependencies.
    Doctor,
    /// Validate one standalone provider bundle.
    Validate(ValidateArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ListArgs {
    /// Restrict results to one work mode.
    #[arg(long, value_enum)]
    pub(crate) mode: Option<Mode>,

    /// Restrict results to capabilities or operations.
    #[arg(long, value_enum)]
    pub(crate) kind: Option<EntryKind>,
}

#[derive(Debug, Args)]
pub(crate) struct ShowArgs {
    /// Stable capability or operation ID.
    #[arg(value_name = "ID")]
    pub(crate) id: String,
}

#[derive(Debug, Args)]
pub(crate) struct ValidateArgs {
    /// Standalone provider bundle directory.
    #[arg(value_name = "BUNDLE")]
    pub(crate) bundle: PathBuf,
}

impl ValueEnum for Mode {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Use, Self::Operate, Self::Develop]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        Some(clap::builder::PossibleValue::new(self.as_str()))
    }
}

impl ValueEnum for EntryKind {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Capability, Self::Operation]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        Some(clap::builder::PossibleValue::new(self.as_str()))
    }
}
