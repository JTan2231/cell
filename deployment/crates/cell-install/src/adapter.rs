//! The fixed coordinator protocol and sealed candidate boundary.

use crate::{Error, Result, file_digest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
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
    Verify,
    Recover,
    Release,
}

impl FromStr for Operation {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "inspect" => Ok(Self::Inspect),
            "hold" => Ok(Self::Hold),
            "drain" => Ok(Self::Drain),
            "apply" => Ok(Self::Apply),
            "verify" => Ok(Self::Verify),
            "recover" => Ok(Self::Recover),
            "release" => Ok(Self::Release),
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
    pub recovery: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    schema: u32,
    product: String,
    source_commit: String,
    source_key: String,
    source_inputs: BTreeMap<String, String>,
    binaries: BTreeMap<String, Binary>,
    #[serde(rename = "candidate_id")]
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binary {
    path: String,
    sha256: String,
    version: String,
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
        return Err(Error::new("candidate path escapes its sealed root"));
    }
    Ok(path)
}

fn canonical(value: &Value) -> Result<Vec<u8>> {
    // Preserve Python schema-one sorted JSON, including UTF-16 surrogate pairs.
    let mut encoded = String::new();
    for character in serde_json::to_string(value)?.chars() {
        if character.is_ascii() {
            encoded.push(character);
        } else {
            let mut buffer = [0; 2];
            for unit in character.encode_utf16(&mut buffer) {
                write!(encoded, "\\u{unit:04x}")
                    .map_err(|_| Error::new("candidate encoding failed"))?;
            }
        }
    }
    encoded.push('\n');
    Ok(encoded.into_bytes())
}

impl Context {
    /// Read and verify one bounded coordinator request and its sealed installer.
    ///
    /// # Errors
    /// Refuses malformed requests, missing candidates, changed source or binary
    /// bytes, invalid paths, and an executing installer from another candidate.
    #[allow(clippy::too_many_lines)] // One ordered proof of the fixed request and complete candidate.
    pub fn read(
        product: &str,
        source_directory: &str,
        installer_name: &str,
        installer_version: &str,
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
            .ok_or_else(|| Error::new("adapter requires a sealed candidate"))?;
        let value = request
            .candidate
            .as_ref()
            .ok_or_else(|| Error::new("adapter requires candidate evidence"))?;
        let candidate: Candidate = serde_json::from_value(value.clone())?;
        let mut content = value.clone();
        content
            .as_object_mut()
            .ok_or_else(|| Error::new("invalid candidate"))?
            .remove("candidate_id");
        if candidate.schema != 1
            || candidate.product != product
            || candidate.source_commit.is_empty()
            || candidate.source_key.is_empty()
            || candidate.id != format!("sha256:{:x}", Sha256::digest(canonical(&content)?))
        {
            return Err(Error::new("candidate identity does not match"));
        }
        let mut binaries = BTreeMap::new();
        for (name, binary) in &candidate.binaries {
            relative(name)?;
            if binary.path != format!("bin/{name}")
                || file_digest(&directory.join(&binary.path))? != binary.sha256
            {
                return Err(Error::new("candidate binary differs from sealed evidence"));
            }
            binaries.insert(name.clone(), directory.join(&binary.path));
        }
        let installer = candidate
            .binaries
            .get(installer_name)
            .ok_or_else(|| Error::new("candidate installer missing"))?;
        if installer.version != format!("{installer_name} {installer_version}")
            || file_digest(&std::env::current_exe()?)? != installer.sha256
        {
            return Err(Error::new(
                "executing installer differs from admitted candidate",
            ));
        }
        for (path, hash) in &candidate.source_inputs {
            if file_digest(&request.source_root.join(relative(path)?))? != *hash {
                return Err(Error::new("candidate packaging differs from pinned source"));
            }
        }
        for prefix in [
            format!("{source_directory}/chancery"),
            format!("{source_directory}/provider"),
        ] {
            for entry in std::fs::read_dir(request.source_root.join(source_directory))? {
                let path = entry?.path();
                let relative_path = path
                    .strip_prefix(&request.source_root)
                    .map_err(|_| Error::new("invalid source root"))?
                    .to_str()
                    .ok_or_else(|| Error::new("source path is not UTF-8"))?
                    .to_owned();
                if relative_path.starts_with(&prefix) && path.is_dir() {
                    let (files, _) = crate::artifact::inventory(&path)?;
                    let actual: BTreeMap<_, _> = files
                        .into_iter()
                        .map(|(name, file)| (format!("{relative_path}/{name}"), file.sha256))
                        .collect();
                    let expected: BTreeMap<_, _> = candidate
                        .source_inputs
                        .iter()
                        .filter(|(name, _)| name.starts_with(&format!("{relative_path}/")))
                        .map(|(name, hash)| (name.clone(), hash.clone()))
                        .collect();
                    if actual.is_empty() || actual != expected {
                        return Err(Error::new(
                            "provider inventory differs from sealed evidence",
                        ));
                    }
                }
            }
        }
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

    /// Return an exact admitted executable path.
    ///
    /// # Errors
    /// Returns an error if that executable is absent from the sealed candidate.
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
