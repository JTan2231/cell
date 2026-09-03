#![allow(clippy::missing_errors_doc)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

mod error;
mod executor;
mod launchd;
mod lock;
mod manifest;
mod model;
mod paths;
mod store;

use std::io::Read as _;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::json;

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
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Check private state and local runtime prerequisites.
    Doctor,
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
    List,
    /// Show one immutable definition by digest.
    Show { digest: String },
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
    /// List stable bindings.
    List,
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
    let mut store = Store::open(&layout)?;
    match cli.command {
        Command::Definition { command } => match command {
            DefinitionCommand::Register { file } => {
                let (definition, digest) = manifest::load(&file, &layout)?;
                let record = store.register_definition(&digest, &definition)?;
                emit(&record, cli.json)
            }
            DefinitionCommand::List => emit(&store.definitions()?, cli.json),
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
            BindingCommand::List => emit(&store.bindings()?, cli.json),
            BindingCommand::Show { key } => {
                manifest::validate_key(&key)?;
                emit(&store.binding(&key)?, cli.json)
            }
        },
        Command::Run { key } => {
            let activation = executor::run(&mut store, &layout, &key, Trigger::Manual).await?;
            emit(&activation, cli.json)
        }
        Command::History { key, limit } => {
            if !(1..=1000).contains(&limit) {
                return Err(Error::new(
                    "history_limit_invalid",
                    "history --limit must be from 1 through 1000",
                ));
            }
            if let Some(key) = key.as_deref() {
                manifest::validate_key(key)?;
            }
            emit(&store.history(key.as_deref(), limit)?, cli.json)
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
                &json!({
                    "database": layout.database(),
                    "state_root": layout.state_root(),
                    "sqlite": sqlite,
                    "recovered_lost_activations": recovered,
                    "pending_binding_transitions": pending_transitions,
                    "clockwork_binary": binary,
                    "launchctl": launchctl,
                }),
                cli.json,
            )
        }
        Command::Launchd { key } => {
            let activation = executor::run(&mut store, &layout, &key, Trigger::Launchd).await?;
            emit(&activation, cli.json)
        }
        Command::Exec { .. } => Err(Error::new(
            "activation_gate_invalid",
            "execution gate was not dispatched through its private handshake",
        )),
    }
}

fn emit<T: Serialize>(data: &T, compact: bool) -> Result<()> {
    let value = json!({"ok": true, "data": data});
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
        let rendered = serde_json::to_string(&json!({
            "ok": false,
            "error": {"code": error.code(), "message": error.message()}
        }))
        .unwrap_or_else(|_| "{\"ok\":false}".to_owned());
        eprintln!("{rendered}");
    } else {
        eprintln!("clockwork: {error}");
    }
}
