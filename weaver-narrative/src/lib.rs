//! Free-form narrative authoring over Annals, executed through Nucleus.
#![allow(clippy::missing_errors_doc)]

pub mod agent;
pub mod installation;
pub mod operations;
pub mod store;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn state_root() -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is required")?);
    ensure!(home.is_absolute(), "HOME must be absolute");
    Ok(home.join("Library/Application Support/Weaver"))
}

#[must_use]
pub fn gate(root: &Path) -> cell_maintenance::Gate {
    cell_maintenance::Gate::new(root.join("deployment-maintenance"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub annals_binary: PathBuf,
    pub annals_config: PathBuf,
}

impl Config {
    pub fn read(root: &Path) -> Result<Self> {
        let path = root.join("config.json");
        store::regular(&path)?;
        let config: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.annals_binary.is_absolute() && self.annals_config.is_absolute(),
            "Annals executable and config paths must be absolute"
        );
        Ok(())
    }

    #[must_use]
    pub fn reader(&self) -> annals_api::Client {
        annals_api::Client::new(&self.annals_binary, &self.annals_config)
    }
}
