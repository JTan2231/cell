use super::{
    BTreeMap, Error, HomeArgs, InstallArgs, Path, PathBuf, Result, VERSION, Value, call, fs,
    install_root, json, lifecycle, optional_private, release, schedule, state, toml_value,
};
use cell_install::InstallSnapshot;
use cell_install::adapter::{Context, reply};

fn scheduled_libraries(home: &Path) -> Result<Vec<PathBuf>> {
    let mut result = vec![state(home)];
    let decisions = state(home).join("decisions");
    if fs::symlink_metadata(&decisions).is_ok() {
        if !decisions.join("config.toml").is_file() {
            return Err(Error::new("dedicated decisions state is incomplete"));
        }
        result.push(decisions);
    }
    Ok(result)
}

fn libraries(home: &Path) -> Result<Vec<PathBuf>> {
    let mut result = scheduled_libraries(home)?;
    for entry in annals::api::registered_libraries(&state(home))
        .map_err(|error| Error::new(error.to_string()))?
    {
        if entry.state != "ready" {
            return Err(Error::new(
                "named Annals library provisioning requires recovery",
            ));
        }
        let root = entry
            .library
            .parent()
            .ok_or_else(|| Error::new("named Annals library has no parent directory"))?;
        if entry.library != root.join("annals.db") || entry.spool != root.join("spool") {
            return Err(Error::new("named Annals library has an unsupported layout"));
        }
        if !result.iter().any(|path| path == root) {
            result.push(root.to_owned());
        }
    }
    Ok(result)
}

fn catalog_hold(payload: &Path, home: &Path, owner: &str, operation: &str) -> Result<Value> {
    let mut args = vec![
        "--library".into(),
        state(home).join("catalog.db").into_os_string(),
        "--json".into(),
        "maintenance".into(),
        operation.into(),
    ];
    if operation != "status" {
        args.push(owner.into());
    }
    let value = call(payload, &args, home, Some(owner))?;
    Ok(cell_install::command::maintenance(&value)?.clone())
}

fn no_installer_hold(home: &Path) -> Result<()> {
    for library in libraries(home)? {
        if fs::symlink_metadata(library.join("spool/.maintenance")).is_ok() {
            return Err(Error::new(
                "Annals installer retained maintenance; product recovery required",
            ));
        }
    }
    if install_root(home).exists() {
        for entry in fs::read_dir(install_root(home))? {
            let name = entry?.file_name();
            if name == ".update-lock" || name.to_string_lossy().starts_with("transaction.") {
                return Err(Error::new(
                    "Annals installer lock or transaction requires product recovery",
                ));
            }
        }
    }
    Ok(())
}

fn selection(snapshot: &InstallSnapshot) -> String {
    snapshot.current.as_ref().map_or_else(
        || "absent".into(),
        |info| format!("releases/{}", info.release_id),
    )
}

fn payload(home: &Path, snapshot: &InstallSnapshot, candidate: Option<&Path>) -> Result<PathBuf> {
    snapshot
        .current
        .as_ref()
        .map(|info| release::root(home, info).join("libexec/annals"))
        .or_else(|| candidate.map(Path::to_owned))
        .ok_or_else(|| Error::new("Annals has no installed or admitted payload"))
}

fn pauses(home: &Path) -> Result<BTreeMap<String, bool>> {
    libraries(home)?
        .into_iter()
        .filter(|p| p.join("config.toml").exists())
        .map(|p| {
            Ok((
                p.to_string_lossy().into_owned(),
                optional_private(&p.join("spool/.paused"))?,
            ))
        })
        .collect()
}

fn controls(
    home: &Path,
    snapshot: &InstallSnapshot,
    clockwork: &Path,
) -> Result<BTreeMap<String, schedule::Control>> {
    let mut result = BTreeMap::new();
    for library in scheduled_libraries(home)? {
        let key = if library == state(home) {
            "annals/inbox"
        } else {
            "annals/decisions-inbox"
        };
        let control = schedule::inspect(home, clockwork, key)?;
        let selected = if key == "annals/decisions-inbox" {
            schedule::selected_release(home, clockwork, &control)?
        } else {
            snapshot.current.clone()
        };
        schedule::prove(home, clockwork, key, &library, &control, selected.as_ref())?;
        result.insert(key.into(), control);
    }
    Ok(result)
}

pub(super) fn inspect(home: &Path, context: Option<&Context>) -> Result<Value> {
    no_installer_hold(home)?;
    let snapshot = cell_install::inspect_installation(&release::layout(), home, &release::legacy)?;
    let mut runtime = Vec::new();
    if snapshot.current.is_some() || context.is_some() {
        let candidate = context.map(|ctx| ctx.binary("annals")).transpose()?;
        let payload = payload(home, &snapshot, candidate.as_deref())?;
        let owner = context.map_or("inspection", |ctx| ctx.request.run_id.as_str());
        let catalog = catalog_hold(&payload, home, owner, "status")?;
        if catalog["holds"] != json!([]) && catalog["holds"] != json!([owner]) {
            return Err(Error::new("another owner holds the Annals catalog"));
        }
        runtime.push(catalog);
        for library in libraries(home)? {
            let owner = context.map_or("inspection", |ctx| ctx.request.run_id.as_str());
            let status = lifecycle::hold(&payload, &library, home, owner, "status")?;
            if status["holds"] != json!([]) && status["holds"] != json!([owner]) {
                return Err(Error::new("another owner holds Annals admission"));
            }
            runtime.push(status);
        }
    }
    Ok(reply(
        "ready",
        "Annals programs, independent library controls and admission inspected",
        json!({"current":selection(&snapshot),"installed":snapshot,"controls":controls(home,&snapshot,&context.map(|ctx| ctx.dependency_binary("clockwork")).transpose()?.unwrap_or_else(|| home.join(".local/bin/clockwork")))?,"operator_pauses":pauses(home)?,"runtime":runtime,"maintenance_products":[],"after":["nucleus","clockwork"]}),
    ))
}

fn socket(home: &Path) -> Result<PathBuf> {
    let config = state(home).join("config.toml");
    if optional_private(&config)? {
        let config = toml_value(&fs::read_to_string(config)?)?;
        if let Some(socket) = config
            .get("liaison")
            .and_then(|v| v.get("nucleus_socket"))
            .and_then(toml::Value::as_str)
        {
            return Ok(socket.into());
        }
    }
    Ok(home.join("Library/Application Support/Nucleus/nucleus.sock"))
}

/// Execute Annals-owned setup as one opaque manifest command.
pub(super) fn deploy() -> Result<Value> {
    let context = Context::read("annals", "products/annals", "annals-install", VERSION)?;
    context.validate_settings(&[], &["enabled"])?;
    let snapshot =
        cell_install::inspect_installation(&release::layout(), &context.home, &release::legacy)?;
    let args = install_args(&context, &snapshot)?;
    lifecycle::deploy(&args)
}

fn install_args(context: &Context, snapshot: &InstallSnapshot) -> Result<InstallArgs> {
    let socket = socket(&context.home)?;
    let clockwork = context.home.join(".local/bin/clockwork");
    Ok(InstallArgs {
        binary: context.binary("annals")?,
        usage_binary: context.binary("annals-usage")?,
        bundle: context
            .request
            .source_root
            .join("products/annals/chancery/annals"),
        usage_bundle: context
            .request
            .source_root
            .join("products/annals/chancery/annals-usage"),
        nucleus: context.home.join(".local/bin/nucleus"),
        nucleus_socket: socket.clone(),
        clockwork: clockwork.clone(),
        home: HomeArgs {
            home: Some(context.home.clone()),
        },
        expected_current: Some(selection(snapshot)),
        no_start: false,
        enabled: context
            .request
            .settings
            .as_ref()
            .and_then(|value| value.get("enabled"))
            .and_then(Value::as_bool),
        migration_clockwork_handoff: false,
        launchctl: "/bin/launchctl".into(),
    })
}
