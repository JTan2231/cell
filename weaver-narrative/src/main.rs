use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Compose a narrative from a direction and the Annals decision history"
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Configure reading and initialize the private `SQLite` document store.
    Init {
        #[arg(long)]
        annals_config: PathBuf,
        #[arg(long)]
        annals_binary: Option<PathBuf>,
    },
    /// Author one document from a free-form direction. Wait for the result.
    Write {
        direction: String,
        #[arg(long)]
        id: Option<String>,
    },
    /// Author independent new documents in one runner. Return each job outcome.
    WriteMany {
        #[arg(long)]
        jobs: std::num::NonZeroUsize,
        #[arg(required = true, num_args = 1..)]
        directions: Vec<String>,
    },
    /// Author a new document using saved Markdown and a free-form direction.
    Revise {
        id: String,
        direction: String,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Continue the exact saved Nucleus job after interruption or deferral.
    Resume { id: String },
    /// List saved and pending document metadata.
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Print the saved Markdown, or its full record with --json.
    Show { id: String },
    /// Check state and dependency readiness without authoring a document.
    Doctor,
    /// Inspect the current reading configuration.
    Config,
    /// Hold, drain, inspect, or release deployment admission.
    Maintenance {
        #[arg(value_parser=["hold","drain","status","release"])]
        operation: String,
        owner: Option<String>,
    },
}

async fn execute(cli: &Cli) -> Result<Value> {
    let root = weaver::state_root()?;
    match &cli.command {
        Commands::Init {
            annals_config,
            annals_binary,
        } => {
            let binary = annals_binary.clone().unwrap_or(
                PathBuf::from(std::env::var_os("HOME").context("HOME is required")?)
                    .join(".local/bin/annals"),
            );
            weaver::operations::initialize(
                &root,
                &weaver::Config {
                    annals_binary: binary,
                    annals_config: annals_config.clone(),
                },
            )
        }
        Commands::Write { direction, id } => {
            weaver::operations::write_with_id(&root, direction, None, id.as_deref()).await
        }
        Commands::WriteMany { jobs, directions } => Ok(serde_json::to_value(
            weaver::operations::write_many(&root, directions, jobs.get()).await?,
        )?),
        Commands::Revise {
            id,
            direction,
            request_id,
        } => {
            weaver::operations::write_with_id(&root, direction, Some(id), request_id.as_deref())
                .await
        }
        Commands::Resume { id } => weaver::operations::resume(&root, id).await,
        Commands::List { limit } => weaver::store::Store::open(&root, true)?.list(*limit),
        Commands::Show { id } => weaver::store::Store::open(&root, true)?
            .document(id)?
            .view(),
        Commands::Doctor => weaver::operations::doctor(&root).await,
        Commands::Config => Ok(json!({"config":weaver::Config::read(&root)?})),
        Commands::Maintenance { operation, owner } => {
            weaver::operations::maintenance(&root, operation, owner.as_deref()).await
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    if let Some(snapshot) = iatreion_api::requested_status_snapshot_json(
        "weaver",
        env!("CARGO_PKG_VERSION"),
        vec![iatreion_api::declared_unit(
            "weaver",
            "weaver/author",
            None,
            iatreion_api::Intent::OnDemand,
            "weaver.install.operate",
        )],
        false,
    ) {
        chancery_usage::observe("weaver", "status-snapshot");
        println!("{snapshot}");
        return std::process::ExitCode::SUCCESS;
    }
    let cli = chancery_usage::cli::parse::<Cli>("weaver", "");
    match execute(&cli).await {
        Ok(value) => {
            if !cli.json
                && matches!(
                    cli.command,
                    Commands::Show { .. }
                        | Commands::Write { .. }
                        | Commands::Revise { .. }
                        | Commands::Resume { .. }
                )
            {
                if let Some(markdown) = value["markdown"].as_str() {
                    println!("{markdown}");
                } else {
                    println!("{}", json!({"ok":true,"data":value}));
                }
            } else {
                println!("{}", json!({"ok":true,"data":value}));
            }
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            if let Some(code) = nucleus_core::quota_condition(error.as_ref()) {
                println!(
                    "{}",
                    json!({"ok":true,"data":{"outcome":code,"detail":error.to_string()}})
                );
                return std::process::ExitCode::SUCCESS;
            }
            eprintln!("{}", json!({"ok":false,"error":format!("{error:#}")}));
            std::process::ExitCode::FAILURE
        }
    }
}
