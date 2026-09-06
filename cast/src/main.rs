use cast::{
    Result,
    runner::{self, RunOptions},
    store::Store,
};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "cast",
    version,
    about = "Private, deterministic company and job discovery"
)]
struct Cli {
    #[arg(long, global = true, env = "CAST_STATE_DIR")]
    state_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init,
    State {
        #[command(subcommand)]
        command: StateCommand,
    },
    Status {
        #[arg(long)]
        json: bool,
    },
    Doctor,
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Run {
        #[arg(long)]
        due: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        max_requests: Option<u64>,
    },
    #[command(alias = "snapshot")]
    Export {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Companies {
        #[command(subcommand)]
        command: ListCommand,
    },
    Jobs {
        #[command(subcommand)]
        command: ListCommand,
    },
    Sources {
        #[command(subcommand)]
        command: ListCommand,
    },
    Company {
        #[command(subcommand)]
        command: ShowCommand,
    },
    Job {
        #[command(subcommand)]
        command: JobCommand,
    },
    Source {
        #[command(subcommand)]
        command: SourceCommand,
    },
    Unresolved {
        #[command(subcommand)]
        command: ListCommand,
    },
    Search {
        query: String,
    },
}
#[derive(Subcommand)]
enum ConfigCommand {
    Show,
    Set {
        #[arg(long)]
        file: PathBuf,
    },
}
#[derive(Subcommand)]
enum StateCommand {
    ReconcileOwnership,
}
#[derive(Subcommand)]
enum ListCommand {
    List {
        #[arg(long)]
        json: bool,
    },
}
#[derive(Subcommand)]
enum ShowCommand {
    Show { id: String },
}
#[derive(Subcommand)]
enum JobCommand {
    Show { id: String },
    Refresh { id: String },
}
#[derive(Subcommand)]
enum SourceCommand {
    Add {
        url: String,
        #[arg(long)]
        company_id: Option<String>,
    },
    Disable {
        id: String,
    },
}

#[tokio::main]
async fn main() {
    if let Err(error) = execute(Cli::parse()).await {
        eprintln!("cast: {error}");
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_lines)]
async fn execute(cli: Cli) -> Result<()> {
    let directory = cli.state_dir.unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/cast")
    });
    let store = if matches!(&cli.command, Command::Init) {
        Store::init(&directory)?
    } else {
        Store::open(&directory)?
    };
    let output: Value = match cli.command {
        Command::Init => {
            let _lock = store.lock()?;
            json!({"schema_version":1,"state_dir":directory,"initialized":true})
        }
        Command::State {
            command: StateCommand::ReconcileOwnership,
        } => store.reconcile_ownership()?,
        Command::Status { .. } => store.status()?,
        Command::Doctor => {
            json!({"schema_version":1,"database":"ready","state_dir":directory,"credentials":{"theirstack":std::env::var("THEIRSTACK_API_KEY").is_ok_and(|v|!v.is_empty()),"brave":std::env::var("BRAVE_SEARCH_API_KEY").is_ok_and(|v|!v.is_empty())},"agent_runtime":false,"config":store.config()?})
        }
        Command::Config {
            command: ConfigCommand::Show,
        } => serde_json::to_value(store.config()?)?,
        Command::Config {
            command: ConfigCommand::Set { file },
        } => {
            let _lock = store.lock()?;
            let config = serde_json::from_slice(&std::fs::read(file)?)?;
            store.set_config(&config)?;
            json!({"configured":true})
        }
        Command::Run {
            force,
            source,
            max_requests,
            ..
        } => {
            runner::run(
                &store,
                RunOptions {
                    force,
                    provider: source,
                    max_requests,
                    only_source: None,
                },
            )
            .await?
        }
        Command::Export { output, .. } => {
            if let Some(path) = output {
                store.atomic_export(&path)?;
                json!({"exported":path})
            } else {
                serde_json::to_value(store.snapshot()?)?
            }
        }
        Command::Companies { .. } => serde_json::to_value(store.snapshot()?.companies)?,
        Command::Jobs { .. } => serde_json::to_value(store.snapshot()?.jobs)?,
        Command::Sources { .. } => serde_json::to_value(store.snapshot()?.source_health)?,
        Command::Company {
            command: ShowCommand::Show { id },
        } => serde_json::to_value(
            store
                .snapshot()?
                .companies
                .into_iter()
                .find(|c| c.id == id)
                .ok_or("company not found")?,
        )?,
        Command::Job {
            command: JobCommand::Show { id },
        } => serde_json::to_value(
            store
                .snapshot()?
                .jobs
                .into_iter()
                .find(|j| j.id == id)
                .ok_or("job not found")?,
        )?,
        Command::Job {
            command: JobCommand::Refresh { id },
        } => {
            let job = store
                .snapshot()?
                .jobs
                .into_iter()
                .find(|j| j.id == id)
                .ok_or("job not found")?;
            let source = if let Ok(source) = store.source(&job.source_id) {
                source
            } else {
                store.add_source(&job.company_id, &job.url)?
            };
            runner::run(
                &store,
                RunOptions {
                    force: true,
                    only_source: Some(source.id),
                    ..Default::default()
                },
            )
            .await?
        }
        Command::Source {
            command: SourceCommand::Add { url, company_id },
        } => serde_json::to_value(store.add_manual_source(&url, company_id.as_deref())?)?,
        Command::Source {
            command: SourceCommand::Disable { id },
        } => {
            let _lock = store.lock()?;
            serde_json::to_value(store.disable_source(&id)?)?
        }
        Command::Unresolved { .. } => {
            let snapshot = store.snapshot()?;
            json!({"companies":snapshot.companies.into_iter().filter(|c|c.domain.is_none()).collect::<Vec<_>>(),"sources":snapshot.source_health.into_iter().filter(|s|!matches!(s.status.as_str(),"complete"|"resolved"|"observed")).collect::<Vec<_>>()})
        }
        Command::Search { query } => {
            let query = query.to_lowercase();
            let snapshot = store.snapshot()?;
            json!({"companies":snapshot.companies.into_iter().filter(|c|c.name.to_lowercase().contains(&query)||c.domain.as_ref().is_some_and(|d|d.contains(&query))).collect::<Vec<_>>(),"jobs":snapshot.jobs.into_iter().filter(|j|j.title.to_lowercase().contains(&query)||j.description.as_ref().is_some_and(|d|d.to_lowercase().contains(&query))).collect::<Vec<_>>()})
        }
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
