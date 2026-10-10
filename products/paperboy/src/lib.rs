//! Production runs and personal email submission.
#![allow(clippy::missing_errors_doc)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod installation;
pub mod manifest;
pub mod runtime;
pub mod schedule;

use anyhow::{Context, Result, ensure};
use std::path::PathBuf;

pub fn home() -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is required")?);
    ensure!(home.is_absolute(), "HOME must be absolute");
    Ok(home)
}

pub fn state_root() -> Result<PathBuf> {
    Ok(home()?.join("Library/Application Support/Paperboy"))
}

pub fn manifest_path() -> Result<PathBuf> {
    Ok(state_root()?.join("paperboy.toml"))
}
