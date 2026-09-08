//! Verbatim wants and decision associations in a dedicated Annals library.
#![allow(clippy::missing_errors_doc)]

pub mod annals;
pub mod installation;
pub mod operations;
pub mod store;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const LIBRARIAN_INSTRUCTIONS: &str = include_str!("../librarian.md");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub annals: PathBuf,
    pub annals_state_dir: Option<PathBuf>,
    pub library: String,
    pub library_id: String,
    pub decisions_config: PathBuf,
    pub decisions_library_id: String,
}

impl Config {
    #[must_use]
    pub fn library(&self) -> annals::Annals {
        annals::Annals::new(
            self.annals.clone(),
            self.annals_state_dir.clone(),
            self.library.clone(),
            Some(self.library_id.clone()),
        )
    }

    #[must_use]
    pub fn feed(&self) -> annals_api::Client {
        annals_api::Client::new(&self.annals, &self.decisions_config)
    }
}

pub fn state_root() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CONATUS_STATE_DIR") {
        let path = PathBuf::from(path);
        ensure!(path.is_absolute(), "CONATUS_STATE_DIR must be absolute");
        return Ok(path);
    }
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is required")?);
    ensure!(home.is_absolute(), "HOME must be absolute");
    Ok(home.join("Library/Application Support/Conatus"))
}

pub fn now() -> Result<i64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .try_into()?)
}
