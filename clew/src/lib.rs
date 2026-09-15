//! User-reported application history, linked to retained Platter opportunities.
#![allow(clippy::missing_errors_doc)]
pub mod installation;
pub mod store;

use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

pub fn home() -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is unavailable")?);
    ensure!(home.is_absolute(), "HOME must be absolute");
    Ok(home)
}

#[must_use]
pub fn state_dir(home: &Path) -> PathBuf {
    home.join(".local/share/clew")
}

pub fn platter_client() -> Result<platter::api::Client> {
    Ok(platter::api::Client::new(
        home()?.join(".local/bin/platter"),
    ))
}
