use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Research the preceding 24 hours of conversations and email an ASD-STE100 daily report"
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize an absent private report database.
    Init,
    /// Generate and send one report, or resume its exact retained assignment.
    Run {
        #[arg(long,conflicts_with_all=["scheduled","brief"])]
        ad_hoc: bool,
        #[arg(long, conflicts_with = "brief")]
        scheduled: bool,
        #[arg(long)]
        brief: Option<String>,
        #[arg(long, requires = "brief")]
        retry_agent: bool,
    },
    /// List report metadata; the limit applies to briefs.
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Read one brief, its exact email text, and its attempt history.
    Show { id: String },
    /// Show the stored email without starting an agent or sending it.
    Preview { id: String },
    /// Operate the pinned daily local 09:00 Clockwork activation.
    Schedule {
        #[arg(value_parser=["enable","disable","status"])]
        operation: String,
    },
    /// Resolve an uncertain submission using separately observed provider evidence.
    Reconcile {
        attempt: String,
        #[arg(long, conflicts_with = "not_accepted")]
        receipt: Option<String>,
        #[arg(long)]
        not_accepted: bool,
    },
    /// Inspect deployment readiness without creating a report or model job.
    Doctor,
    /// Initialize schema one or preserve a complete deployment backup.
    Migrate {
        #[arg(long)]
        backup: PathBuf,
    },
    /// Hold, drain, inspect, or release requester admission.
    Maintenance {
        #[arg(value_parser=["hold","drain","status","release"])]
        operation: String,
        owner: Option<String>,
    },
}

async fn execute(cli: &Cli) -> Result<Value> {
    let root = paperboy::state_root()?;
    match &cli.command {
        Commands::Init => {
            let _guard = paperboy::gate(&root).enter()?;
            let _lock = paperboy::store::runner_lock(&root)?;
            paperboy::store::Store::initialize(&root)?;
            Ok(json!({"initialized":true,"schema_version":1}))
        }
        Commands::Run {
            ad_hoc,
            scheduled,
            brief,
            retry_agent,
        } => {
            ensure!(
                *ad_hoc || *scheduled || brief.is_some(),
                "choose --ad-hoc, --scheduled, or --brief ID"
            );
            paperboy::operations::run(&root, *ad_hoc, brief.as_deref(), *retry_agent).await
        }
        Commands::List { limit } => paperboy::store::Store::open(&root)?.list(*limit),
        Commands::Show { id } => paperboy::operations::show(&root, id),
        Commands::Preview { id } => {
            let brief = paperboy::store::Store::open(&root)?.brief(id)?;
            Ok(json!({"brief_id":brief.id,"subject":brief.subject,"body":brief.body}))
        }
        Commands::Schedule { operation } => paperboy::operations::schedule(&root, operation),
        Commands::Reconcile {
            attempt,
            receipt,
            not_accepted,
        } => paperboy::operations::reconcile(&root, attempt, receipt.as_deref(), *not_accepted),
        Commands::Doctor => paperboy::operations::doctor(&root).await,
        Commands::Migrate { backup } => paperboy::operations::migrate(&root, backup),
        Commands::Maintenance { operation, owner } => {
            paperboy::operations::maintenance(&root, operation, owner.as_deref()).await
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    if let Some(snapshot) = iatreion_api::requested_status_snapshot_json(
        "paperboy",
        env!("CARGO_PKG_VERSION"),
        vec![iatreion_api::declared_unit(
            "paperboy",
            "paperboy/daily",
            Some("paperboy/daily"),
            iatreion_api::Intent::Active,
            "paperboy.install.operate",
        )],
        false,
    ) {
        println!("{snapshot}");
        return std::process::ExitCode::SUCCESS;
    }
    let cli = Cli::parse();
    match execute(&cli).await {
        Ok(value) => {
            println!("{}", json!({"ok":true,"data":value}));
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", json!({"ok":false,"error":error.to_string()}));
            std::process::ExitCode::FAILURE
        }
    }
}
