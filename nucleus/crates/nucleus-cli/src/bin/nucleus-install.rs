//! Immutable Nucleus packaging around the existing product-owned Rust service
//! installer. Authentication and database compatibility remain in that boundary.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

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
    /// Stage the complete supported Codex runtime without changing the service.
    StageHarness {
        #[arg(long)]
        codex: PathBuf,
        #[command(flatten)]
        home: HomeArgs,
    },
    Install(InstallArgs),
    Inspect(HomeArgs),
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
    if !home.join(".local/bin/nucleus").exists()
        && !home
            .join("Library/Application Support/Nucleus/nucleus.db")
            .exists()
    {
        let gate = cell_maintenance::Gate::new(
            home.join("Library/Application Support/Nucleus/deployment-maintenance"),
        );
        let status = match operation {
            "hold" => gate.hold(owner),
            "release" => gate.release(owner),
            _ => gate.status(),
        }
        .map_err(|error| Error::new(error.to_string()))?;
        return Ok(
            json!({"protocol_version":1,"holds":status.holds,"drained":status.drained,"nonterminal_jobs":0}),
        );
    }
    if operation == "release" {
        let socket = std::env::var_os("NUCLEUS_SOCKET").map_or_else(
            || home.join("Library/Application Support/Nucleus/nucleus.sock"),
            PathBuf::from,
        );
        let client = nucleus_client::NucleusClient::new(socket)
            .map_err(|error| Error::new(error.to_string()))?;
        return tokio::runtime::Runtime::new()?.block_on(release_maintenance(
            &client,
            owner,
            Duration::from_secs(120),
        ));
    }
    let value = json_call(home, Some(owner), &args)?;
    Ok(cell_install::command::maintenance(&value)?.clone())
}

async fn release_maintenance(
    client: &nucleus_client::NucleusClient,
    owner: &str,
    timeout: Duration,
) -> Result<Value> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        // Bootstrap can return before the daemon binds its socket. Retry the
        // idempotent release itself, never a health or readiness probe.
        match tokio::time::timeout_at(deadline, client.maintenance_release(owner)).await {
            Ok(Ok(status)) => return Ok(serde_json::to_value(status)?),
            Ok(Err(nucleus_client::ClientError::Transport { .. })) => {
                tokio::time::sleep_until(
                    deadline.min(tokio::time::Instant::now() + Duration::from_millis(100)),
                )
                .await;
            }
            Ok(Err(error)) => return Err(Error::new(error.to_string())),
            Err(_) => break,
        }
    }
    Err(Error {
        message: "Nucleus maintenance release exceeded the service-start deadline".into(),
        disposition: Disposition::Uncertain,
    })
}

fn sole(status: &Value, owner: &str) -> Result<()> {
    if status["holds"] != json!([owner]) || status["drained"] != true {
        return Err(Error::new("Nucleus requires this run's sole drained hold"));
    }
    Ok(())
}

fn staged_runtime(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Nucleus/harnesses/codex")
        .join(nucleus_codex::SUPPORTED_CODEX_VERSION)
        .join("runtime")
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
    for (key, path) in [("nucleus", &args.binary), ("nucleusd", &args.daemon)] {
        cell_install::signing::verify_native("nucleus", key, path)?;
        call(path, &strings(&["--version"]), home, None, 30)?;
        call(path, &strings(&["--help"]), home, None, 30)?;
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
    if let Some(owner) = owner {
        sole(&maintenance(home, owner, "status", false)?, owner)?;
        write_cutover(
            home,
            &json!({"owner":owner,"before":before,"candidate":prepared.info,"codex":args.codex,"codex_home":args.codex_home}),
        )?;
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
        error.message = format!(
            "Nucleus service cutover failed; candidate package and recovery evidence retained: {}",
            error.message
        );
        error.disposition = Disposition::Uncertain;
        return Err(error);
    }
    if owner.is_some() {
        fs::remove_file(cutover_path(home))?;
    }
    Ok(receipt.after)
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep all operations beside their shared service, harness, and installation recovery evidence"
)]
fn adapter(operation: Operation) -> Result<Value> {
    let ctx = Context::read("nucleus", "nucleus", "nucleus-install", VERSION)?;
    ctx.validate_settings(&["codex_bin", "codex_home"], &[])?;
    if !matches!(operation, Operation::Recover)
        && std::fs::symlink_metadata(
            ctx.home
                .join("Library/Application Support/Nucleus/.deploy-lock"),
        )
        .is_ok()
    {
        return Err(Error::new(
            "Nucleus installation transaction is active or retained; product recovery is required",
        ));
    }
    if matches!(operation, Operation::Recover) {
        recover_cutover(&ctx)?;
    } else if cutover_path(&ctx.home).exists() {
        return Err(Error::new(
            "Nucleus has an unfinished service cutover; recover its recorded deployment",
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
            codex_home: ctx.prior()?["codex_home"].as_str().map(PathBuf::from),
            home: HomeArgs {
                home: Some(ctx.home.clone()),
            },
            expected_current: Some(selection(&prior()?)),
        })
    };
    match operation {
        Operation::Configure => Ok(reply(
            "configured",
            "Nucleus service configuration retained",
            json!({}),
        )),
        Operation::Activate => Ok(reply(
            "activated",
            "product install transaction preserved operational intent",
            json!({}),
        )),
        Operation::Inspect => {
            if snapshot.current.is_none() {
                let (codex, codex_home) = fresh_settings(&ctx)?;
                let status = maintenance(&ctx.home, &ctx.request.run_id, "status", false)?;
                if status["holds"] != json!([]) {
                    return Err(Error::new("another operation holds fresh Nucleus"));
                }
                plan(
                    &InstallArgs {
                        binary: ctx.binary("nucleus")?,
                        daemon: ctx.binary("nucleusd")?,
                        bundle: ctx.request.source_root.join("nucleus/chancery"),
                        codex: codex.clone(),
                        codex_home: codex_home.clone(),
                        home: HomeArgs {
                            home: Some(ctx.home.clone()),
                        },
                        expected_current: Some("absent".into()),
                    },
                    &ctx.home,
                )?;
                return Ok(reply(
                    "ready",
                    "fresh Nucleus configuration and explicit authentication source inspected",
                    json!({"current":"absent","installed":snapshot,"runtime":status,"harness_executable":codex,"codex_home":codex_home,"maintenance_products":[],"after":[]}),
                ));
            }
            let status = maintenance(&ctx.home, &ctx.request.run_id, "status", false)?;
            if status["holds"] != json!([]) {
                return Err(Error::new("another operation holds Nucleus"));
            }
            let configured_harness = configured_harness(&ctx.home)?;
            let selected_harness = if ctx.selected() {
                ctx.request
                    .settings
                    .as_ref()
                    .and_then(|settings| settings["codex_bin"].as_str())
                    .map_or_else(|| staged_runtime(&ctx.home).join("codex"), PathBuf::from)
            } else {
                configured_harness.clone()
            };
            if ctx.selected() {
                plan(
                    &InstallArgs {
                        binary: ctx.binary("nucleus")?,
                        daemon: ctx.binary("nucleusd")?,
                        bundle: ctx.request.source_root.join("nucleus/chancery"),
                        codex: selected_harness.clone(),
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
                json!({"current":selection(&snapshot),"installed":snapshot,"runtime":status,"harness_executable":selected_harness,"prior_harness_executable":configured_harness,"maintenance_products":[],"after":[]}),
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
            let status = maintenance(&ctx.home, &ctx.request.run_id, "status", false)?;
            if status["holds"] != json!([ctx.request.run_id]) {
                return Err(Error::new("drain requires the sole deployment owner"));
            }
            Ok(reply(
                if status["drained"] == true {
                    "drained"
                } else {
                    "waiting"
                },
                "Nucleus jobs and slots",
                status,
            ))
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
            let unchanged = snapshot == prior()?;
            if unchanged && snapshot.current.is_none() {
                return Ok(reply(
                    "recovered",
                    "Nucleus remains absent with no service cutover",
                    json!({"safe_to_release":true,"installed":"prior"}),
                ));
            }
            let status = maintenance(&ctx.home, &ctx.request.run_id, "status", false)?;
            if status["holds"] != json!([]) && status["holds"] != json!([ctx.request.run_id]) {
                return Err(Error::new(
                    "Nucleus recovery has a foreign maintenance hold",
                ));
            }
            Ok(reply(
                "recovered",
                "Nucleus installation recovery completed",
                json!({"safe_to_release":true,"installed":if unchanged {"prior"}else{"candidate"}}),
            ))
        }
    }
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
        Command::Adapter { operation } => return adapter(operation),
        Command::Install(args) => {
            let home = home(args.home.home.clone())?;
            let owner = std::env::var("CELL_DEPLOYMENT_RUN_ID")
                .ok()
                .filter(|value| !value.is_empty());
            json!(install(&args, &home, owner.as_deref(), None)?)
        }
        Command::Inspect(args) => json!(inspect(&home(args.home)?)?),
    };
    Ok(json!({"ok":true,"data":data}))
}

fn main() -> ExitCode {
    cell_install::adapter::finish(run(Cli::parse().command))
}

fn cutover_path(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Nucleus/service-cutover.json")
}

fn write_cutover(home: &Path, value: &Value) -> Result<()> {
    let path = cutover_path(home);
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| Error::new("invalid Nucleus state path"))?,
    )?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| Error::new(error.to_string()))?
        .as_nanos();
    let next = path.with_extension(format!("next-{}-{stamp}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&next)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()?;
    fs::rename(next, &path)?;
    fs::File::open(
        path.parent()
            .ok_or_else(|| Error::new("invalid Nucleus state path"))?,
    )?
    .sync_all()?;
    Ok(())
}

fn fresh_settings(ctx: &Context) -> Result<(PathBuf, Option<PathBuf>)> {
    let settings = ctx.request.settings.as_ref().unwrap_or(&Value::Null);
    let codex = settings["codex_bin"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| Error::new("fresh Nucleus requires settings.nucleus.codex_bin"))?;
    if !codex.is_absolute() {
        return Err(Error::new("Nucleus codex_bin must be absolute"));
    }
    let owned = ctx
        .home
        .join("Library/Application Support/Nucleus/codex-home");
    let source = if owned.join("auth.json").exists() {
        owned
    } else {
        settings["codex_home"].as_str().map(PathBuf::from).ok_or_else(|| Error::new("fresh Nucleus requires settings.nucleus.codex_home pointing to an authenticated Codex home"))?
    };
    if !source.is_absolute() {
        return Err(Error::new("Nucleus codex_home must be absolute"));
    }
    Ok((fs::canonicalize(codex)?, Some(fs::canonicalize(source)?)))
}

fn recover_cutover(ctx: &Context) -> Result<()> {
    {
        let layout = layout();
        let _lock = cell_install::transaction::lock_installation(&layout, &ctx.home, &legacy)?;
    }
    let path = cutover_path(&ctx.home);
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.uid() != fs::metadata(&ctx.home)?.uid()
        || meta.mode() & 0o777 != 0o600
    {
        return Err(Error::new(
            "Nucleus recovery evidence is not private owned state",
        ));
    }
    let saved: Value = serde_json::from_slice(&fs::read(&path)?)?;
    if saved["owner"] != ctx.request.run_id {
        return Err(Error::new(
            "Nucleus service cutover belongs to another owner",
        ));
    }
    let before: InstallSnapshot = serde_json::from_value(saved["before"].clone())?;
    if json!(before) != ctx.prior()?["installed"] {
        return Err(Error::new(
            "Nucleus cutover baseline differs from deployment",
        ));
    }
    let info: ReleaseInfo = serde_json::from_value(saved["candidate"].clone())?;
    let root = release_root(&ctx.home, &info);
    let codex = saved["codex"]
        .as_str()
        .ok_or_else(|| Error::new("missing Nucleus recovery harness"))?;
    if saved["codex"] != ctx.prior()?["harness_executable"] {
        return Err(Error::new("Nucleus recovery harness differs"));
    }
    let layout = layout();
    let mut tx = cell_install::transaction::lock_installation(&layout, &ctx.home, &legacy)?;
    let prepared = cell_install::transaction::PreparedRelease {
        root: root.clone(),
        info: info.clone(),
    };
    tx.recover(&before, &prepared, true, |_| Ok(()))?;
    let mut arguments = strings(&["service", "recover", "--daemon"]);
    arguments.push(root.join("libexec/nucleusd").into_os_string());
    arguments.extend(strings(&["--codex", codex]));
    if !ctx
        .home
        .join("Library/Application Support/Nucleus/codex-home/auth.json")
        .exists()
        && let Some(source) = saved["codex_home"].as_str()
    {
        arguments.extend(strings(&["--codex-home", source]));
    }
    call(
        &root.join("bin/nucleus"),
        &arguments,
        &ctx.home,
        Some(&ctx.request.run_id),
        180,
    )?;
    fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    async fn respond(
        listener: tokio::net::UnixListener,
        status: &str,
        body: &str,
    ) -> Result<String> {
        let (mut stream, _) = listener.accept().await?;
        let mut request = Vec::new();
        loop {
            let mut buffer = [0; 4096];
            let count = stream.read(&mut buffer).await?;
            request.extend_from_slice(&buffer[..count]);
            if count == 0 || request.ends_with(b"{\"run_id\":\"test-owner\"}") {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await?;
        Ok(String::from_utf8_lossy(&request).into_owned())
    }

    #[tokio::test]
    async fn maintenance_release_waits_for_the_starting_socket() -> Result<()> {
        let directory = tempfile::tempdir_in("/tmp")?;
        let socket = directory.path().join("nucleus.sock");
        let client = nucleus_client::NucleusClient::new(&socket)
            .map_err(|error| Error::new(error.to_string()))?;
        let server = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let listener = tokio::net::UnixListener::bind(&socket)?;
            respond(
                listener,
                "200 OK",
                r#"{"protocol_version":1,"holds":[],"drained":true,"nonterminal_jobs":0}"#,
            )
            .await
        });
        let status = release_maintenance(&client, "test-owner", Duration::from_secs(2)).await?;
        assert_eq!(status["holds"], json!([]));
        let request = server
            .await
            .map_err(|error| Error::new(error.to_string()))??;
        assert!(request.starts_with("POST /v1/maintenance/release HTTP/1.1\r\n"));
        assert!(request.ends_with(r#"{"run_id":"test-owner"}"#));
        Ok(())
    }

    #[tokio::test]
    async fn maintenance_release_does_not_retry_an_api_rejection() -> Result<()> {
        let directory = tempfile::tempdir_in("/tmp")?;
        let socket = directory.path().join("nucleus.sock");
        let listener = tokio::net::UnixListener::bind(&socket)?;
        let client = nucleus_client::NucleusClient::new(&socket)
            .map_err(|error| Error::new(error.to_string()))?;
        let server = tokio::spawn(async move {
            respond(
                listener,
                "409 Conflict",
                r#"{"version":1,"code":"maintenance_conflict","message":"owner rejected"}"#,
            )
            .await
        });
        let result = tokio::time::timeout(
            Duration::from_millis(500),
            release_maintenance(&client, "test-owner", Duration::from_secs(2)),
        )
        .await
        .map_err(|error| Error::new(error.to_string()))?;
        assert!(result.is_err_and(|error| error.message.contains("maintenance_conflict")));
        server
            .await
            .map_err(|error| Error::new(error.to_string()))??;
        Ok(())
    }

    #[tokio::test]
    async fn maintenance_release_stops_at_its_deadline() -> Result<()> {
        let directory = tempfile::tempdir_in("/tmp")?;
        let client = nucleus_client::NucleusClient::new(directory.path().join("nucleus.sock"))
            .map_err(|error| Error::new(error.to_string()))?;
        let result = tokio::time::timeout(
            Duration::from_millis(500),
            release_maintenance(&client, "test-owner", Duration::from_millis(50)),
        )
        .await
        .map_err(|error| Error::new(error.to_string()))?;
        assert!(result.is_err_and(|error| error.disposition == Disposition::Uncertain));
        Ok(())
    }
}
