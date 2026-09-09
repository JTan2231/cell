//! Incident diagnosis and one-off interventions through personal email.
#![allow(clippy::missing_errors_doc)]

pub mod agent;
pub mod installation;
pub mod mail;
pub mod runner;
pub mod store;

use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn fail(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(message.into()).into()
}

pub fn home() -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or_else(|| fail("HOME is required"))?);
    if !home.is_absolute() {
        return Err(fail("HOME must be absolute"));
    }
    Ok(home)
}

pub fn state_root() -> Result<PathBuf> {
    Ok(home()?.join("Library/Application Support/EMT"))
}

#[must_use]
pub fn gate(root: &Path) -> cell_maintenance::Gate {
    cell_maintenance::Gate::new(root.join("deployment-maintenance"))
}

#[must_use]
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

pub fn random_token() -> Result<String> {
    Ok(uuid::Uuid::now_v7().simple().to_string())
}
