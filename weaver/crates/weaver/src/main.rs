use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};

use crate::error::AppResult;
use crate::nucleus::NucleusRunner;
use crate::pipeline::WorkerOutcome;
use crate::project::Project;
use crate::state::{CurrentRun, RunStatus, StateStore};
use crate::validator::Verdict;

mod error;
mod nucleus;
mod pipeline;
mod project;
mod state;
mod validator;

const WAIT_RECOVERY_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Parser)]
#[command(
    name = "weaver",
    version,
    about = "Run durable public-facing narrative workflows through Nucleus",
    long_about = None
)]
struct Cli {
    /// Career repository containing narratives/ and workflow/narrative/.
    #[arg(long, global = true, env = "WEAVER_REPO", default_value = ".")]
    repo: PathBuf,

    /// Weaver's private current-workflow state directory.
    #[arg(long, global = true, env = "WEAVER_STATE_DIR")]
    state_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: TopLevelCommand,
}

#[derive(Debug, Subcommand)]
enum TopLevelCommand {
    /// Durably queue one narrative build and activate the background worker.
    Submit(NarrativeArgs),
    /// Show the sole current workflow.
    Status(RunSelection),
    /// Wait for the sole current workflow to become terminal.
    Wait(RunSelection),
    /// Request cancellation of the sole current workflow.
    Cancel(RunSelection),
    /// Mechanically validate one persisted narrative without invoking Nucleus.
    Check(NarrativeArgs),
    /// Check private state and strict Nucleus readiness.
    Doctor,
    /// Internal background worker entry point.
    Worker {
        #[command(subcommand)]
        command: WorkerCommand,
    },
    /// Coordinate safe requester deployment and maintenance.
    Maintenance {
        #[command(subcommand)]
        command: MaintenanceCommand,
    },
}

#[derive(Clone, Debug, Args)]
struct NarrativeArgs {
    /// Direct narrative child, such as how-i-work or narratives/how-i-work.
    narrative: String,
}

#[derive(Clone, Debug, Args)]
struct RunSelection {
    /// Expected run ID; omit to address the sole current workflow.
    run_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Subcommand)]
enum WorkerCommand {
    /// Claim and run or recover the current workflow.
    Run,
}

#[derive(Clone, Copy, Debug, Subcommand)]
enum MaintenanceCommand {
    /// Prevent new claims and wait for the active worker lock to become free.
    Begin {
        #[arg(long, default_value_t = 60)]
        wait_seconds: u64,
    },
    /// Permit new submissions and worker claims.
    End,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("weaver: {error}");
            error.exit_code()
        }
    }
}

async fn run(cli: Cli) -> AppResult<ExitCode> {
    let Cli {
        repo,
        state_dir,
        command,
    } = cli;
    match command {
        TopLevelCommand::Submit(args) => submit_command(&repo, state_dir, &args),
        TopLevelCommand::Status(selection) => status_command(state_dir, &selection),
        TopLevelCommand::Wait(selection) => wait_command(state_dir, &selection).await,
        TopLevelCommand::Cancel(selection) => cancel_command(state_dir, &selection),
        TopLevelCommand::Check(args) => check_command(&repo, &args),
        TopLevelCommand::Doctor => doctor_command(state_dir).await,
        TopLevelCommand::Worker {
            command: WorkerCommand::Run,
        } => worker_command(state_dir).await,
        TopLevelCommand::Maintenance { command } => maintenance_command(state_dir, command),
    }
}

fn submit_command(
    repo: &std::path::Path,
    state_dir: Option<PathBuf>,
    args: &NarrativeArgs,
) -> AppResult<ExitCode> {
    let project = Project::resolve(repo, &args.narrative, true)?;
    project.validate_existing_stage_tree()?;
    let store = state_store(state_dir)?;
    let current = store.enqueue(project.repo_root, project.slug)?;
    println!(
        "weaver: submitted {}: narratives/{}",
        current.run_id, current.narrative
    );
    if let Err(error) = store.activate_worker() {
        eprintln!("weaver: workflow is durably queued, but worker activation failed: {error}");
    }
    Ok(ExitCode::SUCCESS)
}

fn status_command(state_dir: Option<PathBuf>, selection: &RunSelection) -> AppResult<ExitCode> {
    let store = state_store(state_dir)?;
    let current = store.read_current(selection.run_id.as_deref())?;
    print_current(&current);
    Ok(ExitCode::SUCCESS)
}

async fn wait_command(state_dir: Option<PathBuf>, selection: &RunSelection) -> AppResult<ExitCode> {
    let store = state_store(state_dir)?;
    let mut next_activation = tokio::time::Instant::now();
    loop {
        let current = store.read_current(selection.run_id.as_deref())?;
        if current.status.is_terminal() {
            print_current(&current);
            return Ok(run_exit_code(current.status));
        }
        let now = tokio::time::Instant::now();
        if now >= next_activation {
            if let Err(error) = store.activate_worker() {
                eprintln!("weaver: workflow is durable, but worker activation failed: {error}");
            }
            next_activation = now + WAIT_RECOVERY_INTERVAL;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn cancel_command(state_dir: Option<PathBuf>, selection: &RunSelection) -> AppResult<ExitCode> {
    let store = state_store(state_dir)?;
    let current = store.request_cancel(selection.run_id.as_deref())?;
    println!("weaver: cancellation requested: {}", current.run_id);
    if !current.status.is_terminal()
        && let Err(error) = store.activate_worker()
    {
        eprintln!("weaver: cancellation is durable, but worker activation failed: {error}");
    }
    Ok(ExitCode::SUCCESS)
}

fn check_command(repo: &std::path::Path, args: &NarrativeArgs) -> AppResult<ExitCode> {
    let project = Project::resolve(repo, &args.narrative, false)?;
    let verdict = validator::check(&project)?;
    match verdict {
        Verdict::Pass | Verdict::Revise => {
            println!(
                "weaver: check passed ({}): {}",
                verdict.as_str(),
                project.narrative_relative
            );
            Ok(ExitCode::SUCCESS)
        }
        Verdict::Blocked => {
            eprintln!(
                "weaver: check found a blocked editorial review: {}/04-review/output.md",
                project.narrative_relative
            );
            Ok(ExitCode::from(3))
        }
    }
}

async fn doctor_command(state_dir: Option<PathBuf>) -> AppResult<ExitCode> {
    let store = state_store(state_dir)?;
    store.validate_operational_shape()?;
    let runner = NucleusRunner::for_current_user()?;
    runner.ensure_ready().await?;
    println!("weaver: state and Nucleus are ready");
    Ok(ExitCode::SUCCESS)
}

async fn worker_command(state_dir: Option<PathBuf>) -> AppResult<ExitCode> {
    let store = state_store(state_dir)?;
    match pipeline::run_worker(&store).await? {
        WorkerOutcome::Idle => {
            println!("weaver: worker has no claimable workflow");
            Ok(ExitCode::SUCCESS)
        }
        WorkerOutcome::Busy => {
            println!("weaver: another worker owns the current workflow");
            Ok(ExitCode::SUCCESS)
        }
        WorkerOutcome::Finished(current) => {
            print_current(&current);
            Ok(run_exit_code(current.status))
        }
    }
}

fn maintenance_command(
    state_dir: Option<PathBuf>,
    command: MaintenanceCommand,
) -> AppResult<ExitCode> {
    let store = state_store(state_dir)?;
    match command {
        MaintenanceCommand::Begin { wait_seconds } => {
            store.begin_maintenance(Duration::from_secs(wait_seconds))?;
            println!("weaver: maintenance enabled; no worker holds the run lock");
        }
        MaintenanceCommand::End => {
            store.end_maintenance()?;
            println!("weaver: maintenance disabled");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn state_store(configured: Option<PathBuf>) -> AppResult<StateStore> {
    let root = match configured {
        Some(root) => root,
        None => StateStore::default_root()?,
    };
    StateStore::open(root)
}

fn print_current(current: &CurrentRun) {
    println!("Run: {}", current.run_id);
    println!("Narrative: narratives/{}", current.narrative);
    println!("State: {}", current.status.as_str());
    println!("Completed stages: {}/5", current.next_stage);
    if let Some(verdict) = &current.verdict {
        println!("Verdict: {verdict}");
    }
    if let Some(job_id) = &current.active_job_id {
        println!("Nucleus job: {job_id}");
    }
    if let Some(detail) = &current.detail {
        println!("Detail: {detail}");
    }
}

fn run_exit_code(status: RunStatus) -> ExitCode {
    match status {
        RunStatus::Blocked => ExitCode::from(3),
        RunStatus::Failed | RunStatus::Cancelled => ExitCode::FAILURE,
        RunStatus::Queued | RunStatus::Running | RunStatus::Succeeded => ExitCode::SUCCESS,
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::{Cli, TopLevelCommand};

    #[test]
    fn parses_the_public_run_and_forget_commands() {
        let submit = Cli::try_parse_from(["weaver", "submit", "how-i-work"]);
        assert!(matches!(
            submit.map(|cli| cli.command),
            Ok(TopLevelCommand::Submit(_))
        ));
        let worker = Cli::try_parse_from(["weaver", "worker", "run"]);
        assert!(matches!(
            worker.map(|cli| cli.command),
            Ok(TopLevelCommand::Worker { .. })
        ));
        let maintenance =
            Cli::try_parse_from(["weaver", "maintenance", "begin", "--wait-seconds", "0"]);
        assert!(matches!(
            maintenance.map(|cli| cli.command),
            Ok(TopLevelCommand::Maintenance { .. })
        ));
    }
}
