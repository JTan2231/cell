use super::{Candidate, HomeArgs, Result, fail, lifecycle};
use cell_install::adapter::{Context, Operation, reply};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::time::Duration;

fn home(context: &Context) -> Result<HomeArgs> {
    Ok(HomeArgs {
        home: context.home.clone(),
        clockwork: Some(context.dependency_binary("clockwork")?),
        launchctl: "/bin/launchctl".into(),
    })
}

fn candidate(context: &Context) -> Result<Candidate> {
    Ok(Candidate {
        binary: context.binary("semantics")?,
        bundle: context.request.source_root.join("semantics/chancery"),
        home: home(context)?,
        expected_current: context
            .prior()?
            .get("current")
            .and_then(Value::as_str)
            .map(str::to_owned),
        final_decisions_watermark: None,
        keep_maintenance: false,
    })
}

fn no_installer_hold(paths: &lifecycle::Paths) -> Result<()> {
    for path in [
        paths.install.join(".update-lock"),
        paths.state.join(".clockwork-maintenance"),
        paths.state.join(".deployment-maintenance.json"),
    ] {
        if std::fs::symlink_metadata(path).is_ok() {
            return fail("Semantics installer has retained maintenance or unfinished recovery");
        }
    }
    if paths.install.is_dir() {
        for entry in std::fs::read_dir(&paths.install)? {
            if entry?
                .file_name()
                .to_string_lossy()
                .starts_with(".transaction.")
            {
                return fail("Semantics installer retained an unfinished transaction");
            }
        }
    }
    Ok(())
}

fn maintenance(context: &Context, operation: &str) -> Result<Value> {
    let paths = lifecycle::Paths::new(&home(context)?)?;
    let cli = context.home.join(".local/bin/semantics");
    let binary = if cli.exists() {
        cli
    } else {
        context.binary("semantics")?
    };
    let mut arguments: Vec<OsString> = vec![
        "--database".into(),
        paths.database.into_os_string(),
        "--json".into(),
        "maintenance".into(),
        operation.into(),
    ];
    if operation != "status" {
        arguments.push(context.request.run_id.clone().into());
    }
    let environment = BTreeMap::from([(
        OsString::from("CELL_DEPLOYMENT_RUN_ID"),
        OsString::from(&context.request.run_id),
    )]);
    let value =
        cell_install::command::json(&binary, &arguments, &environment, Duration::from_secs(60))?;
    let mut status = &value;
    for _ in 0..4 {
        if status.get("holds").is_some() {
            break;
        }
        status = status
            .get("maintenance")
            .or_else(|| status.get("data"))
            .ok_or("invalid Semantics maintenance response")?;
    }
    if status["protocol_version"] != 1
        || status["contract_version"] != 1
        || !status["holds"].is_array()
        || !status["drained"].is_boolean()
    {
        return fail("Semantics maintenance support is unavailable");
    }
    Ok(status.clone())
}

fn same_prior(context: &Context) -> Result<()> {
    let observed = lifecycle::inspect(&home(context)?)?;
    let prior = context.prior()?;
    if observed["installed"] != prior["installed"] || observed["current"] != prior["current"] {
        return fail("Semantics installation changed since inspection");
    }
    Ok(())
}

fn drain(context: &Context) -> Result<bool> {
    {
        let status = maintenance(context, "status")?;
        if status["holds"] != json!([context.request.run_id]) {
            return fail("Semantics drain requires the sole run-owned hold");
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
        let mut args: Vec<OsString> = [
            "--compact",
            "jobs",
            "list",
            "--requester",
            "semantics",
            "--limit",
            "1000",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        if let Some(cursor) = &after {
            args.extend(["--after".into(), cursor.to_string().into()]);
        }
        let value = cell_install::command::json(
            &context.home.join(".local/bin/nucleus"),
            &args,
            &BTreeMap::new(),
            Duration::from_secs(60),
        )?;
        let page: nucleus_core::ListJobsResponseV1 = serde_json::from_value(value)?;
        if page.version != 1 {
            return fail("unsupported Semantics Nucleus job response");
        }
        if page.jobs.iter().any(|job| !job.state.is_terminal()) {
            return Ok(false);
        }
        let Some(next) = page.next else {
            break;
        };
        if after.as_ref() == Some(&next) {
            return fail("Semantics Nucleus job cursor did not advance");
        }
        after = Some(next);
    }
    Ok(true)
}

fn controls(context: &Context, activate: bool) -> Result<Value> {
    let paths = lifecycle::Paths::new(&home(context)?)?;
    let observed = lifecycle::inspect(&home(context)?)?;
    let selected = &observed["controls"][lifecycle::KEY];
    let prior = &context.prior()?["controls"][lifecycle::KEY];
    let enabled = activate
        && context.activation_enabled(
            prior
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        )?;
    if let Some(digest) = selected["definition_digest"].as_str() {
        if enabled {
            paths.clock(&["binding", "switch", lifecycle::KEY, digest])?;
        } else {
            paths.clock(&["binding", "disable", lifecycle::KEY, "--select", digest])?;
        }
    }
    Ok(lifecycle::inspect(&home(context)?)?["controls"].clone())
}

fn readiness(context: &Context) -> Result<()> {
    let paths = lifecycle::Paths::new(&home(context)?)?;
    paths.doctor(&paths.payload(), None, Some(&context.request.run_id))?;
    Ok(())
}

pub(super) fn run(operation: &str) -> Result<Value> {
    let operation: Operation = operation.parse()?;
    let context = Context::read(
        "semantics",
        "semantics",
        "semantics-install",
        env!("CARGO_PKG_VERSION"),
    )?;
    context.validate_settings(&[], &["enabled"])?;
    let paths = lifecycle::Paths::new(&home(&context)?)?;
    let recovered_candidate = if matches!(operation, Operation::Recover) {
        recover_transactions(&context)?
    } else {
        None
    };
    no_installer_hold(&paths)?;
    let (status, data) = match operation {
        Operation::Apply => {
            same_prior(&context)?;
            let prepared = lifecycle::prepare(&candidate(&context)?)?;
            (
                "applied",
                json!({"staged":prepared.info,"configuration_pending":true}),
            )
        }
        Operation::Activate => {
            if maintenance(&context, "status")?["holds"] != json!([]) {
                return fail("Semantics activation requires released admission");
            }
            ("activated", controls(&context, true)?)
        }
        Operation::Inspect => {
            let mut installed = lifecycle::inspect(&home(&context)?)?;
            let maintenance = maintenance(&context, "status")?;
            if maintenance["holds"] != json!([])
                && maintenance["holds"] != json!([context.request.run_id])
            {
                return fail("another owner holds Semantics admission");
            }
            installed["maintenance_products"] = json!([]);
            installed["after"] = json!(["annals", "nucleus", "clockwork"]);
            ("ready", installed)
        }
        Operation::Hold => {
            let held = maintenance(&context, "hold")?;
            if !held["holds"]
                .as_array()
                .is_some_and(|v| v.contains(&json!(context.request.run_id)))
            {
                return fail("Semantics did not retain the requested hold");
            }
            controls(&context, false)?;
            ("held", held)
        }
        Operation::Drain => {
            let settled = drain(&context)?;
            (
                if settled { "drained" } else { "waiting" },
                json!({"drained":settled}),
            )
        }
        Operation::Configure => {
            if !context.selected() {
                same_prior(&context)?;
                readiness(&context)?;
                return Ok(reply(
                    "configured",
                    "Semantics retained configuration verified",
                    json!({}),
                ));
            }
            same_prior(&context)?;
            if !drain(&context)? {
                return fail("Semantics apply requires settled durable work");
            }
            let args = candidate(&context)?;
            let command_line: Vec<OsString> = vec![
                "install".into(),
                "--binary".into(),
                args.binary.into_os_string(),
                "--bundle".into(),
                args.bundle.into_os_string(),
                "--home".into(),
                context.home.clone().into_os_string(),
                "--expected-current".into(),
                context.prior()?["current"]
                    .as_str()
                    .ok_or("missing prior selector")?
                    .into(),
            ];
            let env = BTreeMap::from([(
                OsString::from("CELL_DEPLOYMENT_RUN_ID"),
                OsString::from(&context.request.run_id),
            )]);
            cell_install::command::json(
                &std::env::current_exe()?,
                &command_line,
                &env,
                Duration::from_secs(1200),
            )?;
            controls(&context, false)?;
            ("configured", lifecycle::inspect(&home(&context)?)?)
        }
        Operation::Verify => {
            let args = candidate(&context)?;
            let installed = lifecycle::inspect(&home(&context)?)?;
            if context.selected()
                && installed["installed"]["current"]["release_id"]
                    != lifecycle::prepare(&args)?.info.release_id
            {
                return fail("Semantics installed artifacts differ from the candidate");
            }
            if !context.selected() {
                same_prior(&context)?;
            }
            require_disabled(&context)?;
            readiness(&context)?;
            ("verified", installed)
        }
        Operation::Release => {
            require_disabled(&context)?;
            ("released", maintenance(&context, "release")?)
        }
        Operation::Recover => {
            let observed = lifecycle::inspect(&home(&context)?)?;
            let prior = observed["installed"] == context.prior()?["installed"];
            let evidence = context
                .request
                .recovery
                .as_ref()
                .ok_or("recovery evidence is missing")?;
            let kept_candidate = recovered_candidate.unwrap_or(
                !prior || evidence["configured"] == true || evidence["installed"] == "candidate",
            );
            if !prior {
                let prepared = lifecycle::prepare(&candidate(&context)?)?;
                if observed["installed"]["current"]["release_id"] != prepared.info.release_id {
                    return fail(
                        "Semantics recovery found neither recorded prior nor exact candidate",
                    );
                }
            }
            controls(&context, false)?;
            if !(prior && evidence["configure_started"] != true) {
                let maintenance = maintenance(&context, "status")?;
                if evidence["verified"] != true
                    && (maintenance["holds"] != json!([context.request.run_id])
                        || maintenance["drained"] != true)
                {
                    return fail("Semantics recovery requires drained owned admission");
                }
                readiness(&context)?;
            }
            (
                "recovered",
                json!({"safe_to_release":true,"installed":if kept_candidate {"candidate"} else {"prior"}}),
            )
        }
    };
    Ok(reply(
        status,
        "Semantics product-owned operation completed",
        data,
    ))
}

fn recover_transactions(context: &Context) -> Result<Option<bool>> {
    lifecycle::recover_lock(&home(context)?)?;
    let paths = lifecycle::Paths::new(&home(context)?)?;
    if !paths.install.exists() {
        return Ok(None);
    }
    let mut candidate = None;
    for entry in std::fs::read_dir(&paths.install)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(".transaction.")
        {
            continue;
        }
        let transaction: Value =
            serde_json::from_slice(&std::fs::read(entry.path().join("transaction.json"))?)?;
        let mut arguments: Vec<OsString> = vec![
            "recover".into(),
            "--transaction".into(),
            entry.path().into_os_string(),
            "--home".into(),
            context.home.clone().into_os_string(),
        ];
        let forward = transaction["candidate_selected"] == true
            && transaction["binding"]["definition_digest"].is_null();
        if forward {
            arguments.push("--forward".into());
        }
        let environment = BTreeMap::from([(
            OsString::from("CELL_DEPLOYMENT_RUN_ID"),
            OsString::from(&context.request.run_id),
        )]);
        cell_install::command::json(
            &std::env::current_exe()?,
            &arguments,
            &environment,
            Duration::from_secs(1200),
        )?;
        candidate = Some(candidate.unwrap_or(true) && forward);
    }
    Ok(candidate)
}

fn require_disabled(context: &Context) -> Result<()> {
    let controls = lifecycle::inspect(&home(context)?)?["controls"].clone();
    if controls[lifecycle::KEY]["enabled"] == true {
        return fail("Semantics worker activated before final activation");
    }
    Ok(())
}
