use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

use crate::model::NodeId;

/// Annals command-line arguments.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "annals",
    version,
    about = "Construct and search local conceptual trees",
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
    /// Check `SQLite`, tree, grounding, and derived-index invariants.
    Validate,
    /// Create a consistent backup without replacing the destination.
    Backup(BackupArgs),
    /// Rebuild all derived search data from canonical nodes.
    Reindex,
    /// Generate and atomically store one conceptual tree from raw UTF-8 input.
    Ingest(IngestArgs),
    /// Create, inspect, and delete rooted trees.
    #[command(subcommand)]
    Tree(TreeCommand),
    /// Create, inspect, edit, move, and delete nodes.
    #[command(subcommand)]
    Node(NodeCommand),
    /// Search indexed conceptual node text.
    Search(SearchArgs),
}

/// Arguments for `ingest`.
#[derive(Debug, Clone, Args)]
pub struct IngestArgs {
    /// Raw UTF-8 input file, or `-` for standard input.
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,

    /// Maximum number of generated nodes, including the root.
    #[arg(long, value_name = "N", default_value_t = 32)]
    pub node_budget: usize,

    /// Maximum generated depth, with the root at depth zero.
    #[arg(long, value_name = "N", default_value_t = 6)]
    pub max_depth: usize,

    /// Maximum number of children beneath any generated node.
    #[arg(long, value_name = "N", default_value_t = 6)]
    pub max_children: usize,
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
    /// Append a root to the library.
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
    /// The root's canonical conceptual string.
    #[arg(long, value_name = "TEXT")]
    pub text: String,
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
    /// Add a child beneath a node.
    Add(NodeAddArgs),
    /// Show one node.
    Show(NodeShowArgs),
    /// List a node's immediate children.
    Children(NodeChildrenArgs),
    /// Replace one node's conceptual string.
    Edit(NodeEditArgs),
    /// Move a non-root subtree within its current tree.
    Move(NodeMoveArgs),
    /// Delete a leaf or, explicitly, a subtree.
    Delete(NodeDeleteArgs),
}

/// Arguments for `node add`.
#[derive(Debug, Clone, Args)]
pub struct NodeAddArgs {
    /// Parent node identifier.
    #[arg(long, value_name = "NODE_ID")]
    pub parent: NodeId,

    /// The new node's canonical conceptual string.
    #[arg(long, value_name = "TEXT")]
    pub text: String,

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

    /// Replacement canonical conceptual string.
    #[arg(long, value_name = "TEXT")]
    pub text: String,
}

/// Arguments for `node move`.
#[derive(Debug, Clone, Args)]
pub struct NodeMoveArgs {
    /// Node identifier.
    #[arg(value_name = "NODE_ID")]
    pub node_id: NodeId,

    /// Destination parent identifier.
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

    /// Maximum number of results.
    #[arg(long, value_name = "N", default_value_t = 10)]
    pub limit: usize,

    /// Include unstable ranking diagnostics.
    #[arg(long)]
    pub explain: bool,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command, NodeCommand, TreeCommand};

    #[test]
    fn global_library_is_accepted_after_init() {
        assert!(Cli::try_parse_from(["annals", "init", "--library", "notes.db"]).is_ok());
    }

    #[test]
    fn ingest_defaults_are_the_resolution_policy() {
        let parsed = Cli::try_parse_from(["annals", "ingest", "-"]);
        let Ok(cli) = parsed else {
            panic!("ingest syntax should parse");
        };
        let Command::Ingest(args) = cli.command else {
            panic!("expected ingest command");
        };
        assert_eq!(
            (args.node_budget, args.max_depth, args.max_children),
            (32, 6, 6)
        );
    }

    #[test]
    fn homogeneous_node_syntax_parses() {
        let parsed = Cli::try_parse_from([
            "annals",
            "node",
            "add",
            "--parent",
            "1",
            "--text",
            "A narrower concept",
        ]);
        let Ok(cli) = parsed else {
            panic!("node add syntax should parse");
        };
        assert!(matches!(cli.command, Command::Node(NodeCommand::Add(_))));
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
