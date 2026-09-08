//! Program installation is separate from Conatus intake and schedule activation.

use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use cell_install::legacy::LegacySpec;
use cell_install::simple::Spec;
use cell_install::transaction::{self, LockKind};
use clap::Parser;
use serde::Serialize;
use serde_json::{Value, json};

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "conatus",
        application: "Conatus",
        source_directory: "conatus",
        provider_source: "conatus/chancery",
        legacy_provider_path: "share/chancery/conatus",
        legacy: &LegacySpec {
            format: "",
            manifest: "",
            metadata: &[],
            proofs: &[],
            providers: &[],
            hash_path_lines: false,
        },
        wrapper: None,
        lock_kind: LockKind::Shlock,
        lock_at_state: false,
        maintained: false,
    }
}

#[derive(Parser)]
#[command(
    about = "Write a Clockwork definition for the selected Conatus release; do not activate it"
)]
pub struct ScheduleDefinitionArgs {
    /// Existing Conatus state directory used by the scheduled update.
    #[arg(long)]
    state_dir: PathBuf,
    /// New private TOML file; existing files are not replaced.
    #[arg(long)]
    output: PathBuf,
    /// Current-user installation home; defaults to HOME.
    #[arg(long)]
    home: Option<PathBuf>,
}

#[derive(Serialize)]
struct Definition {
    schema_version: u32,
    key: &'static str,
    release_id: String,
    release_root: PathBuf,
    authority: &'static str,
    overlap: &'static str,
    arguments: Vec<String>,
    cwd: PathBuf,
    schedule: Schedule,
    launch: Launch,
    environment: BTreeMap<&'static str, String>,
    output: Output,
}

#[derive(Serialize)]
struct Schedule {
    kind: &'static str,
    seconds: u64,
    run_at_load: bool,
}

#[derive(Serialize)]
struct Launch {
    kind: &'static str,
    program: PathBuf,
    sha256: String,
}

#[derive(Serialize)]
struct Output {
    stdout: PathBuf,
    stderr: PathBuf,
}

/// Write the schedule candidate without registering or switching Clockwork.
///
/// # Errors
/// Rejects an absent or unproved release, nonabsolute paths, and existing output.
pub fn schedule_definition(args: ScheduleDefinitionArgs) -> Result<Value> {
    let home = args
        .home
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .context("HOME or --home is required")?;
    if !home.is_absolute() || !args.state_dir.is_absolute() || !args.output.is_absolute() {
        bail!("schedule paths must be absolute");
    }
    let home = fs::canonicalize(home)?;
    let state_dir = fs::canonicalize(args.state_dir)?;
    if !state_dir.is_dir() {
        bail!("--state-dir must be an existing directory");
    }
    let spec = specification();
    let release =
        transaction::inspect_installation(&spec.layout(), &home, &|path| spec.legacy(path))?
            .current
            .context("install Conatus before preparing its schedule")?;
    let release_root = home
        .join("Library/Application Support/Conatus/install/releases")
        .join(&release.release_id);
    let binary = release
        .files
        .get("bin/conatus")
        .context("selected release has no Conatus program")?;
    let logs = state_dir.join("logs");
    if !logs.exists() {
        DirBuilder::new().mode(0o700).create(&logs)?;
    }
    if !fs::symlink_metadata(&logs)?.is_dir() {
        bail!("Conatus logs path must be a regular directory");
    }
    let definition = Definition {
        schema_version: 1,
        key: "conatus/update",
        release_id: release.release_id,
        release_root: release_root.clone(),
        authority: "current-user-background",
        overlap: "skip",
        arguments: vec![
            "--state-dir".to_owned(),
            state_dir
                .to_str()
                .context("state directory must be UTF-8")?
                .to_owned(),
            "update".to_owned(),
        ],
        cwd: state_dir,
        schedule: Schedule {
            kind: "interval",
            seconds: 300,
            run_at_load: true,
        },
        launch: Launch {
            kind: "direct",
            program: release_root.join("bin/conatus"),
            sha256: binary.sha256.clone(),
        },
        environment: BTreeMap::from([
            (
                "HOME",
                home.to_str().context("home must be UTF-8")?.to_owned(),
            ),
            ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin".to_owned()),
        ]),
        output: Output {
            stdout: logs.join("update.out.log"),
            stderr: logs.join("update.err.log"),
        },
    };
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&args.output)
        .context("schedule output must be a new file in an existing directory")?;
    output.write_all(toml::to_string_pretty(&definition)?.as_bytes())?;
    output.sync_all()?;
    Ok(json!({
        "key": definition.key,
        "release_id": definition.release_id,
        "definition": args.output,
        "registered": false,
        "activated": false,
    }))
}
