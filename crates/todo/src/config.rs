use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::AppError;
use crate::model::ModelQuality;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) database: Option<PathBuf>,
    #[serde(default)]
    pub(crate) liaison: LiaisonConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct LiaisonConfig {
    pub(crate) quality: ModelQuality,
    pub(crate) model: Option<String>,
    pub(crate) codex: PathBuf,
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
    pub(crate) fn load(explicit: Option<&PathBuf>) -> Result<Self, AppError> {
        let environment = std::env::var_os("TODO_CONFIG");
        let path = resolve_config_path(explicit.map(PathBuf::as_path), environment.as_deref());
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
        if let Some(database) = &mut config.database
            && database.is_relative()
        {
            *database = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&*database);
        }
        Ok(config)
    }

    fn validate(&self) -> Result<(), AppError> {
        if self
            .database
            .as_ref()
            .is_some_and(|database| database.as_os_str().is_empty())
        {
            return Err(AppError::invalid(
                "invalid_config",
                "database must not be empty",
            ));
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
}

fn resolve_config_path(explicit: Option<&Path>, environment: Option<&OsStr>) -> Option<PathBuf> {
    explicit.map(Path::to_path_buf).or_else(|| {
        environment
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::path::Path;

    use super::{Config, resolve_config_path};
    use crate::model::ModelQuality;

    #[test]
    fn configuration_is_strict_and_resolves_the_database_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("todo.toml");
        fs::write(
            &path,
            "database = \"state/todo.db\"\n[liaison]\nquality = \"medium\"\n",
        )?;
        let config = Config::read(&path)?;
        assert_eq!(
            config.database.as_deref(),
            Some(directory.path().join("state/todo.db").as_path())
        );
        assert_eq!(config.liaison.quality, ModelQuality::Medium);

        fs::write(&path, "unknown = true\n")?;
        assert!(Config::read(&path).is_err());
        Ok(())
    }

    #[test]
    fn explicit_config_precedes_a_nonempty_environment_value() {
        assert_eq!(
            resolve_config_path(
                Some(Path::new("explicit.toml")),
                Some(OsStr::new("environment.toml"))
            )
            .as_deref(),
            Some(Path::new("explicit.toml"))
        );
        assert_eq!(
            resolve_config_path(None, Some(OsStr::new("environment.toml"))).as_deref(),
            Some(Path::new("environment.toml"))
        );
        assert_eq!(resolve_config_path(None, Some(OsStr::new(""))), None);
    }
}
