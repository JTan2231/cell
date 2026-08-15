use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::AppError;
use crate::model_runner::ModelQuality;

const DEFAULT_MAX_ITEMS: usize = 5;
const DEFAULT_MAX_ELAPSED_SECONDS: u64 = 45 * 60;
const DEFAULT_SETTLE_SECONDS: u64 = 60;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub library: Option<PathBuf>,
    pub inbox: Option<InboxConfig>,
    #[serde(default)]
    pub liaison: LiaisonConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InboxConfig {
    pub root: PathBuf,
    #[serde(default = "default_max_items")]
    pub max_items: usize,
    #[serde(default = "default_max_elapsed_seconds")]
    pub max_elapsed_seconds: u64,
    #[serde(default = "default_settle_seconds")]
    pub settle_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct LiaisonConfig {
    pub quality: ModelQuality,
    pub model: Option<String>,
    pub codex: PathBuf,
}

impl Default for LiaisonConfig {
    fn default() -> Self {
        Self {
            quality: ModelQuality::default(),
            model: None,
            codex: PathBuf::from("codex"),
        }
    }
}

impl Config {
    pub fn load(explicit: Option<&PathBuf>) -> Result<Self, AppError> {
        let path = explicit.cloned().or_else(|| {
            std::env::var_os("ANNALS_CONFIG")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        });
        let Some(path) = path else {
            return Ok(Self::default());
        };
        Self::read(&path)
    }

    fn read(path: &Path) -> Result<Self, AppError> {
        let document = fs::read_to_string(path).map_err(|error| {
            AppError::invalid(
                "config_read_failed",
                format!("unable to read configuration {}: {error}", path.display()),
            )
        })?;
        let mut config: Self = toml::from_str(&document).map_err(|error| {
            AppError::invalid(
                "invalid_config",
                format!("invalid configuration {}: {error}", path.display()),
            )
        })?;
        config.validate()?;
        config.resolve_relative_paths(path.parent().unwrap_or_else(|| Path::new(".")));
        Ok(config)
    }

    fn resolve_relative_paths(&mut self, directory: &Path) {
        if let Some(library) = &mut self.library
            && library.is_relative()
        {
            *library = directory.join(&*library);
        }
        if let Some(inbox) = &mut self.inbox
            && inbox.root.is_relative()
        {
            inbox.root = directory.join(&inbox.root);
        }
    }

    fn validate(&self) -> Result<(), AppError> {
        if self
            .library
            .as_ref()
            .is_some_and(|library| library.as_os_str().is_empty())
        {
            return Err(AppError::invalid(
                "invalid_config",
                "library must not be empty",
            ));
        }
        if let Some(inbox) = &self.inbox {
            if inbox.root.as_os_str().is_empty() {
                return Err(AppError::invalid(
                    "invalid_config",
                    "inbox.root must not be empty",
                ));
            }
            if inbox.max_items == 0 {
                return Err(AppError::invalid(
                    "invalid_config",
                    "inbox.max_items must be positive",
                ));
            }
            if inbox.max_elapsed_seconds == 0 {
                return Err(AppError::invalid(
                    "invalid_config",
                    "inbox.max_elapsed_seconds must be positive",
                ));
            }
        }
        if self.liaison.codex.as_os_str().is_empty() {
            return Err(AppError::invalid(
                "invalid_config",
                "liaison.codex must not be empty",
            ));
        }
        if self
            .liaison
            .model
            .as_ref()
            .is_some_and(|model| model.trim().is_empty())
        {
            return Err(AppError::invalid(
                "invalid_config",
                "liaison.model must not be blank",
            ));
        }
        Ok(())
    }

    pub fn inbox(&self) -> Result<&InboxConfig, AppError> {
        self.inbox.as_ref().ok_or_else(|| {
            AppError::invalid(
                "inbox_not_configured",
                "the selected configuration does not define [inbox]",
            )
        })
    }
}

const fn default_max_items() -> usize {
    DEFAULT_MAX_ITEMS
}

const fn default_max_elapsed_seconds() -> u64 {
    DEFAULT_MAX_ELAPSED_SECONDS
}

const fn default_settle_seconds() -> u64 {
    DEFAULT_SETTLE_SECONDS
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::Config;
    use crate::model_runner::ModelQuality;

    #[test]
    fn parses_defaults_and_rejects_unknown_fields() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.toml");
        fs::write(
            &path,
            "library = \"library.db\"\n[inbox]\nroot = \"spool\"\n[liaison]\nquality = \"medium\"\n",
        )?;
        let config = Config::read(&path)?;
        let inbox = config.inbox()?;
        assert_eq!(inbox.max_items, 5);
        assert_eq!(inbox.max_elapsed_seconds, 2_700);
        assert_eq!(inbox.settle_seconds, 60);
        assert_eq!(inbox.root, directory.path().join("spool"));
        assert_eq!(config.library, Some(directory.path().join("library.db")));
        assert_eq!(config.liaison.quality, ModelQuality::Medium);

        fs::write(&path, "unknown = true\n")?;
        assert!(Config::read(&path).is_err());
        Ok(())
    }

    #[test]
    fn rejects_zero_batch_bounds() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.toml");
        fs::write(
            &path,
            "[inbox]\nroot = \"spool\"\nmax_items = 0\nmax_elapsed_seconds = 1\n",
        )?;
        let error = Config::read(&path).err().ok_or("configuration succeeded")?;
        assert_eq!(error.code(), "invalid_config");
        Ok(())
    }
}
