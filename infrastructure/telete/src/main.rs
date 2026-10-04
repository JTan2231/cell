mod autofix;
mod broker;
mod deployment;
mod git;
mod host_setup;
mod installation;
mod inventory;
mod manager;
mod model;
mod paths;
mod process;
mod providers;
mod signing;
mod store;
mod validation;
mod workspace;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "telete", version, about = "Rust Cell CI")]
struct Cli {
    /// Separate Telete directory inside the configured external workspace.
    #[arg(long, global = true)]
    state: Option<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Configure or inspect Cell's shared external work volume.
    Storage {
        #[command(subcommand)]
        command: Storage,
    },
    /// Create a paused queue with an explicitly selected baseline.
    Init {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        accepted_baseline: String,
    },
    /// Freeze a committed input and enqueue its delivery lifecycle.
    Submit {
        commit: String,
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        request_id: Option<String>,
        #[arg(long, conflicts_with = "skip_tests")]
        run_tests: bool,
        #[arg(long, conflicts_with = "run_tests")]
        skip_tests: bool,
        #[arg(long, conflicts_with = "no_deploy")]
        deploy: Vec<String>,
        #[arg(long)]
        no_deploy: bool,
        #[arg(long)]
        no_repair: bool,
        #[arg(long)]
        no_notify: bool,
    },
    Status {
        job: Option<String>,
    },
    Wait {
        job: String,
        #[arg(long, default_value_t = 60)]
        timeout: u64,
    },
    Pause,
    Resume,
    Cancel {
        job: String,
    },
    Recover {
        job: String,
    },
    /// Abandon a blocked preparation after reviewing its retained effects.
    AcknowledgePreparation {
        job: String,
    },
    /// Release an interrupted deployment after reviewing its retained effects.
    AcknowledgeDeployment {
        request: String,
    },
    /// Run the queue worker. Admission remains controlled by pause and holds.
    Worker {
        #[arg(long)]
        once: bool,
    },
    Maintenance {
        #[command(subcommand)]
        command: Maintenance,
    },
    Signing {
        #[command(subcommand)]
        command: Signing,
    },
    /// Publish Telete's own executable, provider, and service definition.
    Install,
    Service {
        #[command(subcommand)]
        command: Service,
    },
    /// Install the pinned native test runner on the external workspace.
    PrepareTools,
    /// Inspect Telete gate records.
    Gates,
    #[command(hide = true)]
    InternalValidate {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        base: String,
        #[arg(long)]
        candidate: String,
        #[arg(long)]
        receipt: PathBuf,
        #[arg(long)]
        run_tests: bool,
        #[arg(long)]
        defer_release_builds: bool,
    },
    #[command(hide = true)]
    InternalExecute {
        #[arg(long)]
        directory: PathBuf,
    },
}
#[derive(Subcommand)]
enum Maintenance {
    Hold {
        #[arg(long)]
        owner: String,
    },
    Status,
    Release {
        #[arg(long)]
        owner: String,
    },
}
#[derive(Subcommand)]
enum Signing {
    Status,
    /// Create the initial shared Cell certificate and signing selection.
    CreateLocal,
    Configure {
        #[arg(long)]
        certificate_sha1: String,
        #[arg(long)]
        keychain: PathBuf,
        #[arg(long, default_value = "local.cell")]
        identifier_namespace: String,
        /// Write Cell's shared host policy instead of a Telete state override.
        #[arg(long)]
        host: bool,
    },
}
#[derive(Subcommand)]
enum Storage {
    Configure {
        #[arg(long)]
        volume: PathBuf,
    },
    Status,
}
#[derive(Subcommand)]
enum Service {
    Status,
    Start,
    Stop,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let machine = cli.json;
    let result = execute(cli).await.and_then(|value| {
        if machine {
            Ok(value.to_string())
        } else {
            Ok(serde_json::to_string_pretty(&value)?)
        }
    });
    match result {
        Ok(output) => println!("{output}"),
        Err(error) => {
            if machine {
                eprintln!(
                    "{}",
                    serde_json::json!({"ok":false,"error":format!("{error:#}")})
                );
            } else {
                eprintln!("telete: {error:#}");
            }
            std::process::exit(1);
        }
    }
}

#[allow(clippy::too_many_lines)] // Keep command dispatch in one place.
async fn execute(cli: Cli) -> Result<serde_json::Value> {
    // Bootstrap must work before the workspace selector or a queue exists.
    if let Command::Storage { command } = &cli.command {
        return match command {
            Storage::Configure { volume } => workspace::configure(volume),
            Storage::Status => workspace::status(),
        };
    }
    let state = match cli.state {
        Some(path) => path,
        None => paths::default_root()?,
    };
    let paths = paths::Paths::new(state)?;
    match cli.command {
        Command::Storage { .. } => unreachable!("storage was dispatched before queue paths"),
        Command::Init {
            repo,
            accepted_baseline,
        } => manager::init(&paths, &repo, &accepted_baseline),
        Command::Submit {
            commit,
            repo,
            request_id,
            run_tests,
            skip_tests: _,
            deploy,
            no_deploy,
            no_repair,
            no_notify,
        } => manager::submit(
            &paths,
            &repo,
            &commit,
            manager::SubmitOptions {
                request_id,
                run_tests,
                deploy,
                no_deploy,
                repair: !no_repair,
                notify: !no_notify,
            },
        ),
        Command::Status { job } => manager::status(&paths, job.as_deref()),
        Command::Wait { job, timeout } => manager::wait(&paths, &job, timeout).await,
        Command::Pause => manager::pause(&paths),
        Command::Resume => manager::resume(&paths),
        Command::Cancel { job } => manager::cancel(&paths, &job),
        Command::Recover { job } => manager::recover(&paths, &job).await,
        Command::AcknowledgePreparation { job } => manager::acknowledge_preparation(&paths, &job),
        Command::AcknowledgeDeployment { request } => Ok(serde_json::to_value(
            deployment::acknowledge(&paths, &request)?,
        )?),
        Command::Worker { once } => {
            if once {
                manager::worker_once(&paths).await
            } else {
                manager::worker(&paths).await?;
                Ok(serde_json::json!({"state":"stopped"}))
            }
        }
        Command::Maintenance { command } => match command {
            Maintenance::Hold { owner } => manager::maintenance_hold(&paths, &owner),
            Maintenance::Status => manager::maintenance_status(&paths),
            Maintenance::Release { owner } => manager::maintenance_release(&paths, &owner),
        },
        Command::Signing { command } => match command {
            Signing::Status => signing::status(&paths),
            Signing::CreateLocal => signing::create_local(&paths),
            Signing::Configure {
                certificate_sha1,
                keychain,
                identifier_namespace,
                host,
            } => Ok(serde_json::to_value(if host {
                signing::configure_host(
                    &paths,
                    &certificate_sha1,
                    &keychain,
                    &identifier_namespace,
                )?
            } else {
                signing::configure(&paths, &certificate_sha1, &keychain, &identifier_namespace)?
            })?),
        },
        Command::Install => installation::install(&paths),
        Command::Service { command } => match command {
            Service::Status => installation::service_status(&paths),
            Service::Start => installation::service_start(&paths),
            Service::Stop => installation::service_stop(&paths),
        },
        Command::PrepareTools => {
            Ok(serde_json::json!({"executable":validation::prepare_tools(&paths)?}))
        }
        Command::Gates => broker::status(&paths),
        Command::InternalExecute { directory } => {
            process::execute_request(&paths, &directory)?;
            Ok(serde_json::json!({"state":"completed"}))
        }
        Command::InternalValidate {
            repo,
            base,
            candidate,
            receipt,
            run_tests,
            defer_release_builds,
        } => {
            anyhow::ensure!(
                receipt.starts_with(&paths.root)
                    && !receipt
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir)),
                "receipt must stay inside Telete state"
            );
            let report = validation::validate(
                &paths,
                &repo,
                &model::CommitId::new(base)?,
                &model::CommitId::new(candidate)?,
                run_tests,
                defer_release_builds,
            )?;
            paths::atomic_json(&receipt, &report)?;
            Ok(serde_json::to_value(report)?)
        }
    }
}
