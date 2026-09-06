use std::path::Path;

use crate::api::MaintenanceStatus;
use crate::store::Store;
use crate::{Error, Result};
use cell_maintenance::{Admission, Gate};

pub(crate) fn gate(database: &Path) -> Result<Gate> {
    let database = cell_maintenance::canonical_database_path(database).map_err(error)?;
    Ok(Gate::new(
        database
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("deployment-maintenance"),
    ))
}

#[allow(clippy::needless_pass_by_value)] // Consumes Result::map_err input.
pub(crate) fn error(error: cell_maintenance::Error) -> Error {
    Error::domain("deployment_maintenance", error.to_string())
}

pub(crate) fn enter(database: &Path, migration: bool) -> Result<Admission> {
    let gate = gate(database)?;
    if migration && let Ok(owner) = std::env::var("CELL_DEPLOYMENT_RUN_ID") {
        return gate.enter_for(&owner).map_err(error);
    }
    gate.enter().map_err(error)
}

pub(crate) fn status(database: &Path) -> Result<MaintenanceStatus> {
    let status = gate(database)?.status().map_err(error)?;
    let (unsettled_updates, worker_alive) = if database.exists() {
        Store::maintenance_work(database)?
    } else {
        (0, false)
    };
    Ok(MaintenanceStatus {
        protocol_version: 1,
        holds: status.holds,
        drained: status.drained && unsettled_updates == 0 && !worker_alive,
        unsettled_updates,
        worker_alive,
    })
}
