use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

use crate::model::{
    ConcernId, DesignId, ModelQuality, RoutingProposalId, SituationAssessmentId, TodoId,
};

/// A researched todo list for people and agents.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "todo",
    version,
    about = "Research and maintain durable, actionable todos",
    long_about = "Capture concerns and maintain durable, actionable todo umbrellas.\n\n`todo new` is intended for agents as well as people: give it the file where a need arose (usually a conversation transcript) and a short direction. Todo records that provenance before research and asks its liaison for a pending routing proposal. Accepting that proposal is a separate command.",
    arg_required_else_help = true
)]
pub(crate) struct Cli {
    /// Todo TOML configuration path. Defaults to `TODO_CONFIG` when set.
    #[arg(long, global = true, value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,

    /// `SQLite` database path. Defaults to nonempty `TODO_DATABASE`, then config.
    #[arg(long, global = true, value_name = "PATH")]
    pub(crate) database: Option<PathBuf>,

    /// Emit one JSON document.
    #[arg(long, global = true)]
    pub(crate) json: bool,

    /// Suppress successful human-oriented mutation output.
    #[arg(long, global = true)]
    pub(crate) quiet: bool,

    /// Increase diagnostic detail on standard error.
    #[arg(short = 'v', global = true, action = ArgAction::Count)]
    pub(crate) verbose: u8,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Command {
    /// Create a new database without replacing an existing file.
    Init,
    /// Upgrade an older database after writing a caller-selected backup.
    Migrate(MigrateArgs),
    /// Capture a concern and research a pending routing proposal.
    New(NewArgs),
    /// Capture and inspect originating concerns.
    #[command(subcommand)]
    Concern(ConcernCommand),
    /// Inspect and explicitly decide pending routing proposals.
    #[command(subcommand)]
    Routing(RoutingCommand),
    /// Record a dated situation assessment for one todo umbrella.
    Assess(AssessArgs),
    /// Inspect immutable situation assessments.
    #[command(subcommand)]
    Situation(SituationCommand),
    /// Propose, correct, inspect, and decide designs.
    #[command(subcommand)]
    Design(DesignCommand),
    /// List open todos, newest first.
    List(ListArgs),
    /// Search current directions, concerns, assessments, designs, and notes.
    Search(SearchArgs),
    /// Show one umbrella with inherited concerns, assessments, designs, and notes.
    Show(TodoArgs),
    /// Append working notes to a todo.
    #[command(subcommand)]
    Note(NoteCommand),
    /// Preview or send the outstanding-todo email.
    #[command(subcommand)]
    Email(EmailCommand),
    /// Mark a todo done.
    Done(TodoArgs),
    /// Reopen a completed todo.
    Reopen(TodoArgs),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct MigrateArgs {
    /// Absolute, nonexistent path at which to retain the pre-migration database.
    #[arg(long, value_name = "ABSOLUTE_PATH")]
    pub(crate) backup: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct NewArgs {
    /// Short need or concern for the research agent to investigate.
    #[arg(value_name = "DIRECTION")]
    pub(crate) direction: String,

    /// File where the need arose, usually a conversation transcript.
    ///
    /// The research agent begins here and may inspect related local or external
    /// material. The resolved path, but not the file contents, is retained.
    #[arg(long, value_name = "PATH")]
    pub(crate) source: PathBuf,

    /// Research-agent quality preset.
    #[arg(long, value_enum)]
    pub(crate) quality: Option<ModelQuality>,

    /// Exact Codex model, overriding the model selected by --quality.
    #[arg(long, value_name = "MODEL")]
    pub(crate) model: Option<String>,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum ConcernCommand {
    /// Record immutable source provenance without running research.
    Add(ConcernAddArgs),
    /// List concerns awaiting a terminal routing decision.
    List(ConcernListArgs),
    /// Show one concern and its routing history.
    Show(ConcernArgs),
    /// Research one concern and record a pending routing proposal.
    Assess(ConcernAssessArgs),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ConcernAddArgs {
    /// Short need or concern to retain.
    #[arg(value_name = "DIRECTION")]
    pub(crate) direction: String,

    /// File where the concern arose, usually a conversation transcript.
    #[arg(long, value_name = "PATH")]
    pub(crate) source: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ConcernListArgs {
    /// Include concerns already attached to an umbrella or dismissed.
    #[arg(long)]
    pub(crate) all: bool,

    /// Maximum number of concerns.
    #[arg(long, value_name = "N", default_value_t = 20)]
    pub(crate) limit: u32,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ConcernArgs {
    /// Durable public concern ID.
    #[arg(value_name = "CONCERN")]
    pub(crate) id: ConcernId,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ConcernAssessArgs {
    /// Durable public concern ID.
    #[arg(value_name = "CONCERN")]
    pub(crate) id: ConcernId,

    #[command(flatten)]
    pub(crate) research: ResearchArgs,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum RoutingCommand {
    /// Show one sealed routing proposal and its exact basis.
    Show(RoutingArgs),
    /// Authorize the action in one still-current routing proposal.
    Accept(RoutingAcceptArgs),
    /// Reject one routing proposal with a retained reason.
    Reject(RoutingRejectArgs),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct RoutingArgs {
    /// Durable public routing proposal ID.
    #[arg(value_name = "ROUTING")]
    pub(crate) id: RoutingProposalId,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct RoutingAcceptArgs {
    /// Durable public routing proposal ID.
    #[arg(value_name = "ROUTING")]
    pub(crate) id: RoutingProposalId,

    /// Readable UTF-8 file containing the authorization decision.
    #[arg(long, value_name = "PATH")]
    pub(crate) source: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct RoutingRejectArgs {
    /// Durable public routing proposal ID.
    #[arg(value_name = "ROUTING")]
    pub(crate) id: RoutingProposalId,

    /// Why the proposal is being rejected, or - to read UTF-8 text from stdin.
    #[arg(long, value_name = "TEXT")]
    pub(crate) reason: String,

    /// Readable UTF-8 file containing the rejection decision.
    #[arg(long, value_name = "PATH")]
    pub(crate) source: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct AssessArgs {
    /// Durable public todo umbrella ID.
    #[arg(value_name = "TODO")]
    pub(crate) id: TodoId,

    #[command(flatten)]
    pub(crate) research: ResearchArgs,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum SituationCommand {
    /// Show one immutable, dated situation assessment and its bases.
    Show(SituationArgs),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct SituationArgs {
    /// Durable public situation assessment ID.
    #[arg(value_name = "SITUATION")]
    pub(crate) id: SituationAssessmentId,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum DesignCommand {
    /// Propose a design from the umbrella's current ready assessment.
    Propose(DesignProposeArgs),
    /// Show one sealed design version and its decision history.
    Show(DesignArgs),
    /// Ask for a corrected proposal without accepting it.
    Correct(DesignCorrectArgs),
    /// Accept one still-current proposed design.
    Accept(DesignAcceptArgs),
    /// Reject one proposed design with a retained reason.
    Reject(DesignRejectArgs),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct DesignProposeArgs {
    /// Todo umbrella whose latest current ready assessment is the design basis.
    #[arg(value_name = "TODO")]
    pub(crate) todo: TodoId,

    #[command(flatten)]
    pub(crate) research: ResearchArgs,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct DesignArgs {
    /// Durable public design ID.
    #[arg(value_name = "DESIGN")]
    pub(crate) id: DesignId,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct DesignAcceptArgs {
    /// Durable public design ID.
    #[arg(value_name = "DESIGN")]
    pub(crate) id: DesignId,

    /// Readable UTF-8 file containing the authorization decision.
    #[arg(long, value_name = "PATH")]
    pub(crate) source: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct DesignCorrectArgs {
    /// Durable public design ID to correct.
    #[arg(value_name = "DESIGN")]
    pub(crate) id: DesignId,

    /// Correction feedback, or - to read UTF-8 text from stdin.
    #[arg(value_name = "FEEDBACK")]
    pub(crate) feedback: String,

    #[command(flatten)]
    pub(crate) research: ResearchArgs,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct DesignRejectArgs {
    /// Durable public design ID.
    #[arg(value_name = "DESIGN")]
    pub(crate) id: DesignId,

    /// Why the proposal is being rejected, or - to read UTF-8 text from stdin.
    #[arg(long, value_name = "TEXT")]
    pub(crate) reason: String,

    /// Readable UTF-8 file containing the rejection decision.
    #[arg(long, value_name = "PATH")]
    pub(crate) source: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ResearchArgs {
    /// Research-agent quality preset.
    #[arg(long, value_enum)]
    pub(crate) quality: Option<ModelQuality>,

    /// Exact Codex model, overriding the model selected by --quality.
    #[arg(long, value_name = "MODEL")]
    pub(crate) model: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ListArgs {
    /// Include completed todos.
    #[arg(long)]
    pub(crate) all: bool,

    /// Maximum number of todos.
    #[arg(long, value_name = "N", default_value_t = 20)]
    pub(crate) limit: u32,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct SearchArgs {
    /// Literal text to find.
    #[arg(value_name = "QUERY")]
    pub(crate) query: String,

    /// Include completed todos.
    #[arg(long)]
    pub(crate) all: bool,

    /// Maximum number of todos.
    #[arg(long, value_name = "N", default_value_t = 20)]
    pub(crate) limit: u32,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct TodoArgs {
    /// Durable public todo ID.
    #[arg(value_name = "TODO")]
    pub(crate) id: TodoId,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum NoteCommand {
    /// Append one immutable working note.
    Add(NoteAddArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum EmailCommand {
    /// Preview the exact email without sending it.
    Preview,
    /// Send the current outstanding todos through Resend.
    Send(EmailSendArgs),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct EmailSendArgs {
    /// Use the deterministic daily idempotency key intended for launchd.
    #[arg(long)]
    pub(crate) scheduled: bool,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct NoteAddArgs {
    /// Durable public todo ID.
    #[arg(value_name = "TODO")]
    pub(crate) id: TodoId,

    /// Working-note text, or - to read UTF-8 text from standard input.
    #[arg(value_name = "TEXT")]
    pub(crate) text: String,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use clap::Parser as _;

    use super::{Cli, Command, ConcernCommand, DesignCommand, RoutingCommand};

    #[test]
    fn new_is_a_distinct_capture_and_research_command() {
        let cli = Cli::try_parse_from([
            "todo",
            "new",
            "Retain this concern",
            "--source",
            "/tmp/source.jsonl",
            "--quality",
            "medium",
        ])
        .unwrap_or_else(|error| panic!("{error}"));
        let Command::New(args) = cli.command else {
            panic!("new parsed as the wrong command")
        };
        assert_eq!(args.direction, "Retain this concern");
        assert_eq!(args.source, Path::new("/tmp/source.jsonl"));
    }

    #[test]
    fn concern_capture_does_not_accept_research_options() {
        let cli = Cli::try_parse_from([
            "todo",
            "concern",
            "add",
            "Retain this concern",
            "--source",
            "/tmp/source.jsonl",
        ])
        .unwrap_or_else(|error| panic!("{error}"));
        let Command::Concern(ConcernCommand::Add(args)) = cli.command else {
            panic!("concern add parsed as the wrong command")
        };
        assert_eq!(args.direction, "Retain this concern");

        assert!(
            Cli::try_parse_from([
                "todo",
                "concern",
                "add",
                "Retain this concern",
                "--source",
                "/tmp/source.jsonl",
                "--quality",
                "high",
            ])
            .is_err()
        );
    }

    #[test]
    fn routing_rejection_requires_a_reason() {
        assert!(Cli::try_parse_from(["todo", "routing", "reject", "r1"]).is_err());
        let cli = Cli::try_parse_from([
            "todo",
            "routing",
            "reject",
            "r1",
            "--reason",
            "The proposed umbrella is too broad",
            "--source",
            "/tmp/decision.jsonl",
        ])
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(
            cli.command,
            Command::Routing(RoutingCommand::Reject(_))
        ));

        assert!(
            Cli::try_parse_from(["todo", "routing", "accept", "r1"]).is_err(),
            "authorization provenance must be explicit"
        );
    }

    #[test]
    fn design_decisions_require_authorization_provenance() {
        assert!(Cli::try_parse_from(["todo", "design", "accept", "d1"]).is_err());
        assert!(
            Cli::try_parse_from([
                "todo",
                "design",
                "reject",
                "d1",
                "--reason",
                "The ownership boundary is wrong",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "todo",
                "design",
                "accept",
                "d1",
                "--source",
                "/tmp/decision.jsonl",
            ])
            .is_ok()
        );
    }

    #[test]
    fn design_commands_name_exact_immutable_records() {
        let proposed = Cli::try_parse_from(["todo", "design", "propose", "t3"])
            .unwrap_or_else(|error| panic!("{error}"));
        let Command::Design(DesignCommand::Propose(args)) = proposed.command else {
            panic!("design propose parsed as the wrong command")
        };
        assert_eq!(args.todo.to_string(), "t3");

        let corrected = Cli::try_parse_from([
            "todo",
            "design",
            "correct",
            "d4",
            "Narrow the compatibility boundary",
            "--model",
            "exact-model",
        ])
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(
            corrected.command,
            Command::Design(DesignCommand::Correct(_))
        ));
    }

    #[test]
    fn public_id_prefixes_are_part_of_the_command_contract() {
        assert!(Cli::try_parse_from(["todo", "concern", "show", "c1"]).is_ok());
        assert!(Cli::try_parse_from(["todo", "routing", "show", "r1"]).is_ok());
        assert!(Cli::try_parse_from(["todo", "show", "t1"]).is_ok());
        assert!(Cli::try_parse_from(["todo", "situation", "show", "a1"]).is_ok());
        assert!(Cli::try_parse_from(["todo", "design", "show", "d1"]).is_ok());

        assert!(Cli::try_parse_from(["todo", "concern", "show", "t1"]).is_err());
        assert!(Cli::try_parse_from(["todo", "routing", "show", "c1"]).is_err());
        assert!(Cli::try_parse_from(["todo", "situation", "show", "s1"]).is_err());
        assert!(Cli::try_parse_from(["todo", "design", "show", "a1"]).is_err());
    }

    #[test]
    fn migration_requires_an_explicit_backup_path() {
        assert!(Cli::try_parse_from(["todo", "migrate"]).is_err());
        let cli = Cli::try_parse_from(["todo", "migrate", "--backup", "/tmp/todo-v1.db.backup"])
            .unwrap_or_else(|error| panic!("{error}"));
        let Command::Migrate(args) = cli.command else {
            panic!("migrate parsed as the wrong command")
        };
        assert_eq!(args.backup, Path::new("/tmp/todo-v1.db.backup"));
    }
}
