use super::{
    DecisionsArgs, Duration, Error, InstallArgs, MINUTE, Path, PathBuf, Result, Value, agent,
    annals, call, directory, environment, expected, fs, home, install_root, json, optional_private,
    private_directory, private_file, release, schedule, state, toml_bytes, toml_value,
    write_private,
};
use cell_install::{InstallSnapshot, PreparedRelease, ReleaseInfo, SelectionReceipt};
use serde::{Deserialize, Serialize};
use std::time::Instant;

// Independent durable facts must round-trip even after a partially completed operation.
#[allow(clippy::struct_excessive_bools)]
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    home: PathBuf,
    owner: String,
    outer_hold: bool,
    key: String,
    prior: InstallSnapshot,
    candidate: ReleaseInfo,
    prior_control: schedule::Control,
    candidate_digest: Option<String>,
    config_before: Option<Vec<u8>>,
    usage_before: Option<Vec<u8>>,
    config_after: Vec<u8>,
    usage_after: Option<Vec<u8>>,
    database_existed: bool,
    backup_ready: bool,
    backup_sha256: Option<String>,
    mutation_started: bool,
    fresh_state: bool,
    marker_created: bool,
    marker_owned: bool,
    hold_before: Option<Vec<u8>>,
    hold_after: Option<Vec<u8>>,
    publication: Option<SelectionReceipt>,
    suspension: Option<SelectionReceipt>,
    committed: bool,
    no_start: bool,
    keep_maintenance: bool,
    clockwork: PathBuf,
    launchctl: PathBuf,
    agent: Option<agent::Agent>,
}

impl Journal {
    fn library(&self) -> PathBuf {
        if self.key == "annals/decisions-inbox" {
            state(&self.home).join("decisions")
        } else {
            state(&self.home)
        }
    }
    fn save(&self, path: &Path) -> Result<()> {
        write_private(&path.join("journal.json"), &serde_json::to_vec(self)?, true)
    }
    fn payload(&self) -> PathBuf {
        release::root(&self.home, &self.candidate).join("libexec/annals")
    }
}

fn owner() -> (String, bool) {
    std::env::var("CELL_DEPLOYMENT_RUN_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .map_or_else(
            || (format!("annals-install-{}", std::process::id()), false),
            |owner| (owner, true),
        )
}

fn capture(path: &Path) -> Result<Option<Vec<u8>>> {
    if optional_private(path)? {
        Ok(Some(fs::read(path)?))
    } else {
        Ok(None)
    }
}

fn config(
    home: &Path,
    library: &Path,
    socket: &Path,
    before: Option<&[u8]>,
    decisions_id: Option<&str>,
) -> Result<Vec<u8>> {
    let mut value = if let Some(bytes) = before {
        toml_value(
            std::str::from_utf8(bytes).map_err(|_| Error::new("Annals config is not UTF-8"))?,
        )?
    } else {
        toml_value(
            "library='annals.db'\n[inbox]\nroot='spool'\nsettle_seconds=60\nminimum_available_bytes=7000000000\n[liaison]\nquality='high'\n",
        )?
    };
    let table = value
        .as_table_mut()
        .ok_or_else(|| Error::new("Annals config must be a table"))?;
    let configured_library = table
        .get("library")
        .and_then(toml::Value::as_str)
        .unwrap_or("annals.db");
    let expected_library = library.join("annals.db");
    if before.is_some()
        && Path::new(configured_library) != Path::new("annals.db")
        && Path::new(configured_library) != expected_library
    {
        return Err(Error::new(
            "Annals installer does not own the configured library location",
        ));
    }
    table.insert(
        "library".into(),
        toml::Value::String(expected_library.to_string_lossy().into_owned()),
    );
    let inbox = table
        .get_mut("inbox")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| Error::new("Annals inbox config missing"))?;
    let configured_spool = inbox
        .get("root")
        .and_then(toml::Value::as_str)
        .unwrap_or("spool");
    if before.is_some()
        && Path::new(configured_spool) != Path::new("spool")
        && Path::new(configured_spool) != library.join("spool")
    {
        return Err(Error::new(
            "Annals installer does not own the configured spool",
        ));
    }
    inbox.insert(
        "root".into(),
        toml::Value::String(library.join("spool").to_string_lossy().into_owned()),
    );
    let liaison = table
        .get_mut("liaison")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| Error::new("Annals liaison config missing"))?;
    liaison.remove("codex");
    liaison.insert(
        "nucleus_socket".into(),
        toml::Value::String(socket.to_string_lossy().into_owned()),
    );
    if let Some(id) = decisions_id {
        table.insert(
            "decision_feed".into(),
            toml::Value::Table(toml::map::Map::from_iter([(
                "expected_library_id".into(),
                toml::Value::String(id.into()),
            )])),
        );
    } else if table.contains_key("decision_feed") && library == state(home) {
        return Err(Error::new(
            "primary Annals config cannot select the decisions feed",
        ));
    }
    toml_bytes(&value)
}

fn usage_config(home: &Path, nucleus: &Path, socket: &Path) -> Result<Vec<u8>> {
    let text = toml::to_string(&json!({"nucleus":nucleus,"nucleus_socket":socket,"library":state(home).join("annals.db"),"spool":state(home).join("spool")})).map_err(|_| Error::new("cannot render Annals Usage config"))?;
    Ok(text.into_bytes())
}

fn private_state(library: &Path, decisions: bool) -> Result<()> {
    for dir in [
        library.to_owned(),
        library.join("spool"),
        library.join("log"),
        library.join("backups"),
    ] {
        private_directory(&dir, decisions)?;
    }
    for name in ["config.toml", "annals.db"] {
        private_file(&library.join(name))?;
    }
    for name in [
        "annals.db-wal",
        "annals.db-shm",
        "annals.db-journal",
        "spool/.queue.json",
        "spool/.queue.json.tmp",
        "spool/.run.lock",
        "spool/.control.lock",
        "spool/.paused",
        "spool/.maintenance",
        "spool/.decision-feed-library.json",
        "spool/.decision-feed-library.json.tmp",
    ] {
        optional_private(&library.join(name))?;
    }
    if decisions {
        private_file(&library.join("spool/.decision-feed-library.json"))?;
    }
    for name in ["log/inbox.stdout.log", "log/inbox.stderr.log"] {
        let path = library.join(name);
        if !optional_private(&path)? {
            write_private(&path, b"", false)?;
        }
    }
    Ok(())
}

pub(super) fn hold(
    payload: &Path,
    library: &Path,
    home: &Path,
    owner: &str,
    operation: &str,
) -> Result<Value> {
    let config = library.join("config.toml");
    let mut args = if config.exists() {
        vec!["--config".into(), config.into_os_string()]
    } else {
        vec![
            "--library".into(),
            library.join("annals.db").into_os_string(),
        ]
    };
    args.extend(["--json".into(), "maintenance".into(), operation.into()]);
    if operation != "status" {
        args.push(owner.into());
    }
    let value = call(payload, &args, home, Some(owner))?;
    let data = cell_install::command::maintenance(&value)?;
    if data.get("protocol_version") != Some(&json!(1))
        || data.get("contract_version") != Some(&json!(1))
    {
        return Err(Error::new(
            "Annals maintenance compatibility deployment required",
        ));
    }
    Ok(data.clone())
}

pub(super) fn drained(payload: &Path, library: &Path, home: &Path, owner: &str) -> Result<()> {
    let until = Instant::now() + Duration::from_secs(45);
    loop {
        let status = hold(payload, library, home, owner, "status")?;
        if status["holds"] != json!([owner]) {
            return Err(Error::new("Annals requires the sole named admission hold"));
        }
        if status["drained"] == true {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(Error::new(
                "Annals admitted commands have not drained; hold retained",
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn wait_inbox(journal: &Journal, config: &Path) -> Result<()> {
    let duration = std::env::var(if journal.key == "annals/inbox" {
        "ANNALS_UPDATE_WAIT_SECONDS"
    } else {
        "ANNALS_DECISIONS_UPDATE_WAIT_SECONDS"
    })
    .ok()
    .map_or(Ok(3900), |v| v.parse::<u64>())
    .map_err(|_| Error::new("Annals update wait must be an integer"))?;
    let until = Instant::now() + Duration::from_secs(duration);
    loop {
        let status = annals(
            &journal.payload(),
            config,
            &["inbox", "status"],
            &journal.home,
            Some(&journal.owner),
        )?;
        if status.get("locked") == Some(&json!(false)) {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(Error::new(
                "Annals inbox remains active; maintenance retained",
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn initialize(
    payload: &Path,
    home: &Path,
    library: &Path,
    socket: &Path,
    decisions: bool,
) -> Result<Vec<u8>> {
    directory(library)?;
    for name in ["log", "backups"] {
        directory(&library.join(name))?;
    }
    let result = call(
        payload,
        &[
            "--library".into(),
            library.join("annals.db").into_os_string(),
            "--json".into(),
            "init".into(),
            "--kind".into(),
            if decisions { "decisions" } else { "general" }.into(),
        ],
        home,
        None,
    )?;
    let id = if decisions {
        if result.pointer("/data/kind") != Some(&json!("decisions")) {
            return Err(Error::new(
                "fresh decisions library has wrong immutable kind",
            ));
        }
        Some(
            result
                .pointer("/data/library_id")
                .and_then(Value::as_str)
                .filter(|s| s.len() == 32)
                .ok_or_else(|| Error::new("Annals init returned no library identity"))?,
        )
    } else {
        None
    };
    let config = config(home, library, socket, None, id)?;
    write_private(&library.join("config.toml"), &config, false)?;
    if decisions {
        annals(
            payload,
            &library.join("config.toml"),
            &["inbox", "run"],
            home,
            None,
        )?;
    } else {
        annals(
            payload,
            &library.join("config.toml"),
            &["inbox", "status"],
            home,
            None,
        )?;
    }
    directory(&library.join("spool"))?;
    if !optional_private(&library.join("spool/.maintenance"))? {
        write_private(&library.join("spool/.maintenance"), b"", false)?;
    }
    private_state(library, decisions)?;
    Ok(config)
}

fn restore_database(backup: &Path, database: &Path) -> Result<()> {
    private_file(backup)?;
    private_file(database)?;
    let source =
        rusqlite::Connection::open_with_flags(backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| Error::new("cannot read Annals backup"))?;
    let mut destination = rusqlite::Connection::open_with_flags(
        database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .map_err(|_| Error::new("cannot open Annals database for recovery"))?;
    let backup = rusqlite::backup::Backup::new(&source, &mut destination)
        .map_err(|_| Error::new("cannot start Annals database recovery"))?;
    let until = Instant::now() + MINUTE;
    loop {
        match backup
            .step(128)
            .map_err(|_| Error::new("Annals database recovery failed"))?
        {
            rusqlite::backup::StepResult::Done => return Ok(()),
            _ if Instant::now() >= until => {
                return Err(Error::new(
                    "Annals recovery could not acquire database access",
                ));
            }
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

pub(super) fn install(args: &InstallArgs) -> Result<Value> {
    let (owner, outer) = owner();
    install_owned(args, &owner, outer)
}

pub(super) fn install_owned(args: &InstallArgs, owner: &str, outer: bool) -> Result<Value> {
    let home = home(args.home.home.clone())?;
    let prior = cell_install::inspect_installation(&release::layout(), &home, &release::legacy)?;
    expected(&prior, args.expected_current.as_deref())?;
    let candidate = release::prepare(args, &home)?;
    deploy_library(
        &home,
        &candidate,
        prior,
        &args.nucleus_socket,
        &args.clockwork,
        Some(&args.nucleus),
        owner,
        outer,
        false,
        args.fresh_state,
        args.no_start,
        args.migration_clockwork_handoff,
        false,
        &args.launchctl,
    )
}

pub(super) fn provision(args: &DecisionsArgs) -> Result<Value> {
    let (owner, outer) = owner();
    provision_owned(args, &owner, outer)
}

pub(super) fn provision_owned(args: &DecisionsArgs, owner: &str, outer: bool) -> Result<Value> {
    let home = home(args.home.home.clone())?;
    let info =
        cell_install::verify_release_at(&release::layout(), &args.release_root, &release::legacy)?;
    if info.format != cell_install::TRANSACTION_FORMAT
        || cell_install::file_digest(&std::env::current_exe()?)?
            != cell_install::file_digest(&args.release_root.join("bin/annals-install"))?
    {
        return Err(Error::new(
            "decisions provisioner must be this exact immutable release's installer",
        ));
    }
    let candidate = PreparedRelease {
        root: args.release_root.clone(),
        info,
    };
    let prior = cell_install::inspect_installation(&release::layout(), &home, &release::legacy)?;
    deploy_library(
        &home,
        &candidate,
        prior,
        &args.nucleus_socket,
        &args.clockwork,
        None,
        owner,
        outer,
        true,
        false,
        false,
        false,
        args.keep_maintenance,
        Path::new("/bin/launchctl"),
    )
}

// These explicit product flags preserve the two existing library installation surfaces.
#[allow(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    clippy::too_many_lines
)]
fn deploy_library(
    home: &Path,
    candidate: &PreparedRelease,
    prior: InstallSnapshot,
    socket: &Path,
    clockwork: &Path,
    nucleus: Option<&Path>,
    owner: &str,
    outer_hold: bool,
    decisions: bool,
    fresh_state: bool,
    no_start: bool,
    handoff: bool,
    keep_maintenance: bool,
    launchctl: &Path,
) -> Result<Value> {
    let layout = release::layout();
    let mut transaction = cell_install::lock_installation(&layout, home, &release::legacy)?;
    transaction.recheck(&prior)?;
    let library = if decisions {
        state(home).join("decisions")
    } else {
        state(home)
    };
    let key = if decisions {
        "annals/decisions-inbox"
    } else {
        "annals/inbox"
    };
    let journal_path = install_root(home).join(format!(
        "transaction.{}.{}",
        if decisions { "decisions" } else { "primary" },
        owner
    ));
    for entry in fs::read_dir(install_root(home))? {
        if entry?
            .file_name()
            .to_string_lossy()
            .starts_with("transaction.")
        {
            return Err(Error::new(
                "Annals has an unfinished installation transaction; run annals-install recover",
            ));
        }
    }
    let control = schedule::inspect(home, clockwork, key)?;
    let schedule_release = if decisions {
        schedule::selected_release(home, clockwork, &control)?
    } else {
        prior.current.clone()
    };
    schedule::prove(
        home,
        clockwork,
        key,
        &library,
        &control,
        schedule_release.as_ref(),
    )?;
    let config_before = capture(&library.join("config.toml"))?;
    let database_existed = optional_private(&library.join("annals.db"))?;
    if decisions && library.exists() && (!database_existed || config_before.is_none()) {
        return Err(Error::new("dedicated decisions state is incomplete"));
    }
    if !database_existed && control.present {
        return Err(Error::new("Annals binding exists without its library"));
    }
    let usage_before = if decisions {
        None
    } else {
        capture(&state(home).join("usage.toml"))?
    };
    let id = config_before
        .as_ref()
        .map(|bytes| {
            toml_value(std::str::from_utf8(bytes).map_err(|_| Error::new("invalid Annals config"))?)
        })
        .transpose()?
        .and_then(|v| {
            v.get("decision_feed")
                .and_then(|v| v.get("expected_library_id"))
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        });
    if decisions && database_existed && id.is_none() {
        return Err(Error::new(
            "decisions configuration has no persistent library identity",
        ));
    }
    let config_after = config(
        home,
        &library,
        socket,
        config_before.as_deref(),
        id.as_deref(),
    )?;
    let usage_after = nucleus
        .map(|nucleus| usage_config(home, nucleus, socket))
        .transpose()?;
    let hold_before = if decisions {
        capture(&library.join(".provision-maintenance.json"))?
    } else {
        None
    };
    let marker_owned = if let Some(bytes) = &hold_before {
        let receipt: Value = serde_json::from_slice(bytes)?;
        private_file(&library.join("spool/.maintenance"))?;
        if receipt.get("version") != Some(&json!(1))
            || receipt.get("key") != Some(&json!(key))
            || receipt.get("library_id") != Some(&json!(id))
            || receipt.get("definition_digest") != Some(&json!(control.digest))
        {
            return Err(Error::new(
                "decisions maintenance receipt does not match owned state",
            ));
        }
        true
    } else {
        false
    };
    let agent = if decisions {
        None
    } else {
        Some(agent::capture(
            home,
            launchctl,
            no_start || handoff,
            control.enabled,
        )?)
    };
    directory(&journal_path)?;
    let mut journal = Journal {
        schema: 1,
        home: home.to_owned(),
        owner: owner.into(),
        outer_hold,
        key: key.into(),
        prior,
        candidate: candidate.info.clone(),
        prior_control: control,
        candidate_digest: None,
        config_before,
        usage_before,
        config_after,
        usage_after,
        database_existed,
        backup_ready: false,
        backup_sha256: None,
        mutation_started: false,
        fresh_state,
        marker_created: false,
        marker_owned,
        hold_before,
        hold_after: None,
        publication: None,
        suspension: None,
        committed: false,
        no_start,
        keep_maintenance: keep_maintenance || handoff,
        clockwork: clockwork.to_owned(),
        launchctl: launchctl.to_owned(),
        agent,
    };
    journal.save(&journal_path)?;
    let result = apply_library(
        &mut journal,
        &journal_path,
        socket,
        handoff,
        &mut transaction,
    );
    if let Err(mut error) = result {
        error.disposition = match rollback(&mut journal, &journal_path, &mut transaction) {
            Ok(()) => cell_install::Disposition::Restored,
            Err(_) => cell_install::Disposition::Uncertain,
        };
        error.message = format!(
            "{}; Annals recovery evidence: {}",
            error.message,
            journal_path.display()
        );
        return Err(error);
    }
    let result = result?;
    archive(&journal, &journal_path)?;
    Ok(result)
}

// Keep the ordered lifecycle and its recovery checks together for review.
#[allow(clippy::too_many_lines)]
fn apply_library(
    journal: &mut Journal,
    path: &Path,
    socket: &Path,
    handoff: bool,
    tx: &mut cell_install::InstallTransaction<'_>,
) -> Result<Value> {
    let library = journal.library();
    let decisions = journal.key == "annals/decisions-inbox";
    let payload = journal.payload();
    if journal.database_existed {
        private_state(&library, decisions)?;
        let source = journal.prior.current.as_ref().map_or_else(
            || payload.clone(),
            |info| release::root(&journal.home, info).join("libexec/annals"),
        );
        hold(&source, &library, &journal.home, &journal.owner, "hold")?;
        drained(&source, &library, &journal.home, &journal.owner)?;
        if !optional_private(&library.join("spool/.maintenance"))? {
            journal.marker_created = true;
            journal.marker_owned = true;
            journal.save(path)?;
            write_private(&library.join("spool/.maintenance"), b"", false)?;
        }
    }
    let definition =
        schedule::definition(&journal.home, &journal.key, &library, &journal.candidate)?;
    schedule::render(&path.join("definition.toml"), &definition)?;
    if !handoff && journal.database_existed {
        journal.candidate_digest = Some(schedule::register(
            &journal.home,
            &journal.clockwork,
            &journal.key,
            &path.join("definition.toml"),
        )?);
        journal.save(path)?;
    }
    let disabled = if journal.no_start || handoff {
        journal.prior_control.clone()
    } else {
        schedule::disable(
            &journal.home,
            &journal.clockwork,
            &journal.key,
            &journal.prior_control,
        )?
    };
    if journal.database_existed {
        wait_inbox(journal, &library.join("config.toml"))?;
    }
    if !journal.no_start
        && !handoff
        && let Some(agent) = &journal.agent
    {
        agent::retire(&journal.home, &journal.launchctl, agent)?;
    }
    if !decisions {
        journal.suspension = Some(tx.suspend(
            &journal.prior,
            &[".local/bin/annals".into(), ".local/bin/annals-usage".into()],
        )?);
        journal.save(path)?;
    }
    journal.mutation_started = true;
    journal.save(path)?;
    if journal.fresh_state || !journal.database_existed {
        let stage = path.join("fresh-state");
        initialize(&payload, &journal.home, &stage, socket, decisions)?;
        let staged_config = toml_value(&fs::read_to_string(stage.join("config.toml"))?)?;
        let id = staged_config
            .get("decision_feed")
            .and_then(|v| v.get("expected_library_id"))
            .and_then(toml::Value::as_str);
        journal.config_after = config(
            &journal.home,
            &library,
            socket,
            journal.config_before.as_deref(),
            id,
        )?;
        journal.marker_created = true;
        journal.marker_owned = true;
        journal.save(path)?;
        if decisions && !journal.database_existed {
            write_private(&stage.join("config.toml"), &journal.config_after, true)?;
            fs::rename(&stage, &library)?;
        } else {
            directory(&library)?;
            directory(&path.join("prior-generation"))?;
            for name in ["annals.db", "annals.db-wal", "annals.db-shm", "spool"] {
                if fs::symlink_metadata(library.join(name)).is_ok() {
                    fs::rename(library.join(name), path.join("prior-generation").join(name))?;
                }
                if fs::symlink_metadata(stage.join(name)).is_ok() {
                    fs::rename(stage.join(name), library.join(name))?;
                }
            }
            for name in ["log", "backups"] {
                directory(&library.join(name))?;
            }
            write_private(&library.join("config.toml"), &journal.config_after, true)?;
        }
        hold(&payload, &library, &journal.home, &journal.owner, "hold")?;
        if journal.fresh_state {
            annals(
                &payload,
                &library.join("config.toml"),
                &["inbox", "pause"],
                &journal.home,
                Some(&journal.owner),
            )?;
            let old_spool = path.join("prior-generation/spool");
            let old_spool_str = old_spool
                .to_str()
                .ok_or_else(|| Error::new("invalid backlog path"))?;
            let result = annals(
                &payload,
                &library.join("config.toml"),
                &["inbox", "import-backlog", "--from", old_spool_str],
                &journal.home,
                Some(&journal.owner),
            )?;
            if !result.get("imported").is_some_and(Value::is_u64) {
                return Err(Error::new("Annals backlog import returned invalid receipt"));
            }
            let status = annals(
                &payload,
                &library.join("config.toml"),
                &["inbox", "status"],
                &journal.home,
                Some(&journal.owner),
            )?;
            if status.get("queued") != result.get("imported")
                || status.get("processing") != Some(&json!(0))
                || status.get("paused") != Some(&json!(true))
                || status.get("maintenance") != Some(&json!(true))
            {
                return Err(Error::new(
                    "fresh Annals backlog failed gated count verification",
                ));
            }
            annals(
                &payload,
                &library.join("config.toml"),
                &["inbox", "resume"],
                &journal.home,
                Some(&journal.owner),
            )?;
        }
    } else {
        let backup = path.join("library.before.db");
        let source = journal.prior.current.as_ref().map_or_else(
            || payload.clone(),
            |info| release::root(&journal.home, info).join("libexec/annals"),
        );
        annals(
            &source,
            &library.join("config.toml"),
            &[
                "backup",
                backup
                    .to_str()
                    .ok_or_else(|| Error::new("invalid backup path"))?,
            ],
            &journal.home,
            Some(&journal.owner),
        )?;
        private_file(&backup)?;
        journal.backup_ready = true;
        journal.backup_sha256 = Some(cell_install::file_digest(&backup)?);
        journal.save(path)?;
        annals(
            &payload,
            &library.join("config.toml"),
            &["migrate"],
            &journal.home,
            Some(&journal.owner),
        )?;
        write_private(&library.join("config.toml"), &journal.config_after, true)?;
    }
    private_state(&library, decisions)?;
    if !handoff && journal.candidate_digest.is_none() {
        journal.candidate_digest = Some(schedule::register(
            &journal.home,
            &journal.clockwork,
            &journal.key,
            &path.join("definition.toml"),
        )?);
        journal.save(path)?;
    }
    let smoke = annals(
        &payload,
        &library.join("config.toml"),
        &["inbox", "run"],
        &journal.home,
        Some(&journal.owner),
    )?;
    if smoke.get("stopped_for_maintenance") != Some(&json!(true)) {
        return Err(Error::new("Annals candidate did not honor maintenance"));
    }
    readiness(
        &payload,
        &library,
        &journal.home,
        Some(&journal.owner),
        decisions,
    )?;
    if let Some(config) = &journal.usage_after {
        write_private(&state(&journal.home).join("usage.toml"), config, true)?;
        cell_install::command::checked(
            &release::root(&journal.home, &journal.candidate).join("libexec/annals-usage"),
            &[
                "doctor".into(),
                "--config".into(),
                state(&journal.home).join("usage.toml").into_os_string(),
            ],
            &environment(&journal.home, Some(&journal.owner)),
            MINUTE * 3,
        )?;
    }
    if !decisions {
        journal.publication = Some(
            tx.publish(
                &PreparedRelease {
                    root: release::root(&journal.home, &journal.candidate),
                    info: journal.candidate.clone(),
                },
                &journal
                    .suspension
                    .as_ref()
                    .ok_or_else(|| Error::new("Annals publication has no suspended prior"))?
                    .after,
                |_| {
                    for command in ["annals", "annals-usage"] {
                        for option in ["--version", "--help"] {
                            cell_install::command::checked(
                                &journal.home.join(".local/bin").join(command),
                                &[option.into()],
                                &environment(&journal.home, Some(&journal.owner)),
                                Duration::from_secs(30),
                            )?;
                        }
                    }
                    readiness(
                        &journal.home.join(".local/bin/annals"),
                        &library,
                        &journal.home,
                        Some(&journal.owner),
                        false,
                    )
                },
            )?,
        );
        journal.save(path)?;
    }
    if !journal.no_start && !handoff {
        if decisions && journal.marker_owned {
            let watermark = annals(
                &payload,
                &library.join("config.toml"),
                &["decision-feed", "watermark"],
                &journal.home,
                Some(&journal.owner),
            )?;
            journal.hold_after = Some(serde_json::to_vec(
                &json!({"version":1,"key":journal.key,"library_id":watermark["library_id"],"definition_digest":journal.candidate_digest}),
            )?);
            journal.save(path)?;
            write_private(
                &library.join(".provision-maintenance.json"),
                journal
                    .hold_after
                    .as_deref()
                    .ok_or_else(|| Error::new("decisions receipt missing"))?,
                true,
            )?;
        }
        let digest = journal
            .candidate_digest
            .as_deref()
            .ok_or_else(|| Error::new("Annals candidate schedule missing"))?;
        schedule::select(
            &journal.home,
            &journal.clockwork,
            &journal.key,
            &disabled,
            digest,
            if journal.outer_hold {
                !journal.prior_control.present || journal.prior_control.enabled
            } else {
                true
            },
        )?;
    }
    if handoff {
        write_private(
            &install_root(&journal.home).join("clockwork-handoff.toml"),
            &fs::read(path.join("definition.toml"))?,
            true,
        )?;
    }
    journal.committed = true;
    journal.save(path)?;
    if !handoff && !journal.keep_maintenance && journal.marker_owned {
        private_file(&library.join("spool/.maintenance"))?;
        fs::remove_file(library.join("spool/.maintenance"))?;
        if decisions && optional_private(&library.join(".provision-maintenance.json"))? {
            fs::remove_file(library.join(".provision-maintenance.json"))?;
        }
    }
    if !journal.outer_hold {
        hold(&payload, &library, &journal.home, &journal.owner, "release")?;
    }
    let mut data = json!({"contract_version":1,"release_id":journal.candidate.release_id,"config":library.join("config.toml"),"clockwork_key":journal.key,"clockwork_definition":journal.candidate_digest,"maintenance":library.join("spool/.maintenance").exists(),"selected":!handoff,"enabled":!journal.no_start && !handoff && (!journal.outer_hold || !journal.prior_control.present || journal.prior_control.enabled)});
    if decisions {
        data["library_id"] = annals(
            &payload,
            &library.join("config.toml"),
            &["decision-feed", "watermark"],
            &journal.home,
            Some(&journal.owner),
        )?["library_id"]
            .clone();
    }
    if handoff {
        data["clockwork_handoff"] =
            json!(install_root(&journal.home).join("clockwork-handoff.toml"));
    }
    Ok(json!({"ok":true,"data":data}))
}

pub(super) fn readiness(
    payload: &Path,
    library: &Path,
    home: &Path,
    owner: Option<&str>,
    decisions: bool,
) -> Result<()> {
    annals(
        payload,
        &library.join("config.toml"),
        &["stats"],
        home,
        owner,
    )?;
    annals(
        payload,
        &library.join("config.toml"),
        &["inbox", "status"],
        home,
        owner,
    )?;
    if decisions {
        let expected = toml_value(&fs::read_to_string(library.join("config.toml"))?)?
            .get("decision_feed")
            .and_then(|v| v.get("expected_library_id"))
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| Error::new("decisions library identity missing"))?;
        let watermark = annals(
            payload,
            &library.join("config.toml"),
            &["decision-feed", "watermark"],
            home,
            owner,
        )?;
        if watermark.get("library_id") != Some(&json!(expected)) {
            return Err(Error::new("decisions feed library identity differs"));
        }
    }
    Ok(())
}

fn restore_file(path: &Path, prior: Option<&[u8]>, candidate: Option<&[u8]>) -> Result<()> {
    let actual = capture(path)?;
    if actual.as_deref() != prior && actual.as_deref() != candidate {
        return Err(Error::new(
            "Annals mutable config changed outside this transaction",
        ));
    }
    if let Some(prior) = prior {
        write_private(path, prior, true)?;
    } else if actual.is_some() {
        fs::remove_file(path)?;
    }
    Ok(())
}

// Keep the ordered lifecycle and its recovery checks together for review.
#[allow(clippy::too_many_lines)]
fn rollback(
    journal: &mut Journal,
    path: &Path,
    tx: &mut cell_install::InstallTransaction<'_>,
) -> Result<()> {
    if journal.committed {
        return Err(Error::new(
            "committed Annals transaction requires completion recovery",
        ));
    }
    let library = journal.library();
    if !journal.no_start {
        let observed = schedule::inspect(&journal.home, &journal.clockwork, &journal.key)?;
        if observed.digest != journal.prior_control.digest
            && observed.digest != journal.candidate_digest
        {
            return Err(Error::new(
                "Annals binding changed outside this transaction",
            ));
        }
        schedule::disable(&journal.home, &journal.clockwork, &journal.key, &observed)?;
    }
    if journal.mutation_started {
        if journal.fresh_state || !journal.database_existed {
            if journal.key == "annals/decisions-inbox" && !journal.database_existed {
                if library.exists() {
                    fs::rename(&library, path.join("failed-new-state"))?;
                }
            } else {
                directory(&path.join("failed-generation"))?;
                for name in ["annals.db", "annals.db-wal", "annals.db-shm", "spool"] {
                    let prior = path.join("prior-generation").join(name);
                    if prior.exists() {
                        if library.join(name).exists() {
                            fs::rename(
                                library.join(name),
                                path.join("failed-generation").join(name),
                            )?;
                        }
                        fs::rename(prior, library.join(name))?;
                    } else if !journal.database_existed && library.join(name).exists() {
                        fs::rename(
                            library.join(name),
                            path.join("failed-generation").join(name),
                        )?;
                    }
                }
            }
        } else if journal.backup_ready {
            if journal.backup_sha256.as_ref()
                != Some(&cell_install::file_digest(&path.join("library.before.db"))?)
            {
                return Err(Error::new("Annals retained database backup changed"));
            }
            restore_database(&path.join("library.before.db"), &library.join("annals.db"))?;
        }
    }
    restore_file(
        &library.join("config.toml"),
        journal.config_before.as_deref(),
        Some(&journal.config_after),
    )?;
    if journal.key == "annals/inbox" {
        restore_file(
            &state(&journal.home).join("usage.toml"),
            journal.usage_before.as_deref(),
            journal.usage_after.as_deref(),
        )?;
    }
    if journal.key == "annals/inbox" {
        tx.recover(
            &journal.prior,
            &PreparedRelease {
                root: release::root(&journal.home, &journal.candidate),
                info: journal.candidate.clone(),
            },
            false,
            |_| Ok(()),
        )?;
    }
    if !journal.no_start {
        schedule::restore(
            &journal.home,
            &journal.clockwork,
            &journal.key,
            &journal.prior_control,
            journal.candidate_digest.as_deref(),
            journal.key == "annals/decisions-inbox",
        )?;
        if let Some(agent) = &journal.agent {
            agent::restore(&journal.home, &journal.launchctl, agent)?;
        }
    }
    if journal.key == "annals/decisions-inbox" && library.exists() {
        restore_file(
            &library.join(".provision-maintenance.json"),
            journal.hold_before.as_deref(),
            journal.hold_after.as_deref(),
        )?;
    }
    if journal.marker_created && optional_private(&library.join("spool/.maintenance"))? {
        fs::remove_file(library.join("spool/.maintenance"))?;
    }
    if !journal.outer_hold && journal.database_existed {
        hold(
            &journal.payload(),
            &library,
            &journal.home,
            &journal.owner,
            "release",
        )?;
    }
    archive(journal, path)
}

fn archive(journal: &Journal, path: &Path) -> Result<()> {
    let parent = state(&journal.home).join("backups/deployments");
    directory(&parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| Error::new("invalid Annals transaction path"))?;
    let destination = parent.join(name);
    if destination.exists() {
        return Err(Error::new("Annals recovery archive already exists"));
    }
    fs::rename(path, destination)?;
    Ok(())
}

// Keep the ordered lifecycle and its recovery checks together for review.
#[allow(clippy::too_many_lines)]
pub(super) fn recover(home: &Path, path: &Path) -> Result<Value> {
    if path.parent() != Some(install_root(home).as_path())
        || !path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("transaction."))
    {
        return Err(Error::new(
            "recovery must name one retained Annals installation transaction",
        ));
    }
    private_directory(path, true)?;
    private_file(&path.join("journal.json"))?;
    let mut journal: Journal = serde_json::from_slice(&fs::read(path.join("journal.json"))?)?;
    if journal.schema != 1
        || journal.home != home
        || !matches!(
            journal.key.as_str(),
            "annals/inbox" | "annals/decisions-inbox"
        )
    {
        return Err(Error::new("foreign Annals recovery transaction"));
    }
    cell_install::verify_release_at(
        &release::layout(),
        &release::root(home, &journal.candidate),
        &release::legacy,
    )?;
    let layout = release::layout();
    let mut tx = cell_install::lock_installation(&layout, home, &release::legacy)?;
    if journal.committed {
        let installed = cell_install::inspect_installation(&layout, home, &release::legacy)?;
        if journal.key == "annals/inbox"
            && journal
                .publication
                .as_ref()
                .is_none_or(|receipt| receipt.after != installed)
        {
            return Err(Error::new(
                "committed Annals program selection changed; maintenance retained",
            ));
        }
        let observed = schedule::inspect(home, &journal.clockwork, &journal.key)?;
        let expected = if journal.no_start || journal.candidate_digest.is_none() {
            journal.prior_control.clone()
        } else {
            schedule::Control {
                present: true,
                enabled: !journal.outer_hold
                    || !journal.prior_control.present
                    || journal.prior_control.enabled,
                digest: journal.candidate_digest.clone(),
            }
        };
        if observed != expected {
            return Err(Error::new(
                "committed Annals scheduler control changed; maintenance retained",
            ));
        }
        let selected = schedule::selected_release(home, &journal.clockwork, &observed)?;
        schedule::prove(
            home,
            &journal.clockwork,
            &journal.key,
            &journal.library(),
            &observed,
            selected.as_ref(),
        )?;
        if journal.marker_owned
            && !journal.keep_maintenance
            && optional_private(&journal.library().join("spool/.maintenance"))?
        {
            drained(&journal.payload(), &journal.library(), home, &journal.owner)?;
        }
        readiness(
            &journal.payload(),
            &journal.library(),
            home,
            Some(&journal.owner),
            journal.key == "annals/decisions-inbox",
        )?;
        if !journal.keep_maintenance
            && journal.marker_owned
            && optional_private(&journal.library().join("spool/.maintenance"))?
        {
            fs::remove_file(journal.library().join("spool/.maintenance"))?;
            if journal.key == "annals/decisions-inbox"
                && optional_private(&journal.library().join(".provision-maintenance.json"))?
            {
                fs::remove_file(journal.library().join(".provision-maintenance.json"))?;
            }
        }
        if !journal.outer_hold {
            hold(
                &journal.payload(),
                &journal.library(),
                home,
                &journal.owner,
                "release",
            )?;
        }
        archive(&journal, path)?;
    } else {
        rollback(&mut journal, path, &mut tx)?;
    }
    Ok(json!({"ok":true,"data":{"recovered":true,"committed":journal.committed}}))
}
