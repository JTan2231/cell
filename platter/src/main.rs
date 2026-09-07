use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fs2::FileExt as _;
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
    },
    PrepareDaily,
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
    use std::os::unix::fs::OpenOptionsExt as _;
    let cli = Cli::parse();
    anyhow::ensure!(
        cli.stop_after_seconds.is_none()
            || matches!(cli.command, Command::Prepare { .. } | Command::PrepareDaily),
        "--stop-after-seconds applies only to prepare and prepare-daily"
    );
    let home = maintenance::home()?;
    let root = if let Some(root) = cli.state_dir {
        root
    } else {
        platter::default_state_dir(&home)?
    };
    anyhow::ensure!(root.is_absolute(), "state directory must be absolute");
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
    platter::private_dir(&root)?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(root.join("runner.lock"))?;
    lock.try_lock_exclusive()
        .context("another Platter runner is active")?;
    let deadline = cli
        .stop_after_seconds
        .map(|seconds| Instant::now() + Duration::from_secs(seconds));
    match cli.command {
        Command::Init { resume } => {
            workflow::initialize(&root, &resume)?;
            println!("initialized: {}", root.display());
        }
        Command::Prepare { job_id } => {
            let result = workflow::prepare(&root, &job_id, deadline).await?;
            println!("{}: {} ({})", result.id, result.status, result.directory);
        }
        Command::PrepareDaily => {
            let ready = workflow::prepare_daily(&root, deadline).await?;
            println!("{} complete new packets ready", ready.len());
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
            let (_admission, _runner) = maintenance::install_admission(home, root)?;
            let report = readiness::doctor(root).await?;
            if readiness::local_state(root)? {
                Store::open_read_only(root)?.backup(backup)?;
            }
            Some(
                serde_json::json!({"schema_version":1,"backup":if root.join("packets.sqlite3").exists() {Some(backup)} else {None},"readiness":report}),
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
        "daily: up to {} at {:02}:{:02} {}; model=gpt-5.6-sol/max; preparation budget=none; schedule=not installed",
        settings.daily_count, settings.delivery_hour, settings.delivery_minute, settings.timezone
    );
    for record in Store::open_read_only(root)?.list()? {
        println!(
            "{} {} — {} — {}",
            record.id, record.status, record.company, record.title
        );
    }
    Ok(())
}
