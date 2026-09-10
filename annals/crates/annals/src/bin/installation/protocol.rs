use super::{
    BTreeMap, DecisionsArgs, Error, HomeArgs, InstallArgs, MINUTE, Operation, Path, PathBuf,
    Result, VERSION, Value, call, environment, fs, install_root, json, lifecycle, optional_private,
    release, schedule, state, toml_value,
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

fn prior(context: &Context) -> Result<InstallSnapshot> {
    serde_json::from_value(context.prior()?["installed"].clone()).map_err(Into::into)
}

fn drain(context: &Context, snapshot: &InstallSnapshot) -> Result<bool> {
    let payload = payload(&context.home, snapshot, Some(&context.binary("annals")?))?;
    let catalog = catalog_hold(&payload, &context.home, &context.request.run_id, "status")?;
    if catalog["holds"] != json!([context.request.run_id]) {
        return Err(Error::new(
            "Annals catalog admission has not drained under this owner",
        ));
    }
    if catalog["drained"] != true {
        return Ok(false);
    }
    for library in libraries(&context.home)? {
        let status = lifecycle::hold(
            &payload,
            &library,
            &context.home,
            &context.request.run_id,
            "hold",
        )?;
        if status["holds"] != json!([context.request.run_id]) {
            return Err(Error::new("another owner holds an Annals library"));
        }
        if status["drained"] != true {
            return Ok(false);
        }
    }
    if !context.home.join(".local/bin/nucleus").exists()
        && !context
            .home
            .join("Library/Application Support/Nucleus/nucleus.db")
            .exists()
    {
        return Ok(true);
    }
    let mut after: Option<nucleus_core::JobId> = None;
    loop {
        let mut args: Vec<std::ffi::OsString> = [
            "--compact",
            "jobs",
            "list",
            "--requester",
            "annals",
            "--limit",
            "1000",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        if let Some(cursor) = &after {
            args.extend(["--after".into(), cursor.to_string().into()]);
        }
        let value = call(
            &context.home.join(".local/bin/nucleus"),
            &args,
            &context.home,
            Some(&context.request.run_id),
        )?;
        let page: nucleus_core::ListJobsResponseV1 = serde_json::from_value(value)?;
        if page.version != 1 {
            return Err(Error::new("unsupported Annals Nucleus job response"));
        }
        if page.jobs.iter().any(|job| !job.state.is_terminal()) {
            return Ok(false);
        }
        let Some(next) = page.next else {
            break;
        };
        if after.as_ref() == Some(&next) {
            return Err(Error::new("Annals Nucleus job cursor did not advance"));
        }
        after = Some(next);
    }
    Ok(true)
}

fn verify(context: &Context, snapshot: &InstallSnapshot, candidate: bool) -> Result<()> {
    no_installer_hold(&context.home)?;
    let info = snapshot
        .current
        .as_ref()
        .ok_or_else(|| Error::new("Annals is absent"))?;
    let root = release::root(&context.home, info);
    if candidate {
        release::exact_candidate(
            info,
            &root,
            &context.binary("annals")?,
            &context.binary("annals-usage")?,
        )?;
        let mut expected_paths: std::collections::BTreeSet<String> = [
            "libexec/annals",
            "libexec/annals-usage",
            "bin/annals",
            "bin/annals-inbox",
            "bin/annals-install",
            "package/install",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for name in ["annals", "annals-usage"] {
            let bundle = context
                .request
                .source_root
                .join("annals/chancery")
                .join(name);
            let spec = cell_install::InstallSpec {
                product: name,
                application: "Annals",
                commands: &["annals"],
                provider: name,
            };
            for (path, file) in cell_install::provider_inventory(&bundle, &spec)? {
                let relative = format!("share/chancery/{name}/{path}");
                if info.files.get(&relative) != Some(&file) {
                    return Err(Error::new(
                        "installed Annals contract differs from candidate source",
                    ));
                }
                expected_paths.insert(relative);
            }
        }
        if info
            .files
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            != expected_paths
        {
            return Err(Error::new("Annals candidate exact file inventory differs"));
        }
    }
    let observed = controls(
        &context.home,
        snapshot,
        &context.dependency_binary("clockwork")?,
    )?;
    if observed.values().any(|control| control.enabled) {
        return Err(Error::new(
            "Annals schedule activated before final activation",
        ));
    }
    let prior_pauses: BTreeMap<String, bool> =
        serde_json::from_value(context.prior()?["operator_pauses"].clone())?;
    let observed_pauses = pauses(&context.home)?;
    if prior_pauses
        .iter()
        .any(|(path, paused)| *paused && observed_pauses.get(path) != Some(&true))
    {
        return Err(Error::new("Annals operator pause was cleared"));
    }
    for library in libraries(&context.home)? {
        let decisions = toml_value(&fs::read_to_string(library.join("config.toml"))?)?
            .get("decision_feed")
            .is_some();
        lifecycle::readiness(
            &root.join("libexec/annals"),
            &library,
            &context.home,
            Some(&context.request.run_id),
            decisions,
        )?;
    }
    cell_install::command::checked(
        &root.join("libexec/annals-usage"),
        &[
            "doctor".into(),
            "--config".into(),
            state(&context.home).join("usage.toml").into_os_string(),
        ],
        &environment(&context.home, Some(&context.request.run_id)),
        MINUTE * 3,
    )?;
    Ok(())
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

// Keep the adapter operation dispatch and its product admission checks together.
#[allow(clippy::too_many_lines)]
pub(super) fn run(operation: Operation) -> Result<Value> {
    let context = Context::read("annals", "annals", "annals-install", VERSION)?;
    context.validate_settings(&[], &["enabled"])?;
    if matches!(operation, Operation::Inspect) {
        return inspect(&context.home, Some(&context));
    }
    let recovered_candidate = if matches!(operation, Operation::Recover) {
        recover_transactions(&context)?
    } else {
        None
    };
    let snapshot =
        cell_install::inspect_installation(&release::layout(), &context.home, &release::legacy)?;
    let payload = payload(&context.home, &snapshot, Some(&context.binary("annals")?))?;
    match operation {
        Operation::Apply => {
            if !context.selected() || snapshot != prior(&context)? {
                return Err(Error::new("Annals staging lacks matching prior proof"));
            }
            let prepared = release::prepare(&install_args(&context, &snapshot)?, &context.home)?;
            Ok(reply(
                "applied",
                "Annals immutable release staged; configuration and publication pending",
                json!({"staged":prepared.info}),
            ))
        }
        Operation::Activate => {
            if snapshot.current.is_none() {
                return Ok(reply("activated", "Annals remains absent", json!({})));
            }
            no_installer_hold(&context.home)?;
            for library in libraries(&context.home)? {
                if lifecycle::hold(
                    &payload,
                    &library,
                    &context.home,
                    &context.request.run_id,
                    "status",
                )?["holds"]
                    != json!([])
                {
                    return Err(Error::new("Annals activation requires released admission"));
                }
            }
            let before: BTreeMap<String, schedule::Control> =
                serde_json::from_value(context.prior()?["controls"].clone())?;
            let clockwork = context.home.join(".local/bin/clockwork");
            for (key, control) in controls(
                &context.home,
                &snapshot,
                &context.dependency_binary("clockwork")?,
            )? {
                let enabled = context.activation_enabled(
                    before
                        .get(&key)
                        .is_none_or(|prior| !prior.present || prior.enabled),
                )?;
                if let Some(digest) = &control.digest {
                    schedule::select(&context.home, &clockwork, &key, &control, digest, enabled)?;
                }
            }
            Ok(reply(
                "activated",
                "Annals captured scheduling intent restored",
                json!({"controls":controls(&context.home,&snapshot,&context.dependency_binary("clockwork")?)?}),
            ))
        }
        Operation::Inspect => unreachable!(),
        Operation::Hold | Operation::Release => {
            if matches!(operation, Operation::Release) {
                no_installer_hold(&context.home)?;
            }
            let mut values = Vec::new();
            if matches!(operation, Operation::Hold) {
                let catalog =
                    catalog_hold(&payload, &context.home, &context.request.run_id, "hold")?;
                if catalog["drained"] != true {
                    return Ok(reply(
                        "held",
                        "Annals catalog held while admitted library creation finishes",
                        json!({"targets":[catalog]}),
                    ));
                }
                values.push(catalog);
            }
            for library in libraries(&context.home)? {
                values.push(lifecycle::hold(
                    &payload,
                    &library,
                    &context.home,
                    &context.request.run_id,
                    if matches!(operation, Operation::Hold) {
                        "hold"
                    } else {
                        "release"
                    },
                )?);
            }
            if matches!(operation, Operation::Release) {
                values.push(catalog_hold(
                    &payload,
                    &context.home,
                    &context.request.run_id,
                    "release",
                )?);
            }
            if matches!(operation, Operation::Hold) {
                let clockwork = context.home.join(".local/bin/clockwork");
                for (key, control) in controls(
                    &context.home,
                    &snapshot,
                    &context.dependency_binary("clockwork")?,
                )? {
                    if control.enabled {
                        schedule::disable(&context.home, &clockwork, &key, &control)?;
                    }
                }
            }
            Ok(reply(
                if matches!(operation, Operation::Hold) {
                    "held"
                } else {
                    "released"
                },
                "Annals named admission ownership updated",
                json!({"targets":values}),
            ))
        }
        Operation::Drain => {
            let settled = drain(&context, &snapshot)?;
            Ok(reply(
                if settled { "drained" } else { "waiting" },
                "Annals admitted commands and durable jobs settled",
                json!({"drained":settled}),
            ))
        }
        Operation::Configure => {
            if !context.selected() {
                verify(&context, &snapshot, false)?;
                return Ok(reply(
                    "configured",
                    "Annals retained configuration verified",
                    json!({}),
                ));
            }
            if snapshot != prior(&context)? {
                return Err(Error::new(
                    "Annals apply lacks matching selected prior proof",
                ));
            }
            if !drain(&context, &snapshot)? {
                return Err(Error::new("Annals apply requires settled durable work"));
            }
            let socket = socket(&context.home)?;
            let clockwork = context.home.join(".local/bin/clockwork");
            let args = install_args(&context, &snapshot)?;
            lifecycle::install_owned(&args, &context.request.run_id, true)?;
            let installed = cell_install::inspect_installation(
                &release::layout(),
                &context.home,
                &release::legacy,
            )?;
            let root = release::root(
                &context.home,
                installed
                    .current
                    .as_ref()
                    .ok_or_else(|| Error::new("Annals candidate selection missing"))?,
            );
            lifecycle::provision_owned(
                &DecisionsArgs {
                    release_root: root,
                    nucleus_socket: socket,
                    clockwork,
                    home: HomeArgs {
                        home: Some(context.home.clone()),
                    },
                    keep_maintenance: false,
                },
                &context.request.run_id,
                true,
            )?;
            for library in libraries(&context.home)? {
                lifecycle::drained(&payload, &library, &context.home, &context.request.run_id)?;
            }
            Ok(reply(
                "configured",
                "Annals primary and decisions configured and published with scheduling disabled",
                json!({"installed":installed,"current":selection(&installed)}),
            ))
        }
        Operation::Verify => {
            let selected = context.selected();
            if !selected && snapshot != prior(&context)? {
                return Err(Error::new(
                    "unselected Annals installation changed since inspection",
                ));
            }
            verify(&context, &snapshot, selected)?;
            Ok(reply(
                "verified",
                "Annals installation and both library readiness boundaries verified",
                json!({"installed":snapshot}),
            ))
        }
        Operation::Recover => {
            no_installer_hold(&context.home)?;
            let before = prior(&context)?;
            let is_prior = snapshot == before;
            let recovery = context.request.recovery.clone().unwrap_or_default();
            let candidate = recovered_candidate.unwrap_or(
                !is_prior || recovery["configured"] == true || recovery["installed"] == "candidate",
            );
            if !(is_prior && (recovery["configure_started"] != true || before.current.is_none())) {
                for library in libraries(&context.home)? {
                    let held = lifecycle::hold(
                        &payload,
                        &library,
                        &context.home,
                        &context.request.run_id,
                        "status",
                    )?;
                    if held["holds"] == json!([context.request.run_id]) {
                        lifecycle::drained(
                            &payload,
                            &library,
                            &context.home,
                            &context.request.run_id,
                        )?;
                    } else if recovery["verified"] != true {
                        return Err(Error::new(
                            "Annals recovery lacks sole hold or completed verification",
                        ));
                    }
                }
                verify(&context, &snapshot, !is_prior)?;
            }
            Ok(reply(
                "recovered",
                "Coherent Annals programs and domain state proved without inferred database rollback",
                json!({"safe_to_release":true,"installed":if candidate {"candidate"}else{"prior"}}),
            ))
        }
    }
}

fn recover_transactions(context: &Context) -> Result<Option<bool>> {
    {
        let layout = release::layout();
        let _lock = cell_install::lock_installation(&layout, &context.home, &release::legacy)?;
    }
    let root = install_root(&context.home);
    if !root.exists() {
        return Ok(None);
    }
    let mut transactions = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("transaction.")
        {
            transactions.push(entry.path());
        }
    }
    transactions.sort();
    let mut candidate = None;
    for path in transactions {
        let journal: Value = serde_json::from_slice(&fs::read(path.join("journal.json"))?)?;
        if journal["owner"] != context.request.run_id || journal["outer_hold"] != true {
            return Err(Error::new(
                "Annals recovery transaction belongs to another owner",
            ));
        }
        let recovered = lifecycle::recover(&context.home, &path)?;
        candidate = Some(candidate.unwrap_or(true) && recovered["data"]["committed"] == true);
    }
    Ok(candidate)
}

fn install_args(context: &Context, snapshot: &InstallSnapshot) -> Result<InstallArgs> {
    let socket = socket(&context.home)?;
    let clockwork = context.home.join(".local/bin/clockwork");
    Ok(InstallArgs {
        binary: context.binary("annals")?,
        usage_binary: context.binary("annals-usage")?,
        bundle: context.request.source_root.join("annals/chancery/annals"),
        usage_bundle: context
            .request
            .source_root
            .join("annals/chancery/annals-usage"),
        nucleus: context.home.join(".local/bin/nucleus"),
        nucleus_socket: socket.clone(),
        clockwork: clockwork.clone(),
        home: HomeArgs {
            home: Some(context.home.clone()),
        },
        expected_current: Some(selection(snapshot)),
        fresh_state: false,
        no_start: false,
        migration_clockwork_handoff: false,
        launchctl: "/bin/launchctl".into(),
    })
}
