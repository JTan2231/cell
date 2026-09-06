//! Usher's installation boundary, separate from its read-only recognition CLI.

#[path = "usher-install/adapter.rs"]
mod adapter;

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cell_install::{Disposition, InstallSpec, ReleaseInput};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::{Value, json};

const SPEC: InstallSpec = InstallSpec {
    product: "usher",
    application: "Usher",
    commands: &["usher", "usher-install"],
    provider: "usher",
};

#[derive(Parser)]
#[command(
    name = "usher-install",
    version,
    about = "Install and recover owned Usher program releases"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Install exact binaries and the matching provider through one owned selector.
    Install(CandidateArgs),
    /// Inspect the owned installation without changing it.
    Inspect(HomeArgs),
    /// Verify the installation against the supplied exact candidate.
    Verify(CandidateArgs),
    /// Validate a retained release without executing its contents.
    VerifyRelease { release: PathBuf },
    /// Restore an owned retained release using the current installer.
    Recover {
        #[arg(long)]
        release: PathBuf,
        #[command(flatten)]
        home: HomeArgs,
        #[arg(long)]
        expected_current: Option<String>,
    },
    /// Fixed coordinator protocol; accepts one version-one JSON request on stdin.
    #[command(hide = true)]
    Adapter { operation: adapter::Operation },
}

#[derive(Args)]
struct HomeArgs {
    /// Intentional isolated or alternate current-user installation boundary.
    #[arg(long)]
    home: Option<PathBuf>,
}

#[derive(Args)]
struct CandidateArgs {
    /// Exact tested Usher recognition executable.
    #[arg(long)]
    binary: PathBuf,
    /// Version-matched product-owned Chancery bundle.
    #[arg(long)]
    bundle: PathBuf,
    #[command(flatten)]
    home: HomeArgs,
    /// Require absent or `releases/<sha256>`; omission snapshots before locking.
    #[arg(long)]
    expected_current: Option<String>,
}

#[derive(Debug, Serialize)]
struct Failure {
    code: &'static str,
    detail: String,
    disposition: Disposition,
}

impl Failure {
    fn input(detail: &str) -> Self {
        Self {
            code: "invalid_input",
            detail: detail.to_owned(),
            disposition: Disposition::Unchanged,
        }
    }
}

impl From<cell_install::Error> for Failure {
    fn from(error: cell_install::Error) -> Self {
        Self {
            code: "installation_failed",
            detail: error.message,
            disposition: error.disposition,
        }
    }
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Self {
            code: "io_failed",
            detail: format!("installation I/O failed: {}", error.kind()),
            disposition: Disposition::Unchanged,
        }
    }
}

impl From<serde_json::Error> for Failure {
    fn from(_: serde_json::Error) -> Self {
        Self::input("invalid installation JSON")
    }
}

type Result<T> = std::result::Result<T, Failure>;

fn home_path(home: Option<PathBuf>) -> Result<PathBuf> {
    let path = home
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .ok_or_else(|| Failure::input("HOME or --home is required"))?;
    if !path.is_absolute() {
        return Err(Failure::input("installation home must be absolute"));
    }
    Ok(path)
}

fn release_input(binary: PathBuf, provider_dir: PathBuf) -> Result<ReleaseInput> {
    if !binary.is_absolute() || !provider_dir.is_absolute() {
        return Err(Failure::input("candidate paths must be absolute"));
    }
    cell_install::file_digest(&provider_dir.join("provider.json"))?;
    let provider: Value =
        serde_json::from_slice(&std::fs::read(provider_dir.join("provider.json"))?)?;
    if provider
        .pointer("/provider/release")
        .and_then(Value::as_str)
        != Some(env!("CARGO_PKG_VERSION"))
    {
        return Err(Failure::input(
            "Usher candidate requires its version-matched installer",
        ));
    }
    let installer = std::env::current_exe()?;
    Ok(ReleaseInput {
        binaries: BTreeMap::from([
            ("usher".to_owned(), binary),
            ("usher-install".to_owned(), installer.clone()),
        ]),
        provider_dir,
        installer,
    })
}

fn retained_selector(home: &Path, release: &Path) -> Result<String> {
    if !release.is_absolute() {
        return Err(Failure::input("retained release path must be absolute"));
    }
    let parent = home.join("Library/Application Support/Usher/install/releases");
    if release.parent() != Some(parent.as_path()) {
        return Err(Failure::input(
            "recovery release must belong to this Usher installation",
        ));
    }
    let verified = cell_install::verify_release(&SPEC, release)?;
    Ok(verified.current)
}

fn run(command: Command) -> Result<Value> {
    let data = match command {
        Command::Install(args) => {
            let home = home_path(args.home.home)?;
            let input = release_input(args.binary, args.bundle)?;
            json!(cell_install::install(
                &SPEC,
                &home,
                &input,
                args.expected_current.as_deref()
            )?)
        }
        Command::Inspect(args) => json!(cell_install::inspect(&SPEC, &home_path(args.home)?)?),
        Command::Verify(args) => {
            if args.expected_current.is_some() {
                return Err(Failure::input(
                    "--expected-current applies to installation, not verification",
                ));
            }
            let input = release_input(args.binary, args.bundle)?;
            json!(cell_install::verify_candidate(
                &SPEC,
                &home_path(args.home.home)?,
                &input
            )?)
        }
        Command::VerifyRelease { release } => {
            if !release.is_absolute() {
                return Err(Failure::input("retained release path must be absolute"));
            }
            json!(cell_install::verify_release(&SPEC, &release)?)
        }
        Command::Recover {
            release,
            home,
            expected_current,
        } => {
            let home = home_path(home.home)?;
            let selector = retained_selector(&home, &release)?;
            json!(cell_install::restore(
                &SPEC,
                &home,
                &selector,
                expected_current.as_deref()
            )?)
        }
        Command::Adapter { operation } => return adapter::run(operation),
    };
    Ok(json!({"ok": true, "data": data}))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let adapter_mode = matches!(cli.command, Command::Adapter { .. });
    match run(cli.command) {
        Ok(reply) => {
            println!("{reply}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let reply = if adapter_mode {
                json!({"schema": 1, "status": "stopped", "detail": error.detail, "data": {"error": error}})
            } else {
                json!({"ok": false, "error": error})
            };
            println!("{reply}");
            ExitCode::from(1)
        }
    }
}
