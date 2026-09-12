use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Research conversations or accepted decisions and email an ASD-STE100 report"
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
        #[command(flatten)]
        source: SourceArgs,
        /// Inclusive period start in RFC3339, for an ad hoc report.
        #[arg(long, requires_all = ["ad_hoc", "until"], conflicts_with_all = ["scheduled", "brief"])]
        from: Option<chrono::DateTime<chrono::FixedOffset>>,
        /// Exclusive period end in RFC3339, for an ad hoc report.
        #[arg(long, requires_all = ["ad_hoc", "from"], conflicts_with_all = ["scheduled", "brief"])]
        until: Option<chrono::DateTime<chrono::FixedOffset>>,
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
        #[command(flatten)]
        source: SourceArgs,
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
    Doctor {
        #[command(flatten)]
        source: SourceArgs,
    },
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

#[derive(clap::Args)]
struct SourceArgs {
    /// Report source. Defaults to conversations.
    #[arg(long, value_enum)]
    report: Option<paperboy::ReportKind>,
    /// Explicit identity-bound Annals decisions-library config.
    #[arg(long)]
    annals_config: Option<PathBuf>,
}

impl SourceArgs {
    fn options(&self, period: Option<(i64, i64)>) -> paperboy::operations::ReportOptions {
        paperboy::operations::ReportOptions {
            kind: self.report.unwrap_or_default(),
            annals_config: self.annals_config.clone(),
            period,
        }
    }
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
            source,
            from,
            until,
        } => {
            ensure!(
                *ad_hoc || *scheduled || brief.is_some(),
                "choose --ad-hoc, --scheduled, or --brief ID"
            );
            ensure!(
                brief.is_none() || source.report.is_none() && source.annals_config.is_none(),
                "--brief uses its retained report source"
            );
            let period = from
                .zip(*until)
                .map(|(from, until)| (from.timestamp(), until.timestamp()));
            paperboy::operations::run(
                &root,
                *ad_hoc,
                brief.as_deref(),
                *retry_agent,
                &source.options(period),
            )
            .await
        }
        Commands::List { limit } => paperboy::store::Store::open(&root)?.list(*limit),
        Commands::Show { id } => paperboy::operations::show(&root, id),
        Commands::Preview { id } => {
            let brief = paperboy::store::Store::open(&root)?.brief(id)?;
            Ok(json!({"brief_id":brief.id,"subject":brief.subject,"body":brief.body}))
        }
        Commands::Schedule { operation, source } => {
            ensure!(
                operation == "enable" || source.report.is_none() && source.annals_config.is_none(),
                "source options apply to schedule enable"
            );
            paperboy::operations::schedule(
                &root,
                operation,
                source.report.unwrap_or_default(),
                source.annals_config.as_deref(),
            )
        }
        Commands::Reconcile {
            attempt,
            receipt,
            not_accepted,
        } => paperboy::operations::reconcile(&root, attempt, receipt.as_deref(), *not_accepted),
        Commands::Doctor { source } => {
            paperboy::operations::doctor(&root, &source.options(None)).await
        }
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
            if let Some(code) = nucleus_core::quota_condition(error.as_ref()) {
                println!("{}", json!({"ok":true,"data":{"outcome":code}}));
                return std::process::ExitCode::SUCCESS;
            }
            eprintln!("{}", json!({"ok":false,"error":error.to_string()}));
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_period_requires_both_bounds_and_an_ad_hoc_run() {
        let args = [
            "paperboy",
            "run",
            "--ad-hoc",
            "--report",
            "decisions",
            "--annals-config",
            "/private/decisions.toml",
            "--from",
            "2026-09-09T09:00:00-05:00",
            "--until",
            "2026-09-10T09:00:00-05:00",
        ];
        assert!(Cli::try_parse_from(args).is_ok());
        assert!(Cli::try_parse_from(&args[..9]).is_err());
        let mut scheduled = args;
        scheduled[2] = "--scheduled";
        assert!(Cli::try_parse_from(scheduled).is_err());
        assert!(
            Cli::try_parse_from([
                "paperboy",
                "schedule",
                "enable",
                "--report",
                "decisions",
                "--annals-config",
                "/private/decisions.toml"
            ])
            .is_ok()
        );
    }
}
