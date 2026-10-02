//! Product-owned deployment helpers over the public Clockwork interface.
//! These preserve selected schedule policy and never approve a failure incident.
use crate::api::{BindingRecord, Client, DefinitionRecord, Error, LaunchImage, Manifest};
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScheduleState {
    pub binding: Option<BindingRecord>,
    pub definition: Option<DefinitionRecord>,
}

impl ScheduleState {
    /// Inspect a user's installed scheduling state without requiring a command
    /// when neither the installation nor a runtime projection exists.
    /// # Errors
    /// Refuses retained runtime state without a usable command or partial inventory.
    pub fn capture_installed(home: &Path, key: &str) -> Result<Self, Error> {
        let executable = home.join(".local/bin/clockwork");
        let database = home.join("Library/Application Support/Clockwork/clockwork.db");
        if executable
            .try_exists()
            .map_err(|error| Error(error.to_string()))?
            || executable.is_symlink()
            || database
                .try_exists()
                .map_err(|error| Error(error.to_string()))?
            || database.is_symlink()
        {
            return Self::capture(&Client::new(executable), key);
        }
        let agents = home.join("Library/LaunchAgents");
        if agents
            .try_exists()
            .map_err(|error| Error(error.to_string()))?
            || agents.is_symlink()
        {
            for entry in std::fs::read_dir(agents).map_err(|error| Error(error.to_string()))? {
                let name = entry.map_err(|error| Error(error.to_string()))?.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("org.clockwork.") && name.ends_with(".plist") {
                    return Err(Error(
                        "Clockwork runtime inventory is absent while a generated plist remains"
                            .into(),
                    ));
                }
            }
        }
        Ok(Self {
            binding: None,
            definition: None,
        })
    }

    /// Capture absence separately from a disabled selection.
    /// # Errors
    /// Refuses unavailable or incomplete inventories and inconsistent definitions.
    pub fn capture(client: &Client, key: &str) -> Result<Self, Error> {
        let page = client.bindings_limit(10000)?;
        if page.has_more {
            return Err(Error("Clockwork binding inventory is incomplete".into()));
        }
        let binding = page.items.into_iter().find(|binding| binding.key == key);
        let definition = binding
            .as_ref()
            .and_then(|binding| binding.definition_digest.as_deref())
            .map(|digest| client.definition(digest))
            .transpose()?;
        if definition
            .as_ref()
            .is_some_and(|definition| definition.key != key)
        {
            return Err(Error(
                "Clockwork definition has a different owner key".into(),
            ));
        }
        Ok(Self {
            binding,
            definition,
        })
    }

    /// Suspend an unchanged selection; a provider hold may already have disabled it.
    /// # Errors
    /// Refuses changed selections and propagates transition failures.
    pub fn suspend(&self, client: &Client, key: &str) -> Result<(), Error> {
        let current = Self::capture(client, key)?;
        match (&self.binding, &current.binding) {
            (None, None) => Ok(()),
            (Some(prior), Some(now))
                if prior.definition_digest == now.definition_digest
                    && prior.halted_incident == now.halted_incident =>
            {
                if now.enabled {
                    client.disable(key, None)?;
                }
                Ok(())
            }
            _ => Err(Error(
                "Clockwork selection changed since deployment inspection".into(),
            )),
        }
    }

    /// Pin a verified direct program while retaining the selected policy and paths.
    /// # Errors
    /// Refuses an interpreted prior definition; the product must migrate it explicitly.
    pub fn retarget(
        &self,
        fallback: Manifest,
        root: &Path,
        executable: &Path,
        sha256: String,
    ) -> Result<Manifest, Error> {
        let mut manifest = self
            .definition
            .as_ref()
            .map_or(fallback, |record| record.manifest.clone());
        if !matches!(manifest.launch, LaunchImage::Direct { .. }) {
            return Err(Error(
                "deployment cannot implicitly replace an interpreted schedule".into(),
            ));
        }
        manifest.release_root = root
            .to_str()
            .ok_or_else(|| Error("release path must be UTF-8".into()))?
            .into();
        manifest.release_id = root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error("release identity missing".into()))?
            .into();
        manifest.launch = LaunchImage::Direct {
            program: executable
                .to_str()
                .ok_or_else(|| Error("program path must be UTF-8".into()))?
                .into(),
            sha256,
        };
        Ok(manifest)
    }

    fn register(
        client: &Client,
        manifest: &Manifest,
        path: &Path,
    ) -> Result<DefinitionRecord, Error> {
        let bytes = manifest.to_toml()?;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(mut file) => file
                .write_all(bytes.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|error| Error(error.to_string()))?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata =
                    std::fs::symlink_metadata(path).map_err(|error| Error(error.to_string()))?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || std::fs::read(path).map_err(|error| Error(error.to_string()))?
                        != bytes.as_bytes()
                {
                    return Err(Error(
                        "prepared Clockwork definition conflicts with retained bytes".into(),
                    ));
                }
            }
            Err(error) => return Err(Error(error.to_string())),
        }
        let result = client.register(path);
        std::fs::remove_file(path).map_err(|error| Error(error.to_string()))?;
        result
    }

    /// Publish this product's definition with its requested or saved enable intent.
    /// # Errors
    /// Returns definition-file or Clockwork registration and selection failures.
    pub fn publish(
        &self,
        client: &Client,
        manifest: &Manifest,
        path: &Path,
        enabled: Option<bool>,
    ) -> Result<BindingRecord, Error> {
        let definition = Self::register(client, manifest, path)?;
        let enabled =
            enabled.unwrap_or_else(|| self.binding.as_ref().is_some_and(|binding| binding.enabled));
        if enabled {
            client.switch(&manifest.key, &definition.digest)
        } else {
            client.disable(&manifest.key, Some(&definition.digest))
        }
    }

    /// Register and select a definition without enabling it.
    /// # Errors
    /// Returns definition-file or Clockwork registration and selection failures.
    pub fn prepare(
        client: &Client,
        manifest: &Manifest,
        path: &Path,
    ) -> Result<BindingRecord, Error> {
        let definition = Self::register(client, manifest, path)?;
        client.disable(&manifest.key, Some(&definition.digest))
    }

    /// Restore intent using the prepared selection. An incident is never cleared.
    /// # Errors
    /// Refuses a lost prior halt or an enabled selection without a definition.
    pub fn activate(&self, client: &Client, key: &str, enabled: Option<bool>) -> Result<(), Error> {
        let current = Self::capture(client, key)?;
        let intended =
            enabled.unwrap_or_else(|| self.binding.as_ref().is_some_and(|binding| binding.enabled));
        if let Some(prior) = &self.binding
            && prior.halted_incident.is_some()
            && current
                .binding
                .as_ref()
                .and_then(|binding| binding.halted_incident.as_ref())
                != prior.halted_incident.as_ref()
        {
            return Err(Error(
                "deployment lost the captured Clockwork failure halt".into(),
            ));
        }
        if let Some(binding) = current.binding {
            if intended {
                let digest = binding
                    .definition_digest
                    .ok_or_else(|| Error("prepared Clockwork selection is missing".into()))?;
                if !binding.enabled {
                    client.switch(key, &digest)?;
                }
            } else if binding.enabled {
                client.disable(key, None)?;
            }
        } else if intended {
            return Err(Error("prepared Clockwork binding is missing".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retarget_preserves_the_operator_schedule_and_environment()
    -> Result<(), Box<dyn std::error::Error>> {
        let manifest: Manifest = serde_json::from_value(
            serde_json::json!({"schema_version":2,"key":"test/daily","release_id":"old","release_root":"/old","authority":"current-user-background","overlap":"skip","arguments":["run"],"cwd":"/state","schedule":{"kind":"local-calendar","hour":17,"minute":12,"run_at_load":false},"launch":{"kind":"direct","program":"/old/bin/test","sha256":"old"},"environment":{"CUSTOM_SETTING":"kept"},"output":{"stdout":"/state/out","stderr":"/state/err"}}),
        )?;
        let state = ScheduleState {
            binding: None,
            definition: Some(DefinitionRecord {
                digest: "old".into(),
                key: "test/daily".into(),
                registered_at: 1,
                manifest: manifest.clone(),
            }),
        };
        let next = state.retarget(
            manifest.clone(),
            Path::new("/releases/new"),
            Path::new("/releases/new/bin/test"),
            "new".into(),
        )?;
        assert_eq!(next.schedule, manifest.schedule);
        assert_eq!(next.environment, manifest.environment);
        assert_eq!(next.output, manifest.output);
        assert_eq!(next.release_id, "new");
        Ok(())
    }
}
