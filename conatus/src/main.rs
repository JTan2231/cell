use anyhow::{Context, Result, ensure};
use clap::{ArgGroup, Args, Parser, Subcommand};
use serde_json::{Value, json};
use std::io::Read as _;
use std::path::PathBuf;
use std::process::ExitCode;

use conatus::{operations, store::Store};

#[derive(Parser)]
#[command(
    version,
    about = "Preserve wants and organize decisions in relation to them"
)]
struct Cli {
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// All output is JSON; this flag is accepted for explicit selection.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Select a general Annals library and begin at the current decision-feed watermark.
    Init {
        #[arg(long)]
        annals: PathBuf,
        #[arg(long)]
        decisions_config: PathBuf,
        #[arg(long)]
        annals_state_dir: Option<PathBuf>,
        #[arg(long, default_value = "conatus")]
        library: String,
    },
    #[command(subcommand)]
    Want(WantCommand),
    #[command(subcommand)]
    Decision(ReadCommand),
    /// Consume new accounts, forward captured sources, and run the Annals inbox.
    Update,
    /// Inspect intake, latest update results, and Annals processing state.
    Status,
    /// Read the bounded concept graph with completeness information.
    Graph,
    /// Read corpus revision history; the limit selects revisions.
    History {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    #[command(subcommand)]
    Instructions(InstructionsCommand),
    /// Retry the inclusive interval of failed Annals job IDs.
    Retry {
        #[arg(long)]
        from: String,
        #[arg(long)]
        through: String,
    },
    /// Reinterpret one already retained source under the current library instructions.
    Reexamine { id: String },
    /// Stop subsequent update activations; an active update can finish.
    Pause,
    /// Allow subsequent update activations; this does not start or schedule one.
    Resume,
}

#[derive(Subcommand)]
enum WantCommand {
    /// Capture source wording exactly without invoking a model.
    Add(WantArgs),
    /// List captured wants; the limit selects local intake records.
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Read one captured want and its available Annals evidence and associations.
    Show { id: String },
}

#[derive(Args)]
#[command(group(ArgGroup::new("input").required(true).args(["wording", "file", "stdin"])))]
struct WantArgs {
    wording: Option<String>,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    stdin: bool,
    /// Exact source locator, such as conversation host/thread/turn/item/span.
    #[arg(long)]
    source: String,
}

#[derive(Subcommand)]
enum ReadCommand {
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Show {
        id: String,
    },
}

#[derive(Subcommand)]
enum InstructionsCommand {
    Show,
    /// Select exact UTF-8 instructions; existing sources are not reexamined.
    Set {
        #[arg(long)]
        file: PathBuf,
    },
}

fn execute(cli: Cli) -> Result<Value> {
    let root = match cli.state_dir {
        Some(root) => root,
        None => conatus::state_root()?,
    };
    ensure!(root.is_absolute(), "state directory must be absolute");
    match cli.command {
        Command::Init {
            annals,
            decisions_config,
            annals_state_dir,
            library,
        } => operations::initialize(
            &root,
            &annals,
            &decisions_config,
            annals_state_dir.as_deref(),
            &library,
        ),
        Command::Want(WantCommand::Add(args)) => {
            let wording = if let Some(wording) = args.wording {
                wording
            } else if let Some(file) = args.file {
                std::fs::read_to_string(file)
                    .context("want file must contain UTF-8 source wording")?
            } else {
                let mut wording = String::new();
                std::io::stdin().read_to_string(&mut wording)?;
                wording
            };
            operations::capture_want(&root, &wording, &args.source)
        }
        Command::Want(WantCommand::List { limit }) => Store::open(&root)?.list("want", limit),
        Command::Want(WantCommand::Show { id }) => operations::show(&root, &id, "want"),
        Command::Decision(ReadCommand::List { limit }) => {
            Store::open(&root)?.list("decision", limit)
        }
        Command::Decision(ReadCommand::Show { id }) => operations::show(&root, &id, "decision"),
        Command::Update => operations::update(&root),
        Command::Status => operations::status(&root),
        Command::Graph => Store::open(&root)?.config()?.library().graph(),
        Command::History { limit } => Store::open(&root)?.config()?.library().history(limit),
        Command::Instructions(InstructionsCommand::Show) => operations::instructions(&root, None),
        Command::Instructions(InstructionsCommand::Set { file }) => {
            operations::instructions(&root, Some(&std::fs::read_to_string(file)?))
        }
        Command::Retry { from, through } => operations::retry(&root, &from, &through),
        Command::Reexamine { id } => operations::reexamine(&root, &id),
        Command::Pause => operations::pause(&root, true),
        Command::Resume => operations::pause(&root, false),
    }
}

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(data) => {
            println!("{}", json!({"ok":true,"data":data}));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", json!({"ok":false,"error":error.to_string()}));
            ExitCode::FAILURE
        }
    }
}
