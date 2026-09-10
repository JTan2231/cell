#![allow(clippy::missing_errors_doc)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

mod error;
mod executor;
mod launchd;
mod lock;
mod manifest;
mod model;
mod notification;
mod paths;
mod status;
mod store;

use std::io::Read as _;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use clockwork::api;
use serde::Serialize;

use crate::error::{Context as _, Error, Result};
use crate::launchd::SystemLaunchd;
use crate::model::Trigger;
use crate::paths::Layout;
use crate::store::Store;

#[derive(Debug, Parser)]
#[command(
    name = "clockwork",
    version,
    about = "Run immutable, current-user scheduled activations"
)]
struct Cli {
    /// Emit one JSON envelope on stdout, or an error envelope on stderr.
    #[arg(long, global = true)]
    json: bool,
    /// Isolate all Clockwork-owned paths (for installation tests only).
    #[arg(long, global = true, hide = true)]
    state_root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Emit Iatreion's read-only operational observation.
    StatusSnapshot,
    /// Register and inspect immutable activation definitions.
    Definition {
        #[command(subcommand)]
        command: DefinitionCommand,
    },
    /// Select, disable, and inspect stable scheduled bindings.
    Binding {
        #[command(subcommand)]
        command: BindingCommand,
    },
    /// Run the currently selected definition for a key now.
    Run { key: String },
    /// Read activation history, newest first.
    History {
        key: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Include definition digests and process identities.
        #[arg(long)]
        details: bool,
    },
    /// Check private state and local runtime prerequisites.
    Doctor,
    /// Explicitly migrate the quiescent schema-one database after retaining a backup.
    Migrate {
        #[arg(long)]
        backup: PathBuf,
    },
    /// Inspect retained scheduling failure incidents.
    Incident {
        #[command(subcommand)]
        command: IncidentCommand,
    },
    /// Deliver or explicitly recover pause notifications, independently of product admission.
    Notification {
        #[command(subcommand)]
        command: NotificationCommand,
    },
    /// Report a terminal product failure for the current activation (normally use `api::report_abend`).
    Abend {
        activation_id: String,
        #[arg(long)]
        code: String,
        #[arg(long)]
        occurrence: String,
    },
    /// Private launchd admission path. It accepts a stable key only.
    #[command(name = "__launchd", hide = true)]
    Launchd { key: String },
    /// Private parent-child handshake before the registered image replaces this process.
    #[command(name = "__exec", hide = true)]
    Exec {
        key: String,
        activation_id: String,
        status_file: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum DefinitionCommand {
    /// Validate and register one TOML definition.
    Register { file: PathBuf },
    /// List immutable registered definitions.
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show one immutable definition by digest.
    Show { digest: String },
}

#[derive(Debug, Subcommand)]
enum IncidentCommand {
    Feed {
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    List {
        key: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Show {
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum NotificationCommand {
    /// Configure EMT preference for new incidents. Existing routes and claims remain retained.
    Emt {
        #[arg(long, required_unless_present = "disable", conflicts_with = "disable")]
        receiving_domain: Option<String>,
        #[arg(long)]
        disable: bool,
    },
    Show {
        id: String,
    },
    Claim {
        id: String,
        #[arg(long)]
        delivery_id: String,
    },
    /// Attempt one due pause email; safe within the retained deduplication window.
    Send,
    /// Explicitly approve duplicate risk and retry an uncertain email after provider inspection.
    Retry {
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum BindingCommand {
    /// Atomically select a registered definition and load its `LaunchAgent`.
    Switch { key: String, digest: String },
    /// Stop future admission without terminating a running activation.
    Disable {
        key: String,
        /// Select this registered definition while leaving the binding disabled.
        #[arg(long, value_name = "DEFINITION_DIGEST")]
        select: Option<String>,
    },
    /// Import one pre-existing product failure halt without enabling a schedule.
    Halt {
        key: String,
        #[arg(long)]
        code: String,
        #[arg(long)]
        occurrence: String,
    },
    /// Explicitly approve future scheduling for the exact current incident.
    Resume { key: String, incident_id: String },
    /// List stable bindings.
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show one stable binding.
    Show { key: String },
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let json_requested = std::env::args_os().any(|argument| argument == "--json");
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let exit_code = error.exit_code();
            if json_requested && exit_code != 0 {
                emit_error(&Error::new("cli_invalid", error.to_string()), true);
                std::process::exit(1);
            } else {
                let _ = error.print();
            }
            std::process::exit(exit_code);
        }
    };
    let json = cli.json;
    let private_exec = matches!(&cli.command, Command::Exec { .. });
    if let Err(error) = run(cli).await {
        if !private_exec {
            emit_error(&error, json);
        }
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_lines)]
async fn run(cli: Cli) -> Result<()> {
    if let Command::Exec {
        key,
        activation_id,
        status_file,
    } = &cli.command
    {
        let mut handshake = [0_u8; 1];
        let layout = Layout::discover(cli.state_root.clone())?;
        let mut status = executor::claim_gate_status(&layout, status_file)?;
        let result = (|| {
            std::io::stdin()
                .read_exact(&mut handshake)
                .context("activation_gate_closed", "read parent execution handshake")?;
            if handshake != *b"G" {
                return Err(Error::new(
                    "activation_gate_invalid",
                    "parent execution handshake was invalid",
                ));
            }
            let store = Store::open(&layout)?;
            executor::exec_registered(&store, &layout, key, activation_id, &mut status)
        })();
        if let Err(error) = &result {
            let _ = executor::write_gate_failure(&mut status, error);
        }
        return result;
    }
    let layout = Layout::discover(cli.state_root)?;
    if matches!(&cli.command, Command::StatusSnapshot) {
        let snapshot = status::snapshot(&layout)?;
        println!(
            "{}",
            serde_json::to_string(&snapshot)
                .context("output_failed", "serialize status snapshot")?
        );
        return Ok(());
    }
    if let Command::Migrate { backup } = &cli.command {
        store::migrate(&layout, backup)?;
        return emit(
            &serde_json::json!({"schema_version": 2, "backup": backup}),
            cli.json,
        );
    }
    let mut store = Store::open(&layout)?;
    match cli.command {
        Command::Definition { command } => match command {
            DefinitionCommand::Register { file } => {
                let (definition, digest) = manifest::load(&file, &layout)?;
                let record = store.register_definition(&digest, &definition)?;
                emit(&record, cli.json)
            }
            DefinitionCommand::List { limit } => emit_page(store.definitions()?, limit, cli.json),
            DefinitionCommand::Show { digest } => {
                manifest::validate_definition_digest(&digest)?;
                emit(&store.definition(&digest)?, cli.json)
            }
        },
        Command::Binding { command } => match command {
            BindingCommand::Switch { key, digest } => {
                manifest::validate_definition_digest(&digest)?;
                let launchd = SystemLaunchd::discover()?;
                let binding =
                    launchd::switch_binding(&mut store, &layout, &launchd, &key, &digest)?;
                emit(&binding, cli.json)
            }
            BindingCommand::Disable { key, select } => {
                if let Some(digest) = select.as_deref() {
                    manifest::validate_definition_digest(digest)?;
                }
                let launchd = SystemLaunchd::discover()?;
                let binding = launchd::disable_binding(
                    &mut store,
                    &layout,
                    &launchd,
                    &key,
                    select.as_deref(),
                )?;
                emit(&binding, cli.json)
            }
            BindingCommand::Halt {
                key,
                code,
                occurrence,
            } => {
                manifest::validate_key(&key)?;
                let _gate = lock::KeyLock::acquire_transition(&layout, &key)?;
                let incident = store.import_halt(&key, &code, &occurrence)?;
                emit(&incident, cli.json)
            }
            BindingCommand::Resume { key, incident_id } => {
                manifest::validate_key(&key)?;
                let _gate =
                    lock::KeyLock::try_acquire_transition(&layout, &key)?.ok_or_else(|| {
                        Error::new(
                            "activation_busy",
                            "wait for the current activation before approving continuation",
                        )
                    })?;
                launchd::require_no_pending_transition(&layout, &key)?;
                store.recover_stale(Some(&key))?;
                emit(&store.resume(&key, &incident_id)?, cli.json)
            }
            BindingCommand::List { limit } => emit_page(store.bindings()?, limit, cli.json),
            BindingCommand::Show { key } => {
                manifest::validate_key(&key)?;
                emit(&store.binding(&key)?, cli.json)
            }
        },
        Command::Run { key } => {
            let activation = executor::run(&mut store, &layout, &key, Trigger::Manual).await?;
            emit(&activation, cli.json)
        }
        Command::History {
            key,
            limit,
            details,
        } => {
            if limit == 0 || limit == usize::MAX {
                return Err(Error::new(
                    "history_limit_invalid",
                    "history --limit must be positive and below the platform maximum",
                ));
            }
            if let Some(key) = key.as_deref() {
                manifest::validate_key(key)?;
            }
            let records = store.history(key.as_deref(), limit + 1)?;
            if details {
                emit_page(records, limit, cli.json)
            } else {
                emit_page(
                    records
                        .into_iter()
                        .map(api::ActivationSummary::from)
                        .collect(),
                    limit,
                    cli.json,
                )
            }
        }
        Command::Doctor => {
            let recovered = store.recover_stale(None)?;
            let sqlite = store.quick_check()?;
            let pending_transitions = launchd::pending_transitions(&layout)?;
            let binary = std::env::current_exe()
                .context(
                    "clockwork_binary_unavailable",
                    "locate Clockwork executable",
                )?
                .canonicalize()
                .context(
                    "clockwork_binary_unavailable",
                    "canonicalize Clockwork executable",
                )?;
            let launchctl = PathBuf::from("/bin/launchctl");
            let launchctl_available = launchctl.is_file();
            if !launchctl_available {
                return Err(Error::new(
                    "launchd_unavailable",
                    "/bin/launchctl is not available",
                ));
            }
            emit(
                &api::DoctorReport {
                    database: layout.database(),
                    state_root: layout.state_root().to_path_buf(),
                    sqlite,
                    recovered_lost_activations: recovered,
                    pending_binding_transitions: pending_transitions,
                    clockwork_binary: binary,
                    launchctl,
                },
                cli.json,
            )
        }
        Command::Launchd { key } => {
            match executor::run(&mut store, &layout, &key, Trigger::Launchd).await {
                Ok(activation) => emit(&activation, cli.json),
                Err(error) if error.code() == "binding_halted" => {
                    emit(&store.binding(&key)?, cli.json)
                }
                Err(error) => Err(error),
            }
        }
        Command::Incident { command } => match command {
            IncidentCommand::Feed { after, limit } => {
                emit(&store.incident_feed(after, limit)?, cli.json)
            }
            IncidentCommand::List { key, limit } => {
                if limit == 0 || limit == usize::MAX {
                    return Err(Error::new(
                        "limit_invalid",
                        "incident limit must be positive and bounded",
                    ));
                }
                if let Some(key) = &key {
                    manifest::validate_key(key)?;
                }
                emit_page(store.incidents(key.as_deref(), limit + 1)?, limit, cli.json)
            }
            IncidentCommand::Show { id } => emit(&store.incident(&id)?, cli.json),
        },
        Command::Notification { command } => match command {
            NotificationCommand::Emt {
                receiving_domain, ..
            } => {
                notification::configure_emt(&layout, receiving_domain.as_deref())?;
                emit(
                    &serde_json::json!({"emt_enabled":receiving_domain.is_some()}),
                    cli.json,
                )
            }
            NotificationCommand::Show { id } => {
                emit(&notification::view(&store, &layout, &id, None)?, cli.json)
            }
            NotificationCommand::Claim { id, delivery_id } => emit(
                &notification::view(&store, &layout, &id, Some(&delivery_id))?,
                cli.json,
            ),
            command => {
                let selected = if let NotificationCommand::Retry { id } = command {
                    notification::approve_retry(&mut store, &layout, &id)?;
                    Some(id)
                } else {
                    None
                };
                let attempted =
                    notification::send_selected(&mut store, &layout, selected.as_deref()).await?;
                emit(&serde_json::json!({"attempted": attempted}), cli.json)
            }
        },
        Command::Abend {
            activation_id,
            code,
            occurrence,
        } => emit(
            &store.report_abend(&activation_id, &code, &occurrence)?,
            cli.json,
        ),
        Command::Migrate { .. } => {
            unreachable!("migration is dispatched before opening runtime state")
        }
        Command::Exec { .. } => Err(Error::new(
            "activation_gate_invalid",
            "execution gate was not dispatched through its private handshake",
        )),
        Command::StatusSnapshot => unreachable!("status is dispatched before the ordinary store"),
    }
}

fn emit<T: Serialize>(data: &T, compact: bool) -> Result<()> {
    let value = serde_json::to_value(api::Success { ok: true, data })
        .context("output_failed", "serialize command result")?;
    let rendered = if compact {
        serde_json::to_string(&value)
    } else {
        serde_json::to_string_pretty(&value)
    }
    .context("output_failed", "serialize command result")?;
    println!("{rendered}");
    Ok(())
}

fn emit_error(error: &Error, compact: bool) {
    if compact {
        let rendered = serde_json::to_string(&api::Failure {
            ok: false,
            error: api::ErrorBody {
                code: error.code().to_owned(),
                message: error.message().to_owned(),
            },
        })
        .unwrap_or_else(|_| "{\"ok\":false}".to_owned());
        eprintln!("{rendered}");
    } else {
        eprintln!("clockwork: {error}");
    }
}

fn emit_page<T: Serialize>(mut items: Vec<T>, limit: usize, compact: bool) -> Result<()> {
    if limit == 0 {
        return Err(Error::new("limit_invalid", "limit must be positive"));
    }
    let has_more = items.len() > limit;
    items.truncate(limit);
    emit(
        &api::SelectionPage {
            output_version: 2,
            items,
            has_more,
        },
        compact,
    )
}
