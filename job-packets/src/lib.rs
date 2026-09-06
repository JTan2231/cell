//! Prepare private job briefs and resumes with a fixed original template.
#![allow(clippy::missing_errors_doc)]
pub mod ad_hoc;
pub mod agent;
pub mod resume;
pub mod source;
pub mod store;
pub mod workflow;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub daily_count: usize,
    pub delivery_hour: u32,
    pub delivery_minute: u32,
    pub timezone: String,
    pub cast_executable: PathBuf,
    pub crm_executable: PathBuf,
    pub email_executable: PathBuf,
    pub original_resume: PathBuf,
}

impl Config {
    pub fn new(original_resume: PathBuf) -> Result<Self> {
        let home = std::env::var_os("HOME").context("HOME is unavailable")?;
        let bin = PathBuf::from(home).join(".local/bin");
        Ok(Self {
            daily_count: 3,
            delivery_hour: 9,
            delivery_minute: 0,
            timezone: "America/Chicago".into(),
            cast_executable: bin.join("cast"),
            crm_executable: bin.join("crm"),
            email_executable: bin.join("email"),
            original_resume,
        })
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.daily_count == 3,
            "this contract sends at most three jobs"
        );
        anyhow::ensure!(
            self.delivery_hour == 9 && self.delivery_minute == 0,
            "delivery is at 09:00"
        );
        let _: chrono_tz::Tz = self.timezone.parse().context("invalid time zone")?;
        for path in [
            &self.cast_executable,
            &self.crm_executable,
            &self.email_executable,
            &self.original_resume,
        ] {
            anyhow::ensure!(path.is_absolute(), "configured paths must be absolute");
        }
        Ok(())
    }
}

pub fn private_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::create_dir_all(path)?;
    anyhow::ensure!(
        !std::fs::symlink_metadata(path)?.file_type().is_symlink(),
        "private directory must not be a symlink"
    );
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write as _;
    let parent = path.parent().context("file has no parent")?;
    private_dir(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(file.as_file_mut(), value)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}
