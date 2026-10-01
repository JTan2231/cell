//! The fixed coordinator protocol and candidate paths.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

const MAX_REQUEST: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Inspect,
    Hold,
    Drain,
    Apply,
    Configure,
    Recover,
    Release,
    Activate,
}

impl FromStr for Operation {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "inspect" => Ok(Self::Inspect),
            "hold" => Ok(Self::Hold),
            "drain" => Ok(Self::Drain),
            "apply" => Ok(Self::Apply),
            "configure" => Ok(Self::Configure),
            "recover" => Ok(Self::Recover),
            "release" => Ok(Self::Release),
            "activate" => Ok(Self::Activate),
            _ => Err(Error::new("unsupported adapter operation")),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: u32,
    pub product: String,
    pub run_id: String,
    pub run_dir: PathBuf,
    pub source_root: PathBuf,
    pub candidate_dir: Option<PathBuf>,
    pub candidate: Option<Value>,
    pub prior: Option<Value>,
    pub selected_products: Vec<String>,
    #[serde(default)]
    pub affected_products: Vec<String>,
    #[serde(default)]
    pub activation_bindings: Vec<String>,
    #[serde(default)]
    pub settings: Option<Value>,
    #[serde(default)]
    pub dependency_settings: BTreeMap<String, Value>,
    #[serde(default)]
    pub dependency_candidates: BTreeMap<String, DependencyCandidate>,
    pub recovery: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyCandidate {
    pub candidate_dir: PathBuf,
    pub candidate: Value,
}

#[derive(Deserialize)]
struct Candidate {
    binaries: BTreeMap<String, Binary>,
}

#[derive(Deserialize)]
struct Binary {
    path: String,
}

pub struct Context {
    pub request: Request,
    pub home: PathBuf,
    binaries: BTreeMap<String, PathBuf>,
}

fn relative(value: &str) -> Result<&Path> {
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(Error::new("candidate path escapes its candidate root"));
    }
    Ok(path)
}

impl Context {
    /// Validate the product's supported deployment settings before maintenance.
    ///
    /// # Errors
    /// Rejects unknown keys and values with the wrong type.
    pub fn validate_settings(&self, string_keys: &[&str], boolean_keys: &[&str]) -> Result<()> {
        let Some(settings) = &self.request.settings else {
            return Ok(());
        };
        let settings = settings
            .as_object()
            .ok_or_else(|| Error::new("deployment settings must be an object"))?;
        for (key, value) in settings {
            if !(string_keys.contains(&key.as_str()) && value.is_string()
                || boolean_keys.contains(&key.as_str()) && value.is_boolean())
            {
                return Err(Error::new(format!(
                    "unsupported deployment setting or type: {key}"
                )));
            }
        }
        Ok(())
    }

    /// Apply requested activation only to the selected, configured generation.
    ///
    /// # Errors
    /// Rejects an activation setting that is not a boolean.
    pub fn activation_enabled(&self, captured: bool) -> Result<bool> {
        if !self.selected()
            || self
                .request
                .recovery
                .as_ref()
                .is_some_and(|value| value["installed"] != "candidate")
        {
            return Ok(captured);
        }
        self.request
            .settings
            .as_ref()
            .and_then(|value| value.get("enabled"))
            .map_or(Ok(captured), |value| {
                value
                    .as_bool()
                    .ok_or_else(|| Error::new("enabled must be a boolean"))
            })
    }

    /// Select the installed dependency command or, only when it is absent, a
    /// supplied dependency candidate for read-only first-install discovery.
    ///
    /// # Errors
    /// Rejects a missing dependency candidate or invalid path.
    pub fn dependency_binary(&self, product: &str) -> Result<PathBuf> {
        self.resolve_dependency_binary(product, false)
    }

    /// Use a selected dependency's candidate for read-only inspection,
    /// including when its installed predecessor lacks a required interface.
    /// Never use this selection for product mutations or schedule activation.
    ///
    /// # Errors
    /// Rejects a missing dependency candidate or invalid path.
    pub fn dependency_inspection_binary(&self, product: &str) -> Result<PathBuf> {
        self.resolve_dependency_binary(
            product,
            self.request
                .selected_products
                .iter()
                .any(|name| name == product),
        )
    }

    fn resolve_dependency_binary(
        &self,
        product: &str,
        selected_candidate: bool,
    ) -> Result<PathBuf> {
        relative(product)?;
        let installed = self.home.join(".local/bin").join(product);
        if !selected_candidate && (installed.exists() || installed.is_symlink()) {
            return Ok(installed);
        }
        let supplied = self
            .request
            .dependency_candidates
            .get(product)
            .ok_or_else(|| Error::new("dependency candidate is unavailable"))?;
        let manifest: Candidate = serde_json::from_value(supplied.candidate.clone())?;
        let binary = manifest
            .binaries
            .get(product)
            .ok_or_else(|| Error::new("dependency candidate command is missing"))?;
        let path = supplied.candidate_dir.join(relative(&binary.path)?);
        Ok(path)
    }

    /// Read one bounded coordinator request and its candidate paths.
    ///
    /// # Errors
    /// Refuses malformed requests, missing candidates, or invalid paths.
    pub fn read(
        product: &str,
        source_directory: &str,
        _installer_name: &str,
        _installer_version: &str,
    ) -> Result<Self> {
        let mut bytes = Vec::new();
        io::stdin().take(MAX_REQUEST + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_REQUEST {
            return Err(Error::new("adapter request exceeds one MiB"));
        }
        let request: Request = serde_json::from_slice(&bytes)?;
        let owner = request.run_id.as_bytes();
        if request.schema != 1
            || request.product != product
            || owner.is_empty()
            || owner.len() > 128
            || !owner[0].is_ascii_alphanumeric()
            || !owner
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(byte))
            || !request.source_root.is_absolute()
            || !request.run_dir.is_absolute()
        {
            return Err(Error::new("unsupported adapter request"));
        }
        relative(source_directory)?;
        let directory = request
            .candidate_dir
            .as_ref()
            .filter(|path| path.is_absolute())
            .ok_or_else(|| Error::new("adapter requires a candidate"))?;
        let value = request
            .candidate
            .as_ref()
            .ok_or_else(|| Error::new("adapter requires candidate evidence"))?;
        let candidate: Candidate = serde_json::from_value(value.clone())?;
        let binaries = candidate
            .binaries
            .iter()
            .map(|(name, binary)| {
                relative(name)?;
                Ok((name.clone(), directory.join(relative(&binary.path)?)))
            })
            .collect::<Result<_>>()?;
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| Error::new("absolute HOME is required"))?;
        Ok(Self {
            request,
            home,
            binaries,
        })
    }

    /// Return an supplied executable path.
    ///
    /// # Errors
    /// Returns an error if that executable is absent from the candidate.
    pub fn binary(&self, name: &str) -> Result<PathBuf> {
        self.binaries
            .get(name)
            .cloned()
            .ok_or_else(|| Error::new("required candidate executable is absent"))
    }

    /// Read the captured inspection result.
    ///
    /// # Errors
    /// Returns an error if this operation has no prior inspection evidence.
    pub fn prior(&self) -> Result<&Value> {
        self.request
            .prior
            .as_ref()
            .ok_or_else(|| Error::new("operation requires captured prior inspection"))
    }

    #[must_use]
    pub fn selected(&self) -> bool {
        self.request
            .selected_products
            .contains(&self.request.product)
    }
}

#[must_use]
pub fn reply(status: &str, detail: &str, data: Value) -> Value {
    let mut value = json!({"schema":1,"status":status,"detail":detail});
    value["data"] = data;
    value
}

/// Emit one bounded protocol reply. Callers keep product bodies out of errors.
#[must_use]
pub fn finish(result: Result<Value>) -> ExitCode {
    match result {
        Ok(value) => {
            let encoded = value.to_string();
            if encoded.len() as u64 > MAX_REQUEST {
                return finish(Err(Error::new("adapter reply exceeds the protocol bound")));
            }
            println!("{encoded}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!(
                "{}",
                reply(
                    "stopped",
                    &error.message,
                    json!({"error":{"disposition":error.disposition}})
                )
            );
            ExitCode::FAILURE
        }
    }
}
