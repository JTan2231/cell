//! Todo owns database migration and daily-email configuration. The zsh runner
//! remains a runtime credential boundary; installation never invokes email.

#[path = "todo-install/schedule.rs"]
mod clockwork_schedule;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode, Output};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cell_install::adapter::{Context, Operation, reply};
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::transaction::{
    InstallLayout, InstallSnapshot, LockKind, LockSpec, ProviderSpec, PublicEntry, PublicKind,
    ReleaseInfo, ReleasePlan, SourceFile,
};
use cell_install::{Disposition, Error, Result};
use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const LABEL: &str = "org.todo.daily-email";
#[allow(
    clippy::duration_suboptimal_units,
    reason = "Duration::from_mins is unavailable on the supported Rust 1.89 compiler"
)]
const MAINTENANCE_TIMEOUT: Duration = Duration::from_secs(180);
#[allow(
    clippy::duration_suboptimal_units,
    reason = "Duration::from_mins is unavailable on the supported Rust 1.89 compiler"
)]
const DRAIN_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Parser)]
#[command(
    name = "todo-install",
    version,
    about = "Install and verify Todo programs, database and daily-email schedule"
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
    /// Exact directory containing the runtime email runner and plist template.
    #[arg(long)]
    package: PathBuf,
    #[command(flatten)]
    home: HomeArgs,
    #[arg(long)]
    expected_current: Option<String>,
    #[arg(long, default_value = "/bin/launchctl")]
    launchctl: PathBuf,
    #[arg(long, requires = "email_from")]
    email_to: Option<String>,
    #[arg(long, requires = "email_to")]
    email_from: Option<String>,
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
    home.join("Library/Application Support/Todo")
}
fn install_root(home: &Path) -> PathBuf {
    state(home).join("install")
}
fn selection(snapshot: &InstallSnapshot) -> String {
    snapshot.current.as_ref().map_or_else(
        || "absent".into(),
        |info| format!("releases/{}", info.release_id),
    )
}
fn release_root(home: &Path, info: &ReleaseInfo) -> PathBuf {
    install_root(home).join("releases").join(&info.release_id)
}

fn public(installer: bool) -> Vec<PublicEntry> {
    let mut entries = vec![
        PublicEntry {
            path: ".local/bin/todo".into(),
            artifact: "bin/todo".into(),
            kind: PublicKind::Symlink,
            mode: 0o755,
        },
        PublicEntry {
            path: "Library/Application Support/Chancery/providers/todo".into(),
            artifact: "share/chancery/todo".into(),
            kind: PublicKind::Symlink,
            mode: 0o755,
        },
    ];
    if installer {
        entries.push(PublicEntry {
            path: ".local/bin/todo-install".into(),
            artifact: "bin/todo-install".into(),
            kind: PublicKind::Symlink,
            mode: 0o755,
        });
    }
    entries
}

fn layout() -> InstallLayout {
    InstallLayout {
        product: "todo".into(),
        application: "Todo".into(),
        public: public(true),
        product_lock: LockSpec {
            path: "Library/Application Support/Todo/install/.update-lock".into(),
            kind: LockKind::Directory,
        },
        catalog_lock: Some(LockSpec {
            path: "Library/Application Support/Chancery/.catalog-update-lock".into(),
            kind: LockKind::Shlock,
        }),
    }
}

fn legacy(root: &Path) -> Result<ReleaseInfo> {
    cell_install::legacy::verify(
        root,
        &LegacySpec {
            format: "1",
            manifest: "manifest.txt",
            metadata: &[],
            proofs: &[
                LegacyProof {
                    key: "binary_sha256",
                    paths: &["libexec/todo"],
                },
                LegacyProof {
                    key: "frontend_sha256",
                    paths: &["bin/todo", "package/todo"],
                },
                LegacyProof {
                    key: "deployer_sha256",
                    paths: &["package/deploy-user.sh"],
                },
                LegacyProof {
                    key: "plist_sha256",
                    paths: &["package/org.todo.daily-email.plist"],
                },
                LegacyProof {
                    key: "email_runner_sha256",
                    paths: &["bin/todo-daily-email", "package/todo-daily-email"],
                },
            ],
            providers: &[LegacyProvider {
                key: "chancery_bundle_sha256",
                provider: "todo",
                path: "share/chancery/todo",
                version_key: "",
            }],
            hash_path_lines: true,
        },
        public(false),
    )
}

fn env(home: &Path, owner: Option<&str>) -> BTreeMap<OsString, OsString> {
    let mut result = BTreeMap::from([
        ("TODO_CONFIG".into(), "".into()),
        ("TODO_DATABASE".into(), "".into()),
        ("HOME".into(), home.as_os_str().to_owned()),
        ("TODO_STATE_DIR".into(), state(home).into_os_string()),
    ]);
    if let Some(owner) = owner {
        result.insert("CELL_DEPLOYMENT_RUN_ID".into(), owner.into());
    }
    result
}
fn strings(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}
fn call(
    exe: &Path,
    args: &[OsString],
    home: &Path,
    owner: Option<&str>,
    seconds: u64,
) -> Result<Output> {
    cell_install::command::checked(exe, args, &env(home, owner), Duration::from_secs(seconds))
}

fn owned(path: &Path, home: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.is_file()
                || meta.nlink() != 1
                || meta.uid() != fs::metadata(home)?.uid()
                || meta.mode() & 0o022 != 0
            {
                return Err(Error::new(
                    "Todo installation file is not an owned regular file",
                ));
            }
            Ok(Some(fs::read(path)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn directory(path: &Path, home: &Path, mode: u32) -> Result<()> {
    if !path.starts_with(home) {
        return Err(Error::new("Todo installation directory escapes home"));
    }
    if !path.exists() {
        let parent = path
            .parent()
            .ok_or_else(|| Error::new("invalid directory"))?;
        if parent != home {
            directory(parent, home, 0o700)?;
        }
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.uid() != fs::metadata(home)?.uid() || meta.mode() & 0o022 != 0 {
        return Err(Error::new("unsafe Todo installation directory"));
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    use std::io::Write as _;
    let mut file = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| Error::new("invalid destination"))?,
    )?;
    file.write_all(bytes)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|_| Error::new("unable to promote Todo installation file"))?;
    Ok(())
}

fn restore_file(path: &Path, bytes: Option<&[u8]>, mode: u32) -> Result<()> {
    if let Some(bytes) = bytes {
        write_atomic(path, bytes, mode)
    } else {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

fn configuration(args: &InstallArgs, prior: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut config: toml::Value = if let Some(bytes) = prior {
        toml::from_str(
            std::str::from_utf8(bytes).map_err(|_| Error::new("Todo config is not UTF-8"))?,
        )
        .map_err(|_| Error::new("invalid Todo configuration"))?
    } else {
        toml::from_str("database = 'todo.db'\n[liaison]\nquality = 'high'\n")
            .map_err(|_| Error::new("invalid default config"))?
    };
    match (&args.email_to, &args.email_from) {
        (Some(to), Some(from)) => {
            if [to, from]
                .iter()
                .any(|value| value.trim().is_empty() || value.contains(['\r', '\n']))
            {
                return Err(Error::new("email addresses must be nonblank single lines"));
            }
            let email = toml::Value::Table(toml::map::Map::from_iter([
                ("to".into(), toml::Value::String(to.clone())),
                ("from".into(), toml::Value::String(from.clone())),
            ]));
            config
                .as_table_mut()
                .ok_or_else(|| Error::new("Todo config is not a table"))?
                .insert("email".into(), email);
        }
        (None, None) if prior.is_some() && config.get("email").is_some() => {
            for name in ["to", "from"] {
                if !config
                    .get("email")
                    .and_then(|email| email.get(name))
                    .and_then(toml::Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty() && !value.contains(['\r', '\n']))
                {
                    return Err(Error::new(
                        "Todo requires complete configured email addresses",
                    ));
                }
            }
            return prior
                .map(<[u8]>::to_vec)
                .ok_or_else(|| Error::new("missing config"));
        }
        _ => {
            return Err(Error::new(
                "fresh Todo installation requires both --email-to and --email-from",
            ));
        }
    }
    toml::to_string(&config)
        .map(String::into_bytes)
        .map_err(|_| Error::new("cannot render Todo configuration"))
}

fn database(home: &Path, config: &[u8]) -> Result<PathBuf> {
    let value: toml::Value =
        toml::from_str(std::str::from_utf8(config).map_err(|_| Error::new("invalid config"))?)
            .map_err(|_| Error::new("invalid Todo config"))?;
    let path = value
        .get("database")
        .and_then(toml::Value::as_str)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::new("Todo database is not configured"))?;
    let path = if path.is_absolute() {
        path
    } else {
        state(home).join(path)
    };
    cell_maintenance::canonical_database_path(&path)
        .map_err(|_| Error::new("Todo database identity is unsafe"))
}

fn maintenance(
    raw: &Path,
    database: &Path,
    home: &Path,
    owner: &str,
    operation: &str,
    named: bool,
) -> Result<Value> {
    if operation != "ready"
        && !database.exists()
        && !home.join(".local/bin/nucleus").exists()
        && !home
            .join("Library/Application Support/Nucleus/nucleus.db")
            .exists()
    {
        let database = cell_maintenance::canonical_database_path(database)
            .map_err(|error| Error::new(error.to_string()))?;
        let gate = cell_maintenance::Gate::new(
            database
                .parent()
                .ok_or_else(|| Error::new("invalid Todo database parent"))?
                .join("deployment-maintenance"),
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
    let mut args = strings(&["--database"]);
    args.push(database.as_os_str().to_owned());
    args.extend(strings(&["--json", "maintenance", operation]));
    if named {
        args.push(owner.into());
    }
    let value =
        cell_install::command::json(raw, &args, &env(home, Some(owner)), MAINTENANCE_TIMEOUT)?;
    Ok(cell_install::command::maintenance(&value)?.clone())
}

fn sole(status: &Value, owner: &str) -> Result<()> {
    if status["holds"] != json!([owner]) || status["drained"] != true {
        return Err(Error::new("Todo requires this run's sole drained hold"));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Schedule {
    loaded: bool,
    disabled: bool,
}

fn plist_value(path: &Path, home: &Path) -> Result<Value> {
    let out = call(
        Path::new("/usr/bin/plutil"),
        &[
            "-convert".into(),
            "json".into(),
            "-o".into(),
            "-".into(),
            path.as_os_str().to_owned(),
        ],
        home,
        None,
        30,
    )?;
    Ok(serde_json::from_slice(&out.stdout)?)
}

fn rendered_plist(template: &Path, home: &Path) -> Result<Value> {
    let mut value = plist_value(template, home)?;
    value["WorkingDirectory"] = json!(state(home));
    value["EnvironmentVariables"]["HOME"] = json!(home);
    value["ProgramArguments"][1] = json!(install_root(home).join("current/bin/todo-daily-email"));
    value["StandardOutPath"] = json!(home.join("Library/Logs/Todo/email.stdout.log"));
    value["StandardErrorPath"] = json!(home.join("Library/Logs/Todo/email.stderr.log"));
    if value["Label"] != LABEL
        || value["ProgramArguments"][0] != "/bin/zsh"
        || value["StartCalendarInterval"] != json!({"Hour":9,"Minute":0})
        || value.get("RunAtLoad").is_some()
    {
        return Err(Error::new("Todo email template is not its owned schedule"));
    }
    Ok(value)
}

fn legacy_schedule(home: &Path, launchctl: &Path, template: Option<&Path>) -> Result<Schedule> {
    let plist = home.join(format!("Library/LaunchAgents/{LABEL}.plist"));
    if let Some(template) = template {
        owned(&plist, home)?.ok_or_else(|| Error::new("Todo schedule plist is absent"))?;
        if plist_value(&plist, home)? != rendered_plist(template, home)? {
            return Err(Error::new(
                "Todo schedule differs from the complete owned definition",
            ));
        }
    }
    let domain = format!("gui/{}", fs::metadata(home)?.uid());
    let output = cell_install::command::run(
        launchctl,
        &strings(&["print", &format!("{domain}/{LABEL}")]),
        &env(home, None),
        Duration::from_secs(30),
    )?;
    let loaded = match output.status.code() {
        Some(0) => true,
        Some(113) => false,
        _ => return Err(Error::new("cannot prove Todo schedule load state")),
    };
    let out = call(
        launchctl,
        &strings(&["print-disabled", &domain]),
        home,
        None,
        30,
    )?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !text.contains("disabled services = {") {
        return Err(Error::new("unrecognized Todo disabled state"));
    }
    let lines: Vec<_> = text
        .lines()
        .filter(|line| line.trim_start().starts_with(&format!("\"{LABEL}\"")))
        .collect();
    if lines.len() > 1
        || lines.first().is_some_and(|line| {
            !matches!(
                line.split("=>").nth(1).map(str::trim),
                Some("true" | "false")
            )
        })
    {
        return Err(Error::new("unrecognized Todo disabled override"));
    }
    Ok(Schedule {
        loaded,
        disabled: lines
            .first()
            .is_some_and(|line| line.split("=>").nth(1).map(str::trim) == Some("true")),
    })
}

fn schedule(home: &Path, launchctl: &Path, template: Option<&Path>) -> Result<Schedule> {
    schedule_with(
        home,
        launchctl,
        template,
        &home.join(".local/bin/clockwork"),
    )
}

fn restore_legacy_activation(
    home: &Path,
    launchctl: &Path,
    template: &Path,
    captured_digest: &str,
    captured: &Schedule,
) -> Result<Schedule> {
    let plist = home.join(format!("Library/LaunchAgents/{LABEL}.plist"));
    owned(&plist, home)?.ok_or_else(|| Error::new("captured Todo legacy schedule is absent"))?;
    if cell_install::file_digest(&plist)? != captured_digest {
        return Err(Error::new(
            "Todo legacy schedule differs from captured recovery proof",
        ));
    }
    let observed = legacy_schedule(home, launchctl, Some(template))?;
    if observed.disabled != captured.disabled || (observed.loaded && !captured.loaded) {
        return Err(Error::new(
            "Todo legacy scheduling intent changed outside recovery",
        ));
    }
    if let Some(binding) = clockwork_schedule::binding(home)? {
        clockwork_schedule::verify(home, &binding, None)?;
        if binding.enabled {
            return Err(Error::new(
                "Todo legacy activation conflicts with Clockwork",
            ));
        }
    }
    if captured.loaded && !observed.loaded {
        call(
            launchctl,
            &[
                "bootstrap".into(),
                format!("gui/{}", fs::metadata(home)?.uid()).into(),
                plist.into_os_string(),
            ],
            home,
            None,
            180,
        )?;
    }
    let restored = legacy_schedule(home, launchctl, Some(template))?;
    if &restored != captured {
        return Err(Error::new(
            "Todo legacy activation did not restore captured intent",
        ));
    }
    Ok(restored)
}

fn schedule_with(
    home: &Path,
    launchctl: &Path,
    template: Option<&Path>,
    clockwork: &Path,
) -> Result<Schedule> {
    let plist = home.join(format!("Library/LaunchAgents/{LABEL}.plist"));
    let legacy_present = owned(&plist, home)?.is_some();
    let legacy = legacy_schedule(home, launchctl, template.filter(|_| legacy_present))?;
    if legacy.loaded && !legacy_present {
        return Err(Error::new(
            "Todo legacy service is loaded without its owned plist",
        ));
    }
    let selected = clockwork_schedule::binding_with(home, clockwork)?;
    if let Some(binding) = selected
        .as_ref()
        .filter(|binding| binding.definition_digest.is_some())
    {
        clockwork_schedule::verify(
            home,
            binding,
            if legacy_present {
                None
            } else {
                template.and_then(Path::parent).and_then(Path::parent)
            },
        )?;
        if binding.enabled && (legacy.loaded || legacy_present) {
            return Err(Error::new(
                "Todo has competing legacy and Clockwork schedules",
            ));
        }
        if !legacy_present {
            return Ok(Schedule {
                loaded: binding.enabled,
                disabled: legacy.disabled,
            });
        }
    }
    Ok(legacy)
}

fn plan(args: &InstallArgs, home: &Path) -> Result<ReleasePlan> {
    if [&args.binary, &args.bundle, &args.package]
        .iter()
        .any(|path| !path.is_absolute())
    {
        return Err(Error::new("Todo candidate paths must be absolute"));
    }
    cell_install::file_digest(&args.binary)?;
    let out = call(&args.binary, &strings(&["--version"]), home, None, 30)?;
    if String::from_utf8_lossy(&out.stdout).trim() != format!("todo {VERSION}") {
        return Err(Error::new("Todo requires its version-matched installer"));
    }
    call(&args.binary, &strings(&["--help"]), home, None, 30)?;
    let installer = std::env::current_exe()?;
    let mut files = BTreeMap::from([
        (
            "libexec/todo".into(),
            SourceFile {
                source: args.binary.clone(),
                mode: 0o755,
            },
        ),
        (
            "bin/todo".into(),
            SourceFile {
                source: installer.clone(),
                mode: 0o755,
            },
        ),
        (
            "bin/todo-install".into(),
            SourceFile {
                source: installer.clone(),
                mode: 0o755,
            },
        ),
        (
            "package/install".into(),
            SourceFile {
                source: installer.clone(),
                mode: 0o755,
            },
        ),
        (
            "bin/todo-daily-email".into(),
            SourceFile {
                source: installer,
                mode: 0o755,
            },
        ),
        (
            "package/todo-daily-email".into(),
            SourceFile {
                source: args.package.join("todo-daily-email"),
                mode: 0o755,
            },
        ),
        (
            "package/org.todo.daily-email.plist".into(),
            SourceFile {
                source: args.package.join("org.todo.daily-email.plist"),
                mode: 0o644,
            },
        ),
    ]);
    for relative in cell_install::provider_inventory(
        &args.bundle,
        &cell_install::InstallSpec {
            product: "todo",
            application: "Todo",
            commands: &["todo"],
            provider: "todo",
        },
    )?
    .keys()
    {
        files.insert(
            format!("share/chancery/todo/{relative}"),
            SourceFile {
                source: args.bundle.join(relative),
                mode: 0o644,
            },
        );
    }
    Ok(ReleasePlan {
        files,
        versions: ["todo", "todo-install"]
            .into_iter()
            .map(|name| (name.into(), VERSION.into()))
            .collect(),
        providers: BTreeMap::from([(
            "todo".into(),
            ProviderSpec {
                path: "share/chancery/todo".into(),
                version: VERSION.into(),
            },
        )]),
    })
}

fn verify_plan(info: &ReleaseInfo, plan: &ReleasePlan) -> Result<()> {
    if info.files.len() != plan.files.len() || info.versions != plan.versions {
        return Err(Error::new("Todo candidate inventory differs"));
    }
    for (path, file) in &plan.files {
        let actual = info
            .files
            .get(path)
            .ok_or_else(|| Error::new("Todo candidate artifact missing"))?;
        if actual.sha256 != cell_install::file_digest(&file.source)? || actual.mode != file.mode {
            return Err(Error::new(
                "Todo installed bytes differ from exact candidate",
            ));
        }
    }
    Ok(())
}

fn restore_database(backup: &Path, database: &Path) -> Result<()> {
    cell_install::file_digest(backup)?;
    let source =
        rusqlite::Connection::open_with_flags(backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| Error::new("cannot read Todo migration backup"))?;
    let mut destination = rusqlite::Connection::open_with_flags(
        database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .map_err(|_| Error::new("cannot open Todo database for recovery"))?;
    let backup = rusqlite::backup::Backup::new(&source, &mut destination)
        .map_err(|_| Error::new("cannot start Todo database recovery"))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match backup
            .step(128)
            .map_err(|_| Error::new("Todo database recovery failed"))?
        {
            rusqlite::backup::StepResult::Done => return Ok(()),
            _ if Instant::now() >= deadline => {
                return Err(Error::new(
                    "Todo database recovery could not acquire exclusive access",
                ));
            }
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn nucleus_ready(home: &Path, owner: Option<&str>) -> Result<()> {
    let cli = home.join(".local/bin/nucleus");
    if call(&cli, &strings(&["--compact", "health"]), home, owner, 180).is_ok() {
        return Ok(());
    }
    if let Some(owner) = owner {
        call(
            &cli,
            &strings(&["--compact", "maintenance", "health", owner]),
            home,
            Some(owner),
            180,
        )?;
        return Ok(());
    }
    Err(Error::new("Nucleus is not ready for Todo installation"))
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep database, selector, configuration, and schedule cutover beside their ordered compensation"
)]
fn install(
    args: &InstallArgs,
    home: &Path,
    requested_owner: Option<&str>,
    exact: Option<&InstallSnapshot>,
    exact_schedule: Option<&Schedule>,
) -> Result<InstallSnapshot> {
    let layout = layout();
    let before = cell_install::transaction::inspect_installation(&layout, home, &legacy)?;
    if args
        .expected_current
        .as_ref()
        .is_some_and(|value| value != &selection(&before))
        || exact.is_some_and(|value| value != &before)
    {
        return Err(Error::new("stale Todo deployment plan"));
    }
    nucleus_ready(home, requested_owner)?;
    let prepared = cell_install::transaction::prepare_release(&layout, home, &plan(args, home)?)?;
    let mut tx = cell_install::transaction::lock_installation(&layout, home, &legacy)?;
    tx.recheck(&before)?;
    let config_path = state(home).join("config.toml");
    let prior_config = owned(&config_path, home)?;
    let next_config = configuration(args, prior_config.as_deref())?;
    let database = database(home, &next_config)?;
    let database_existed = fs::symlink_metadata(&database).is_ok();
    if database_existed {
        let meta = fs::symlink_metadata(&database)?;
        if !meta.is_file()
            || meta.nlink() != 1
            || meta.uid() != fs::metadata(home)?.uid()
            || meta.mode() & 0o022 != 0
        {
            return Err(Error::new("Todo database is not an owned regular file"));
        }
    }
    let plist_path = home.join(format!("Library/LaunchAgents/{LABEL}.plist"));
    directory(
        plist_path
            .parent()
            .ok_or_else(|| Error::new("invalid plist path"))?,
        home,
        0o755,
    )?;
    directory(&home.join("Library/Logs/Todo"), home, 0o700)?;
    let prior_plist = owned(&plist_path, home)?;
    let prior_template = before
        .current
        .as_ref()
        .map(|info| release_root(home, info).join("package/org.todo.daily-email.plist"));
    let prior_schedule = schedule(home, &args.launchctl, prior_template.as_deref())?;
    if exact_schedule.is_some_and(|value| value != &prior_schedule) {
        return Err(Error::new(
            "Todo operator schedule changed since inspection",
        ));
    }
    let prior_clockwork = clockwork_schedule::binding(home)?;
    if let Some(binding) = &prior_clockwork {
        clockwork_schedule::verify(home, binding, None)?;
    }
    let legacy_loaded = legacy_schedule(home, &args.launchctl, None)?.loaded;
    if legacy_loaded && (prior_schedule.disabled || prior_plist.is_none()) {
        return Err(Error::new(
            "Todo loaded legacy schedule cannot be safely restored",
        ));
    }
    let transaction = tempfile::Builder::new()
        .prefix(".transaction.")
        .tempdir_in(install_root(home))?;
    fs::set_permissions(transaction.path(), fs::Permissions::from_mode(0o700))?;
    let staged_config = transaction.path().join("config.next.toml");
    write_atomic(&staged_config, &next_config, 0o600)?;
    if let Some(bytes) = &prior_config {
        write_atomic(&transaction.path().join("config.before.toml"), bytes, 0o600)?;
    }
    if let Some(bytes) = &prior_plist {
        write_atomic(
            &transaction.path().join("schedule.before.plist"),
            bytes,
            0o600,
        )?;
    }
    write_atomic(
        &transaction.path().join("clockwork.before.json"),
        &serde_json::to_vec(&prior_clockwork)?,
        0o600,
    )?;
    let candidate_raw = prepared.root.join("libexec/todo");
    let active = before.current.as_ref().map_or_else(
        || candidate_raw.clone(),
        |info| release_root(home, info).join("libexec/todo"),
    );
    let owner = requested_owner.map_or_else(
        || {
            format!(
                "todo-install-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |value| value.as_nanos())
            )
        },
        str::to_owned,
    );
    let own_hold = requested_owner.is_none();
    let gate = cell_maintenance::Gate::new(
        database
            .parent()
            .ok_or_else(|| Error::new("invalid database parent"))?
            .join("deployment-maintenance"),
    );
    let fresh = transaction.path().join("fresh.db");
    if !database_existed {
        // Initialize a private prospective database before the live gate is
        // held. No program selector exposes this file.
        call(
            &candidate_raw,
            &[
                "--database".into(),
                fresh.as_os_str().to_owned(),
                "init".into(),
            ],
            home,
            None,
            180,
        )?;
    }
    if own_hold {
        maintenance(&active, &database, home, &owner, "hold", true)?;
    }
    let deadline = Instant::now() + DRAIN_TIMEOUT;
    loop {
        let status = maintenance(&active, &database, home, &owner, "status", false)?;
        if status["holds"] != json!([owner]) {
            return Err(Error::new(
                "Todo installation requires its sole maintenance owner",
            ));
        }
        if status["drained"] == true {
            break;
        }
        if Instant::now() >= deadline {
            return Err(Error::new("Todo did not drain; installation hold retained"));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    let domain = format!("gui/{}", fs::metadata(home)?.uid());
    let target = format!("{domain}/{LABEL}");
    let suspended_paths: Vec<_> = layout
        .public
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    let journal = json!({"version":1,"home":home,"owner":owner,"before":before,"candidate":prepared.info,"database":database,"database_existed":database_existed,"prior_schedule":prior_schedule,"legacy_loaded":legacy_loaded,"prior_clockwork":prior_clockwork});
    write_atomic(
        &transaction.path().join("recovery.json"),
        &serde_json::to_vec(&journal)?,
        0o600,
    )?;
    let suspension = tx.suspend(&before, &suspended_paths)?;
    let mut service_changed = false;
    let mut state_changed = false;
    let mut publication = None;
    let backup = transaction.path().join("database.before.db");
    let result = (|| -> Result<InstallSnapshot> {
        if clockwork_schedule::binding(home)? != prior_clockwork {
            return Err(Error::new(
                "Todo Clockwork selection changed since inspection",
            ));
        }
        if prior_clockwork
            .as_ref()
            .is_some_and(|binding| binding.enabled)
        {
            service_changed = true;
            clockwork_schedule::disable(home, None)?;
        }
        if legacy_loaded {
            service_changed = true;
            call(
                &args.launchctl,
                &strings(&["bootout", &target]),
                home,
                Some(&owner),
                180,
            )?;
        }
        if owned(&config_path, home)? != prior_config || owned(&plist_path, home)? != prior_plist {
            return Err(Error::new("Todo configuration changed during installation"));
        }
        {
            let _guard = gate
                .enter_for(&owner)
                .map_err(|_| Error::new("Todo migration requires exclusive drained admission"))?;
            write_atomic(&transaction.path().join("state-changing"), b"1", 0o600)?;
            state_changed = true;
            write_atomic(&config_path, &next_config, 0o600)?;
            if !database_existed {
                let bytes = owned(&fresh, home)?
                    .ok_or_else(|| Error::new("fresh Todo database missing"))?;
                write_atomic(&database, &bytes, 0o600)?;
            }
        }
        if database_existed {
            call(
                &candidate_raw,
                &[
                    "--database".into(),
                    database.as_os_str().to_owned(),
                    "--config".into(),
                    config_path.as_os_str().to_owned(),
                    "migrate".into(),
                    "--backup".into(),
                    backup.as_os_str().to_owned(),
                ],
                home,
                Some(&owner),
                180,
            )?;
        }
        sole(
            &maintenance(&candidate_raw, &database, home, &owner, "ready", true)?,
            &owner,
        )?;
        // Retire only the fully attributed legacy projection while held. The
        // Clockwork candidate has no run-at-load trigger and is registered inert.
        if prior_plist.is_some() {
            fs::remove_file(&plist_path)?;
        }
        let registered = clockwork_schedule::register(home, &prepared.root, transaction.path())?;
        let selected = tx.publish(&prepared, &suspension.after, |_| {
            call(
                &home.join(".local/bin/todo"),
                &[
                    "--config".into(),
                    config_path.as_os_str().to_owned(),
                    "--json".into(),
                    "list".into(),
                    "--limit".into(),
                    "1".into(),
                ],
                home,
                Some(&owner),
                180,
            )?;
            Ok(())
        })?;
        let result = selected.after.clone();
        publication = Some(selected);
        let should_load = own_hold
            && (prior_schedule.loaded
                || (before.current.is_none()
                    && prior_plist.is_none()
                    && prior_clockwork.is_none()
                    && !prior_schedule.disabled));
        service_changed = true;
        clockwork_schedule::select(home, &registered.digest, should_load)?;
        if schedule(
            home,
            &args.launchctl,
            Some(&prepared.root.join("package/org.todo.daily-email.plist")),
        )? != (Schedule {
            loaded: should_load,
            disabled: prior_schedule.disabled,
        }) {
            return Err(Error::new("Todo schedule did not preserve operator state"));
        }
        write_atomic(&transaction.path().join("committed"), b"1", 0o600)?;
        Ok(result)
    })();
    match result {
        Ok(snapshot) => {
            if own_hold {
                maintenance(&candidate_raw, &database, home, &owner, "release", true).map_err(|_| Error { message: format!("Todo committed with maintenance retained; use todo maintenance release {owner}"), disposition: Disposition::Uncertain })?;
            }
            Ok(snapshot)
        }
        Err(mut error) => {
            let rollback = (|| -> Result<()> {
                if !own_hold
                    && prior_clockwork.is_none()
                    && clockwork_schedule::binding(home)?
                        .is_some_and(|binding| binding.definition_digest.is_some())
                {
                    return Err(Error::new(
                        "fresh Todo selection requires owned forward recovery",
                    ));
                }
                let observed_config = owned(&config_path, home)?;
                if observed_config != prior_config
                    && observed_config.as_deref() != Some(next_config.as_slice())
                {
                    return Err(Error::new(
                        "Todo configuration changed independently; recovery hold retained",
                    ));
                }
                let observed_plist = owned(&plist_path, home)?;
                if observed_plist.is_some() && observed_plist != prior_plist {
                    return Err(Error::new(
                        "Todo legacy schedule changed independently; hold retained",
                    ));
                }
                if service_changed {
                    if let Some(binding) = clockwork_schedule::binding(home)? {
                        clockwork_schedule::verify(home, &binding, None)?;
                        clockwork_schedule::disable(home, None)?;
                    }
                    if legacy_schedule(home, &args.launchctl, None)?.loaded {
                        call(
                            &args.launchctl,
                            &strings(&["bootout", &target]),
                            home,
                            Some(&owner),
                            180,
                        )?;
                    }
                }
                let unpublished = if let Some(receipt) = &publication {
                    let paused = tx.suspend(&receipt.after, &suspended_paths)?;
                    Some(cell_install::transaction::SelectionReceipt {
                        before: suspension.after.clone(),
                        after: paused.after,
                    })
                } else {
                    None
                };
                if state_changed {
                    let _guard = gate.enter_for(&owner).map_err(|_| {
                        Error::new("Todo recovery cannot prove exclusive database admission")
                    })?;
                    if backup.exists() {
                        restore_database(&backup, &database)?;
                    } else if !database_existed {
                        for suffix in ["", "-wal", "-shm"] {
                            let path = PathBuf::from(format!("{}{suffix}", database.display()));
                            restore_file(&path, None, 0o600)?;
                        }
                    }
                    restore_file(&config_path, prior_config.as_deref(), 0o600)?;
                    restore_file(&plist_path, prior_plist.as_deref(), 0o644)?;
                }
                if let Some(receipt) = &unpublished {
                    tx.restore(receipt, |_| Ok(()))?;
                }
                tx.restore(&suspension, |_| Ok(()))?;
                if let Some(binding) = &prior_clockwork
                    && let Some(digest) = &binding.definition_digest
                {
                    clockwork_schedule::select(home, digest, own_hold && binding.enabled)?;
                }
                if own_hold && legacy_loaded {
                    call(
                        &args.launchctl,
                        &[
                            "bootstrap".into(),
                            domain.into(),
                            plist_path.as_os_str().to_owned(),
                        ],
                        home,
                        Some(&owner),
                        180,
                    )?;
                }
                if schedule(home, &args.launchctl, prior_template.as_deref())?
                    != (Schedule {
                        loaded: own_hold && prior_schedule.loaded,
                        disabled: prior_schedule.disabled,
                    })
                {
                    return Err(Error::new("Todo schedule recovery is incomplete"));
                }
                if own_hold {
                    maintenance(&active, &database, home, &owner, "release", true)?;
                }
                Ok(())
            })();
            if rollback.is_err() {
                let retained = transaction.keep();
                error.message = format!(
                    "Todo recovery is incomplete; maintenance retained and private recovery artifacts retained at {}",
                    retained.display()
                );
                error.disposition = Disposition::Uncertain;
            }
            Err(error)
        }
    }
}

fn adapter_args(ctx: &Context, expected: Option<String>) -> Result<InstallArgs> {
    let settings = ctx.request.settings.as_ref().unwrap_or(&Value::Null);
    Ok(InstallArgs {
        binary: ctx.binary("todo")?,
        bundle: ctx.request.source_root.join("todo/chancery"),
        package: ctx.request.source_root.join("todo/packaging/macos"),
        home: HomeArgs {
            home: Some(ctx.home.clone()),
        },
        expected_current: expected,
        launchctl: "/bin/launchctl".into(),
        email_to: settings["email_to"].as_str().map(str::to_owned),
        email_from: settings["email_from"].as_str().map(str::to_owned),
    })
}

fn installed_state(ctx: &Context) -> Result<(InstallSnapshot, PathBuf, PathBuf, Schedule)> {
    if fs::symlink_metadata(install_root(&ctx.home).join(".update-lock")).is_ok() {
        return Err(Error::new(
            "Todo installation transaction is active or retained",
        ));
    }
    if install_root(&ctx.home).exists() {
        for entry in fs::read_dir(install_root(&ctx.home))? {
            if entry?
                .file_name()
                .to_string_lossy()
                .starts_with(".transaction.")
            {
                return Err(Error::new(
                    "Todo has retained installation recovery artifacts",
                ));
            }
        }
    }
    let snapshot = cell_install::transaction::inspect_installation(&layout(), &ctx.home, &legacy)?;
    let root = snapshot
        .current
        .as_ref()
        .map(|info| release_root(&ctx.home, info));
    let config = owned(&state(&ctx.home).join("config.toml"), &ctx.home)?;
    let next = configuration(&adapter_args(ctx, None)?, config.as_deref())?;
    let database = database(&ctx.home, &next)?;
    let template = root
        .as_ref()
        .map(|root| root.join("package/org.todo.daily-email.plist"));
    let schedule = schedule_with(
        &ctx.home,
        Path::new("/bin/launchctl"),
        template.as_deref(),
        &ctx.dependency_binary("clockwork")?,
    )?;
    let raw = root.map_or_else(|| ctx.binary("todo"), |root| Ok(root.join("libexec/todo")))?;
    Ok((snapshot, raw, database, schedule))
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep all operations beside their shared database, installation, and schedule recovery evidence"
)]
fn adapter(operation: Operation) -> Result<Value> {
    let ctx = Context::read("todo", "todo", "todo-install", VERSION)?;
    ctx.validate_settings(&["email_from", "email_to"], &["enabled"])?;
    let recovered_candidate = if matches!(operation, Operation::Recover) {
        recover_transactions(&ctx)?
    } else {
        None
    };
    let (snapshot, raw, database, schedule_state) = installed_state(&ctx)?;
    let prior = || -> Result<InstallSnapshot> {
        Ok(serde_json::from_value(ctx.prior()?["installed"].clone())?)
    };
    let prior_schedule =
        || -> Result<Schedule> { Ok(serde_json::from_value(ctx.prior()?["schedule"].clone())?) };
    let check_schedule = || -> Result<()> {
        if schedule_state.disabled != prior_schedule()?.disabled
            || (schedule_state.loaded && !prior_schedule()?.loaded)
        {
            return Err(Error::new(
                "Todo schedule differs from captured operator state; hold retained",
            ));
        }
        Ok(())
    };
    let args = || adapter_args(&ctx, Some(selection(&prior()?)));
    let prove = || -> Result<()> {
        if ctx.selected() {
            verify_plan(
                snapshot
                    .current
                    .as_ref()
                    .ok_or_else(|| Error::new("Todo is absent"))?,
                &plan(&args()?, &ctx.home)?,
            )
        } else if snapshot == prior()? {
            Ok(())
        } else {
            Err(Error::new("affected Todo installation changed"))
        }
    };
    let status = |operation, named| {
        maintenance(
            &raw,
            &database,
            &ctx.home,
            &ctx.request.run_id,
            operation,
            named,
        )
    };
    match operation {
        Operation::Apply => {
            check_schedule()?;
            let prepared = cell_install::transaction::prepare_release(
                &layout(),
                &ctx.home,
                &plan(&args()?, &ctx.home)?,
            )?;
            Ok(reply(
                "applied",
                "Todo immutable release staged; configuration and publication pending",
                json!({"staged":prepared.info}),
            ))
        }
        Operation::Activate => {
            if snapshot.current.is_none() {
                return Ok(reply("activated", "Todo remains absent", json!({})));
            }
            if status("status", false)?["holds"] != json!([]) {
                return Err(Error::new("Todo activation requires released admission"));
            }
            if snapshot == prior()?
                && let Some(digest) = ctx.prior()?["legacy_plist_digest"].as_str()
                && owned(
                    &ctx.home.join(format!("Library/LaunchAgents/{LABEL}.plist")),
                    &ctx.home,
                )?
                .is_some()
            {
                let info = snapshot
                    .current
                    .as_ref()
                    .ok_or_else(|| Error::new("Todo prior release is absent"))?;
                let restored = restore_legacy_activation(
                    &ctx.home,
                    Path::new("/bin/launchctl"),
                    &release_root(&ctx.home, info).join("package/org.todo.daily-email.plist"),
                    digest,
                    &prior_schedule()?,
                )?;
                return Ok(reply(
                    "activated",
                    "Todo captured legacy scheduling intent restored",
                    json!({"schedule":restored}),
                ));
            }
            let binding = clockwork_schedule::binding(&ctx.home)?
                .ok_or_else(|| Error::new("Todo schedule is absent"))?;
            clockwork_schedule::verify(&ctx.home, &binding, None)?;
            let fresh = prior()?.current.is_none();
            let enabled = ctx.activation_enabled(
                prior_schedule()?.loaded
                    || (fresh
                        && ctx.prior()?["clockwork_present"] == false
                        && !prior_schedule()?.disabled),
            )?;
            let digest = binding
                .definition_digest
                .as_deref()
                .ok_or_else(|| Error::new("Todo definition is absent"))?;
            let selected = clockwork_schedule::select(&ctx.home, digest, enabled)?;
            Ok(reply(
                "activated",
                "Todo captured scheduling intent restored",
                json!({"binding":selected}),
            ))
        }
        Operation::Inspect => {
            if ctx.selected() {
                plan(
                    &InstallArgs {
                        binary: ctx.binary("todo")?,
                        bundle: ctx.request.source_root.join("todo/chancery"),
                        package: ctx.request.source_root.join("todo/packaging/macos"),
                        home: HomeArgs {
                            home: Some(ctx.home.clone()),
                        },
                        expected_current: None,
                        launchctl: "/bin/launchctl".into(),
                        email_to: None,
                        email_from: None,
                    },
                    &ctx.home,
                )?;
            }
            let current = status("status", false)?;
            if current["holds"] != json!([]) {
                return Err(Error::new("another operation holds Todo"));
            }
            Ok(reply(
                "ready",
                "owned Todo installation and schedule inspected",
                json!({"current":selection(&snapshot),"installed":snapshot,"schedule":schedule_state,"clockwork_present":clockwork_schedule::binding_with(&ctx.home,&ctx.dependency_binary("clockwork")?)?.is_some(),"legacy_plist_digest":owned(&ctx.home.join(format!("Library/LaunchAgents/{LABEL}.plist")),&ctx.home)?.map(|_| cell_install::file_digest(&ctx.home.join(format!("Library/LaunchAgents/{LABEL}.plist")))).transpose()?,"runtime":current,"maintenance_products":[],"after":["nucleus","clockwork"]}),
            ))
        }
        Operation::Hold => {
            let current = status("hold", true)?;
            if !current["holds"]
                .as_array()
                .is_some_and(|holds| holds.contains(&json!(ctx.request.run_id)))
            {
                return Err(Error::new("Todo did not retain deployment hold"));
            }
            if let Some(binding) =
                clockwork_schedule::binding_with(&ctx.home, &ctx.dependency_binary("clockwork")?)?
            {
                clockwork_schedule::verify(&ctx.home, &binding, None)?;
                if binding.enabled {
                    clockwork_schedule::disable(&ctx.home, None)?;
                }
            }
            Ok(reply("held", "Todo admission held", current))
        }
        Operation::Drain => {
            let current = status("status", false)?;
            if current["holds"] != json!([ctx.request.run_id]) {
                return Err(Error::new("drain requires the sole deployment owner"));
            }
            Ok(reply(
                if current["drained"] == true {
                    "drained"
                } else {
                    "waiting"
                },
                "Todo admitted work",
                current,
            ))
        }
        Operation::Configure => {
            if !ctx.selected() {
                prove()?;
                sole(&status("ready", true)?, &ctx.request.run_id)?;
                return Ok(reply(
                    "configured",
                    "Todo retained configuration verified",
                    json!({}),
                ));
            }
            if !ctx.selected() {
                return Err(Error::new("affected Todo cannot be upgraded"));
            }
            check_schedule()?;
            sole(&status("status", false)?, &ctx.request.run_id)?;
            let installed = install(
                &args()?,
                &ctx.home,
                Some(&ctx.request.run_id),
                Some(&prior()?),
                Some(&schedule_state),
            )?;
            Ok(reply(
                "configured",
                "Todo database and email configured and published with scheduling disabled",
                json!({"current":selection(&installed),"installed":installed}),
            ))
        }
        Operation::Verify => {
            prove()?;
            check_schedule()?;
            if schedule_state.loaded {
                return Err(Error::new(
                    "Todo schedule activated before final activation",
                ));
            }
            sole(&status("ready", true)?, &ctx.request.run_id)?;
            call(
                &raw,
                &strings(&["--version"]),
                &ctx.home,
                Some(&ctx.request.run_id),
                30,
            )?;
            call(
                &raw,
                &strings(&["--help"]),
                &ctx.home,
                Some(&ctx.request.run_id),
                30,
            )?;
            Ok(reply(
                "verified",
                "Todo program, schema and operator schedule verified",
                json!({"installed":snapshot,"schedule":schedule_state}),
            ))
        }
        Operation::Release => {
            check_schedule()?;
            let current = status("release", true)?;
            if current["holds"]
                .as_array()
                .is_some_and(|holds| holds.contains(&json!(ctx.request.run_id)))
            {
                return Err(Error::new("Todo did not release deployment hold"));
            }
            Ok(reply(
                "released",
                "only this Todo deployment hold released",
                current,
            ))
        }
        Operation::Recover => {
            check_schedule()?;
            let unchanged = snapshot == prior()?;
            if !unchanged {
                prove()?;
            }
            let recovery = ctx.request.recovery.clone().unwrap_or_default();
            let candidate = recovered_candidate.unwrap_or(
                !unchanged
                    || recovery["configured"] == true
                    || recovery["installed"] == "candidate",
            );
            if !(unchanged && (recovery["configure_started"] != true || snapshot.current.is_none()))
            {
                let current = status("status", false)?;
                if current["holds"] == json!([ctx.request.run_id]) {
                    sole(&status("ready", true)?, &ctx.request.run_id)?;
                } else if recovery["verified"] != true {
                    return Err(Error::new(
                        "Todo recovery lacks owned hold or captured verification",
                    ));
                }
            }
            Ok(reply(
                "recovered",
                "coherent Todo program, schema and schedule verified",
                json!({"safe_to_release":true,"installed":if candidate {"candidate"}else{"prior"}}),
            ))
        }
    }
}

fn frontend() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let home = home(None)?;
    let state = std::env::var_os("TODO_STATE_DIR").map_or_else(|| state(&home), PathBuf::from);
    let selected = ["TODO_CONFIG", "TODO_DATABASE"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
        || args.iter().any(|value| {
            value == "--config"
                || value == "--database"
                || value.to_string_lossy().starts_with("--config=")
                || value.to_string_lossy().starts_with("--database=")
        });
    let mut command = Process::new(state.join("install/current/libexec/todo"));
    command.args(args);
    if !selected {
        command.env("TODO_CONFIG", state.join("config.toml"));
    }
    Err(command.exec().into())
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
                json!(install(&args, &home, owner.as_deref(), None, None)?)
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
                let info = snapshot
                    .current
                    .as_ref()
                    .ok_or_else(|| Error::new("Todo is absent"))?;
                verify_plan(info, &plan(&args, &home)?)?;
                schedule(
                    &home,
                    &args.launchctl,
                    Some(&release_root(&home, info).join("package/org.todo.daily-email.plist")),
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
    if std::env::args_os()
        .next()
        .as_deref()
        .and_then(|path| Path::new(path).file_name())
        == Some(OsStr::new("todo-daily-email"))
    {
        if clockwork_schedule::frontend().is_err() {
            eprintln!("todo: scheduled payload is unavailable");
        }
        return ExitCode::FAILURE;
    }
    if std::env::args_os()
        .next()
        .as_deref()
        .and_then(|path| Path::new(path).file_name())
        == Some(OsStr::new("todo"))
    {
        return if frontend().is_ok() {
            ExitCode::SUCCESS
        } else {
            eprintln!("todo: installed payload is unavailable");
            ExitCode::FAILURE
        };
    }
    cell_install::adapter::finish(run(Cli::parse().command))
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep each transaction's identity proof and ordered restoration together"
)]
fn recover_transactions(ctx: &Context) -> Result<Option<bool>> {
    {
        let layout = layout();
        let _lock = cell_install::transaction::lock_installation(&layout, &ctx.home, &legacy)?;
    }
    let root = install_root(&ctx.home);
    if !root.exists() {
        return Ok(None);
    }
    let mut candidate = None;
    let entries = fs::read_dir(&root)?.collect::<std::io::Result<Vec<_>>>()?;
    for entry in entries {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(".transaction.")
        {
            continue;
        }
        let path = entry.path();
        directory(&path, &ctx.home, 0o700)?;
        let bytes = owned(&path.join("recovery.json"), &ctx.home)?
            .ok_or_else(|| Error::new("Todo transaction lacks recovery evidence"))?;
        let saved: Value = serde_json::from_slice(&bytes)?;
        if saved["version"] != 1
            || saved["owner"] != ctx.request.run_id
            || saved["home"] != json!(ctx.home)
            || saved["before"] != ctx.prior()?["installed"]
        {
            return Err(Error::new(
                "Todo transaction does not match the recorded deployment",
            ));
        }
        let before: InstallSnapshot = serde_json::from_value(saved["before"].clone())?;
        let info: ReleaseInfo = serde_json::from_value(saved["candidate"].clone())?;
        verify_plan(&info, &plan(&adapter_args(ctx, None)?, &ctx.home)?)?;
        let prepared = cell_install::transaction::PreparedRelease {
            root: release_root(&ctx.home, &info),
            info,
        };
        let next = owned(&path.join("config.next.toml"), &ctx.home)?
            .ok_or_else(|| Error::new("Todo candidate configuration is missing"))?;
        let database = database(&ctx.home, &next)?;
        if json!(database) != saved["database"] {
            return Err(Error::new(
                "Todo recovery database differs from saved configuration",
            ));
        }
        let prior_config = owned(&path.join("config.before.toml"), &ctx.home)?;
        let prior_plist = owned(&path.join("schedule.before.plist"), &ctx.home)?;
        let config = state(&ctx.home).join("config.toml");
        let actual_config = owned(&config, &ctx.home)?;
        if actual_config != prior_config && actual_config.as_deref() != Some(next.as_slice()) {
            return Err(Error::new("Todo configuration changed outside recovery"));
        }
        let layout = layout();
        let mut tx = cell_install::transaction::lock_installation(&layout, &ctx.home, &legacy)?;
        let actual =
            cell_install::transaction::inspect_detached_installation(&layout, &ctx.home, &legacy)?;
        if actual.current != before.current && actual.current.as_ref() != Some(&prepared.info) {
            return Err(Error::new("Todo recovery found a foreign generation"));
        }
        let binding = clockwork_schedule::binding(&ctx.home)?;
        if let Some(binding) = &binding {
            clockwork_schedule::verify(&ctx.home, binding, None)?;
            let prior_binding: Option<clockwork::api::BindingRecord> =
                serde_json::from_value(saved["prior_clockwork"].clone())?;
            let candidate = clockwork_schedule::register(&ctx.home, &prepared.root, &path)?;
            if binding.definition_digest != prior_binding.and_then(|prior| prior.definition_digest)
                && binding.definition_digest.as_deref() != Some(&candidate.digest)
            {
                return Err(Error::new("Todo recovery found a foreign binding"));
            }
            clockwork_schedule::disable(&ctx.home, None)?;
        }
        let raw = prepared.root.join("libexec/todo");
        sole(
            &maintenance(
                &raw,
                &database,
                &ctx.home,
                &ctx.request.run_id,
                "status",
                false,
            )?,
            &ctx.request.run_id,
        )?;
        if owned(&path.join("committed"), &ctx.home)?.is_some()
            || (before.current.is_none()
                && actual.current.as_ref() == Some(&prepared.info)
                && actual_config.as_deref() == Some(next.as_slice()))
        {
            sole(
                &maintenance(
                    &raw,
                    &database,
                    &ctx.home,
                    &ctx.request.run_id,
                    "ready",
                    true,
                )?,
                &ctx.request.run_id,
            )?;
            tx.recover(&before, &prepared, true, |_| Ok(()))?;
            candidate = Some(candidate.unwrap_or(true));
        } else {
            let public: Vec<_> = layout
                .public
                .iter()
                .map(|entry| entry.path.clone())
                .collect();
            tx.suspend(&actual, &public)?;
            let gate = cell_maintenance::Gate::new(
                database
                    .parent()
                    .ok_or_else(|| Error::new("invalid Todo database path"))?
                    .join("deployment-maintenance"),
            );
            let _guard = gate
                .enter_for(&ctx.request.run_id)
                .map_err(|error| Error::new(error.to_string()))?;
            if owned(&path.join("state-changing"), &ctx.home)?.is_some() {
                let backup = path.join("database.before.db");
                if owned(&backup, &ctx.home)?.is_some() {
                    restore_database(&backup, &database)?;
                } else if saved["database_existed"] == false {
                    for suffix in ["", "-wal", "-shm", "-journal"] {
                        restore_file(
                            &PathBuf::from(format!("{}{suffix}", database.display())),
                            None,
                            0o600,
                        )?;
                    }
                }
                restore_file(&config, prior_config.as_deref(), 0o600)?;
            }
            let plist = ctx.home.join(format!("Library/LaunchAgents/{LABEL}.plist"));
            let actual_plist = owned(&plist, &ctx.home)?;
            if actual_plist.is_some() && actual_plist != prior_plist {
                return Err(Error::new(
                    "Todo recovery found foreign legacy configuration",
                ));
            }
            restore_file(&plist, prior_plist.as_deref(), 0o644)?;
            tx.recover(&before, &prepared, false, |_| Ok(()))?;
            let binding: Option<clockwork::api::BindingRecord> =
                serde_json::from_value(saved["prior_clockwork"].clone())?;
            if let Some(digest) = binding.and_then(|binding| binding.definition_digest) {
                clockwork_schedule::select(&ctx.home, &digest, false)?;
            }
            candidate = Some(false);
        }
        let retained = state(&ctx.home).join("backups/deployments");
        directory(&retained, &ctx.home, 0o700)?;
        fs::rename(&path, retained.join(entry.file_name()))?;
    }
    Ok(candidate)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn recovered_legacy_schedule_restores_only_captured_loaded_intent() -> Result<()> {
        let root = tempfile::tempdir()?;
        let home = root.path();
        directory(&home.join(".local/bin"), home, 0o755)?;
        directory(&home.join("Library/LaunchAgents"), home, 0o755)?;
        let launchctl = home.join("launchctl");
        write_atomic(
            &launchctl,
            br#"#!/bin/sh
case "$1" in
 print) [ -f "$HOME/loaded" ] && exit 0; exit 113;;
 print-disabled) echo 'disabled services = {'; echo '}';;
 bootstrap) : >"$HOME/loaded";;
 *) exit 64;;
esac
"#,
            0o755,
        )?;
        write_atomic(
            &home.join(".local/bin/clockwork"),
            b"#!/bin/sh\necho '{\"ok\":true,\"data\":{\"output_version\":2,\"items\":[],\"has_more\":false}}'\n",
            0o755,
        )?;
        let template = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/macos/org.todo.daily-email.plist");
        let plist = home.join(format!("Library/LaunchAgents/{LABEL}.plist"));
        write_atomic(
            &plist,
            &serde_json::to_vec(&rendered_plist(&template, home)?)?,
            0o644,
        )?;
        let digest = cell_install::file_digest(&plist)?;
        let stopped = Schedule {
            loaded: false,
            disabled: false,
        };
        assert_eq!(
            restore_legacy_activation(home, &launchctl, &template, &digest, &stopped)?,
            stopped
        );
        assert!(!home.join("loaded").exists());
        let loaded = Schedule {
            loaded: true,
            disabled: false,
        };
        assert_eq!(
            restore_legacy_activation(home, &launchctl, &template, &digest, &loaded)?,
            loaded
        );
        assert!(home.join("loaded").exists());
        assert_eq!(
            restore_legacy_activation(home, &launchctl, &template, &digest, &loaded)?,
            loaded
        );
        fs::remove_file(home.join("loaded"))?;
        write_atomic(&plist, b"foreign", 0o644)?;
        assert!(restore_legacy_activation(home, &launchctl, &template, &digest, &loaded).is_err());
        assert!(!home.join("loaded").exists());
        Ok(())
    }
}
