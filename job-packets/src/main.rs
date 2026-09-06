use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fs2::FileExt as _;
use job_packets::{ad_hoc, store::Store, workflow};
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
    state_dir: Option<PathBuf>,
    /// Bound preparation duration and cancel its live Nucleus job (prepare commands only).
    #[arg(long, global = true)]
    stop_after_seconds: Option<u64>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("job-packets: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let cli = Cli::parse();
    anyhow::ensure!(
        cli.stop_after_seconds.is_none()
            || matches!(cli.command, Command::Prepare { .. } | Command::PrepareDaily),
        "--stop-after-seconds applies only to prepare and prepare-daily"
    );
    let root = cli.state_dir.unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/job-packets")
    });
    anyhow::ensure!(root.is_absolute(), "state directory must be absolute");
    job_packets::private_dir(&root)?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(root.join("runner.lock"))?;
    if !matches!(cli.command, Command::Status) {
        lock.try_lock_exclusive()
            .context("another Job Packets runner is active")?;
    }
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
        Command::Status => {
            let settings = workflow::config(&root)?;
            println!(
                "daily: up to {} at {:02}:{:02} {}; model=gpt-5.6-sol/max; preparation budget=none; schedule=not installed",
                settings.daily_count,
                settings.delivery_hour,
                settings.delivery_minute,
                settings.timezone
            );
            for record in Store::open(&root)?.list()? {
                println!(
                    "{} {} — {} — {}",
                    record.id, record.status, record.company, record.title
                );
            }
        }
    }
    Ok(())
}
