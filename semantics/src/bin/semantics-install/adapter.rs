use super::{Candidate, HomeArgs, Result, fail, lifecycle};
use cell_install::adapter::{Context, Operation, reply};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::time::{Duration, Instant};

fn home(context: &Context) -> HomeArgs {
    HomeArgs {
        home: context.home.clone(),
        clockwork: None,
        launchctl: "/bin/launchctl".into(),
    }
}

fn candidate(context: &Context) -> Result<Candidate> {
    Ok(Candidate {
        binary: context.binary("semantics")?,
        bundle: context.request.source_root.join("semantics/chancery"),
        home: home(context),
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
    let paths = lifecycle::Paths::new(&home(context))?;
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
    let observed = lifecycle::inspect(&home(context))?;
    let prior = context.prior()?;
    if observed["installed"] != prior["installed"] || observed["current"] != prior["current"] {
        return fail("Semantics installation changed since inspection");
    }
    Ok(())
}

fn drain(context: &Context) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let status = maintenance(context, "status")?;
        if status["holds"] != json!([context.request.run_id]) {
            return fail("Semantics drain requires the sole run-owned hold");
        }
        if status["drained"] == true {
            break;
        }
        if Instant::now() >= deadline {
            return fail("Semantics commands did not drain; hold remains");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    for state in ["accepted", "running", "waiting-on-requester"] {
        let args: Vec<OsString> = [
            "--compact",
            "jobs",
            "list",
            "--requester",
            "semantics",
            "--state",
            state,
            "--limit",
            "1",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        let value = cell_install::command::json(
            &context.home.join(".local/bin/nucleus"),
            &args,
            &BTreeMap::new(),
            Duration::from_secs(60),
        )?;
        if value["version"] != 1 || !value["jobs"].as_array().is_some_and(Vec::is_empty) {
            return fail("Semantics durable requester work is not drained; hold remains");
        }
    }
    Ok(())
}

fn controls(context: &Context) -> Result<Value> {
    let paths = lifecycle::Paths::new(&home(context))?;
    let observed = lifecycle::inspect(&home(context))?;
    let prior = &context.prior()?["controls"][lifecycle::KEY];
    if prior.is_object() && observed["controls"][lifecycle::KEY]["enabled"] != prior["enabled"] {
        if prior["enabled"] != false {
            return fail("previously enabled Semantics worker is unexpectedly disabled");
        }
        paths.clock(&["binding", "disable", lifecycle::KEY])?;
    }
    Ok(lifecycle::inspect(&home(context))?["controls"].clone())
}

fn readiness(context: &Context) -> Result<()> {
    let paths = lifecycle::Paths::new(&home(context))?;
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
    let paths = lifecycle::Paths::new(&home(&context))?;
    no_installer_hold(&paths)?;
    let (status, data) = match operation {
        Operation::Inspect => {
            let mut installed = lifecycle::inspect(&home(&context))?;
            let maintenance = maintenance(&context, "status")?;
            if maintenance["holds"] != json!([])
                && maintenance["holds"] != json!([context.request.run_id])
            {
                return fail("another owner holds Semantics admission");
            }
            installed["maintenance_products"] = json!(["nucleus"]);
            installed["after"] = json!(["annals", "nucleus", "clockwork", "conversations"]);
            ("ready", installed)
        }
        Operation::Hold => {
            same_prior(&context)?;
            let held = maintenance(&context, "hold")?;
            if !held["holds"]
                .as_array()
                .is_some_and(|v| v.contains(&json!(context.request.run_id)))
            {
                return fail("Semantics did not retain the requested hold");
            }
            ("held", held)
        }
        Operation::Drain => {
            drain(&context)?;
            ("drained", json!({"drained":true}))
        }
        Operation::Apply => {
            same_prior(&context)?;
            drain(&context)?;
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
            controls(&context)?;
            ("applied", lifecycle::inspect(&home(&context))?)
        }
        Operation::Verify => {
            let args = candidate(&context)?;
            let installed = lifecycle::inspect(&home(&context))?;
            if context.selected()
                && installed["installed"]["current"]["release_id"]
                    != lifecycle::prepare(&args)?.info.release_id
            {
                return fail("Semantics installed artifacts differ from the candidate");
            }
            if !context.selected() {
                same_prior(&context)?;
            }
            controls(&context)?;
            readiness(&context)?;
            ("verified", installed)
        }
        Operation::Release => {
            controls(&context)?;
            ("released", maintenance(&context, "release")?)
        }
        Operation::Recover => {
            let observed = lifecycle::inspect(&home(&context))?;
            let prior = observed["installed"] == context.prior()?["installed"];
            let evidence = context
                .request
                .recovery
                .as_ref()
                .ok_or("recovery evidence is missing")?;
            if !prior {
                let prepared = lifecycle::prepare(&candidate(&context)?)?;
                if observed["installed"]["current"]["release_id"] != prepared.info.release_id {
                    return fail(
                        "Semantics recovery found neither recorded prior nor exact candidate",
                    );
                }
            }
            controls(&context)?;
            if !(prior && evidence["any_apply_started"] == false) {
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
                json!({"safe_to_release":true,"installed":if prior {"prior"} else {"candidate"}}),
            )
        }
    };
    Ok(reply(
        status,
        "Semantics product-owned operation completed",
        data,
    ))
}
