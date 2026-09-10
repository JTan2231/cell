//! Non-producing installation checks: no discovery, model admission or send.
use anyhow::{Context, Result, ensure};
use nucleus_client::NucleusClient;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{Config, store::Store};

pub fn renderer(name: &str) -> Result<PathBuf> {
    let variable = match name {
        "tectonic" => "PLATTER_TECTONIC",
        "python3" => "PLATTER_PYTHON",
        _ => anyhow::bail!("unknown renderer"),
    };
    if let Some(value) = std::env::var_os(variable) {
        let path = PathBuf::from(value);
        executable(&path)
            .with_context(|| format!("{variable} must name an absolute executable"))?;
        return Ok(path);
    }
    let home = std::env::var_os("HOME").context("HOME is required")?;
    // These locations also work from a sparse launchd or installer environment.
    for directory in [
        PathBuf::from(home).join(".local/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/bin"),
    ] {
        let path = directory.join(name);
        if executable(&path).is_ok() {
            return Ok(path);
        }
    }
    anyhow::bail!("{name} is unavailable; configure {variable} with its absolute executable")
}

fn executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    ensure!(path.is_absolute(), "executable path must be absolute");
    let metadata = std::fs::metadata(path)?;
    ensure!(
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
        "executable is unavailable"
    );
    Ok(())
}

pub fn local_state(root: &Path) -> Result<bool> {
    if !root.join(crate::store::DATABASE).try_exists()? {
        ensure!(
            !root.join("config.json").try_exists()?,
            "legacy initialization requires migration"
        );
        return Ok(false);
    }
    let store = Store::open_read_only(root)?;
    let Some(settings) = store.setting::<Config>("config")? else {
        return Ok(false);
    };
    settings.validate()?;
    store.template()?.validate()?;
    Ok(true)
}

fn probe(path: &Path, arguments: &[&str], label: &str) -> Result<String> {
    executable(path).with_context(|| format!("{label} executable unavailable"))?;
    let output = cell_install::command::checked(
        path,
        &arguments.iter().map(|a| (*a).into()).collect::<Vec<_>>(),
        &BTreeMap::new(),
        Duration::from_secs(20),
    )
    .with_context(|| format!("{label} prerequisite check failed"))?;
    Ok(String::from_utf8(output.stdout)?)
}

pub fn local_dependencies(root: &Path) -> Result<Value> {
    let initialized = local_state(root)?;
    let settings = if initialized {
        crate::workflow::config(root)?
    } else {
        Config::new(root.join("original-resume.json"))?
    };
    for (name, path) in [
        ("cast", &settings.cast_executable),
        ("crm", &settings.crm_executable),
        ("email", &settings.email_executable),
    ] {
        let version = probe(path, &["--version"], name)?;
        ensure!(
            version.starts_with(&format!("{name} ")),
            "unexpected {name} executable identity"
        );
    }
    let cast_help = probe(
        &settings.cast_executable,
        &["job", "--help"],
        "Cast job URL collection",
    )?;
    ensure!(
        cast_help.split_whitespace().any(|word| word == "collect"),
        "Cast must support job collect before Platter installation"
    );
    let help = probe(&settings.email_executable, &["--help"], "Email attachments")?;
    ensure!(
        help.split_whitespace()
            .any(|word| word == "--payload-stdin"),
        "Email must support --payload-stdin before Platter installation; deploy the byte-payload Email release first"
    );
    let tectonic = renderer("tectonic")?;
    let python = renderer("python3")?;
    probe(&tectonic, &["--version"], "Tectonic")?;
    probe(
        &python,
        &["-c", "from pypdf import PdfReader; print('pypdf ready')"],
        "Python pypdf",
    )?;
    Ok(
        json!({"initialized":initialized,"state_dir":root,"schema_version":if initialized {Some(crate::store::SCHEMA_VERSION)} else {None},"cast_job_collection":true,"email_attachments":true,"tectonic":tectonic,"python":python,"schedule":"external; not checked"}),
    )
}

pub async fn doctor(root: &Path) -> Result<Value> {
    let local = local_dependencies(root)?;
    let client = NucleusClient::for_current_user()?;
    let health = tokio::time::timeout(Duration::from_secs(20), async {
        if let Ok(owner) = std::env::var("CELL_DEPLOYMENT_RUN_ID") {
            crate::agent::check_deployment_readiness(&client, &owner).await
        } else {
            crate::agent::check_readiness(&client).await
        }
    })
    .await
    .context("Nucleus readiness timed out")??;
    Ok(json!({"local":local,"nucleus_protocol":health.version,"ready":true}))
}
