use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

use crate::model_runner::ModelQuality;

/// Annals command-line arguments.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "annals",
    version,
    about = "Integrate immutable works into a grounded conceptual corpus",
    arg_required_else_help = true
)]
pub struct Cli {
    /// `SQLite` library path. Defaults to `ANNALS_LIBRARY`, then `./annals.db`.
    #[arg(long, global = true, value_name = "PATH")]
    pub library: Option<PathBuf>,

    /// Emit one JSON document.
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress successful human-oriented mutation output.
    #[arg(long, global = true)]
    pub quiet: bool,

    /// Increase diagnostic detail on standard error.
    #[arg(short = 'v', global = true, action = ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Create a new library without replacing an existing file.
    Init,
    /// Report corpus, work, proposal, history, and index statistics.
    Stats,
    /// Check canonical, historical, provenance, and index invariants.
    Validate,
    /// Create a consistent backup without replacing the destination.
    Backup(BackupArgs),
    /// Rebuild derived concept-search data.
    Reindex,
    /// Retain and inspect immutable source works.
    #[command(subcommand)]
    Work(WorkCommand),
    /// Ask the liaison to examine one work and record one proposal.
    Integrate(IntegrateArgs),
    /// Submit, inspect, validate, and apply coherent corpus changes.
    #[command(subcommand)]
    Change(ChangeCommand),
    /// Show the complete corpus at HEAD or an earlier revision.
    Show(ShowArgs),
    /// Search current concept labels and paths.
    Search(SearchArgs),
    /// Show the append-only corpus commit log.
    Log(LogArgs),
    /// Compare two corpus revisions.
    Diff(DiffArgs),
    /// Create a new commit that inverses an earlier commit.
    Revert(RevertArgs),
    /// Internal revision-scoped liaison tool server.
    #[command(name = "__liaison-server", hide = true)]
    LiaisonServer(LiaisonServerArgs),
}

#[derive(Debug, Clone, Args)]
pub struct BackupArgs {
    /// Destination database path.
    #[arg(value_name = "OUTPUT")]
    pub output: PathBuf,
}

#[derive(Debug, Clone, Subcommand)]
pub enum WorkCommand {
    /// Retain a nonempty UTF-8 work without changing the corpus revision.
    Add(WorkAddArgs),
    /// List retained works.
    List,
    /// Show one retained work's metadata and structure.
    Show(WorkShowArgs),
}

#[derive(Debug, Clone, Args)]
pub struct WorkAddArgs {
    /// UTF-8 source path, or - for standard input.
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,
    /// Human-readable work label. Defaults to the input filename.
    #[arg(long, value_name = "LABEL")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct WorkShowArgs {
    /// Exact retained-work label.
    #[arg(value_name = "LABEL")]
    pub label: String,
}

#[derive(Debug, Clone, Args)]
pub struct IntegrateArgs {
    /// New UTF-8 work to retain and examine.
    #[arg(value_name = "INPUT", required_unless_present = "work")]
    pub input: Option<PathBuf>,
    /// Examine an already-retained work by exact label.
    #[arg(long, value_name = "LABEL", conflicts_with = "input")]
    pub work: Option<String>,
    /// Label for a newly retained work. Defaults to its filename.
    #[arg(long, value_name = "LABEL", requires = "input")]
    pub name: Option<String>,
    /// Liaison quality preset.
    #[arg(long, value_enum, default_value_t)]
    pub quality: ModelQuality,
    /// Exact Codex model, overriding the model selected by --quality.
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,
    /// Apply a certain change immediately after the liaison submits it.
    #[arg(long)]
    pub apply: bool,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ChangeCommand {
    /// Submit one language-level request without invoking a model.
    Submit(ChangeSubmitArgs),
    /// Show a selected proposal or an accepted change at a corpus revision.
    Show(ChangeShowArgs),
    /// Re-resolve and validate the selected pending proposal without writing corpus state.
    Validate(ChangeSelectArgs),
    /// Atomically apply the selected pending proposal.
    Apply(ChangeSelectArgs),
    /// List proposal and no-change examination records.
    List,
}

#[derive(Debug, Clone, Args)]
pub struct ChangeShowArgs {
    /// Select the latest applicable record for this exact work label.
    #[arg(long, value_name = "LABEL", conflicts_with = "at")]
    pub work: Option<String>,
    /// Show the accepted change recorded at this corpus revision.
    #[arg(long, value_name = "REVISION", conflicts_with = "work")]
    pub at: Option<i64>,
}

#[derive(Debug, Clone, Args)]
pub struct ChangeSubmitArgs {
    /// UTF-8 change-request JSON path, or - for standard input.
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,
    /// Exact retained-work label providing evidence for this request.
    #[arg(long, value_name = "LABEL")]
    pub work: String,
    /// Corpus revision examined by the submitter.
    #[arg(long, value_name = "REVISION")]
    pub base: i64,
}

#[derive(Debug, Clone, Args)]
pub struct ChangeSelectArgs {
    /// Select the latest applicable record for this exact work label.
    #[arg(long, value_name = "LABEL")]
    pub work: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ShowArgs {
    /// Historical corpus revision. Defaults to HEAD.
    #[arg(long, value_name = "REVISION")]
    pub at: Option<i64>,
}

#[derive(Debug, Clone, Args)]
pub struct SearchArgs {
    /// Plain-language concept query.
    #[arg(value_name = "QUERY")]
    pub query: String,
    /// Maximum number of results.
    #[arg(long, value_name = "N", default_value_t = 10)]
    pub limit: usize,
}

#[derive(Debug, Clone, Args)]
pub struct LogArgs {
    /// Maximum number of newest commits.
    #[arg(long, value_name = "N", default_value_t = 20)]
    pub limit: usize,
}

#[derive(Debug, Clone, Args)]
pub struct DiffArgs {
    /// Earlier revision.
    #[arg(value_name = "FROM")]
    pub from: i64,
    /// Later revision.
    #[arg(value_name = "TO")]
    pub to: i64,
}

#[derive(Debug, Clone, Args)]
pub struct RevertArgs {
    /// Revision whose transition should be inversed.
    #[arg(value_name = "REVISION")]
    pub revision: i64,
}

#[derive(Debug, Clone, Args)]
pub struct LiaisonServerArgs {
    /// Opaque model-run scope token.
    #[arg(value_name = "TOKEN")]
    pub token: String,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{ChangeCommand, Cli, Command, WorkCommand};
    use crate::model_runner::ModelQuality;

    #[test]
    fn language_first_commands_parse() {
        let work = Cli::try_parse_from(["annals", "work", "show", "Serializable execution"]);
        assert!(matches!(
            work.map(|cli| cli.command),
            Ok(Command::Work(WorkCommand::Show(_)))
        ));

        let change = Cli::try_parse_from([
            "annals",
            "change",
            "submit",
            "request.json",
            "--work",
            "Paper",
            "--base",
            "17",
        ]);
        assert!(matches!(
            change.map(|cli| cli.command),
            Ok(Command::Change(ChangeCommand::Submit(_)))
        ));
    }

    #[test]
    fn integrate_requires_new_input_or_existing_work() {
        assert!(Cli::try_parse_from(["annals", "integrate"]).is_err());
        assert!(Cli::try_parse_from(["annals", "integrate", "paper.txt"]).is_ok());
        assert!(Cli::try_parse_from(["annals", "integrate", "--work", "Existing paper"]).is_ok());
    }

    #[test]
    fn integrate_model_settings_parse() {
        let default = Cli::try_parse_from(["annals", "integrate", "paper.txt"]);
        let Ok(Command::Integrate(default)) = default.map(|cli| cli.command) else {
            panic!("default integrate arguments did not parse");
        };
        assert_eq!(default.quality, ModelQuality::High);
        assert_eq!(default.model, None);

        for (quality, expected) in [
            ("low", ModelQuality::Low),
            ("medium", ModelQuality::Medium),
            ("high", ModelQuality::High),
        ] {
            let parsed = Cli::try_parse_from([
                "annals",
                "integrate",
                "paper.txt",
                "--quality",
                quality,
                "--model",
                "custom-model",
            ]);
            let Ok(Command::Integrate(args)) = parsed.map(|cli| cli.command) else {
                panic!("integrate arguments for {quality} did not parse");
            };
            assert_eq!(args.quality, expected);
            assert_eq!(args.model.as_deref(), Some("custom-model"));
        }

        assert!(
            Cli::try_parse_from(["annals", "integrate", "paper.txt", "--quality", "invalid"])
                .is_err()
        );
    }
}
