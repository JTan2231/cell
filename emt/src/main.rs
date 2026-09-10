use clap::{Parser, Subcommand, ValueEnum};
use emt::store::{Config, Store};
use emt::{Result, fail};
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "emt",
    version,
    about = "Agent diagnosis and one-off interventions by email"
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init,
    Configure {
        #[arg(long)]
        receiving_domain: Option<String>,
        #[arg(long)]
        cell_root: Option<PathBuf>,
        #[arg(long)]
        agent_cwd: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        email_executable: Option<PathBuf>,
        #[arg(long)]
        clockwork_executable: Option<PathBuf>,
    },
    Pause,
    Resume,
    Status,
    Doctor,
    Worker,
    /// Agent-authored email body on stdin. EMT owns routing and the send identity.
    Send {
        exchange_id: String,
        #[arg(long)]
        subject: String,
    },
    Incident {
        #[command(subcommand)]
        operation: IncidentOperation,
    },
    /// Read an exchange and its Nucleus job reference.
    Run {
        #[command(subcommand)]
        operation: RunOperation,
    },
    Schedule {
        #[arg(value_enum)]
        operation: ScheduleOperation,
    },
    Maintenance {
        #[command(subcommand)]
        operation: MaintenanceOperation,
    },
    Migrate {
        #[arg(long)]
        backup: PathBuf,
    },
}

#[derive(Subcommand)]
enum IncidentOperation {
    List,
    Show { id: String },
}
#[derive(Subcommand)]
enum RunOperation {
    Show { id: String },
}
#[derive(Clone, ValueEnum)]
enum ScheduleOperation {
    Enable,
    Disable,
    Status,
}
#[derive(Subcommand)]
enum MaintenanceOperation {
    Hold { owner: String },
    Status,
    Drain,
    Release { owner: String },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = run(cli.command).await;
    let success = result.is_ok();
    let value = match result {
        Ok(data) => json!({"ok":true,"data":data}),
        Err(error) => json!({"ok":false,"error":{"detail":error.to_string()}}),
    };
    if cli.json {
        println!("{value}");
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
        );
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[allow(clippy::too_many_lines)]
async fn run(command: Command) -> Result<Value> {
    let root = emt::state_root()?;
    match command {
        Command::Init => {
            emt::store::private_directory(&root)?;
            let _admission = emt::gate(&root).enter()?;
            let _lock = emt::store::runner_lock(&root)?;
            Ok(
                json!({"state_root":root,"status":Store::initialize(&root)?.status()?,"paused":true}),
            )
        }
        Command::Configure {
            receiving_domain,
            cell_root,
            agent_cwd,
            model,
            email_executable,
            clockwork_executable,
        } => {
            let _admission = emt::gate(&root).enter()?;
            let _lock = emt::store::runner_lock(&root)?;
            let store = Store::open(&root)?;
            let mut config = Config::load(&root)?;
            if !config.paused || store.outstanding_work()? != 0 {
                return Err(fail(
                    "pause EMT and drain admitted exchanges before changing configuration",
                ));
            }
            if let Some(value) = receiving_domain {
                config.receiving_domain = value;
            }
            if let Some(value) = cell_root {
                config.cell_root = value;
            }
            if let Some(value) = agent_cwd {
                config.agent_cwd = value;
            }
            if let Some(value) = model {
                config.model = value;
            }
            if let Some(value) = email_executable {
                config.email_executable = value;
            }
            if let Some(value) = clockwork_executable {
                config.clockwork_executable = value;
            }
            config.validate()?;
            config.save(&root)?;
            Ok(json!({"configured":true}))
        }
        operation @ (Command::Pause | Command::Resume) => {
            let _admission = if matches!(operation, Command::Pause) {
                emt::gate(&root).recover()?
            } else {
                emt::gate(&root).enter()?
            };
            let _lock = emt::store::runner_lock(&root)?;
            Store::open(&root)?;
            let mut config = Config::load(&root)?;
            let paused = matches!(operation, Command::Pause);
            if !paused {
                config.validate()?;
            }
            let client = clockwork::api::Client::new(&config.clockwork_executable);
            client.configure_emt(if paused {
                None
            } else {
                Some(&config.receiving_domain)
            })?;
            config.paused = paused;
            config.save(&root)?;
            Ok(json!({"paused":paused}))
        }
        Command::Status => {
            let store = Store::open(&root)?;
            Ok(json!({"paused":Config::load(&root)?.paused,"state":store.status()?}))
        }
        Command::Doctor => {
            let store = Store::open(&root)?;
            let config = Config::load(&root)?;
            config.validate()?;
            let sqlite: String = store
                .connection
                .query_row("PRAGMA quick_check", [], |row| row.get(0))?;
            // Inspect providers without submitting a job or changing notification ownership.
            clockwork::api::Client::new(&config.clockwork_executable).incident_feed(0, 1)?;
            let health = tokio::time::timeout(
                std::time::Duration::from_secs(15),
                nucleus_client::NucleusClient::for_current_user()?.health(),
            )
            .await??;
            Ok(
                json!({"sqlite":sqlite,"email_executable_present":config.email_executable.is_file(),
                "nucleus":health,"email_receiving_and_delivery":"not_probed","status":store.status()?}),
            )
        }
        Command::Worker => emt::runner::tick(&root, false).await,
        Command::Send {
            exchange_id,
            subject,
        } => {
            let mut body = String::new();
            std::io::stdin()
                .take(64 * 1024 + 1)
                .read_to_string(&mut body)?;
            emt::mail::compose(&root, &exchange_id, &subject, &body).await
        }
        Command::Incident { operation } => {
            let store = Store::open(&root)?;
            match operation {
                IncidentOperation::List => {
                    let mut query = store.connection.prepare(
                        "SELECT id,binding_key FROM incidents ORDER BY feed_cursor DESC,id",
                    )?;
                    let items=query.query_map([],|row|Ok(json!({"id":row.get::<_,String>(0)?,"binding_key":row.get::<_,String>(1)?})))?
                        .collect::<std::result::Result<Vec<_>,_>>()?;
                    Ok(json!({"items":items}))
                }
                IncidentOperation::Show { id } => {
                    Ok(json!({"incident":store.incident(&id)?,"exchanges":store.exchanges(&id)?}))
                }
            }
        }
        Command::Run {
            operation: RunOperation::Show { id },
        } => Ok(serde_json::to_value(Store::open(&root)?.exchange(&id)?)?),
        Command::Schedule { operation } => emt::installation::schedule(
            &root,
            match operation {
                ScheduleOperation::Enable => "enable",
                ScheduleOperation::Disable => "disable",
                ScheduleOperation::Status => "status",
            },
        ),
        Command::Maintenance { operation } => maintenance(&root, operation).await,
        Command::Migrate { backup } => migrate(&root, &backup),
    }
}

fn maintenance_status(root: &Path) -> Result<Value> {
    let gate = emt::gate(root).status()?;
    let (outstanding, idle) = match emt::store::runner_lock(root) {
        Ok(_lock) => (
            Some(if root.join("emt.sqlite3").exists() {
                Store::open(root)?.outstanding_work()?
            } else {
                0
            }),
            true,
        ),
        Err(_) => (None, false),
    };
    Ok(
        json!({"maintenance":{"protocol_version":1,"holds":gate.holds,
        "drained":gate.drained && idle && outstanding==Some(0),"outstanding_work_record_count":outstanding}}),
    )
}

async fn maintenance(root: &Path, operation: MaintenanceOperation) -> Result<Value> {
    let gate = emt::gate(root);
    match operation {
        MaintenanceOperation::Hold { owner } => {
            gate.hold(&owner)?;
        }
        MaintenanceOperation::Status => {}
        MaintenanceOperation::Drain => {
            if gate.status()?.holds.is_empty() {
                return Err(fail("maintenance drain requires a hold"));
            }
            // A held idle product needs no worker pass. A busy admitted pass
            // remains a waiting drain observation, including an unknown count.
            let status = maintenance_status(root)?;
            if status["maintenance"]["drained"] == true
                || status["maintenance"]["outstanding_work_record_count"].is_null()
            {
                return Ok(status);
            }
            if root.join("emt.sqlite3").exists() {
                emt::runner::tick(root, true).await?;
            }
        }
        MaintenanceOperation::Release { owner } => {
            if maintenance_status(root)?["maintenance"]["drained"] != true {
                return Err(fail(
                    "admitted exchanges must drain before releasing maintenance",
                ));
            }
            gate.release(&owner)?;
        }
    }
    maintenance_status(root)
}

fn migrate(root: &Path, backup: &Path) -> Result<Value> {
    if !backup.is_absolute()
        || backup == root.join("emt.sqlite3")
        || backup == root.join("config.json")
    {
        return Err(fail("backup must be a separate absolute path"));
    }
    let gate = emt::gate(root);
    let owner = std::env::var("CELL_DEPLOYMENT_RUN_ID").ok();
    let _admission = if let Some(owner) = &owner {
        gate.enter_for(owner)?
    } else {
        gate.enter()?
    };
    let _lock = emt::store::runner_lock(root)?;
    if !root.join("emt.sqlite3").exists() {
        Store::initialize(root)?;
        return Ok(json!({"schema_version":1,"backup":null}));
    }
    let store = Store::open(root)?;
    if store.outstanding_work()? != 0 {
        return Err(fail("backup requires drained exchanges"));
    }
    drop(store);
    emt::store::private_directory(
        backup
            .parent()
            .ok_or_else(|| fail("backup has no parent"))?,
    )?;
    copy_backup(&root.join("emt.sqlite3"), backup)?;
    copy_backup(
        &root.join("config.json"),
        &backup.with_extension("config.json"),
    )?;
    Ok(
        json!({"schema_version":1,"backup":backup,"config_backup":backup.with_extension("config.json")}),
    )
}

fn copy_backup(source: &Path, destination: &Path) -> Result<()> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)
    {
        Ok(mut file) => {
            std::io::copy(&mut File::open(source)?, &mut file)?;
            file.flush()?;
            file.sync_all()?;
            File::open(
                destination
                    .parent()
                    .ok_or_else(|| fail("backup has no parent"))?,
            )?
            .sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if fs::read(source)? != fs::read(destination)? {
                return Err(fail("existing backup differs from current drained state"));
            }
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
