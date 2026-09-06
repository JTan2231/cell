//! Weaver owns maintenance and retirement of its former prototype service.
//! Shared installation code owns immutable program files and selectors.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt as _};
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

const PRODUCT: &str = "weaver";
const LABEL: &str = "org.weaver.worker";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(
    name = "weaver-install",
    version,
    about = "Install and verify owned Weaver releases"
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
    bundle: PathBuf,
    #[command(flatten)]
    home: HomeArgs,
    #[arg(long)]
    expected_current: Option<String>,
    #[arg(long, default_value = "/bin/launchctl")]
    launchctl: PathBuf,
    #[arg(long, env = "WEAVER_UPDATE_WAIT_SECONDS", default_value_t = 21_600)]
    wait_seconds: u64,
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

fn state(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Weaver")
}

fn public(installer: bool) -> Vec<PublicEntry> {
    let mut result = vec![
        PublicEntry {
            path: ".local/bin/weaver".into(),
            artifact: "bin/weaver".into(),
            kind: PublicKind::Symlink,
            mode: 0o755,
        },
        PublicEntry {
            path: "Library/Application Support/Chancery/providers/weaver".into(),
            artifact: "share/chancery/weaver".into(),
            kind: PublicKind::Symlink,
            mode: 0o755,
        },
    ];
    if installer {
        result.push(PublicEntry {
            path: ".local/bin/weaver-install".into(),
            artifact: "bin/weaver-install".into(),
            kind: PublicKind::Symlink,
            mode: 0o755,
        });
    }
    result
}

fn layout() -> InstallLayout {
    InstallLayout {
        product: PRODUCT.into(),
        application: "Weaver".into(),
        product_lock: LockSpec {
            path: "Library/Application Support/Weaver/install/.update-lock".into(),
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
            format: "3",
            manifest: "manifest.txt",
            metadata: &["version"],
            proofs: &[
                LegacyProof {
                    key: "binary_sha256",
                    paths: &["bin/weaver"],
                },
                LegacyProof {
                    key: "deployer_sha256",
                    paths: &["package/deploy-user.sh"],
                },
            ],
            providers: &[LegacyProvider {
                key: "chancery_sha256",
                provider: "weaver",
                path: "share/chancery/weaver",
                version_key: "version",
            }],
            hash_path_lines: true,
        },
        public(false),
    )
}

fn environment(home: &Path, owner: Option<&str>) -> BTreeMap<OsString, OsString> {
    let mut env = BTreeMap::from([
        ("HOME".into(), home.as_os_str().to_owned()),
        ("WEAVER_STATE_DIR".into(), state(home).into_os_string()),
    ]);
    if let Some(owner) = owner {
        env.insert("CELL_DEPLOYMENT_RUN_ID".into(), owner.into());
    }
    env
}

fn call(
    exe: &Path,
    args: &[&str],
    env: &BTreeMap<OsString, OsString>,
    seconds: u64,
) -> Result<std::process::Output> {
    cell_install::command::checked(
        exe,
        &args.iter().map(OsString::from).collect::<Vec<_>>(),
        env,
        Duration::from_secs(seconds),
    )
}

fn json_call(
    exe: &Path,
    args: &[&str],
    env: &BTreeMap<OsString, OsString>,
    seconds: u64,
) -> Result<Value> {
    cell_install::command::json(
        exe,
        &args.iter().map(OsString::from).collect::<Vec<_>>(),
        env,
        Duration::from_secs(seconds),
    )
}

fn maintenance(
    exe: &Path,
    operation: &str,
    owner: Option<&str>,
    env: &BTreeMap<OsString, OsString>,
) -> Result<Value> {
    let mut args = vec!["maintenance", operation];
    if let Some(owner) = owner {
        args.push(owner);
    }
    let value = json_call(exe, &args, env, 600)?;
    if value.get("protocol_version") != Some(&json!(1))
        || !value["holds"].is_array()
        || !value["drained"].is_boolean()
    {
        return Err(Error::new("unsupported Weaver maintenance result"));
    }
    Ok(value)
}

fn sole(value: &Value, owner: &str) -> Result<()> {
    if value["holds"] != json!([owner]) || value["drained"] != true {
        return Err(Error::new("Weaver requires this run's sole drained hold"));
    }
    Ok(())
}

fn plan(binary: &Path, bundle: &Path, home: &Path) -> Result<ReleasePlan> {
    if !binary.is_absolute() || !bundle.is_absolute() {
        return Err(Error::new("candidate paths must be absolute"));
    }
    cell_install::file_digest(binary)?;
    let env = environment(home, None);
    let output = call(binary, &["--version"], &env, 30)?;
    if String::from_utf8_lossy(&output.stdout).trim() != format!("weaver {VERSION}") {
        return Err(Error::new("Weaver requires its version-matched installer"));
    }
    call(binary, &["--help"], &env, 30)?;
    let installer = std::env::current_exe()?;
    let mut files = BTreeMap::from([
        (
            "bin/weaver".into(),
            SourceFile {
                source: binary.to_owned(),
                mode: 0o755,
            },
        ),
        (
            "bin/weaver-install".into(),
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
        bundle,
        &cell_install::InstallSpec {
            product: "weaver",
            application: "Weaver",
            commands: &["weaver"],
            provider: "weaver",
        },
    )?
    .keys()
    {
        files.insert(
            format!("share/chancery/weaver/{relative}"),
            SourceFile {
                source: bundle.join(relative),
                mode: 0o644,
            },
        );
    }
    Ok(ReleasePlan {
        files,
        versions: BTreeMap::from([
            ("weaver".into(), VERSION.into()),
            ("weaver-install".into(), VERSION.into()),
        ]),
        providers: BTreeMap::from([(
            "weaver".into(),
            ProviderSpec {
                path: "share/chancery/weaver".into(),
                version: VERSION.into(),
            },
        )]),
    })
}

fn selection(snapshot: &InstallSnapshot) -> String {
    snapshot.current.as_ref().map_or_else(
        || "absent".into(),
        |release| format!("releases/{}", release.release_id),
    )
}

fn installed_cli(home: &Path) -> PathBuf {
    home.join(".local/bin/weaver")
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent captured service facts and completed effects govern exact compensation"
)]
struct Lifecycle {
    home: PathBuf,
    active: PathBuf,
    launchctl: PathBuf,
    env: BTreeMap<OsString, OsString>,
    owner: Option<String>,
    target: String,
    domain: String,
    plist: PathBuf,
    saved_plist: Option<Vec<u8>>,
    was_loaded: bool,
    was_disabled: bool,
    launchd_changed: bool,
    removed_plist: bool,
    own_maintenance: bool,
}

impl Lifecycle {
    fn new(home: &Path, active: PathBuf, launchctl: &Path, owner: Option<String>) -> Result<Self> {
        let uid = fs::metadata(home)?.uid();
        let domain = format!("gui/{uid}");
        Ok(Self {
            home: home.to_owned(),
            active,
            launchctl: launchctl.to_owned(),
            env: environment(home, owner.as_deref()),
            owner,
            target: format!("{domain}/{LABEL}"),
            domain,
            plist: home.join(format!("Library/LaunchAgents/{LABEL}.plist")),
            saved_plist: None,
            was_loaded: false,
            was_disabled: false,
            launchd_changed: false,
            removed_plist: false,
            own_maintenance: false,
        })
    }

    fn loaded(&self) -> Result<bool> {
        let output = cell_install::command::run(
            &self.launchctl,
            &["print".into(), self.target.clone().into()],
            &self.env,
            Duration::from_secs(30),
        )?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(113) => Ok(false),
            _ => Err(Error::new(
                "cannot prove Weaver prototype service load state",
            )),
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "Keep maintenance ownership, service capture, and quiescence ordered in one lifecycle transition"
    )]
    fn begin(&mut self, wait_seconds: u64) -> Result<()> {
        call(&self.active, &["doctor"], &self.env, 180)?;
        match fs::symlink_metadata(&self.plist) {
            Ok(meta) => {
                let parent = self
                    .plist
                    .parent()
                    .ok_or_else(|| Error::new("invalid prototype path"))?;
                if fs::canonicalize(parent)? != parent || !fs::symlink_metadata(parent)?.is_dir() {
                    return Err(Error::new("Weaver prototype directory is symbolic"));
                }
                if !meta.is_file()
                    || meta.nlink() != 1
                    || meta.uid() != fs::metadata(&self.home)?.uid()
                    || meta.mode() & 0o022 != 0
                {
                    return Err(Error::new(
                        "Weaver prototype plist is not an owned regular file",
                    ));
                }
                self.saved_plist = Some(fs::read(&self.plist)?);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        self.was_loaded = self.loaded()?;
        if self.was_loaded && self.saved_plist.is_none() {
            return Err(Error::new(
                "loaded Weaver prototype has no recoverable plist",
            ));
        }
        if let Some(owner) = &self.owner {
            sole(
                &maintenance(&self.active, "ready", Some(owner), &self.env)?,
                owner,
            )?;
        } else {
            let marker = state(&self.home).join(".maintenance");
            match fs::symlink_metadata(marker) {
                Ok(meta) if meta.is_file() && meta.nlink() == 1 => {}
                Ok(_) => return Err(Error::new("invalid Weaver maintenance marker")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.own_maintenance = true;
                }
                Err(error) => return Err(error.into()),
            }
            call(
                &self.active,
                &[
                    "maintenance",
                    "begin",
                    "--wait-seconds",
                    &wait_seconds.to_string(),
                ],
                &self.env,
                wait_seconds.saturating_add(30),
            )?;
        }
        if self.saved_plist.is_some() {
            let disabled = call(
                &self.launchctl,
                &["print-disabled", &self.domain],
                &self.env,
                30,
            )?;
            let text = String::from_utf8_lossy(&disabled.stdout);
            if !text.contains("disabled services = {") {
                return Err(Error::new("unrecognized launchd disabled state"));
            }
            let matches: Vec<_> = text
                .lines()
                .filter(|line| line.trim_start().starts_with(&format!("\"{LABEL}\"")))
                .collect();
            if matches.len() > 1
                || matches.first().is_some_and(|line| {
                    !matches!(
                        line.split("=>").nth(1).map(str::trim),
                        Some("true" | "false")
                    )
                })
            {
                return Err(Error::new(
                    "unrecognized Weaver prototype disabled override",
                ));
            }
            self.was_disabled = matches
                .first()
                .is_some_and(|line| line.split("=>").nth(1).map(str::trim) == Some("true"));
            if self.was_loaded && self.was_disabled {
                return Err(Error::new(
                    "loaded disabled Weaver prototype requires operator recovery",
                ));
            }
            self.launchd_changed = true;
            call(&self.launchctl, &["disable", &self.target], &self.env, 30)?;
            if self.was_loaded {
                call(
                    &self.launchctl,
                    &["bootout", "--wait", &self.target],
                    &self.env,
                    180,
                )?;
            }
            if fs::read(&self.plist)?
                != *self
                    .saved_plist
                    .as_ref()
                    .ok_or_else(|| Error::new("missing captured prototype"))?
            {
                return Err(Error::new("Weaver prototype plist changed before removal"));
            }
            fs::remove_file(&self.plist)?;
            self.removed_plist = true;
        }
        Ok(())
    }

    fn verify(&self) -> Result<()> {
        let cli = installed_cli(&self.home);
        call(&cli, &["--version"], &self.env, 30)?;
        call(&cli, &["doctor"], &self.env, 180)?;
        call(&cli, &["worker", "run"], &self.env, 180)?;
        if self.loaded()? {
            return Err(Error::new(
                "Weaver prototype remained loaded after retirement",
            ));
        }
        if self.launchd_changed {
            call(&self.launchctl, &["enable", &self.target], &self.env, 30)?;
        }
        Ok(())
    }

    fn end(&mut self, cli: &Path) -> Result<()> {
        if self.own_maintenance {
            call(cli, &["maintenance", "end"], &self.env, 180)?;
            self.own_maintenance = false;
        }
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        if self.removed_plist {
            let bytes = self
                .saved_plist
                .as_ref()
                .ok_or_else(|| Error::new("missing prototype recovery bytes"))?;
            if fs::symlink_metadata(&self.plist).is_ok() {
                return Err(Error::new("prototype plist path changed during recovery"));
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o644)
                .open(&self.plist)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        } else if let Some(bytes) = &self.saved_plist
            && (fs::symlink_metadata(&self.plist)?.file_type().is_symlink()
                || fs::read(&self.plist)? != *bytes)
        {
            return Err(Error::new(
                "Weaver prototype changed during recovery; maintenance retained",
            ));
        }
        if self.launchd_changed {
            call(
                &self.launchctl,
                &[
                    if self.was_disabled {
                        "disable"
                    } else {
                        "enable"
                    },
                    &self.target,
                ],
                &self.env,
                30,
            )?;
            if self.was_loaded && !self.loaded()? {
                let path = self
                    .plist
                    .to_str()
                    .ok_or_else(|| Error::new("invalid prototype path"))?;
                call(
                    &self.launchctl,
                    &["bootstrap", &self.domain, path],
                    &self.env,
                    180,
                )?;
            }
        }
        self.end(&self.active.clone())
    }
}

fn install(
    args: &InstallArgs,
    home: &Path,
    owner: Option<String>,
    exact: Option<&InstallSnapshot>,
) -> Result<InstallSnapshot> {
    let layout = layout();
    let before = cell_install::transaction::inspect_installation(&layout, home, &legacy)?;
    if args
        .expected_current
        .as_ref()
        .is_some_and(|expected| expected != &selection(&before))
        || exact.is_some_and(|expected| expected != &before)
    {
        return Err(Error::new("stale Weaver deployment plan"));
    }
    let prepared = cell_install::transaction::prepare_release(
        &layout,
        home,
        &plan(&args.binary, &args.bundle, home)?,
    )?;
    let mut transaction = cell_install::transaction::lock_installation(&layout, home, &legacy)?;
    transaction.recheck(&before)?;
    let active = before
        .current
        .as_ref()
        .map_or_else(|| args.binary.clone(), |_| installed_cli(home));
    let mut lifecycle = Lifecycle::new(home, active, &args.launchctl, owner)?;
    let result = lifecycle
        .begin(args.wait_seconds)
        .and_then(|()| transaction.publish(&prepared, &before, |_| lifecycle.verify()));
    let receipt = match result {
        Ok(receipt) => receipt,
        Err(mut error) => {
            if lifecycle.rollback().is_err() {
                error.disposition = Disposition::Uncertain;
                error.message = "Weaver cutover recovery is incomplete; retain maintenance and use product recovery".into();
            }
            return Err(error);
        }
    };
    lifecycle.end(&installed_cli(home)).map_err(|_| Error { message: "Weaver installation committed but maintenance could not end; use weaver maintenance end".into(), disposition: Disposition::Uncertain })?;
    Ok(receipt.after)
}

fn runtime(ctx: &Context, ready: bool) -> Result<Value> {
    let env = environment(&ctx.home, Some(&ctx.request.run_id));
    let status = maintenance(
        &installed_cli(&ctx.home),
        if ready { "ready" } else { "status" },
        ready.then_some(ctx.request.run_id.as_str()),
        &env,
    )?;
    if ready {
        sole(&status, &ctx.request.run_id)?;
    }
    Ok(status)
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep all operations beside their shared captured installation and maintenance evidence"
)]
fn adapter(operation: Operation) -> Result<Value> {
    let ctx = Context::read("weaver", "weaver", "weaver-install", VERSION)?;
    if std::fs::symlink_metadata(state(&ctx.home).join("install/.update-lock")).is_ok() {
        return Err(Error::new(
            "Weaver installation transaction is active or retained; product recovery is required",
        ));
    }
    let layout = layout();
    let snapshot = cell_install::transaction::inspect_installation(&layout, &ctx.home, &legacy)?;
    let env = environment(&ctx.home, Some(&ctx.request.run_id));
    let cli = installed_cli(&ctx.home);
    let prior_snapshot = || -> Result<InstallSnapshot> {
        Ok(serde_json::from_value(ctx.prior()?["installed"].clone())?)
    };
    let prove_program = || -> Result<()> {
        if !ctx.selected() {
            if snapshot != prior_snapshot()? {
                return Err(Error::new(
                    "affected Weaver installation changed since inspection",
                ));
            }
            return Ok(());
        }
        let expected = plan(
            &ctx.binary("weaver")?,
            &ctx.request.source_root.join("weaver/chancery"),
            &ctx.home,
        )?;
        let installed = snapshot
            .current
            .as_ref()
            .ok_or_else(|| Error::new("Weaver installation is absent"))?;
        if installed.files.len() != expected.files.len() {
            return Err(Error::new(
                "installed Weaver inventory differs from candidate",
            ));
        }
        for (path, file) in expected.files {
            let proof = installed
                .files
                .get(&path)
                .ok_or_else(|| Error::new("candidate Weaver artifact missing"))?;
            if proof.sha256 != cell_install::file_digest(&file.source)? || proof.mode != file.mode {
                return Err(Error::new("installed Weaver differs from exact candidate"));
            }
        }
        Ok(())
    };
    match operation {
        Operation::Inspect => {
            if snapshot.current.is_none() {
                return Err(Error::new(
                    "existing configured Weaver installation required",
                ));
            }
            let status = runtime(&ctx, false)?;
            if status["holds"] != json!([]) {
                return Err(Error::new("another operation holds Weaver"));
            }
            if ctx.selected() {
                plan(
                    &ctx.binary("weaver")?,
                    &ctx.request.source_root.join("weaver/chancery"),
                    &ctx.home,
                )?;
            }
            Ok(reply(
                "ready",
                "owned Weaver installation inspected",
                json!({"current":selection(&snapshot),"installed":snapshot,"runtime":status,"maintenance_products":["nucleus"],"after":["nucleus"]}),
            ))
        }
        Operation::Hold => {
            let status = maintenance(&cli, "hold", Some(&ctx.request.run_id), &env)?;
            if !status["holds"]
                .as_array()
                .is_some_and(|holds| holds.contains(&json!(ctx.request.run_id)))
            {
                return Err(Error::new("Weaver did not retain deployment hold"));
            }
            Ok(reply("held", "Weaver admission held", status))
        }
        Operation::Drain => {
            let deadline = Instant::now() + Duration::from_secs(600);
            loop {
                let status = maintenance(&cli, "drain", None, &env)?;
                if status["holds"] != json!([ctx.request.run_id]) {
                    return Err(Error::new("Weaver drain requires sole deployment hold"));
                }
                if status["drained"] == true {
                    return Ok(reply("drained", "admitted Weaver workflow settled", status));
                }
                if Instant::now() >= deadline {
                    return Err(Error::new("Weaver did not drain; hold retained"));
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        Operation::Apply => {
            if !ctx.selected() {
                return Err(Error::new("affected Weaver cannot be upgraded"));
            }
            runtime(&ctx, true)?;
            let args = InstallArgs {
                binary: ctx.binary("weaver")?,
                bundle: ctx.request.source_root.join("weaver/chancery"),
                home: HomeArgs {
                    home: Some(ctx.home.clone()),
                },
                expected_current: Some(selection(&prior_snapshot()?)),
                launchctl: "/bin/launchctl".into(),
                wait_seconds: 600,
            };
            let installed = install(
                &args,
                &ctx.home,
                Some(ctx.request.run_id.clone()),
                Some(&prior_snapshot()?),
            )?;
            Ok(reply(
                "applied",
                "Weaver installer completed",
                json!({"current":selection(&installed),"installed":installed}),
            ))
        }
        Operation::Verify => {
            prove_program()?;
            let status = runtime(&ctx, true)?;
            call(&cli, &["--version"], &env, 30)?;
            call(&cli, &["--help"], &env, 30)?;
            call(&cli, &["doctor"], &env, 180)?;
            Ok(reply(
                "verified",
                "Weaver program and maintained readiness verified",
                json!({"installed":snapshot,"runtime":status}),
            ))
        }
        Operation::Release => {
            let status = maintenance(&cli, "release", Some(&ctx.request.run_id), &env)?;
            if status["holds"]
                .as_array()
                .is_some_and(|holds| holds.contains(&json!(ctx.request.run_id)))
            {
                return Err(Error::new("Weaver did not release this deployment hold"));
            }
            Ok(reply(
                "released",
                "only this deployment hold released",
                status,
            ))
        }
        Operation::Recover => {
            let prior = snapshot == prior_snapshot()?;
            if !prior {
                prove_program()?;
            }
            let recovery = ctx.request.recovery.clone().unwrap_or_default();
            if !(prior && recovery["any_apply_started"] == false) {
                let status = runtime(&ctx, false)?;
                if status["holds"] == json!([ctx.request.run_id]) {
                    runtime(&ctx, true)?;
                } else if recovery["verified"] != true {
                    return Err(Error::new(
                        "Weaver recovery lacks owned hold or captured verification",
                    ));
                }
                call(&cli, &["doctor"], &env, 180)?;
            }
            Ok(reply(
                "recovered",
                "coherent Weaver program and maintained state verified",
                json!({"safe_to_release":true,"installed":if prior {"prior"}else{"candidate"}}),
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
                json!(install(
                    &args,
                    &home,
                    std::env::var("CELL_DEPLOYMENT_RUN_ID")
                        .ok()
                        .filter(|value| !value.is_empty()),
                    None
                )?)
            }
            Command::Inspect(args) => json!(cell_install::transaction::inspect_installation(
                &layout(),
                &home(args.home)?,
                &legacy
            )?),
            Command::Verify(args) => {
                let home = home(args.home.home.clone())?;
                let snapshot =
                    cell_install::transaction::inspect_installation(&layout(), &home, &legacy)?;
                let plan = plan(&args.binary, &args.bundle, &home)?;
                let release = snapshot
                    .current
                    .as_ref()
                    .ok_or_else(|| Error::new("Weaver is absent"))?;
                if release.files.len() != plan.files.len() {
                    return Err(Error::new("Weaver candidate inventory differs"));
                }
                for (path, file) in plan.files {
                    if release
                        .files
                        .get(&path)
                        .map(|entry| (&entry.sha256, entry.mode))
                        != Some((&cell_install::file_digest(&file.source)?, file.mode))
                    {
                        return Err(Error::new("Weaver candidate artifact differs"));
                    }
                }
                call(
                    &installed_cli(&home),
                    &["doctor"],
                    &environment(&home, None),
                    180,
                )?;
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
