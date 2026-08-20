use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

use crate::model::ConceptId;
use crate::model_runner::ModelQuality;

/// Annals command-line arguments.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "annals",
    version,
    about = "Integrate immutable works into an evidence-grounded concept graph",
    arg_required_else_help = true
)]
pub struct Cli {
    /// Annals TOML configuration path. Defaults to `ANNALS_CONFIG` when set.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// `SQLite` library path. Defaults to nonempty `ANNALS_LIBRARY`, then config.
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
    /// Report storage, workflow, and history statistics.
    Stats,
    /// Summarize the concept graph at one revision.
    Overview(AtArgs),
    /// Page through concepts with no parents.
    Roots(PagedAtArgs),
    /// Inspect one concept and its direct graph neighborhood.
    #[command(subcommand)]
    Concept(ConceptCommand),
    /// Expand a bounded local concept graph.
    Graph(GraphArgs),
    /// Remove parent edges already implied by longer graph paths.
    Shake(ShakeArgs),
    /// Check canonical, historical, and provenance invariants.
    Validate,
    /// Create a consistent backup without replacing the destination.
    Backup(BackupArgs),
    /// Retain and inspect immutable source works.
    #[command(subcommand)]
    Work(WorkCommand),
    /// Ask the liaison to examine one work and record one reconciliation.
    Integrate(IntegrateArgs),
    /// Process and inspect the configured filesystem inbox.
    #[command(subcommand)]
    Inbox(InboxCommand),
    /// Submit, inspect, validate, and apply coherent corpus reconciliations.
    #[command(subcommand)]
    Change(ChangeCommand),
    /// Search concept labels and ancestor context.
    Search(SearchArgs),
    /// Show the append-only corpus commit log.
    Log(LogArgs),
    /// Compare two corpus revisions.
    Diff(DiffArgs),
    /// Create a new commit that inverses an earlier commit.
    Revert(RevertArgs),
}

#[derive(Debug, Clone, Args)]
pub struct AtArgs {
    /// Historical corpus revision. Defaults to HEAD.
    #[arg(long, value_name = "REVISION")]
    pub at: Option<i64>,
}

#[derive(Debug, Clone, Args)]
pub struct PagedAtArgs {
    /// Historical corpus revision. Defaults to HEAD, or to the cursor's revision.
    #[arg(long, value_name = "REVISION")]
    pub at: Option<i64>,
    /// Maximum number of entries.
    #[arg(long, value_name = "N", default_value_t = 20)]
    pub limit: usize,
    /// Opaque continuation cursor.
    #[arg(long, value_name = "TOKEN")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConceptCommand {
    /// Show one concept with bounded parent, child, and evidence previews.
    Show(ConceptShowArgs),
    /// Page through the concept's direct parents.
    Parents(ConceptPageArgs),
    /// Page through the concept's direct children.
    Children(ConceptPageArgs),
    /// Page through evidence attached to the concept.
    Evidence(ConceptPageArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ConceptShowArgs {
    /// Durable public concept ID.
    #[arg(value_name = "CID")]
    pub id: ConceptId,
    /// Historical corpus revision. Defaults to HEAD.
    #[arg(long, value_name = "REVISION")]
    pub at: Option<i64>,
    /// Entries shown in each direct-neighborhood preview.
    #[arg(long, value_name = "N", default_value_t = 5)]
    pub preview_limit: usize,
}

#[derive(Debug, Clone, Args)]
pub struct ConceptPageArgs {
    /// Durable public concept ID.
    #[arg(value_name = "CID")]
    pub id: ConceptId,
    #[command(flatten)]
    pub page: PagedAtArgs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CliGraphDirection {
    Parents,
    Children,
    Both,
}

#[derive(Debug, Clone, Args)]
pub struct GraphArgs {
    /// Durable public concept ID.
    #[arg(value_name = "CID")]
    pub id: ConceptId,
    /// Historical corpus revision. Defaults to HEAD.
    #[arg(long, value_name = "REVISION")]
    pub at: Option<i64>,
    /// Edge directions traversed from the seed.
    #[arg(long, value_enum, default_value_t = CliGraphDirection::Children)]
    pub direction: CliGraphDirection,
    /// Maximum hop distance from the seed.
    #[arg(long, value_name = "N", default_value_t = 2)]
    pub depth: usize,
    /// Maximum distinct concepts in the result.
    #[arg(long, value_name = "N", default_value_t = 100)]
    pub max_nodes: usize,
}

#[derive(Debug, Clone, Args)]
pub struct ShakeArgs {
    /// Apply without an interactive confirmation prompt.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Clone, Args)]
pub struct BackupArgs {
    /// Destination database path.
    #[arg(value_name = "OUTPUT")]
    pub output: PathBuf,
}

#[derive(Debug, Clone, Subcommand)]
pub enum WorkCommand {
    /// Retain a UTF-8 work containing source text without changing the corpus revision.
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
    #[arg(long, value_enum)]
    pub quality: Option<ModelQuality>,
    /// Exact Codex model, overriding the model selected by --quality.
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,
    /// Examine again even when this exact liaison context was already reconciled.
    #[arg(long)]
    pub reexamine: bool,
    /// Apply a pending reconciliation immediately after the liaison submits it.
    #[arg(long)]
    pub apply: bool,
}

#[derive(Debug, Clone, Subcommand)]
pub enum InboxCommand {
    /// Register and drain settled inbox files sequentially until the queue is empty.
    Run(InboxRunArgs),
    /// Report queued, active, completed, and failed inbox state.
    Status,
}

#[derive(Debug, Clone, Args)]
pub struct InboxRunArgs {
    /// Minimum age of an unchanged incoming file.
    #[arg(long, value_name = "SECONDS")]
    pub settle_seconds: Option<u64>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ChangeCommand {
    /// Submit one reconciliation without invoking a model.
    Submit(ChangeSubmitArgs),
    /// Show a selected reconciliation or an accepted change at a corpus revision.
    Show(ChangeShowArgs),
    /// Re-resolve and validate the selected pending reconciliation without writing corpus state.
    Validate(ChangeSelectArgs),
    /// Atomically apply the selected pending reconciliation.
    Apply(ChangeSelectArgs),
    /// List reconciliation records.
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
    /// UTF-8 reconciliation JSON path, or - for standard input.
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
pub struct SearchArgs {
    /// Plain-language concept query.
    #[arg(value_name = "QUERY")]
    pub query: String,
    /// Historical corpus revision. Defaults to HEAD, or to the cursor's revision.
    #[arg(long, value_name = "REVISION")]
    pub at: Option<i64>,
    /// Restrict results to this concept and its descendants.
    #[arg(long, value_name = "CID")]
    pub within: Option<ConceptId>,
    /// Maximum number of results.
    #[arg(long, value_name = "N", default_value_t = 20)]
    pub limit: usize,
    /// Opaque continuation cursor.
    #[arg(long, value_name = "TOKEN")]
    pub cursor: Option<String>,
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{ChangeCommand, Cli, Command, ConceptCommand, InboxCommand, WorkCommand};

    #[test]
    fn graph_native_commands_parse() {
        assert!(matches!(
            Cli::try_parse_from(["annals", "work", "show", "Paper"]).map(|cli| cli.command),
            Ok(Command::Work(WorkCommand::Show(_)))
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "annals",
                "change",
                "submit",
                "request.json",
                "--work",
                "Paper",
                "--base",
                "17"
            ])
            .map(|cli| cli.command),
            Ok(Command::Change(ChangeCommand::Submit(_)))
        ));
        assert!(matches!(
            Cli::try_parse_from(["annals", "concept", "show", "c42"]).map(|cli| cli.command),
            Ok(Command::Concept(ConceptCommand::Show(_)))
        ));
        assert!(matches!(
            Cli::try_parse_from(["annals", "overview", "--at", "7"]).map(|cli| cli.command),
            Ok(Command::Overview(_))
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "annals", "roots", "--at", "7", "--limit", "25", "--cursor", "opaque"
            ])
            .map(|cli| cli.command),
            Ok(Command::Roots(_))
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "annals", "concept", "parents", "c42", "--at", "7", "--limit", "25"
            ])
            .map(|cli| cli.command),
            Ok(Command::Concept(ConceptCommand::Parents(_)))
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "annals",
                "graph",
                "c42",
                "--at",
                "7",
                "--direction",
                "both",
                "--depth",
                "3",
                "--max-nodes",
                "50"
            ])
            .map(|cli| cli.command),
            Ok(Command::Graph(_))
        ));
        assert!(matches!(
            Cli::try_parse_from(["annals", "shake", "--yes"]).map(|cli| cli.command),
            Ok(Command::Shake(args)) if args.yes
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "annals", "search", "locking", "--at", "7", "--within", "c42", "--limit", "25",
                "--cursor", "opaque"
            ])
            .map(|cli| cli.command),
            Ok(Command::Search(_))
        ));
        assert!(Cli::try_parse_from(["annals", "concept", "show", "42"]).is_err());
    }

    #[test]
    fn integrate_requires_new_input_or_existing_work() {
        assert!(Cli::try_parse_from(["annals", "integrate"]).is_err());
        assert!(Cli::try_parse_from(["annals", "integrate", "paper.txt"]).is_ok());
        assert!(Cli::try_parse_from(["annals", "integrate", "--work", "Existing paper"]).is_ok());
    }

    #[test]
    fn inbox_commands_parse() {
        assert!(matches!(
            Cli::try_parse_from(["annals", "inbox", "run", "--settle-seconds", "0"])
                .map(|cli| cli.command),
            Ok(Command::Inbox(InboxCommand::Run(args))) if args.settle_seconds == Some(0)
        ));
        assert!(matches!(
            Cli::try_parse_from(["annals", "--config", "annals.toml", "inbox", "status"])
                .map(|cli| cli.command),
            Ok(Command::Inbox(InboxCommand::Status))
        ));
    }
}
