use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

use crate::model::{Detail, NodeId, NodeKind, SearchKind};

/// Annals command-line arguments.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "annals",
    version,
    about = "Maintain and search local trees of textual topics",
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

    /// Disable terminal colors.
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Increase diagnostic detail on standard error.
    #[arg(short = 'v', global = true, action = ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

/// Top-level Annals command.
#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Create a new library without replacing an existing file.
    Init,
    /// Report library and derived-index statistics.
    Stats,
    /// Check `SQLite`, tree, and derived-index invariants without repairing them.
    Validate,
    /// Create a consistent backup without replacing the destination.
    Backup(BackupArgs),
    /// Rebuild all derived search data from canonical nodes.
    Reindex,
    /// Apply one explicit tree-ingestion document.
    Ingest(IngestArgs),
    /// Create, inspect, and delete rooted trees.
    #[command(subcommand)]
    Tree(TreeCommand),
    /// Create, inspect, edit, move, and delete nodes.
    #[command(subcommand)]
    Node(NodeCommand),
    /// Search indexed topic and source text.
    Search(SearchArgs),
}

/// Arguments for `ingest`.
#[derive(Debug, Clone, Args)]
pub struct IngestArgs {
    /// JSON ingestion document, or `-` for standard input.
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,
}

/// Arguments for `backup`.
#[derive(Debug, Clone, Args)]
pub struct BackupArgs {
    /// Destination database path.
    #[arg(value_name = "OUTPUT")]
    pub output: PathBuf,
}

/// Commands operating on roots and whole trees.
#[derive(Debug, Clone, Subcommand)]
pub enum TreeCommand {
    /// Append a topic root to the library.
    Create(TreeCreateArgs),
    /// List roots in display order.
    List,
    /// Display one tree in depth-first order.
    Show(TreeShowArgs),
    /// Delete an entire tree.
    Delete(TreeDeleteArgs),
}

/// Arguments for `tree create`.
#[derive(Debug, Clone, Args)]
pub struct TreeCreateArgs {
    /// Short navigation title.
    #[arg(long, value_name = "TITLE")]
    pub title: String,

    /// Inline topic body.
    #[arg(long, value_name = "TEXT", conflicts_with = "body_file")]
    pub body: Option<String>,

    /// Read the topic body from PATH, or from standard input with `-`.
    #[arg(long, value_name = "PATH")]
    pub body_file: Option<PathBuf>,
}

/// Arguments for `tree show`.
#[derive(Debug, Clone, Args)]
pub struct TreeShowArgs {
    /// Root node identifier.
    #[arg(value_name = "ROOT_NODE_ID")]
    pub root_node_id: NodeId,

    /// Maximum descendant depth to display.
    #[arg(long, value_name = "N")]
    pub depth: Option<usize>,
}

/// Arguments for `tree delete`.
#[derive(Debug, Clone, Args)]
pub struct TreeDeleteArgs {
    /// Root node identifier.
    #[arg(value_name = "ROOT_NODE_ID")]
    pub root_node_id: NodeId,

    /// Confirm deletion without prompting.
    #[arg(long)]
    pub yes: bool,
}

/// Commands operating on individual nodes and subtrees.
#[derive(Debug, Clone, Subcommand)]
pub enum NodeCommand {
    /// Add a child beneath a topic node.
    Add(NodeAddArgs),
    /// Show one node and its provenance.
    Show(NodeShowArgs),
    /// List a node's immediate children.
    Children(NodeChildrenArgs),
    /// Change explicitly supplied node fields.
    Edit(NodeEditArgs),
    /// Move a non-root subtree within its current tree.
    Move(NodeMoveArgs),
    /// Delete a leaf or, explicitly, a subtree.
    Delete(NodeDeleteArgs),
}

/// Arguments for `node add`.
#[derive(Debug, Clone, Args)]
pub struct NodeAddArgs {
    /// Parent topic node identifier.
    #[arg(long, value_name = "NODE_ID")]
    pub parent: NodeId,

    /// Whether the new node is a topic or source.
    #[arg(long, value_enum)]
    pub kind: NodeKind,

    /// Short navigation title.
    #[arg(long, value_name = "TITLE")]
    pub title: String,

    /// Inline node body.
    #[arg(long, value_name = "TEXT", conflicts_with = "body_file")]
    pub body: Option<String>,

    /// Read the node body from PATH, or from standard input with `-`.
    #[arg(long, value_name = "PATH")]
    pub body_file: Option<PathBuf>,

    /// Opaque source locator such as a URL, path, or citation.
    #[arg(long, value_name = "VALUE")]
    pub locator: Option<String>,

    /// Source media type.
    #[arg(long, value_name = "TYPE")]
    pub media_type: Option<String>,

    /// Versioned source checksum.
    #[arg(long, value_name = "VALUE")]
    pub checksum: Option<String>,

    /// Source capture time in RFC 3339 form.
    #[arg(long, value_name = "RFC3339")]
    pub captured_at: Option<String>,

    /// Zero-based ordinal among the parent's children.
    #[arg(long, value_name = "N")]
    pub position: Option<usize>,
}

/// Arguments for `node show`.
#[derive(Debug, Clone, Args)]
pub struct NodeShowArgs {
    /// Node identifier.
    #[arg(value_name = "NODE_ID")]
    pub node_id: NodeId,
}

/// Arguments for `node children`.
#[derive(Debug, Clone, Args)]
pub struct NodeChildrenArgs {
    /// Parent node identifier.
    #[arg(value_name = "NODE_ID")]
    pub node_id: NodeId,
}

/// Arguments for `node edit`.
#[derive(Debug, Clone, Args)]
pub struct NodeEditArgs {
    /// Node identifier.
    #[arg(value_name = "NODE_ID")]
    pub node_id: NodeId,

    /// Change the node's structural kind.
    #[arg(long, value_enum)]
    pub kind: Option<NodeKind>,

    /// Change the navigation title.
    #[arg(long, value_name = "TITLE")]
    pub title: Option<String>,

    /// Replace the body with inline text.
    #[arg(
        long,
        value_name = "TEXT",
        conflicts_with_all = ["body_file", "clear_body"]
    )]
    pub body: Option<String>,

    /// Replace the body with UTF-8 text from PATH, or standard input with `-`.
    #[arg(long, value_name = "PATH", conflicts_with = "clear_body")]
    pub body_file: Option<PathBuf>,

    /// Replace the body with an empty string.
    #[arg(long)]
    pub clear_body: bool,

    /// Change the opaque source locator.
    #[arg(long, value_name = "VALUE")]
    pub locator: Option<String>,

    /// Change the source media type.
    #[arg(long, value_name = "TYPE")]
    pub media_type: Option<String>,

    /// Change the versioned source checksum.
    #[arg(long, value_name = "VALUE")]
    pub checksum: Option<String>,

    /// Change the source capture time, in RFC 3339 form.
    #[arg(long, value_name = "RFC3339")]
    pub captured_at: Option<String>,
}

impl NodeEditArgs {
    /// Return whether at least one editable field was supplied.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        self.kind.is_some()
            || self.title.is_some()
            || self.body.is_some()
            || self.body_file.is_some()
            || self.clear_body
            || self.locator.is_some()
            || self.media_type.is_some()
            || self.checksum.is_some()
            || self.captured_at.is_some()
    }
}

/// Arguments for `node move`.
#[derive(Debug, Clone, Args)]
pub struct NodeMoveArgs {
    /// Node identifier.
    #[arg(value_name = "NODE_ID")]
    pub node_id: NodeId,

    /// Destination parent topic identifier.
    #[arg(long, value_name = "NEW_PARENT_ID")]
    pub parent: NodeId,

    /// Zero-based ordinal among the destination's children.
    #[arg(long, value_name = "N")]
    pub position: Option<usize>,
}

/// Arguments for `node delete`.
#[derive(Debug, Clone, Args)]
pub struct NodeDeleteArgs {
    /// Node identifier.
    #[arg(value_name = "NODE_ID")]
    pub node_id: NodeId,

    /// Permit deletion when the node has descendants.
    #[arg(long)]
    pub recursive: bool,

    /// Confirm recursive deletion without prompting.
    #[arg(long)]
    pub yes: bool,
}

/// Arguments for `search`.
#[derive(Debug, Clone, Args)]
pub struct SearchArgs {
    /// Plain-text query. Double quotes preserve phrases.
    #[arg(value_name = "QUERY")]
    pub query: String,

    /// Restrict results to this node and its descendants.
    #[arg(long, value_name = "NODE_ID")]
    pub within: Option<NodeId>,

    /// Restrict results by node kind.
    #[arg(long, value_enum, default_value_t = SearchKind::All)]
    pub kind: SearchKind,

    /// Prefer overview topics, balanced results, or sources while grouping.
    #[arg(long, value_enum, default_value_t = Detail::Balanced)]
    pub detail: Detail,

    /// Maximum number of primary results.
    #[arg(long, value_name = "N", default_value_t = 10)]
    pub limit: usize,

    /// Include unstable ranking and grouping diagnostics.
    #[arg(long)]
    pub explain: bool,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command, NodeCommand, TreeCommand};
    use crate::model::{Detail, NodeKind, SearchKind};

    #[test]
    fn global_library_is_accepted_after_init() {
        let parsed = Cli::try_parse_from(["annals", "init", "--library", "notes.db"]);
        assert!(parsed.is_ok());
    }

    #[test]
    fn tree_create_rejects_two_body_sources() {
        let parsed = Cli::try_parse_from([
            "annals",
            "tree",
            "create",
            "--title",
            "Root",
            "--body",
            "inline",
            "--body-file",
            "body.txt",
        ]);
        assert!(parsed.is_err());
    }

    #[test]
    fn parses_node_source_metadata() {
        let parsed = Cli::try_parse_from([
            "annals",
            "node",
            "add",
            "--parent",
            "1",
            "--kind",
            "source",
            "--title",
            "Paper",
            "--locator",
            "paper.pdf",
        ]);
        let Ok(cli) = parsed else {
            panic!("documented node add syntax should parse");
        };
        let Command::Node(NodeCommand::Add(args)) = cli.command else {
            panic!("expected node add command");
        };
        assert_eq!(args.kind, NodeKind::Source);
        assert_eq!(args.locator.as_deref(), Some("paper.pdf"));
    }

    #[test]
    fn search_defaults_match_contract() {
        let parsed = Cli::try_parse_from(["annals", "search", "write skew"]);
        let Ok(cli) = parsed else {
            panic!("documented search syntax should parse");
        };
        let Command::Search(args) = cli.command else {
            panic!("expected search command");
        };
        assert_eq!(args.kind, SearchKind::All);
        assert_eq!(args.detail, Detail::Balanced);
        assert_eq!(args.limit, 10);
    }

    #[test]
    fn nested_tree_commands_are_recognized() {
        let parsed = Cli::try_parse_from(["annals", "tree", "list"]);
        let Ok(cli) = parsed else {
            panic!("tree list should parse");
        };
        assert!(matches!(cli.command, Command::Tree(TreeCommand::List)));
    }
}
