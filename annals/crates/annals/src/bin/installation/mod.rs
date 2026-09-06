use cell_install::adapter::{self, Operation};
use cell_install::{Error, Result};
use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode};
use std::time::Duration;

mod agent;
mod lifecycle;
mod migration;
mod protocol;
mod release;
mod schedule;

const VERSION: &str = env!("CARGO_PKG_VERSION");
// Duration::from_mins is newer than this product's Rust 1.89 MSRV.
#[allow(clippy::duration_suboptimal_units)]
const MINUTE: Duration = Duration::from_secs(60);

#[derive(Parser)]
#[command(
    name = "annals-install",
    version,
    about = "Install and recover owned Annals libraries and releases"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Install(InstallArgs),
    ProvisionDecisions(DecisionsArgs),
    MigrateToUser(migration::Args),
    Inspect(HomeArgs),
    VerifyRelease {
        release: PathBuf,
    },
    Recover {
        transaction: PathBuf,
        #[command(flatten)]
        home: HomeArgs,
    },
    #[command(hide = true)]
    Adapter {
        operation: Operation,
    },
}

#[derive(Args)]
struct HomeArgs {
    #[arg(long)]
    home: Option<PathBuf>,
}

#[derive(Args)]
struct InstallArgs {
    #[arg(long)]
    binary: PathBuf,
    #[arg(long)]
    usage_binary: PathBuf,
    #[arg(long)]
    bundle: PathBuf,
    #[arg(long)]
    usage_bundle: PathBuf,
    #[arg(long)]
    nucleus: PathBuf,
    #[arg(long)]
    nucleus_socket: PathBuf,
    #[arg(long)]
    clockwork: PathBuf,
    #[command(flatten)]
    home: HomeArgs,
    #[arg(long)]
    expected_current: Option<String>,
    #[arg(long)]
    fresh_state: bool,
    #[arg(long)]
    no_start: bool,
    #[arg(long, hide = true)]
    migration_clockwork_handoff: bool,
    #[arg(long, default_value = "/bin/launchctl")]
    launchctl: PathBuf,
}

#[derive(Args)]
struct DecisionsArgs {
    #[arg(long)]
    release_root: PathBuf,
    #[arg(long)]
    nucleus_socket: PathBuf,
    #[arg(long)]
    clockwork: PathBuf,
    #[command(flatten)]
    home: HomeArgs,
    #[arg(long)]
    keep_maintenance: bool,
}

pub fn main() -> ExitCode {
    rustix::process::umask(rustix::fs::Mode::from_bits_truncate(0o077));
    let invoked = std::env::args_os().next().map(PathBuf::from);
    if let Some(name) = invoked
        .as_deref()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        && matches!(name, "annals" | "annals-inbox")
    {
        return adapter::finish(frontend(name));
    }
    let result = match Cli::parse().command {
        Command::Install(args) => lifecycle::install(&args),
        Command::ProvisionDecisions(args) => lifecycle::provision(&args),
        Command::MigrateToUser(args) => migration::run(&args),
        Command::Inspect(args) => home(args.home).and_then(|home| protocol::inspect(&home, None)),
        Command::VerifyRelease { release } => {
            cell_install::verify_release_at(&release::layout(), &release, &release::legacy)
                .map(|info| json!({"ok":true,"data":info}))
        }
        Command::Recover {
            transaction,
            home: args,
        } => home(args.home).and_then(|home| lifecycle::recover(&home, &transaction)),
        Command::Adapter { operation } => protocol::run(operation),
    };
    adapter::finish(result)
}

fn frontend(name: &str) -> Result<Value> {
    let executable = fs::canonicalize(std::env::current_exe()?)?;
    let root = executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| Error::new("invalid Annals frontend location"))?;
    let mut command = Process::new(root.join("libexec/annals"));
    if name == "annals-inbox" {
        command.args(["--quiet", "inbox", "run"]);
    } else {
        let arguments: Vec<_> = std::env::args_os().skip(1).collect();
        let selected = ["ANNALS_CONFIG", "ANNALS_LIBRARY"]
            .iter()
            .any(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty()))
            || arguments.iter().any(|arg| {
                arg.to_str().is_some_and(|a| {
                    matches!(a, "--config" | "--library")
                        || a.starts_with("--config=")
                        || a.starts_with("--library=")
                })
            });
        if !selected {
            let state =
                std::env::var_os("ANNALS_STATE_DIR").map_or(state(&home(None)?), PathBuf::from);
            command.env("ANNALS_CONFIG", state.join("config.toml"));
        }
        command.args(arguments);
    }
    Err(command.exec().into())
}

fn home(value: Option<PathBuf>) -> Result<PathBuf> {
    let path = value
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .ok_or_else(|| Error::new("operator HOME is required"))?;
    if !path.is_absolute() || fs::canonicalize(&path)? != path {
        return Err(Error::new("operator home must be canonical"));
    }
    private_directory(&path, false)?;
    Ok(path)
}

fn state(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Annals")
}
fn install_root(home: &Path) -> PathBuf {
    state(home).join("install")
}

fn uid() -> Result<u32> {
    let output = Process::new("/usr/bin/id").arg("-u").output()?;
    let uid = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|_| Error::new("operator identity unavailable"))?;
    if !output.status.success() || uid == 0 {
        return Err(Error::new(
            "Annals installation requires the non-root current user",
        ));
    }
    Ok(uid)
}

fn private_directory(path: &Path, exact: bool) -> Result<()> {
    let info = fs::symlink_metadata(path)?;
    if !info.is_dir()
        || info.uid() != uid()?
        || info.mode() & 0o7022 != 0
        || exact && info.mode() & 0o777 != 0o700
    {
        return Err(Error::new("unsafe Annals state directory"));
    }
    Ok(())
}

fn directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => private_directory(path, false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                directory(parent)?;
            }
            fs::create_dir(path)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

fn private_file(path: &Path) -> Result<()> {
    let info = fs::symlink_metadata(path)?;
    if !info.is_file() || info.nlink() != 1 || info.uid() != uid()? || info.mode() & 0o7777 != 0o600
    {
        return Err(Error::new("unsafe Annals private file"));
    }
    Ok(())
}

fn optional_private(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            private_file(path)?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn write_private(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    if replace {
        optional_private(path)?;
    } else if fs::symlink_metadata(path).is_ok() {
        return Err(Error::new("Annals output already exists"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("invalid Annals file path"))?;
    directory(parent)?;
    let temporary = tempfile::Builder::new()
        .prefix(".annals-install-")
        .tempfile_in(parent)?;
    fs::write(temporary.path(), bytes)?;
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o600))?;
    temporary.as_file().sync_all()?;
    if replace {
        temporary
            .persist(path)
            .map_err(|_| Error::new("cannot publish Annals private file"))?;
    } else {
        temporary
            .persist_noclobber(path)
            .map_err(|_| Error::new("cannot create Annals private file"))?;
    }
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn environment(home: &Path, owner: Option<&str>) -> BTreeMap<OsString, OsString> {
    let mut result = BTreeMap::from([
        ("HOME".into(), home.as_os_str().to_owned()),
        (
            "PATH".into(),
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin".into(),
        ),
    ]);
    if let Some(owner) = owner {
        result.insert("CELL_DEPLOYMENT_RUN_ID".into(), owner.into());
    }
    result
}

fn call(executable: &Path, args: &[OsString], home: &Path, owner: Option<&str>) -> Result<Value> {
    cell_install::command::json(executable, args, &environment(home, owner), MINUTE * 20)
}

fn annals(
    executable: &Path,
    config: &Path,
    args: &[&str],
    home: &Path,
    owner: Option<&str>,
) -> Result<Value> {
    let args = [
        OsString::from("--config"),
        config.as_os_str().to_owned(),
        "--json".into(),
    ]
    .into_iter()
    .chain(args.iter().map(OsString::from))
    .collect::<Vec<_>>();
    let value = call(executable, &args, home, owner)?;
    value
        .get("data")
        .cloned()
        .ok_or_else(|| Error::new("Annals response has no data"))
}

fn toml_value(text: &str) -> Result<toml::Value> {
    toml::from_str(text).map_err(|_| Error::new("invalid Annals TOML"))
}
fn toml_bytes(value: &toml::Value) -> Result<Vec<u8>> {
    toml::to_string(value)
        .map(String::into_bytes)
        .map_err(|_| Error::new("cannot encode Annals TOML"))
}

fn expected(snapshot: &cell_install::InstallSnapshot, value: Option<&str>) -> Result<()> {
    let selected = snapshot.current.as_ref().map_or_else(
        || "absent".to_owned(),
        |r| format!("releases/{}", r.release_id),
    );
    if value.is_some_and(|value| value != selected) {
        return Err(Error::new("stale Annals release selection"));
    }
    Ok(())
}
