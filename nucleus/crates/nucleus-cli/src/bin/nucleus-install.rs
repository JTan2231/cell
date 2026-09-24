//! Immutable Nucleus packaging around the existing product-owned Rust service
//! installer. Authentication and database compatibility remain in that boundary.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
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

fn unheld_health(home: &Path) -> Result<Value> {
    let status = json_call(home, None, &["--compact", "service", "status"])?;
    health_from_service_status(&status)
}

fn health_from_service_status(status: &Value) -> Result<Value> {
    if status["loaded"] != true || !status["healthError"].is_null() {
        return Err(Error::new("Nucleus service health is unavailable"));
    }
    let value = status
        .get("health")
        .ok_or_else(|| Error::new("Nucleus service did not report health"))?;
    let health: nucleus_core::HealthResponseV1 = serde_json::from_value(value.clone())?;
    if !nucleus_client::unheld_deployment_ready(&health) {
        return Err(Error::new("Nucleus runtime is not ready for deployment"));
    }
    Ok(value.clone())
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

fn candidate_harness(home: &Path, health: &Value) -> Result<PathBuf> {
    let configured = harness(health)?;
    // The CI tool-path proof uses this exact staged pair. Version equality
    // alone cannot justify retaining another pair at a different path.
    let candidate = staged_runtime(home).join("codex");
    let candidate_identity = runtime_identity(&candidate)?;
    if health["harness"]["harnessVersion"] == nucleus_codex::SUPPORTED_CODEX_VERSION
        && runtime_identity(&configured).is_ok_and(|identity| identity == candidate_identity)
    {
        return Ok(configured);
    }
    Ok(fs::canonicalize(candidate)?)
}

fn staged_runtime(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Nucleus/harnesses/codex")
        .join(nucleus_codex::SUPPORTED_CODEX_VERSION)
        .join("runtime")
}

fn runtime_identity(executable: &Path) -> Result<Value> {
    let runtime = nucleus_codex::runtime_bundle::verify_runtime(executable)
        .map_err(|error| Error::new(error.to_string()))?;
    Ok(json!({
        "version": runtime.version,
        "codex_sha256": runtime.codex_sha256,
        "code_mode_host_sha256": runtime.code_mode_host_sha256,
    }))
}

fn verify_runtime_identity(executable: &Path, expected: &Value) -> Result<()> {
    if runtime_identity(executable)? != *expected {
        return Err(Error::new(
            "Codex runtime differs from the inspected bundle",
        ));
    }
    Ok(())
}

fn runtime_harness(prior: &Value, unchanged: bool) -> Result<&str> {
    let field = if unchanged && prior["prior_harness_executable"].is_string() {
        "prior_harness_executable"
    } else {
        "harness_executable"
    };
    prior[field]
        .as_str()
        .ok_or_else(|| Error::new("captured Nucleus harness is missing"))
}

fn verify_prior_harness(health: &Value, candidate: &Path, prior: Option<&Value>) -> Result<()> {
    let expected = prior
        .map(|prior| runtime_harness(prior, true))
        .transpose()?
        .map_or(candidate, Path::new);
    if harness(health)? != expected {
        return Err(Error::new("configured Nucleus harness changed"));
    }
    Ok(())
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
    runtime_identity(&args.codex)?;
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
    prior: Option<&Value>,
) -> Result<InstallSnapshot> {
    if let Some(expected) = prior.and_then(|value| value.get("harness_runtime")) {
        verify_runtime_identity(&args.codex, expected)?;
    }
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
        if before.current.is_some() {
            let health = json_call(
                home,
                Some(owner),
                &["--compact", "maintenance", "health", owner],
            )?;
            verify_prior_harness(&health, &args.codex, prior)?;
        }
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
    if owner.is_some() {
        fs::remove_file(cutover_path(home))?;
    }
    Ok(receipt.after)
}

fn runtime(ctx: &Context, owned: bool) -> Result<Value> {
    let snapshot = inspect(&ctx.home)?;
    let prior = ctx.prior()?;
    let unchanged = json!(snapshot) == prior["installed"];
    let expected = runtime_harness(prior, unchanged)?;
    let identity_key = if unchanged {
        "prior_harness_runtime"
    } else {
        "harness_runtime"
    };
    if let Some(identity) = prior.get(identity_key).filter(|value| !value.is_null()) {
        verify_runtime_identity(Path::new(expected), identity)?;
    }
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
        unheld_health(&ctx.home)?
    };
    if harness(&health)? != Path::new(expected) {
        return Err(Error::new("Nucleus harness changed since inspection"));
    }
    let expected_version = snapshot
        .current
        .as_ref()
        .and_then(|info| info.versions.get("nucleusd"))
        .ok_or_else(|| Error::new("Nucleus daemon version is absent from selected package"))?;
    if health["daemonVersion"] != *expected_version {
        return Err(Error::new(
            "resident Nucleus daemon version differs from selected package",
        ));
    }
    Ok(json!({"service_ready":true,"daemon_version":expected_version}))
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
        Operation::Configure => Ok(reply(
            "configured",
            "Nucleus configured harness, authentication and held service verified",
            runtime(&ctx, true)?,
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
                    json!({"current":"absent","installed":snapshot,"runtime":status,"harness_runtime":runtime_identity(&codex)?,"harness_executable":codex,"codex_home":codex_home,"maintenance_products":[],"after":[]}),
                ));
            }
            let status = maintenance(&ctx.home, &ctx.request.run_id, "status", false)?;
            if status["holds"] != json!([]) {
                return Err(Error::new("another operation holds Nucleus"));
            }
            let health = unheld_health(&ctx.home)?;
            let configured_harness = harness(&health)?;
            let selected_harness = if ctx.selected() {
                candidate_harness(&ctx.home, &health)?
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
                json!({"current":selection(&snapshot),"installed":snapshot,"runtime":status,"harness_runtime":runtime_identity(&selected_harness)?,"prior_harness_runtime":runtime_identity(&configured_harness).ok(),"harness_executable":selected_harness,"prior_harness_executable":configured_harness,"maintenance_products":[],"after":[]}),
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
                Some(ctx.prior()?),
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
            let unchanged = snapshot == prior()?;
            if unchanged && snapshot.current.is_none() {
                return Ok(reply(
                    "recovered",
                    "Nucleus remains absent with no service cutover",
                    json!({"safe_to_release":true,"installed":"prior"}),
                ));
            }
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
    let data = match command {
        Command::StageHarness { codex, home: args } => {
            let home = home(args.home)?;
            let runtime =
                nucleus_codex::runtime_bundle::stage_runtime(&codex, &staged_runtime(&home))
                    .map_err(|error| Error::new(error.to_string()))?;
            json!({"executable":runtime.executable,"runtime":runtime_identity(&runtime.executable)?})
        }
        Command::Adapter { operation } => return adapter(operation),
        Command::Install(args) => {
            let home = home(args.home.home.clone())?;
            let owner = std::env::var("CELL_DEPLOYMENT_RUN_ID")
                .ok()
                .filter(|value| !value.is_empty());
            json!(install(&args, &home, owner.as_deref(), None, None)?)
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
            let health = unheld_health(&home)?;
            if harness(&health)? != args.codex {
                return Err(Error::new("configured Nucleus harness differs"));
            }
            json!(snapshot)
        }
        Command::VerifyRelease { release } => json!(cell_install::transaction::verify_release_at(
            &layout(),
            &release,
            &legacy
        )?),
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
    verify_runtime_identity(
        &codex,
        &runtime_identity(&staged_runtime(&ctx.home).join("codex"))?,
    )?;
    cell_install::file_digest(&codex)?;
    let version = call(&codex, &strings(&["--version"]), &ctx.home, None, 30)?;
    if String::from_utf8_lossy(&version.stdout).trim()
        != format!("codex-cli {}", nucleus_codex::SUPPORTED_CODEX_VERSION)
    {
        return Err(Error::new(
            "fresh Nucleus requires its exactly supported Codex version",
        ));
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
    let auth = source.join("auth.json");
    let meta = fs::symlink_metadata(&auth)?;
    if !meta.is_file()
        || meta.uid() != fs::metadata(&ctx.home)?.uid()
        || meta.permissions().mode() & 0o777 != 0o600
        || meta.nlink() != 1
        || meta.len() > 4 * 1024 * 1024
    {
        return Err(Error::new(
            "Nucleus authentication source must be a private owned regular file",
        ));
    }
    nucleus_codex::validate_auth_document(&fs::read(auth)?)
        .map_err(|error| Error::new(error.to_string()))?;
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
    if let Some(identity) = ctx.prior()?.get("harness_runtime") {
        verify_runtime_identity(Path::new(codex), identity)?;
    }
    let args = InstallArgs {
        binary: ctx.binary("nucleus")?,
        daemon: ctx.binary("nucleusd")?,
        bundle: ctx.request.source_root.join("nucleus/chancery"),
        codex: codex.into(),
        codex_home: None,
        home: HomeArgs {
            home: Some(ctx.home.clone()),
        },
        expected_current: None,
    };
    verify_plan(&info, &plan(&args, &ctx.home)?)?;
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
    verify_copies(&ctx.home, &info)?;
    runtime(ctx, true)?;
    fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service_status_fixture(quota_state: &str) -> Value {
        json!({
            "loaded":true,
            "health":{
                "version":1,"status":"ok","daemonVersion":"0.5.8",
                "acceptingJobs":false,"checkedAt":"2026-09-25T00:00:00Z",
                "supportedProtocolVersions":[1],
                "harness":{"harness":"codex","harnessVersion":"0.154.0-alpha.6.2","adapterVersion":"0.5.8"},
                "harnessExecutable":"/runtime/codex",
                "authentication":{"codexHome":"/private/codex-home","configured":true,"authenticated":true},
                "execution":{"maxActiveJobs":8,"activeJobs":0,"availableSlots":8},
                "quota":{"version":1,"policy":nucleus_core::QuotaPolicyV1::default(),
                    "state":quota_state,"accountKey":"account","limitId":"codex","remainingPercent":6,
                    "observedAt":100,"resetsAt":200,"conditionId":"condition","conditionStartedAt":100}
            }
        })
    }

    #[test]
    fn raw_service_health_preserves_expected_quota_pauses() -> Result<()> {
        for state in ["low", "exhausted", "unknown"] {
            let status = service_status_fixture(state);
            let health = health_from_service_status(&status)?;
            assert_eq!(health, status["health"]);
            assert_eq!(health["acceptingJobs"], false);
        }
        Ok(())
    }

    #[test]
    fn raw_service_health_rejects_unavailable_or_unexplained_readiness() {
        for (pointer, invalid) in [
            ("/loaded", json!(false)),
            ("/health", json!(null)),
            ("/health/status", json!("degraded")),
            ("/health/authentication/authenticated", json!(false)),
            ("/health/harness", json!(null)),
            ("/health/quota", json!(null)),
            ("/health/quota/state", json!("open")),
        ] {
            let mut status = service_status_fixture("low");
            let Some(field) = status.pointer_mut(pointer) else {
                panic!("missing fixture field {pointer}");
            };
            *field = invalid;
            assert!(
                health_from_service_status(&status).is_err(),
                "accepted {pointer}"
            );
        }
        let mut status = service_status_fixture("low");
        status["healthError"] = json!("health observation failed");
        assert!(health_from_service_status(&status).is_err());
    }

    fn stage_fixture(home: &Path) -> Result<PathBuf> {
        let source = home.join("source");
        fs::create_dir_all(&source)?;
        let executable = source.join("codex");
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\necho 'codex-cli {}'\n",
                nucleus_codex::SUPPORTED_CODEX_VERSION
            ),
        )?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
        let host = source.join("codex-code-mode-host");
        fs::write(&host, "#!/bin/sh\nexit 0\n")?;
        fs::set_permissions(&host, fs::Permissions::from_mode(0o755))?;
        let runtime =
            nucleus_codex::runtime_bundle::stage_runtime(&executable, &staged_runtime(home))
                .map_err(|error| Error::new(error.to_string()))?;
        fs::set_permissions(staged_runtime(home), fs::Permissions::from_mode(0o700))?;
        Ok(runtime.executable)
    }

    #[test]
    fn codex_upgrade_requires_a_complete_staged_runtime() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let home = fs::canonicalize(directory.path())?;
        let configured = home.join("configured-codex");
        fs::write(&configured, "prior executable")?;
        let mut health = json!({
            "harnessExecutable": configured,
            "harness": {"harnessVersion": nucleus_codex::SUPPORTED_CODEX_VERSION}
        });
        // Matching version metadata cannot admit an incomplete installation.
        assert!(candidate_harness(&home, &health).is_err());
        let staged = stage_fixture(&home)?;
        assert_eq!(candidate_harness(&home, &health)?, staged);
        health["harness"]["harnessVersion"] = json!("0.146.0");
        assert_eq!(candidate_harness(&home, &health)?, staged);
        health["harnessExecutable"] = json!(staged);
        health["harness"]["harnessVersion"] = json!(nucleus_codex::SUPPORTED_CODEX_VERSION);
        assert_eq!(candidate_harness(&home, &health)?, staged);
        fs::set_permissions(staged_runtime(&home), fs::Permissions::from_mode(0o700))?;
        fs::remove_file(staged_runtime(&home).join("codex-code-mode-host"))?;
        assert!(candidate_harness(&home, &health).is_err());
        Ok(())
    }

    #[test]
    fn runtime_identity_rejects_changed_capture() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let executable = stage_fixture(directory.path())?;
        let identity = runtime_identity(&executable)?;
        verify_runtime_identity(&executable, &identity)?;
        let mut different = identity;
        different["code_mode_host_sha256"] = json!("different");
        assert!(verify_runtime_identity(&executable, &different).is_err());
        Ok(())
    }

    #[test]
    fn same_version_retains_only_the_tested_file_pair() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let home = fs::canonicalize(directory.path())?;
        let staged = stage_fixture(&home)?;
        let source = home.join("source/codex");
        fs::write(
            home.join("source/codex-code-mode-host"),
            "#!/bin/sh\nexit 1\n",
        )?;
        let different =
            nucleus_codex::runtime_bundle::stage_runtime(&source, &home.join("different"))
                .map_err(|error| Error::new(error.to_string()))?;
        fs::set_permissions(home.join("different"), fs::Permissions::from_mode(0o700))?;
        let health = json!({
            "harnessExecutable": different.executable,
            "harness": {"harnessVersion": nucleus_codex::SUPPORTED_CODEX_VERSION}
        });
        assert_ne!(
            runtime_identity(&staged)?,
            runtime_identity(&different.executable)?
        );
        assert_eq!(candidate_harness(&home, &health)?, staged);
        Ok(())
    }

    #[test]
    fn recovery_checks_the_harness_for_the_selected_installation() -> Result<()> {
        let prior = json!({
            "harness_executable": "/candidate/codex",
            "prior_harness_executable": "/prior/codex"
        });
        assert_eq!(runtime_harness(&prior, true)?, "/prior/codex");
        assert_eq!(runtime_harness(&prior, false)?, "/candidate/codex");
        let historical = json!({"harness_executable": "/configured/codex"});
        assert_eq!(runtime_harness(&historical, true)?, "/configured/codex");
        assert_eq!(runtime_harness(&historical, false)?, "/configured/codex");
        Ok(())
    }

    #[test]
    fn upgrade_guard_checks_the_captured_harness_before_cutover() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let configured = directory.path().join("prior-codex");
        let candidate = directory.path().join("candidate-codex");
        fs::write(&configured, "prior executable")?;
        fs::write(&candidate, "candidate executable")?;
        let prior = json!({
            "harness_executable": candidate,
            "prior_harness_executable": configured
        });
        let health = json!({"harnessExecutable": configured});
        verify_prior_harness(&health, &candidate, Some(&prior))?;
        assert!(verify_prior_harness(&health, &candidate, None).is_err());
        let changed = json!({"harnessExecutable": candidate});
        assert!(verify_prior_harness(&changed, &candidate, Some(&prior)).is_err());
        verify_prior_harness(&changed, &candidate, None)?;
        Ok(())
    }
}
