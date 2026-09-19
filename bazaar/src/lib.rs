//! Versioned opaque strings, with an in-process `SQLite` API and a local CLI.
//!
//! [`api::Reader`] opens existing state read-only. [`api::Writer`] initializes
//! state and appends versions. Callers own all interpretation of the content.

pub mod api;
pub mod installation;

use std::path::{Path, PathBuf};

/// The default database beneath the selected user's home.
#[must_use]
pub fn database_path(home: &Path) -> PathBuf {
    home.join(".local/share/bazaar/bazaar.sqlite3")
}
