use anyhow::{Context, Result, ensure};
use clap::{ArgGroup, Args, Parser, Subcommand};
use serde_json::{Value, json};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use conatus::{
    operations,
    store::{Store, WantState},
};

mod output;

#[derive(Parser)]
#[command(
    version,
    about = "Preserve wants and organize decisions in relation to them"
)]
struct Cli {
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// Print the machine JSON response instead of readable text.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Subcommand)]
enum Command {
    /// Initialize library selections or rebind Annals while preserving existing intake.
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
    /// Preview or send the complete active wants list without model work.
    #[command(subcommand)]
    Email(EmailCommand),
    /// Consume new accounts, forward captured sources, and run the Annals inbox.
    Update,
    /// Inspect intake, latest update results, and Annals processing state.
    Status,
    /// Read persistent dependency and library selections without probing Annals.
    Config,
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
    /// Inspect or control one deployment owner's admission hold.
    Maintenance {
        #[arg(value_parser = ["status", "hold", "drain", "release"])]
        operation: String,
        owner: Option<String>,
    },
}

#[derive(Clone, Subcommand)]
enum WantCommand {
    /// Capture source wording exactly without invoking a model.
    Add(WantArgs),
    /// List active wants by default; the limit applies after state selection.
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Select only archived wants.
        #[arg(long, conflicts_with = "all")]
        archived: bool,
        /// Include active and archived wants.
        #[arg(long)]
        all: bool,
    },
    /// Archive one want only when directly requested by the user.
    Archive { id: String },
    /// Restore one want to active only when directly requested by the user.
    Unarchive { id: String },
    /// Read one captured want and its available Annals evidence and associations.
    Show { id: String },
}

#[derive(Clone, Args)]
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

#[derive(Clone, Subcommand)]
enum ReadCommand {
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Show {
        id: String,
    },
}

#[derive(Clone, Subcommand)]
enum InstructionsCommand {
    Show,
    /// Select exact UTF-8 instructions; existing sources are not reexamined.
    Set {
        #[arg(long)]
        file: PathBuf,
    },
}

#[derive(Clone, Subcommand)]
enum EmailCommand {
    Preview {
        /// Read an exact retained email instead of rendering current state.
        #[arg(long)]
        occurrence: Option<String>,
    },
    Send {
        #[arg(long, conflicts_with = "retry")]
        scheduled: bool,
        /// Retry retained bytes only after inspecting uncertain provider acceptance.
        #[arg(long)]
        retry: Option<String>,
    },
}

fn execute(cli: Cli) -> Result<Value> {
    let root = match cli.state_dir {
        Some(root) => root,
        None => conatus::state_root()?,
    };
    ensure!(root.is_absolute(), "state directory must be absolute");
    let mutation = matches!(
        &cli.command,
        Command::Init { .. }
            | Command::Want(
                WantCommand::Add(_) | WantCommand::Archive { .. } | WantCommand::Unarchive { .. }
            )
            | Command::Update
            | Command::Instructions(InstructionsCommand::Set { .. })
            | Command::Retry { .. }
            | Command::Reexamine { .. }
            | Command::Pause
            | Command::Resume
    );
    let _admission = if mutation {
        let gate = conatus::gate(&root);
        Some(if matches!(&cli.command, Command::Init { .. }) {
            if let Ok(owner) = std::env::var("CELL_DEPLOYMENT_RUN_ID") {
                gate.enter_for(&owner)?
            } else {
                gate.enter()?
            }
        } else {
            gate.enter()?
        })
    } else {
        None
    };
    match cli.command {
        Command::Maintenance { operation, owner } => {
            let gate = conatus::gate(&root);
            let status = match operation.as_str() {
                "hold" => gate.hold(owner.as_deref().context("hold requires an owner")?)?,
                "release" => {
                    gate.release(owner.as_deref().context("release requires an owner")?)?
                }
                _ => gate.status()?,
            };
            Ok(
                json!({"maintenance":{"protocol_version":1,"holds":status.holds,"drained":status.drained && conatus::store::runner_idle(&root)?}}),
            )
        }
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
        Command::Want(command) => execute_want(&root, command),
        Command::Decision(ReadCommand::List { limit }) => {
            Store::open(&root)?.list("decision", limit)
        }
        Command::Decision(ReadCommand::Show { id }) => operations::show(&root, &id, "decision"),
        Command::Update => operations::update(&root),
        Command::Email(EmailCommand::Preview { occurrence }) => {
            conatus::digest::preview(&root, occurrence.as_deref())
        }
        Command::Email(EmailCommand::Send { scheduled, retry }) => {
            conatus::digest::send(&root, scheduled, retry.as_deref())
        }
        Command::Status => operations::status(&root),
        Command::Config => Ok(json!({"config":Store::open(&root)?.config()?})),
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

fn execute_want(root: &Path, command: WantCommand) -> Result<Value> {
    match command {
        WantCommand::Add(args) => {
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
            operations::capture_want(root, &wording, &args.source)
        }
        WantCommand::List {
            limit,
            archived,
            all,
        } => {
            let state = if all {
                None
            } else if archived {
                Some(WantState::Archived)
            } else {
                Some(WantState::Active)
            };
            Store::open(root)?.list_wants(limit, state)
        }
        WantCommand::Archive { id } => Store::open(root)?.set_want_state(&id, WantState::Archived),
        WantCommand::Unarchive { id } => Store::open(root)?.set_want_state(&id, WantState::Active),
        WantCommand::Show { id } => operations::show(root, &id, "want"),
    }
}

fn main() -> ExitCode {
    if let Some(snapshot) = iatreion_api::requested_status_snapshot_json(
        "conatus",
        env!("CARGO_PKG_VERSION"),
        vec![
            iatreion_api::declared_unit(
                "conatus",
                "conatus/update",
                Some("conatus/update"),
                iatreion_api::Intent::Active,
                "conatus.update.operate",
            ),
            iatreion_api::declared_unit(
                "conatus",
                "conatus/daily-email",
                Some("conatus/daily-email"),
                iatreion_api::Intent::Active,
                "conatus.digest.email",
            ),
        ],
        false,
    ) {
        chancery_usage::observe("conatus", "status-snapshot");
        println!("{snapshot}");
        return ExitCode::SUCCESS;
    }
    let cli = chancery_usage::cli::parse::<Cli>("conatus", "");
    let json_output = cli.json;
    let command = cli.command.clone();
    match execute(cli) {
        Ok(data) => {
            if json_output {
                println!("{}", json!({"ok":true,"data":data}));
            } else {
                print!("{}", output::render(&command, &data));
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if json_output {
                eprintln!("{}", json!({"ok":false,"error":error.to_string()}));
            } else {
                eprintln!("Error: {error:#}");
            }
            ExitCode::FAILURE
        }
    }
}
