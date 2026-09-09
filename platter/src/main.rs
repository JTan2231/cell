use anyhow::Result;
use clap::{Parser, Subcommand};
use platter::{ad_hoc, maintenance, readiness, store::Store, workflow};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    version,
    about = "Prepare private job briefs and fixed-template resumes"
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// Bound preparation duration and cancel its live Nucleus job (prepare commands only).
    #[arg(long, global = true)]
    stop_after_seconds: Option<u64>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect prerequisites without preparing packets or sending mail.
    Doctor {
        /// Prove retained state compatibility without execution or delivery prerequisites.
        #[arg(long)]
        state_only: bool,
    },
    /// Product-owned deployment admission and drain.
    Maintenance {
        #[command(subcommand)]
        command: Maintenance,
    },
    /// Verify compatible state and retain a consistent pre-install backup.
    Migrate {
        #[arg(long)]
        backup: PathBuf,
    },
    Init {
        #[arg(long)]
        resume: PathBuf,
    },
    Prepare {
        job_id: String,
        /// Restart incomplete preparation with new source capture and no prior model context.
        #[arg(long)]
        fresh: bool,
    },
    PrepareDaily,
    /// Prepare, freeze and send today's edition with applicable send authorization.
    RunDaily,
    Preview {
        day: String,
        #[arg(long)]
        ad_hoc: Option<String>,
        #[arg(long = "packet", requires = "ad_hoc")]
        packets: Vec<String>,
        #[arg(long, requires = "ad_hoc")]
        brief_overrides: Option<PathBuf>,
    },
    Send {
        day: String,
        #[arg(long)]
        ad_hoc: Option<String>,
        #[arg(long, requires = "ad_hoc")]
        email_executable: Option<PathBuf>,
    },
    /// Change whether Platter may select this retained opportunity.
    Eligibility {
        job_id: String,
        #[arg(value_parser = clap::value_parser!(bool))]
        eligible: bool,
    },
    /// Export one retained artifact to an explicitly selected destination.
    Export {
        artifact_id: String,
        output: PathBuf,
    },
    Status,
}

#[derive(Subcommand)]
enum Maintenance {
    Status,
    Hold { owner: String },
    Drain,
    Release { owner: String },
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        if std::env::args().any(|argument| argument == "--json") {
            println!(
                "{}",
                serde_json::json!({"ok":false,"error":{"detail":format!("{error:#}")}})
            );
        } else {
            eprintln!("platter: {error:#}");
        }
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_lines)] // One serial dispatch owns admission and the runner lock.
async fn run() -> Result<()> {
    let cli = Cli::parse();
    anyhow::ensure!(
        cli.stop_after_seconds.is_none()
            || matches!(cli.command, Command::Prepare { .. } | Command::PrepareDaily),
        "--stop-after-seconds applies only to prepare and prepare-daily"
    );
    let home = maintenance::home()?;
    let root = platter::default_state_dir(&home)?;
    if let Some(selected) = cli.state_dir {
        anyhow::ensure!(
            selected == root,
            "Platter uses one canonical database; --state-dir must select its canonical state directory"
        );
    }
    let operation = administrative(&cli.command, &home, &root).await?;
    if let Some(data) = operation {
        println!("{}", serde_json::json!({"ok":true,"data":data}));
        return Ok(());
    }
    anyhow::ensure!(
        !cli.json,
        "--json applies to doctor, migrate and maintenance"
    );
    if matches!(cli.command, Command::Status) {
        print_status(&root)?;
        return Ok(());
    }
    let _admission = maintenance::gate(&home).enter()?;
    let deadline = cli
        .stop_after_seconds
        .map(|seconds| Instant::now() + Duration::from_secs(seconds));
    match cli.command {
        Command::Init { resume } => {
            workflow::initialize(&root, &resume)?;
            println!("initialized: {}", root.display());
        }
        Command::Prepare { job_id, fresh } => {
            let result = if fresh {
                workflow::prepare_fresh(&root, &job_id, deadline).await?
            } else {
                workflow::prepare(&root, &job_id, deadline).await?
            };
            println!("{}: {}", result.id, result.status);
        }
        Command::PrepareDaily => {
            let ready = workflow::prepare_daily(&root, deadline).await?;
            println!("{} complete new packets ready", ready.len());
        }
        Command::RunDaily => {
            if let Some(edition) = workflow::run_daily(&root, chrono::Utc::now()).await? {
                println!(
                    "{}: {} ({} packets)",
                    edition.day,
                    edition.status,
                    edition.packet_ids.len()
                );
            } else {
                println!("No complete new packets ready; no edition created or sent");
            }
        }
        Command::Preview {
            day,
            ad_hoc,
            packets,
            brief_overrides,
        } => {
            let edition = if let Some(run_id) = ad_hoc {
                Some(ad_hoc::preview(
                    &root,
                    &day,
                    &run_id,
                    &packets,
                    brief_overrides.as_deref(),
                )?)
            } else {
                workflow::preview(&root, &day).await?
            };
            if let Some(edition) = edition {
                println!("{}\n\n{}", edition.subject, edition.body);
                for path in edition.attachments {
                    println!("Attachment: {path}");
                }
            } else {
                println!("No complete new packets ready; no edition created");
            }
        }
        Command::Send {
            day,
            ad_hoc,
            email_executable,
        } => {
            let edition = if let Some(run_id) = ad_hoc {
                ad_hoc::send(&root, &day, &run_id, email_executable.as_deref())?
            } else {
                workflow::send(&root, &day)?
            };
            println!("{}: {}", edition.day, edition.status);
        }
        Command::Eligibility { job_id, eligible } => {
            Store::open(&root)?.set_eligible(&job_id, eligible)?;
            println!("{job_id}: eligible={eligible}");
        }
        Command::Export {
            artifact_id,
            output,
        } => {
            use std::io::Write as _;
            use std::os::unix::fs::OpenOptionsExt as _;
            anyhow::ensure!(output.is_absolute(), "export destination must be absolute");
            let artifact = Store::open_read_only(&root)?.artifact(&artifact_id)?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&output)?;
            file.write_all(&artifact.content)?;
            file.sync_all()?;
            println!("exported: {}", output.display());
        }
        Command::Status
        | Command::Doctor { .. }
        | Command::Maintenance { .. }
        | Command::Migrate { .. } => {
            unreachable!("administrative commands returned before mutation admission")
        }
    }
    Ok(())
}

async fn administrative(
    command: &Command,
    home: &std::path::Path,
    root: &std::path::Path,
) -> Result<Option<serde_json::Value>> {
    let operation = match command {
        Command::Doctor { state_only } => Some(if *state_only {
            serde_json::json!({"compatible":true,"initialized":readiness::local_state(root)?,"state_dir":root})
        } else {
            readiness::doctor(root).await?
        }),
        Command::Migrate { backup } => {
            anyhow::ensure!(backup.is_absolute(), "backup path must be absolute");
            let status = maintenance::status(home, root).await?;
            anyhow::ensure!(
                status.drained,
                "migration requires drained requester work; run maintenance drain first"
            );
            let (_admission, _runner) = maintenance::install_admission(home, root)?;
            platter::migration::migrate(root, backup)?;
            let report = readiness::doctor(root).await?;
            Some(
                serde_json::json!({"schema_version":platter::store::SCHEMA_VERSION,"backup":backup,"readiness":report}),
            )
        }

        Command::Maintenance { command } => {
            let status = match command {
                Maintenance::Status => maintenance::status(home, root).await?,
                Maintenance::Hold { owner } => {
                    maintenance::gate(home).hold(owner)?;
                    maintenance::status(home, root).await?
                }
                Maintenance::Drain => maintenance::drain(home, root).await?,
                Maintenance::Release { owner } => {
                    maintenance::gate(home).release(owner)?;
                    maintenance::status(home, root).await?
                }
            };
            Some(serde_json::to_value(status)?)
        }
        _ => None,
    };
    Ok(operation)
}

fn print_status(root: &std::path::Path) -> Result<()> {
    let settings = workflow::config(root)?;
    println!("state: {}", root.display());
    println!(
        "daily: up to {} packets; edition timezone={}; model=gpt-5.6-sol/max; preparation budget=none; schedule=external (inspect Clockwork)",
        settings.daily_count, settings.timezone
    );
    for record in Store::open_read_only(root)?.list()? {
        println!(
            "{} {} eligible={} — {} — {}",
            record.id,
            record.status,
            Store::open_read_only(root)?.is_eligible(&record.opportunity)?,
            record.company,
            record.title
        );
    }
    Ok(())
}
