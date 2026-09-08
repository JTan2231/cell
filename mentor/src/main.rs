use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use mentor::store::{Config, Store, private_directory, runner_lock};
use mentor::{Result, fail};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(
    name = "mentor",
    version,
    about = "Daily system design problems and email critiques"
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create private state with the bundled collection. Starts paused.
    Init,
    /// Set daily delivery time and the installed Email interface.
    Configure {
        #[arg(long)]
        time: Option<String>,
        #[arg(long)]
        timezone: Option<String>,
        /// Earliest delivery date in the configured time zone (YYYY-MM-DD).
        #[arg(long)]
        first_delivery_date: Option<String>,
        #[arg(long)]
        receiving_domain: Option<String>,
        #[arg(long)]
        email_executable: Option<PathBuf>,
    },
    Pause,
    Resume,
    /// Show operational counts. No answer or critique content is exposed.
    Status,
    /// Run one bounded pass through selection, receiving, grading and sending.
    #[command(alias = "worker")]
    Tick,
    /// Import a versioned collection exported from the Mentor desktop source.
    ImportCorpus {
        path: PathBuf,
    },
    /// Inspect local compatibility. Does not send mail or submit model work.
    Doctor,
    Schedule {
        #[arg(value_enum)]
        operation: ScheduleOperation,
    },
    Maintenance {
        #[command(subcommand)]
        operation: MaintenanceOperation,
    },
    /// Back up drained metadata and initialize or accept the supported schema.
    Migrate {
        #[arg(long)]
        backup: PathBuf,
    },
}

#[derive(Clone, ValueEnum)]
enum ScheduleOperation {
    Enable,
    Disable,
    Status,
}

#[derive(Subcommand)]
enum MaintenanceOperation {
    Hold {
        owner: String,
    },
    Status,
    /// Continue one pass of already admitted work while a hold prevents new work.
    Drain,
    Release {
        owner: String,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command).await {
        Ok(data) => {
            let reply = json!({"ok":true,"data":data});
            if cli.json {
                println!("{reply}");
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&reply).unwrap_or_else(|_| reply.to_string())
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            // Worker stages use static error codes. Never include mail or model
            // output in command errors or activation logs.
            let reply = json!({"ok":false,"error":{"detail":error.to_string()}});
            if cli.json {
                println!("{reply}");
            } else {
                eprintln!("{reply}");
            }
            ExitCode::FAILURE
        }
    }
}

async fn run(command: Command) -> Result<Value> {
    let root = mentor::state_root()?;
    match command {
        Command::Tick => mentor::runner::tick(&root, false).await,
        Command::Schedule { operation } => mentor::installation::schedule(
            &root,
            match operation {
                ScheduleOperation::Enable => "enable",
                ScheduleOperation::Disable => "disable",
                ScheduleOperation::Status => "status",
            },
        ),
        Command::Maintenance { operation } => maintenance(&root, operation).await,
        Command::Migrate { backup } => migrate(&root, &backup),
        Command::Doctor => doctor(&root),
        Command::Init => {
            private_directory(&root)?;
            let _admission = mentor::gate(&root).enter()?;
            let _lock = runner_lock(&root)?;
            let store = Store::initialize(&root)?;
            Ok(json!({"state_root":root,"status":store.status()?}))
        }
        Command::Status => {
            let _lock = runner_lock(&root)?;
            Store::open(&root)?.status()
        }
        Command::Configure {
            time,
            timezone,
            first_delivery_date,
            receiving_domain,
            email_executable,
        } => {
            let _admission = mentor::gate(&root).enter()?;
            let _lock = runner_lock(&root)?;
            let store = Store::open(&root)?;
            let mut config = store.config()?;
            if let Some(time) = time {
                let (hour, minute) = time
                    .split_once(':')
                    .ok_or_else(|| fail("time must be HH:MM"))?;
                if hour.len() != 2 || minute.len() != 2 {
                    return Err(fail("time must be HH:MM"));
                }
                config.hour = hour.parse().map_err(|_| fail("time must be HH:MM"))?;
                config.minute = minute.parse().map_err(|_| fail("time must be HH:MM"))?;
            }
            if let Some(timezone) = timezone {
                config.timezone = timezone;
            }
            if let Some(date) = first_delivery_date {
                config.first_delivery_date = Some(date);
            }
            if let Some(domain) = receiving_domain {
                config.receiving_domain = domain;
            }
            if let Some(executable) = email_executable {
                config.email_executable = executable;
            }
            store.set_config(&config)?;
            Ok(json!({"status":store.status()?}))
        }
        Command::Pause => set_pause(&root, true),
        Command::Resume => set_pause(&root, false),
        Command::ImportCorpus { path } => {
            let _admission = mentor::gate(&root).enter()?;
            let _lock = runner_lock(&root)?;
            let mut body = String::new();
            File::open(path)?
                .take(8 * 1024 * 1024 + 1)
                .read_to_string(&mut body)?;
            let corpus = mentor::corpus::Corpus::from_json(&body)?;
            let store = Store::open(&root)?;
            let id = store.import_corpus(&corpus)?;
            Ok(json!({"active_corpus":id,"problem_count":corpus.problems.len()}))
        }
    }
}

fn set_pause(root: &Path, paused: bool) -> Result<Value> {
    // Pausing reduces admission and remains available under a deployment hold.
    let gate = mentor::gate(root);
    let _admission = if paused {
        gate.recover()?
    } else {
        gate.enter()?
    };
    let _lock = runner_lock(root)?;
    let store = Store::open(root)?;
    let mut config = store.config()?;
    config.paused = paused;
    store.set_config(&config)?;
    Ok(json!({"paused":paused}))
}

fn maintenance_status(root: &Path) -> Result<Value> {
    let gate = mentor::gate(root).status()?;
    let (outstanding, runner_idle) = match runner_lock(root) {
        Ok(_lock) => {
            let count = if root.join("mentor.sqlite3").try_exists()? {
                Store::open(root)?.outstanding_work()?
            } else {
                0
            };
            (Some(count), true)
        }
        Err(_) => (None, false),
    };
    Ok(
        json!({"maintenance":{"protocol_version":1,"holds":gate.holds,
        "drained":gate.drained && runner_idle && outstanding==Some(0),
        "outstanding_work_record_count":outstanding}}),
    )
}

async fn maintenance(root: &Path, operation: MaintenanceOperation) -> Result<Value> {
    let gate = mentor::gate(root);
    match operation {
        MaintenanceOperation::Hold { owner } => {
            gate.hold(&owner)?;
        }
        MaintenanceOperation::Status => {}
        MaintenanceOperation::Drain => {
            if gate.status()?.holds.is_empty() {
                return Err(fail("maintenance drain requires a hold"));
            }
            if root.join("mentor.sqlite3").try_exists()? {
                mentor::runner::tick(root, true).await?;
            }
        }
        MaintenanceOperation::Release { owner } => {
            let status = maintenance_status(root)?;
            if status["maintenance"]["drained"] != true {
                return Err(fail(
                    "maintenance cannot be released until admitted work has drained",
                ));
            }
            gate.release(&owner)?;
        }
    }
    maintenance_status(root)
}

fn doctor(root: &Path) -> Result<Value> {
    let _lock = runner_lock(root)?;
    let installation = mentor::installation::installation_status(root)?;
    let (config, state) = if root.join("mentor.sqlite3").try_exists()? {
        let store = Store::open(root)?;
        (
            store.config()?,
            json!({"initialized":true,"schema_version":1,"status":store.status()?}),
        )
    } else {
        (
            Config::defaults()?,
            json!({"initialized":false,"schema_version":null}),
        )
    };
    let email_present = fs::metadata(&config.email_executable)
        .is_ok_and(|metadata| metadata.is_file() && metadata.mode() & 0o111 != 0);
    Ok(
        json!({"local_state_compatible":true,"state":state,"installation":installation,
        "email_executable_present":email_present,"email_executable":config.email_executable,
        "email_api_and_receiving_permission":"not_probed","nucleus_execution_readiness":"not_probed",
        "receiving_domain":config.receiving_domain,"live_delivery_readiness":"not_probed"}),
    )
}

fn migrate(root: &Path, backup: &Path) -> Result<Value> {
    if !backup.is_absolute() || backup == root.join("mentor.sqlite3") {
        return Err(fail("backup must be a separate absolute path"));
    }
    private_directory(root)?;
    let gate = mentor::gate(root);
    let owner = std::env::var("CELL_DEPLOYMENT_RUN_ID").ok();
    let _admission = if let Some(owner) = &owner {
        gate.enter_for(owner)?
    } else {
        gate.enter()?
    };
    let _lock = runner_lock(root)?;
    let source = root.join("mentor.sqlite3");
    let existed = source.try_exists()?;
    if existed {
        let store = Store::open(root)?;
        store.expire(mentor::now())?;
        if store.outstanding_work()? != 0 {
            return Err(fail(
                "migration requires drained work; pending content must not enter a backup",
            ));
        }
        let content_count:i64=store.connection.query_row("SELECT (SELECT count(*) FROM incoming WHERE answer IS NOT NULL OR request_json IS NOT NULL) + (SELECT count(*) FROM outbox WHERE payload IS NOT NULL)",[],|row|row.get(0))?;
        if content_count != 0 {
            return Err(fail(
                "migration found retained temporary content; backup refused",
            ));
        }
        drop(store);
        private_directory(
            backup
                .parent()
                .ok_or_else(|| fail("backup has no parent"))?,
        )?;
        copy_backup(&source, backup)?;
    }
    let store = Store::initialize(root)?;
    Ok(
        json!({"schema_version":1,"backup":if existed {Some(backup)} else {None},"status":store.status()?}),
    )
}

fn copy_backup(source: &Path, destination: &Path) -> Result<()> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)
    {
        Ok(mut backup) => {
            std::io::copy(&mut File::open(source)?, &mut backup)?;
            backup.flush()?;
            backup.sync_all()?;
            File::open(
                destination
                    .parent()
                    .ok_or_else(|| fail("backup has no parent"))?,
            )?
            .sync_all()?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(destination)?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.nlink() != 1
                || metadata.mode() & 0o077 != 0
                || fs::read(source)? != fs::read(destination)?
            {
                return Err(fail(
                    "existing migration backup does not match drained state",
                ));
            }
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}
