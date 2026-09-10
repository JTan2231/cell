//! Daily reports with requester-owned summary and submission state.
#![allow(clippy::missing_errors_doc)]

pub mod agent;
pub mod installation;
pub mod operations;
pub mod store;

use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ReportKind {
    #[default]
    Conversations,
    Decisions,
}

impl ReportKind {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Conversations => "conversations",
            Self::Decisions => "decisions",
        }
    }

    pub fn source_pointers(self, annals_config: Option<&Path>) -> Result<serde_json::Value> {
        use serde_json::json;
        match self {
            Self::Conversations => {
                ensure!(
                    annals_config.is_none(),
                    "--annals-config requires --report decisions"
                );
                Ok(
                    json!({"report":"conversations","source":"Conversations: normal-user local Codex history","discover":"list_conversations","read":"read_conversation"}),
                )
            }
            Self::Decisions => {
                let config =
                    annals_config.context("--report decisions requires --annals-config PATH")?;
                ensure!(config.is_absolute(), "Annals config path must be absolute");
                Ok(
                    json!({"report":"decisions","source":"Annals: accepted Krisis decision documents","read":"read_decisions","time_basis":"accepted_at","annals_binary":home()?.join(".local/bin/annals"),"annals_config":config}),
                )
            }
        }
    }
}

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
