use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

use crate::model::{ModelQuality, TodoId};

/// A researched todo list for people and agents.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "todo",
    version,
    about = "Research and maintain durable, actionable todos",
    long_about = "Research and maintain durable, actionable todos.\n\n`todo new` is intended for agents as well as people: give it the file where a need arose (usually a conversation transcript) and a short direction. Its research agent reads that source, follows relevant references and leads, and creates one actionable todo.",
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
    /// Research a need and create one actionable todo.
    New(NewArgs),
    /// List open todos, newest first.
    List(ListArgs),
    /// Search todo titles, original notes, and working notes.
    Search(SearchArgs),
    /// Show one todo and its working notes.
    Show(TodoArgs),
    /// Append working notes to a todo.
    #[command(subcommand)]
    Note(NoteCommand),
    /// Mark a todo done.
    Done(TodoArgs),
    /// Reopen a completed todo.
    Reopen(TodoArgs),
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

#[derive(Debug, Clone, Args)]
pub(crate) struct NoteAddArgs {
    /// Durable public todo ID.
    #[arg(value_name = "TODO")]
    pub(crate) id: TodoId,

    /// Working-note text, or - to read UTF-8 text from standard input.
    #[arg(value_name = "TEXT")]
    pub(crate) text: String,
}
