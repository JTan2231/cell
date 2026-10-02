use std::path::Path;

use cell_maintenance::{Admission, Gate};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::cli::{
    ChangeCommand, Command, InboxCommand, InboxRetryCommand, InstructionsCommand, LibraryCommand,
    WorkCommand,
};
use crate::error::{AppError, AppResult};
use crate::render::CommandOutput;

#[derive(Debug, Clone, Subcommand)]
pub enum MaintenanceCommand {
    /// Persist this operation's admission hold. Existing commands may finish.
    Hold { run_id: String },
    /// Report all holds and whether admitted commands have finished.
    Status,
    /// Release only the named operation's hold; never change operator pause.
    Release { run_id: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MaintenanceStatus {
    pub protocol_version: u32,
    pub contract_version: u32,
    pub holds: Vec<String>,
    pub drained: bool,
}

pub fn gate(library: &Path) -> AppResult<Gate> {
    let canonical = cell_maintenance::canonical_database_path(library).map_err(failure)?;
    let mut name = canonical.as_os_str().to_os_string();
    name.push(".cell-maintenance");
    Ok(Gate::new(std::path::PathBuf::from(name)))
}

// Result::map_err hands ownership to this product error translation.
#[allow(clippy::needless_pass_by_value)]
fn failure(error: cell_maintenance::Error) -> AppError {
    AppError::conflict("deployment_maintenance", error.to_string())
}

pub fn command(library: &Path, command: &MaintenanceCommand) -> AppResult<CommandOutput> {
    let gate = gate(library)?;
    let status = match command {
        MaintenanceCommand::Hold { run_id } => gate.hold(run_id),
        MaintenanceCommand::Status => gate.status(),
        MaintenanceCommand::Release { run_id } => gate.release(run_id),
    }
    .map_err(failure)?;
    Ok(CommandOutput::new(
        json!({"protocol_version": 1, "contract_version": status.contract_version,
            "holds": status.holds, "drained": status.drained}),
        format!("Deployment admission drained: {}", status.drained),
    ))
}

pub fn enter(library: &Path) -> AppResult<Admission> {
    let gate = gate(library)?;
    let result = match std::env::var("CELL_DEPLOYMENT_RUN_ID") {
        Ok(owner) if !owner.is_empty() && !gate.status().map_err(failure)?.holds.is_empty() => {
            gate.enter_for(&owner)
        }
        _ => gate.enter(),
    };
    result.map_err(failure)
}

pub fn held(library: &Path) -> AppResult<bool> {
    Ok(!gate(library)?.status().map_err(failure)?.holds.is_empty())
}

pub fn mutates(command: &Command) -> bool {
    match command {
        Command::Init(_)
        | Command::Library(LibraryCommand::Create(_))
        | Command::Instructions(InstructionsCommand::Set(_))
        | Command::Migrate
        | Command::Shake(_)
        | Command::Integrate(_)
        | Command::Revert(_) => true,
        Command::Work(command) => matches!(command, WorkCommand::Add(_)),
        Command::Change(command) => {
            matches!(command, ChangeCommand::Submit(_) | ChangeCommand::Apply(_))
        }
        // Operator pause/interrupt remains available while dispatch is held.
        Command::Inbox(command) => !matches!(
            command,
            InboxCommand::Status
                | InboxCommand::Pause
                | InboxCommand::Interrupt(_)
                | InboxCommand::Retry(InboxRetryCommand::Preview(_) | InboxRetryCommand::Status(_))
        ),
        _ => false,
    }
}
