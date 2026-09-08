//! Daily problem delivery and independent email-answer critiques.

pub mod corpus;
pub mod grading;
pub mod installation;
pub mod mail;
pub mod runner;
pub mod store;

use std::io::Read;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn fail(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(message.into()).into()
}

pub fn home() -> Result<PathBuf> {
    let path = PathBuf::from(std::env::var_os("HOME").ok_or_else(|| fail("HOME is required"))?);
    if !path.is_absolute() {
        return Err(fail("HOME must be absolute"));
    }
    Ok(path)
}

pub fn state_root() -> Result<PathBuf> {
    // The desktop Mentor already owns Application Support/Mentor.
    Ok(home()?.join("Library/Application Support/MentorMail"))
}

pub fn gate(root: &Path) -> cell_maintenance::Gate {
    cell_maintenance::Gate::new(root.join("deployment-maintenance"))
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn random_token() -> Result<String> {
    let mut bytes = [0_u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(hex::encode(bytes))
}
