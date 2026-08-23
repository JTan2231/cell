use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

const CONFIG_NAME: &str = "usage.toml";

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct UsageConfig {
    pub(crate) codex: PathBuf,
    pub(crate) database: PathBuf,
    pub(crate) library: PathBuf,
    pub(crate) spool: PathBuf,
    pub(crate) codex_home: PathBuf,
    #[serde(skip)]
    pub(crate) path: PathBuf,
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self {
            codex: PathBuf::from("codex"),
            database: PathBuf::from("usage.db"),
            library: PathBuf::from("annals.db"),
            spool: PathBuf::from("spool"),
            codex_home: PathBuf::from("codex-home"),
            path: PathBuf::new(),
        }
    }
}

impl UsageConfig {
    pub(crate) fn load(explicit: Option<&Path>) -> Result<Self, ConfigError> {
        let path = explicit
            .map(Path::to_path_buf)
            .or_else(default_config_path)
            .ok_or(ConfigError::NoConfigurationPath)?;
        let document = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        let mut config: Self = toml::from_str(&document).map_err(|source| ConfigError::Parse {
            path: path.clone(),
            source,
        })?;
        config.validate()?;
        let directory = path.parent().unwrap_or_else(|| Path::new("."));
        resolve_relative(&mut config.codex, directory);
        resolve_relative(&mut config.database, directory);
        resolve_relative(&mut config.library, directory);
        resolve_relative(&mut config.spool, directory);
        resolve_relative(&mut config.codex_home, directory);
        config.path = path;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        for (name, path) in [
            ("codex", &self.codex),
            ("database", &self.database),
            ("library", &self.library),
            ("spool", &self.spool),
            ("codex_home", &self.codex_home),
        ] {
            if path.as_os_str().is_empty() {
                return Err(ConfigError::EmptyPath(name));
            }
        }
        Ok(())
    }
}

fn default_config_path() -> Option<PathBuf> {
    nonempty_environment("ANNALS_USAGE_CONFIG")
        .map(PathBuf::from)
        .or_else(|| {
            nonempty_environment("ANNALS_CONFIG").map(|path| {
                Path::new(&path)
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(CONFIG_NAME)
            })
        })
        .or_else(|| {
            nonempty_environment("HOME").map(|home| {
                Path::new(&home)
                    .join("Library/Application Support/Annals")
                    .join(CONFIG_NAME)
            })
        })
}

fn nonempty_environment(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name).filter(|value| value != OsStr::new(""))
}

fn resolve_relative(path: &mut PathBuf, directory: &Path) {
    if path.is_relative() {
        *path = directory.join(&*path);
    }
}

#[derive(Debug, Error)]
pub(crate) enum ConfigError {
    #[error("no usage configuration path is available; pass --config or set ANNALS_USAGE_CONFIG")]
    NoConfigurationPath,
    #[error("unable to read usage configuration {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid usage configuration {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("usage configuration path {0} must not be empty")]
    EmptyPath(&'static str),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::UsageConfig;

    #[test]
    fn resolves_relative_paths_from_the_configuration() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("usage.toml");
        fs::write(
            &path,
            "codex = \"bin/codex\"\ndatabase = \"usage.db\"\n\
             library = \"annals.db\"\nspool = \"spool\"\ncodex_home = \"codex-home\"\n",
        )?;

        let config = UsageConfig::load(Some(&path))?;
        assert_eq!(config.codex, directory.path().join("bin/codex"));
        assert_eq!(config.database, directory.path().join("usage.db"));
        assert_eq!(config.library, directory.path().join("annals.db"));
        assert_eq!(config.spool, directory.path().join("spool"));
        assert_eq!(config.codex_home, directory.path().join("codex-home"));
        Ok(())
    }

    #[test]
    fn rejects_unknown_fields() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("usage.toml");
        fs::write(&path, "surprise = true\n")?;
        assert!(UsageConfig::load(Some(&path)).is_err());
        Ok(())
    }
}
