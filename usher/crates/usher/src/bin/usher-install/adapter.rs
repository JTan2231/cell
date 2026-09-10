use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use cell_install::{Installation, ReleaseInput};
use clap::ValueEnum;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{Failure, Result, SPEC, home_path, release_input};

const MAX_REQUEST: u64 = 1024 * 1024;

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum Operation {
    Inspect,
    Hold,
    Drain,
    Apply,
    Configure,
    Verify,
    Recover,
    Release,
    Activate,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: u32,
    product: String,
    run_id: String,
    run_dir: PathBuf,
    source_root: PathBuf,
    candidate_dir: Option<PathBuf>,
    candidate: Option<Value>,
    prior: Option<Prior>,
    selected_products: Vec<String>,
    #[serde(default, rename = "affected_products")]
    _affected_products: Vec<String>,
    #[serde(default, rename = "activation_bindings")]
    _activation_bindings: Vec<String>,
    #[serde(default)]
    settings: Option<Value>,
    #[serde(default)]
    dependency_settings: BTreeMap<String, Value>,
    recovery: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prior {
    installed: Option<Installation>,
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

fn request() -> Result<Request> {
    let mut bytes = Vec::new();
    io::stdin().take(MAX_REQUEST + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_REQUEST {
        return Err(Failure::input("adapter request exceeds one MiB"));
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    let owner = request.run_id.as_bytes();
    if request
        .settings
        .as_ref()
        .is_some_and(|value| value != &json!({}))
        || !request.dependency_settings.is_empty()
        || request.schema != 1
        || request.product != "usher"
        || !request
            .selected_products
            .iter()
            .any(|product| product == "usher")
        || owner.is_empty()
        || owner.len() > 128
        || !owner[0].is_ascii_alphanumeric()
        || !owner
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(byte))
        || !request.source_root.is_absolute()
        || !request.run_dir.is_absolute()
    {
        return Err(Failure::input("unsupported Usher adapter request"));
    }
    Ok(request)
}

fn relative(path: &str) -> Result<&Path> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(Failure::input(
            "candidate path must remain inside its sealed root",
        ));
    }
    Ok(path)
}

fn candidate_bytes(value: &Value) -> Result<Vec<u8>> {
    // Version-one candidates are produced by Python json.dumps(ensure_ascii=True).
    // Preserve that byte contract, including surrogate pairs, for retained IDs.
    let text = serde_json::to_string(value)?;
    let mut encoded = String::new();
    for character in text.chars() {
        if character.is_ascii() {
            encoded.push(character);
        } else {
            let mut buffer = [0; 2];
            for unit in character.encode_utf16(&mut buffer) {
                write!(encoded, "\\u{unit:04x}")
                    .map_err(|_| Failure::input("candidate encoding failed"))?;
            }
        }
    }
    encoded.push('\n');
    Ok(encoded.into_bytes())
}

fn verified_candidate(request: &Request) -> Result<ReleaseInput> {
    let directory = request
        .candidate_dir
        .as_ref()
        .filter(|path| path.is_absolute())
        .ok_or_else(|| Failure::input("Usher adapter requires a sealed candidate"))?;
    let value = request
        .candidate
        .as_ref()
        .ok_or_else(|| Failure::input("Usher adapter requires candidate evidence"))?;
    let candidate: Candidate = serde_json::from_value(value.clone())?;
    let mut content = value.clone();
    content
        .as_object_mut()
        .ok_or_else(|| Failure::input("invalid candidate"))?
        .remove("candidate_id");
    let identity = format!("sha256:{:x}", Sha256::digest(candidate_bytes(&content)?));
    if candidate.schema != 1
        || candidate.product != "usher"
        || candidate.id != identity
        || candidate.source_commit.is_empty()
        || candidate.source_key.is_empty()
        || candidate.binaries.len() != 2
    {
        return Err(Failure::input(
            "candidate identity or product does not match",
        ));
    }
    for name in SPEC.commands {
        let binary = candidate
            .binaries
            .get(*name)
            .ok_or_else(|| Failure::input("candidate is missing a required executable"))?;
        if binary.path != format!("bin/{name}")
            || cell_install::file_digest(&directory.join(&binary.path))? != binary.sha256
            || binary.version != format!("{name} {}", env!("CARGO_PKG_VERSION"))
        {
            return Err(Failure::input(
                "candidate executable differs from its sealed evidence",
            ));
        }
    }
    for (path, hash) in &candidate.source_inputs {
        if cell_install::file_digest(&request.source_root.join(relative(path)?))? != *hash {
            return Err(Failure::input(
                "candidate packaging differs from pinned source",
            ));
        }
    }
    let input = release_input(
        directory.join("bin/usher"),
        request.source_root.join("usher/chancery"),
    )?;
    let expected_provider: BTreeMap<_, _> = candidate
        .source_inputs
        .iter()
        .filter_map(|(path, hash)| {
            path.strip_prefix("usher/chancery/")
                .map(|relative| (relative.to_owned(), hash.clone()))
        })
        .collect();
    let actual_provider: BTreeMap<_, _> =
        cell_install::provider_inventory(&input.provider_dir, &SPEC)?
            .into_iter()
            .map(|(path, entry)| (path, entry.sha256))
            .collect();
    if expected_provider.is_empty() || expected_provider != actual_provider {
        return Err(Failure::input(
            "provider inventory differs from sealed candidate evidence",
        ));
    }
    if cell_install::file_digest(&input.installer)?
        != cell_install::file_digest(&directory.join("bin/usher-install"))?
    {
        return Err(Failure::input(
            "adapter executable is not the tested installer",
        ));
    }
    Ok(input)
}

fn baseline(request: &Request) -> Result<Option<&Installation>> {
    request
        .prior
        .as_ref()
        .map(|prior| prior.installed.as_ref())
        .ok_or_else(|| Failure::input("operation requires the captured installation baseline"))
}

fn same_prior(request: &Request, home: &Path) -> Result<bool> {
    Ok(cell_install::inspect(&SPEC, home)?.as_ref() == baseline(request)?)
}

fn recover(request: &Request, home: &Path, input: &ReleaseInput) -> Result<Value> {
    if !request.recovery.as_ref().is_some_and(Value::is_object) {
        return Err(Failure::input(
            "recovery requires the coordinator's operation evidence",
        ));
    }
    let prior = baseline(request)?;
    let observed = cell_install::recover_installation(&SPEC, home, input, prior)?;
    if observed.as_ref() == prior {
        return Ok(json!({"safe_to_release": true, "installed": "prior"}));
    }
    // An uncertain apply is inspected and verified; it is never repeated.
    let installed = cell_install::verify_candidate(&SPEC, home, input)?;
    Ok(json!({"safe_to_release": true, "installed": "candidate", "installation": installed}))
}

pub(super) fn run(operation: Operation) -> Result<Value> {
    let request = request()?;
    let home = home_path(None)?;
    let input = verified_candidate(&request)?;
    let (status, data) = match operation {
        Operation::Configure => ("configured", json!({})),
        Operation::Activate => ("activated", json!({})),
        Operation::Inspect => (
            "ready",
            json!({"installed": cell_install::inspect(&SPEC, &home)?}),
        ),
        Operation::Hold | Operation::Drain => {
            if !same_prior(&request, &home)? {
                return Err(Failure::input("installation changed since inspection"));
            }
            (
                if matches!(operation, Operation::Hold) {
                    "held"
                } else {
                    "drained"
                },
                json!({"drained": true}),
            )
        }
        Operation::Apply => {
            if !same_prior(&request, &home)? {
                return Err(Failure::input("installation changed since inspection"));
            }
            let expected =
                baseline(&request)?.map_or("absent", |installed| installed.current.as_str());
            (
                "applied",
                json!(cell_install::install(&SPEC, &home, &input, Some(expected))?),
            )
        }
        Operation::Verify => (
            "verified",
            json!(cell_install::verify_candidate(&SPEC, &home, &input)?),
        ),
        Operation::Recover => ("recovered", recover(&request, &home, &input)?),
        Operation::Release => ("released", json!({})),
    };
    Ok(
        json!({"schema": 1, "status": status, "detail": "Usher installation operation completed", "data": data}),
    )
}
