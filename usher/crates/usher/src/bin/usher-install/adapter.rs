use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use cell_install::{Installation, ReleaseInput};
use clap::ValueEnum;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Failure, Result, SPEC, home_path, release_input};

const MAX_REQUEST: u64 = 1024 * 1024;

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum Operation {
    Inspect,
    Hold,
    Drain,
    Apply,
    Configure,
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
struct Candidate {
    binaries: BTreeMap<String, Binary>,
}

#[derive(Deserialize)]
struct Binary {
    path: String,
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

fn candidate_input(request: &Request) -> Result<ReleaseInput> {
    let directory = request
        .candidate_dir
        .as_ref()
        .filter(|path| path.is_absolute())
        .ok_or_else(|| Failure::input("Usher adapter requires a candidate directory"))?;
    let candidate: Candidate = serde_json::from_value(
        request
            .candidate
            .clone()
            .ok_or_else(|| Failure::input("Usher adapter requires candidate paths"))?,
    )?;
    let binary = candidate
        .binaries
        .get("usher")
        .ok_or_else(|| Failure::input("candidate is missing Usher"))?;
    release_input(
        directory.join(relative(&binary.path)?),
        request.source_root.join("usher/chancery"),
    )
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
    Ok(json!({"safe_to_release": true, "installed": "candidate", "installation": observed}))
}

pub(super) fn run(operation: Operation) -> Result<Value> {
    let request = request()?;
    let home = home_path(None)?;
    let input = candidate_input(&request)?;
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
        Operation::Recover => ("recovered", recover(&request, &home, &input)?),
        Operation::Release => ("released", json!({})),
    };
    Ok(
        json!({"schema": 1, "status": status, "detail": "Usher installation operation completed", "data": data}),
    )
}
