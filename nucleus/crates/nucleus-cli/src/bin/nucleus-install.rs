//! Immutable Nucleus packaging around the existing product-owned Rust service
//! installer. Authentication and database compatibility remain in that boundary.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use cell_install::adapter::Context;
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::transaction::{
    InstallLayout, InstallSnapshot, LockKind, LockSpec, ProviderSpec, PublicEntry, PublicKind,
    ReleaseInfo, ReleasePlan, SourceFile,
};
use cell_install::{Error, Result};
use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(
    name = "nucleus-install",
    version,
    about = "Install exact Nucleus programs through its guarded service lifecycle"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Stage the complete supported Codex runtime without changing the service.
    StageHarness {
        #[arg(long)]
        codex: PathBuf,
        #[command(flatten)]
        home: HomeArgs,
    },
    Install(InstallArgs),
    Inspect(HomeArgs),
    /// Execute the installation instructions supplied by the product manifest.
    Deploy,
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
    daemon: PathBuf,
    #[arg(long)]
    bundle: PathBuf,
    #[arg(long)]
    codex: PathBuf,
    #[arg(long)]
    codex_home: Option<PathBuf>,
    #[command(flatten)]
    home: HomeArgs,
    #[arg(long)]
    expected_current: Option<String>,
}

fn home(value: Option<PathBuf>) -> Result<PathBuf> {
    let value = value
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .ok_or_else(|| Error::new("HOME or --home is required"))?;
    if !value.is_absolute() {
        return Err(Error::new("home must be absolute"));
    }
    Ok(value)
}

fn public(installer: bool) -> Vec<PublicEntry> {
    // The service installer places the public CLI and daemon copies.
    let mut entries = vec![PublicEntry {
        path: "Library/Application Support/Chancery/providers/nucleus".into(),
        artifact: "share/chancery/nucleus".into(),
        kind: PublicKind::Symlink,
        mode: 0o755,
    }];
    if installer {
        entries.push(PublicEntry {
            path: ".local/bin/nucleus-install".into(),
            artifact: "bin/nucleus-install".into(),
            kind: PublicKind::Symlink,
            mode: 0o755,
        });
    }
    entries
}

fn layout() -> InstallLayout {
    InstallLayout {
        product: "nucleus".into(),
        application: "Nucleus".into(),
        product_lock: LockSpec {
            path: "Library/Application Support/Nucleus/.deploy-lock".into(),
            kind: LockKind::Directory,
        },
        catalog_lock: Some(LockSpec {
            path: "Library/Application Support/Chancery/.catalog-update-lock".into(),
            kind: LockKind::Shlock,
        }),
        public: public(true),
    }
}

fn legacy(root: &Path) -> Result<ReleaseInfo> {
    cell_install::legacy::read(
        root,
        &LegacySpec {
            format: "1",
            manifest: "manifest.txt",
            metadata: &["version"],
            proofs: &[
                LegacyProof {
                    key: "binary_sha256",
                    paths: &["bin/nucleus"],
                },
                LegacyProof {
                    key: "daemon_sha256",
                    paths: &["libexec/nucleusd"],
                },
                LegacyProof {
                    key: "deployer_sha256",
                    paths: &["package/deploy-user.sh"],
                },
            ],
            providers: &[LegacyProvider {
                key: "chancery_bundle_sha256",
                provider: "nucleus",
                path: "share/chancery/nucleus",
                version_key: "version",
            }],
            hash_path_lines: true,
        },
        public(false),
    )
}

fn environment(home: &Path, owner: Option<&str>) -> BTreeMap<OsString, OsString> {
    let mut env = BTreeMap::from([("HOME".into(), home.as_os_str().to_owned())]);
    if let Some(owner) = owner {
        env.insert("CELL_DEPLOYMENT_RUN_ID".into(), owner.into());
    }
    env
}

fn call(
    exe: &Path,
    args: &[OsString],
    home: &Path,
    owner: Option<&str>,
    seconds: u64,
) -> Result<std::process::Output> {
    cell_install::command::checked(
        exe,
        args,
        &environment(home, owner),
        Duration::from_secs(seconds),
    )
}

fn strings(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn staged_runtime(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Nucleus/harnesses/codex")
        .join(nucleus_codex::SUPPORTED_CODEX_VERSION)
        .join("runtime")
}

fn selection(snapshot: &InstallSnapshot) -> String {
    snapshot.current.as_ref().map_or_else(
        || "absent".into(),
        |info| format!("releases/{}", info.release_id),
    )
}

fn inspect(home: &Path) -> Result<InstallSnapshot> {
    cell_install::transaction::inspect_installation(&layout(), home, &legacy)
}

fn configured_harness(home: &Path) -> Result<PathBuf> {
    let output = call(
        Path::new("/usr/bin/plutil"),
        &[
            "-extract".into(),
            "ProgramArguments".into(),
            "json".into(),
            "-o".into(),
            "-".into(),
            home.join("Library/LaunchAgents/org.nucleus.daemon.plist")
                .into_os_string(),
        ],
        home,
        None,
        30,
    )?;
    let arguments: Vec<String> = serde_json::from_slice(&output.stdout)?;
    arguments
        .windows(2)
        .find(|pair| pair[0] == "--codex")
        .map(|pair| PathBuf::from(&pair[1]))
        .ok_or_else(|| Error::new("Nucleus LaunchAgent has no configured harness"))
}

fn plan(args: &InstallArgs) -> Result<ReleasePlan> {
    for path in [&args.binary, &args.daemon, &args.bundle, &args.codex] {
        if !path.is_absolute() {
            return Err(Error::new("Nucleus candidate paths must be absolute"));
        }
    }
    if args
        .codex_home
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err(Error::new("Codex import home must be absolute"));
    }
    let installer = std::env::current_exe()?;
    let mut files = BTreeMap::from([
        (
            "bin/nucleus".into(),
            SourceFile {
                source: args.binary.clone(),
                mode: 0o755,
            },
        ),
        (
            "libexec/nucleusd".into(),
            SourceFile {
                source: args.daemon.clone(),
                mode: 0o755,
            },
        ),
        (
            "bin/nucleus-install".into(),
            SourceFile {
                source: installer.clone(),
                mode: 0o755,
            },
        ),
        (
            "package/install".into(),
            SourceFile {
                source: installer,
                mode: 0o755,
            },
        ),
    ]);
    for relative in cell_install::provider_inventory(
        &args.bundle,
        &cell_install::InstallSpec {
            product: "nucleus",
            application: "Nucleus",
            commands: &["nucleus"],
            provider: "nucleus",
        },
    )?
    .keys()
    {
        files.insert(
            format!("share/chancery/nucleus/{relative}"),
            SourceFile {
                source: args.bundle.join(relative),
                mode: 0o644,
            },
        );
    }
    Ok(ReleasePlan {
        files,
        versions: ["nucleus", "nucleusd", "nucleus-install"]
            .into_iter()
            .map(|name| (name.into(), VERSION.into()))
            .collect(),
        providers: BTreeMap::from([(
            "nucleus".into(),
            ProviderSpec {
                path: "share/chancery/nucleus".into(),
                version: VERSION.into(),
            },
        )]),
    })
}

fn install(args: &InstallArgs, home: &Path) -> Result<InstallSnapshot> {
    let before = inspect(home)?;
    if args
        .expected_current
        .as_ref()
        .is_some_and(|expected| expected != &selection(&before))
    {
        return Err(Error::new("stale Nucleus deployment plan"));
    }
    let layout = layout();
    let prepared = cell_install::transaction::prepare_release(&layout, home, &plan(args)?)?;
    let mut tx = cell_install::transaction::lock_installation(&layout, home, &legacy)?;
    tx.recheck(&before)?;
    let receipt = tx.publish(&prepared, &before, |_| Ok(()))?;
    let mut service_args = strings(&["service", "install", "--daemon"]);
    service_args.push(prepared.root.join("libexec/nucleusd").into_os_string());
    service_args.extend(strings(&["--codex"]));
    service_args.push(args.codex.as_os_str().to_owned());
    if let Some(path) = &args.codex_home {
        service_args.push("--codex-home".into());
        service_args.push(path.as_os_str().to_owned());
    }
    call(
        &prepared.root.join("bin/nucleus"),
        &service_args,
        home,
        None,
        180,
    )?;
    Ok(receipt.after)
}

fn deploy() -> Result<()> {
    let ctx = Context::read("nucleus", "nucleus", "nucleus-install", VERSION)?;
    ctx.validate_settings(&["codex_bin", "codex_home"], &[])?;
    let settings = ctx.request.settings.as_ref().unwrap_or(&Value::Null);
    let codex = if let Some(path) = settings["codex_bin"].as_str() {
        PathBuf::from(path)
    } else {
        let staged = staged_runtime(&ctx.home).join("codex");
        if staged.exists() {
            staged
        } else {
            configured_harness(&ctx.home)?
        }
    };
    let args = InstallArgs {
        binary: ctx.binary("nucleus")?,
        daemon: ctx.binary("nucleusd")?,
        bundle: ctx.request.source_root.join("nucleus/chancery"),
        codex,
        codex_home: settings["codex_home"].as_str().map(PathBuf::from),
        home: HomeArgs {
            home: Some(ctx.home.clone()),
        },
        expected_current: None,
    };
    install(&args, &ctx.home)?;
    Ok(())
}

fn run(command: Command) -> Result<Value> {
    let data = match command {
        Command::StageHarness { codex, home: args } => {
            let home = home(args.home)?;
            let runtime =
                nucleus_codex::runtime_bundle::stage_runtime(&codex, &staged_runtime(&home))
                    .map_err(|error| Error::new(error.to_string()))?;
            json!({"executable":runtime.executable})
        }
        Command::Deploy => {
            deploy()?;
            Value::Null
        }
        Command::Install(args) => {
            let home = home(args.home.home.clone())?;
            json!(install(&args, &home)?)
        }
        Command::Inspect(args) => json!(inspect(&home(args.home)?)?),
    };
    Ok(json!({"ok":true,"data":data}))
}

fn main() -> ExitCode {
    let command = Cli::parse().command;
    if matches!(command, Command::Deploy) {
        cell_install::adapter::finish_deployment(deploy())
    } else {
        cell_install::adapter::finish(run(command))
    }
}
