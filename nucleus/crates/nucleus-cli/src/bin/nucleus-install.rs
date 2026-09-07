//! Immutable Nucleus packaging around the existing product-owned Rust service
//! installer. Authentication and database compatibility remain in that boundary.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use cell_install::adapter::{Context, Operation, reply};
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::transaction::{
    InstallLayout, InstallSnapshot, LockKind, LockSpec, ProviderSpec, PublicEntry, PublicKind,
    ReleaseInfo, ReleasePlan, SourceFile,
};
use cell_install::{Disposition, Error, Result};
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
    Install(InstallArgs),
    Inspect(HomeArgs),
    Verify(InstallArgs),
    VerifyRelease {
        release: PathBuf,
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
    // The service installer captures and replaces the CLI and daemon copies.
    // Publishing them here would erase its pre-install rollback evidence.
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
    cell_install::legacy::verify(
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

fn json_call(home: &Path, owner: Option<&str>, args: &[&str]) -> Result<Value> {
    cell_install::command::json(
        &home.join(".local/bin/nucleus"),
        &strings(args),
        &environment(home, owner),
        Duration::from_secs(180),
    )
}

fn maintenance(home: &Path, owner: &str, operation: &str, named: bool) -> Result<Value> {
    let mut args = vec!["--compact", "maintenance", operation];
    if named {
        args.push(owner);
    }
    let value = json_call(home, Some(owner), &args)?;
    Ok(cell_install::command::maintenance(&value)?.clone())
}

fn sole(status: &Value, owner: &str) -> Result<()> {
    if status["holds"] != json!([owner]) || status["drained"] != true {
        return Err(Error::new("Nucleus requires this run's sole drained hold"));
    }
    Ok(())
}

fn harness(health: &Value) -> Result<PathBuf> {
    let path = health
        .get("harnessExecutable")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| Error::new("Nucleus did not report its configured harness"))?;
    if !path.is_absolute() {
        return Err(Error::new("Nucleus harness is not absolute"));
    }
    // Preserve the configured path; the harness itself may be a supported
    // external symlink. This follows the service's own executable resolution.
    let resolved = std::fs::canonicalize(&path)?;
    cell_install::file_digest(&resolved)?;
    Ok(path)
}

fn release_root(home: &Path, info: &ReleaseInfo) -> PathBuf {
    home.join("Library/Application Support/Nucleus/install/releases")
        .join(&info.release_id)
}
fn selection(snapshot: &InstallSnapshot) -> String {
    snapshot.current.as_ref().map_or_else(
        || "absent".into(),
        |info| format!("releases/{}", info.release_id),
    )
}

fn verify_copies(home: &Path, info: &ReleaseInfo) -> Result<()> {
    let root = release_root(home, info);
    for (public, artifact) in [
        (".local/bin/nucleus", "bin/nucleus"),
        (".local/libexec/nucleusd", "libexec/nucleusd"),
    ] {
        if cell_install::file_digest(&home.join(public))?
            != cell_install::file_digest(&root.join(artifact))?
        {
            return Err(Error::new(
                "Nucleus public program copy differs from selected release",
            ));
        }
    }
    Ok(())
}

fn inspect(home: &Path) -> Result<InstallSnapshot> {
    let snapshot = cell_install::transaction::inspect_installation(&layout(), home, &legacy)?;
    if let Some(info) = &snapshot.current {
        verify_copies(home, info)?;
    } else if home.join(".local/bin/nucleus").exists()
        || home.join(".local/libexec/nucleusd").exists()
    {
        return Err(Error::new(
            "Nucleus public programs have no owned package selection",
        ));
    }
    Ok(snapshot)
}

fn plan(args: &InstallArgs, home: &Path) -> Result<ReleasePlan> {
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
    for (path, name) in [(&args.binary, "nucleus"), (&args.daemon, "nucleusd")] {
        cell_install::file_digest(path)?;
        let output = call(path, &strings(&["--version"]), home, None, 30)?;
        if String::from_utf8_lossy(&output.stdout).trim() != format!("{name} {VERSION}") {
            return Err(Error::new(
                "Nucleus programs require their version-matched installer",
            ));
        }
        call(path, &strings(&["--help"]), home, None, 30)?;
    }
    call(&args.codex, &strings(&["--version"]), home, None, 30)?;
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

fn verify_plan(info: &ReleaseInfo, plan: &ReleasePlan) -> Result<()> {
    if info.files.len() != plan.files.len() || info.versions != plan.versions {
        return Err(Error::new(
            "Nucleus release differs from candidate inventory",
        ));
    }
    for (path, file) in &plan.files {
        let proof = info
            .files
            .get(path)
            .ok_or_else(|| Error::new("candidate Nucleus artifact missing"))?;
        if proof.sha256 != cell_install::file_digest(&file.source)? || proof.mode != file.mode {
            return Err(Error::new(
                "Nucleus installed bytes differ from selected candidate",
            ));
        }
    }
    Ok(())
}

fn install(
    args: &InstallArgs,
    home: &Path,
    owner: Option<&str>,
    exact: Option<&InstallSnapshot>,
) -> Result<InstallSnapshot> {
    let before = inspect(home)?;
    if args
        .expected_current
        .as_ref()
        .is_some_and(|expected| expected != &selection(&before))
        || exact.is_some_and(|expected| expected != &before)
    {
        return Err(Error::new("stale Nucleus deployment plan"));
    }
    let layout = layout();
    let prepared = cell_install::transaction::prepare_release(&layout, home, &plan(args, home)?)?;
    let mut tx = cell_install::transaction::lock_installation(&layout, home, &legacy)?;
    tx.recheck(&before)?;
    if let Some(info) = &before.current {
        verify_copies(home, info)?;
    }
    if let Some(owner) = owner {
        sole(&maintenance(home, owner, "status", false)?, owner)?;
        let health = json_call(
            home,
            Some(owner),
            &["--compact", "maintenance", "health", owner],
        )?;
        if harness(&health)? != args.codex {
            return Err(Error::new("configured Nucleus harness changed"));
        }
    }
    let receipt = tx.publish(&prepared, &before, |_| Ok(()))?;
    let mut service_args = strings(&["service", "install", "--daemon"]);
    service_args.push(prepared.root.join("libexec/nucleusd").into_os_string());
    service_args.extend(strings(&["--codex"]));
    service_args.push(args.codex.as_os_str().to_owned());
    if let Some(path) = &args.codex_home {
        service_args.push("--codex-home".into());
        service_args.push(path.as_os_str().to_owned());
    }
    let service = call(
        &prepared.root.join("bin/nucleus"),
        &service_args,
        home,
        owner,
        180,
    );
    if let Err(mut error) = service {
        if verify_copies(home, &prepared.info).is_ok() {
            // The inner service installer deliberately leaves candidate programs
            // when a schema change makes binary rollback unsafe. Credentials
            // are forward-only; this layer never reads or copies auth state.
            error.message = "Nucleus service cutover failed with candidate programs retained; matching package remains selected and maintenance must remain held".into();
            error.disposition = Disposition::Uncertain;
        } else if tx.restore(&receipt, |_| Ok(())).is_err() {
            error.message =
                "Nucleus service and package recovery are incomplete; maintenance must remain held"
                    .into();
            error.disposition = Disposition::Uncertain;
        }
        return Err(error);
    }
    verify_copies(home, &prepared.info)?;
    Ok(receipt.after)
}

fn runtime(ctx: &Context, owned: bool) -> Result<Value> {
    let expected = ctx.prior()?["harness_executable"]
        .as_str()
        .ok_or_else(|| Error::new("captured Nucleus harness is missing"))?;
    let health = if owned {
        sole(
            &maintenance(&ctx.home, &ctx.request.run_id, "status", false)?,
            &ctx.request.run_id,
        )?;
        json_call(
            &ctx.home,
            Some(&ctx.request.run_id),
            &["--compact", "maintenance", "health", &ctx.request.run_id],
        )?
    } else {
        json_call(&ctx.home, None, &["--compact", "health"])?
    };
    if harness(&health)? != Path::new(expected) {
        return Err(Error::new("Nucleus harness changed since inspection"));
    }
    Ok(json!({"service_ready":true}))
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep all operations beside their shared service, harness, and installation recovery evidence"
)]
fn adapter(operation: Operation) -> Result<Value> {
    let ctx = Context::read("nucleus", "nucleus", "nucleus-install", VERSION)?;
    if std::fs::symlink_metadata(
        ctx.home
            .join("Library/Application Support/Nucleus/.deploy-lock"),
    )
    .is_ok()
    {
        return Err(Error::new(
            "Nucleus installation transaction is active or retained; product recovery is required",
        ));
    }
    let snapshot = inspect(&ctx.home)?;
    let prior = || -> Result<InstallSnapshot> {
        Ok(serde_json::from_value(ctx.prior()?["installed"].clone())?)
    };
    let args = || -> Result<InstallArgs> {
        Ok(InstallArgs {
            binary: ctx.binary("nucleus")?,
            daemon: ctx.binary("nucleusd")?,
            bundle: ctx.request.source_root.join("nucleus/chancery"),
            codex: ctx.prior()?["harness_executable"]
                .as_str()
                .map(PathBuf::from)
                .ok_or_else(|| Error::new("captured harness missing"))?,
            codex_home: None,
            home: HomeArgs {
                home: Some(ctx.home.clone()),
            },
            expected_current: Some(selection(&prior()?)),
        })
    };
    let prove = || -> Result<()> {
        if ctx.selected() {
            verify_plan(
                snapshot
                    .current
                    .as_ref()
                    .ok_or_else(|| Error::new("Nucleus is absent"))?,
                &plan(&args()?, &ctx.home)?,
            )
        } else if snapshot == prior()? {
            Ok(())
        } else {
            Err(Error::new("affected Nucleus installation changed"))
        }
    };
    match operation {
        Operation::Inspect => {
            if snapshot.current.is_none() {
                return Err(Error::new(
                    "existing configured Nucleus installation required",
                ));
            }
            let status = maintenance(&ctx.home, &ctx.request.run_id, "status", false)?;
            if status["holds"] != json!([]) {
                return Err(Error::new("another operation holds Nucleus"));
            }
            let health = json_call(&ctx.home, None, &["--compact", "health"])?;
            if ctx.selected() {
                plan(
                    &InstallArgs {
                        binary: ctx.binary("nucleus")?,
                        daemon: ctx.binary("nucleusd")?,
                        bundle: ctx.request.source_root.join("nucleus/chancery"),
                        codex: harness(&health)?,
                        codex_home: None,
                        home: HomeArgs {
                            home: Some(ctx.home.clone()),
                        },
                        expected_current: None,
                    },
                    &ctx.home,
                )?;
            }
            Ok(reply(
                "ready",
                "owned Nucleus installation and configured harness inspected",
                json!({"current":selection(&snapshot),"installed":snapshot,"runtime":status,"harness_executable":harness(&health)?,"maintenance_products":["annals","krisis","semantics","crm","todo","weaver","platter","paperboy"],"after":[]}),
            ))
        }
        Operation::Hold => {
            let status = maintenance(&ctx.home, &ctx.request.run_id, "hold", true)?;
            if !status["holds"]
                .as_array()
                .is_some_and(|holds| holds.contains(&json!(ctx.request.run_id)))
            {
                return Err(Error::new("Nucleus did not retain deployment hold"));
            }
            Ok(reply("held", "new Nucleus admission held", status))
        }
        Operation::Drain => {
            let deadline = Instant::now() + Duration::from_secs(600);
            loop {
                let status = maintenance(&ctx.home, &ctx.request.run_id, "status", false)?;
                if status["holds"] != json!([ctx.request.run_id]) {
                    return Err(Error::new("Nucleus drain requires sole owner"));
                }
                if status["drained"] == true {
                    return Ok(reply("drained", "Nucleus jobs and slots settled", status));
                }
                if Instant::now() >= deadline {
                    return Err(Error::new("Nucleus did not drain; hold retained"));
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        Operation::Apply => {
            if !ctx.selected() {
                return Err(Error::new("affected Nucleus cannot be upgraded"));
            }
            let installed = install(
                &args()?,
                &ctx.home,
                Some(&ctx.request.run_id),
                Some(&prior()?),
            )?;
            Ok(reply(
                "applied",
                "Nucleus product service installer completed",
                json!({"current":selection(&installed),"installed":installed}),
            ))
        }
        Operation::Verify => {
            prove()?;
            Ok(reply(
                "verified",
                "Nucleus program and guarded service readiness verified",
                runtime(&ctx, true)?,
            ))
        }
        Operation::Release => {
            let status = maintenance(&ctx.home, &ctx.request.run_id, "release", true)?;
            if status["holds"]
                .as_array()
                .is_some_and(|holds| holds.contains(&json!(ctx.request.run_id)))
            {
                return Err(Error::new("Nucleus did not release deployment hold"));
            }
            Ok(reply(
                "released",
                "only this Nucleus deployment hold released",
                status,
            ))
        }
        Operation::Recover => {
            let recovery = ctx.request.recovery.clone().unwrap_or_default();
            if recovery["apply_started"] == true && recovery["applied"] != true {
                return Err(Error { message: "Nucleus service cutover is uncertain; hold retained. Use supported Nucleus service recovery before resolving deployment".into(), disposition: Disposition::Uncertain });
            }
            let unchanged = snapshot == prior()?;
            if !unchanged {
                prove()?;
            }
            let status = maintenance(&ctx.home, &ctx.request.run_id, "status", false)?;
            if status["holds"] == json!([ctx.request.run_id]) {
                runtime(&ctx, true)?;
            } else if status["holds"] == json!([]) && (unchanged || recovery["verified"] == true) {
                runtime(&ctx, false)?;
            } else {
                return Err(Error::new(
                    "Nucleus recovery lacks owned hold or captured verification",
                ));
            }
            Ok(reply(
                "recovered",
                "coherent Nucleus program and service verified",
                json!({"safe_to_release":true,"installed":if unchanged {"prior"}else{"candidate"}}),
            ))
        }
    }
}

fn run(command: Command) -> Result<Value> {
    let data =
        match command {
            Command::Adapter { operation } => return adapter(operation),
            Command::Install(args) => {
                let home = home(args.home.home.clone())?;
                let owner = std::env::var("CELL_DEPLOYMENT_RUN_ID")
                    .ok()
                    .filter(|value| !value.is_empty());
                json!(install(&args, &home, owner.as_deref(), None)?)
            }
            Command::Inspect(args) => json!(inspect(&home(args.home)?)?),
            Command::Verify(args) => {
                let home = home(args.home.home.clone())?;
                let snapshot = inspect(&home)?;
                verify_plan(
                    snapshot
                        .current
                        .as_ref()
                        .ok_or_else(|| Error::new("Nucleus is absent"))?,
                    &plan(&args, &home)?,
                )?;
                let health = json_call(&home, None, &["--compact", "health"])?;
                if harness(&health)? != args.codex {
                    return Err(Error::new("configured Nucleus harness differs"));
                }
                json!(snapshot)
            }
            Command::VerifyRelease { release } => json!(
                cell_install::transaction::verify_release_at(&layout(), &release, &legacy)?
            ),
        };
    Ok(json!({"ok":true,"data":data}))
}

fn main() -> ExitCode {
    cell_install::adapter::finish(run(Cli::parse().command))
}
