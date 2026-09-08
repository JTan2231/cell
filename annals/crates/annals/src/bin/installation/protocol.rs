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
) -> Result<BTreeMap<String, schedule::Control>> {
    let clockwork = home.join(".local/bin/clockwork");
    let mut result = BTreeMap::new();
    for library in scheduled_libraries(home)? {
        let key = if library == state(home) {
            "annals/inbox"
        } else {
            "annals/decisions-inbox"
        };
        let control = schedule::inspect(home, &clockwork, key)?;
        let selected = if key == "annals/decisions-inbox" {
            schedule::selected_release(home, &clockwork, &control)?
        } else {
            snapshot.current.clone()
        };
        schedule::prove(home, &clockwork, key, &library, &control, selected.as_ref())?;
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
    let mut affected = vec!["nucleus"];
    for name in ["krisis", "semantics"] {
        if home.join(".local/bin").join(name).exists() {
            affected.push(name);
        }
    }
    Ok(reply(
        "ready",
        "Annals programs, independent library controls and admission inspected",
        json!({"current":selection(&snapshot),"installed":snapshot,"controls":controls(home,&snapshot)?,"operator_pauses":pauses(home)?,"runtime":runtime,"maintenance_products":affected,"after":["nucleus","clockwork"]}),
    ))
}

fn prior(context: &Context) -> Result<InstallSnapshot> {
    serde_json::from_value(context.prior()?["installed"].clone()).map_err(Into::into)
}

fn drain(context: &Context, snapshot: &InstallSnapshot) -> Result<()> {
    let payload = payload(&context.home, snapshot, Some(&context.binary("annals")?))?;
    let catalog = catalog_hold(&payload, &context.home, &context.request.run_id, "status")?;
    if catalog["holds"] != json!([context.request.run_id]) || catalog["drained"] != true {
        return Err(Error::new(
            "Annals catalog admission has not drained under this owner",
        ));
    }
    for library in libraries(&context.home)? {
        lifecycle::drained(&payload, &library, &context.home, &context.request.run_id)?;
    }
    for state in ["accepted", "running", "waiting-on-requester"] {
        let value = call(
            &context.home.join(".local/bin/nucleus"),
            &[
                "--compact".into(),
                "jobs".into(),
                "list".into(),
                "--requester".into(),
                "annals".into(),
                "--state".into(),
                state.into(),
                "--limit".into(),
                "1".into(),
            ],
            &context.home,
            Some(&context.request.run_id),
        )?;
        if value["version"] != 1 || value["jobs"] != json!([]) {
            return Err(Error::new("Annals durable Nucleus jobs have not settled"));
        }
    }
    Ok(())
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
    let observed = controls(&context.home, snapshot)?;
    let before: BTreeMap<String, schedule::Control> =
        serde_json::from_value(context.prior()?["controls"].clone())?;
    for (key, prior) in &before {
        if prior.present
            && observed
                .get(key)
                .is_none_or(|now| now.enabled != prior.enabled)
        {
            return Err(Error::new("Annals operator schedule state changed"));
        }
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
    if matches!(operation, Operation::Inspect) {
        return inspect(&context.home, Some(&context));
    }
    let snapshot =
        cell_install::inspect_installation(&release::layout(), &context.home, &release::legacy)?;
    let payload = payload(&context.home, &snapshot, Some(&context.binary("annals")?))?;
    match operation {
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
                    return Err(Error::new(
                        "Annals library creation is still active; catalog hold retained",
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
            drain(&context, &snapshot)?;
            Ok(reply(
                "drained",
                "Annals admitted commands and durable jobs settled",
                json!({"drained":true}),
            ))
        }
        Operation::Apply => {
            if !context.selected() || snapshot != prior(&context)? {
                return Err(Error::new(
                    "Annals apply lacks matching selected prior proof",
                ));
            }
            drain(&context, &snapshot)?;
            let socket = socket(&context.home)?;
            let clockwork = context.home.join(".local/bin/clockwork");
            let args = InstallArgs {
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
                expected_current: Some(selection(&snapshot)),
                fresh_state: false,
                no_start: false,
                migration_clockwork_handoff: false,
                launchctl: "/bin/launchctl".into(),
            };
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
                "applied",
                "Annals primary and decisions installations completed",
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
            if !(is_prior && recovery["any_apply_started"] == false) {
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
                json!({"safe_to_release":true,"installed":if is_prior {"prior"}else{"candidate"}}),
            ))
        }
    }
}
