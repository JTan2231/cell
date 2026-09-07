//! Daily conversation reports with requester-owned summary and submission state.
#![allow(clippy::missing_errors_doc)]

pub mod agent;
pub mod installation;
pub mod operations;
pub mod store;

use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

pub fn home() -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is required")?);
    ensure!(home.is_absolute(), "HOME must be absolute");
    Ok(home)
}

pub fn state_root() -> Result<PathBuf> {
    Ok(home()?.join("Library/Application Support/Paperboy"))
}

#[must_use]
pub fn gate(root: &Path) -> cell_maintenance::Gate {
    cell_maintenance::Gate::new(root.join("deployment-maintenance"))
}

#[must_use]
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
