//! Fixed coordinator protocol with Krisis-owned admission and readiness.
use super::{
    Install, lifecycle, package,
    support::{
        ACTIVE, Paths, Pins, args, binding, binding_receipt, checked, disable, doctor, exists,
        inspect_result, maintenance, require,
    },
};
use cell_install::adapter::{Context, Operation, reply};
use cell_install::transaction::InstallSnapshot;
use cell_install::{Error, Result};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Adapter {
    context: Context,
    paths: Paths,
    clockwork: PathBuf,
}

impl Adapter {
    fn snapshot(&self) -> Result<InstallSnapshot> {
        package::inspect(&self.paths)
    }
    fn payload(&self) -> Result<PathBuf> {
        self.snapshot()?.current.as_ref().map_or_else(
            || self.context.binary("krisis"),
            |info| Ok(package::root(&self.paths, info).join("libexec/krisis")),
        )
    }
    fn clean(&self) -> Result<()> {
        lifecycle::no_unfinished_transaction(&self.paths)?;
        require(
            !exists(&self.paths.gate)? && !exists(&self.paths.install.join(".update-lock"))?,
            "Krisis installer retains maintenance or an unfinished transaction",
        )
    }
    fn status(&self, operation: &str) -> Result<Value> {
        maintenance(
            &self.paths,
            &self.payload()?,
            operation,
            (operation != "status").then_some(self.context.request.run_id.as_str()),
        )
    }
    fn owned_status(&self) -> Result<Value> {
        let value = self.status("status")?;
        require(
            value["holds"] == json!([self.context.request.run_id]) && value["drained"] == true,
            "Krisis requires this run's sole drained admission hold",
        )?;
        Ok(value)
    }
    fn codex(&self) -> Result<PathBuf> {
        if let Some(info) = &self.snapshot()?.current {
            return Ok(lifecycle::pins_from_receipt(&self.paths, &self.clockwork, info)?.codex);
        }
        for path in [
            PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex"),
            PathBuf::from("/opt/homebrew/bin/codex"),
            self.paths.home.join(".local/bin/codex"),
        ] {
            if exists(&path)? {
                cell_install::file_digest(&path)?;
                return Ok(path);
            }
        }
        Err(Error::new(
            "Conversations requires its configured Codex executable",
        ))
    }
    fn candidate_pins(&self) -> Result<Pins> {
        let config = self
            .paths
            .home
            .join("Library/Application Support/Annals/decisions/config.toml");
        cell_install::file_digest(&config)?;
        let parsed: toml::Value = toml::from_str(&std::fs::read_to_string(&config)?)
            .map_err(|_| Error::new("Annals decisions configuration is invalid"))?;
        let library = parsed
            .get("decision_feed")
            .and_then(|v| v.get("expected_library_id"))
            .and_then(toml::Value::as_str)
            .ok_or_else(|| Error::new("Annals decisions-library identity is absent"))?;
        let pins = Pins {
            annals_binary: std::fs::canonicalize(
                self.paths
                    .home
                    .join("Library/Application Support/Annals/install/current/libexec/annals"),
            )?,
            annals_config: config,
            annals_library_id: library.into(),
            codex: self.codex()?,
        };
        pins.validate()?;
        if let Some(prior) = &self.context.request.prior
            && let Some(expected) = prior.get("annals_library_id")
        {
            require(
                expected == &Value::String(pins.annals_library_id.clone()),
                "ordinary deployment cannot replace the Annals library",
            )?;
        }
        Ok(pins)
    }
    fn options(&self) -> Result<Install> {
        let pins = self.candidate_pins()?;
        Ok(Install {
            binary: self.context.binary("krisis")?,
            source_root: Some(self.context.request.source_root.join("decisions")),
            home: Some(self.paths.home.clone()),
            codex: pins.codex,
            annals: pins.annals_binary,
            annals_config: pins.annals_config,
            annals_library_id: pins.annals_library_id,
            clockwork: std::fs::canonicalize(&self.clockwork)?,
            launchctl: "/bin/launchctl".into(),
            final_cutover: false,
            keep_maintenance: false,
            release_maintenance: false,
            expected_current: None,
        })
    }
    fn controls(&self) -> Result<Value> {
        let Some(current) = &self.snapshot()?.current else {
            return Ok(json!({}));
        };
        lifecycle::pins_from_receipt(&self.paths, &self.clockwork, current)?;
        let owned = binding_receipt(&self.paths)?;
        let active = binding(&self.paths, &self.clockwork, ACTIVE)?;
        require(
            active.definition_digest.as_deref() == Some(&owned["definition_digest"]),
            "observer selection differs from ownership receipt",
        )?;
        Ok(json!({ACTIVE:{"enabled":active.enabled,"definition_digest":active.definition_digest}}))
    }
    fn inspect(&self) -> Result<Value> {
        self.clean()?;
        let snapshot = self.snapshot()?;
        if let Some(current) = &snapshot.current {
            require(
                matches!(
                    current.format.as_str(),
                    "legacy-4" | cell_install::transaction::TRANSACTION_FORMAT
                ),
                "legacy Decisions requires the documented direct migration",
            )?;
        }
        let status = self.status("status")?;
        require(
            status["holds"] == json!([]) || status["holds"] == json!([self.context.request.run_id]),
            "another operation owns admission maintenance",
        )?;
        self.candidate_pins()?;
        let mut result = inspect_result(snapshot.current.as_ref());
        result["selection"] = serde_json::to_value(&snapshot)?;
        result["controls"] = self.controls()?;
        result["dependencies"] = json!(["annals", "nucleus", "clockwork", "conversations"]);
        result["maintenance_products"] = json!(["nucleus"]);
        if let Some(current) = &snapshot.current {
            result["annals_library_id"] = json!(
                lifecycle::pins_from_receipt(&self.paths, &self.clockwork, current)?
                    .annals_library_id
            );
        }
        Ok(result)
    }
    fn check_prior(&self) -> Result<()> {
        let prior = self.context.prior()?;
        let expected: InstallSnapshot = serde_json::from_value(prior["selection"].clone())?;
        require(
            self.snapshot()? == expected,
            "installed release or selectors changed since inspection",
        )
    }
    fn drain(&self) -> Result<Value> {
        let start = Instant::now();
        loop {
            let value = self.status("status")?;
            require(
                value["holds"] == json!([self.context.request.run_id]),
                "drain requires this run's sole recorded admission hold",
            )?;
            if value["drained"] == true {
                break;
            }
            require(
                start.elapsed() < Duration::from_secs(45),
                "admitted Krisis commands have not drained; holds remain",
            )?;
            std::thread::sleep(Duration::from_millis(200));
        }
        for requester in ["krisis", "decisions"] {
            for state in ["accepted", "running", "waiting-on-requester"] {
                let bytes = checked(
                    &self.paths,
                    &self.paths.home.join(".local/bin/nucleus"),
                    &args(&[
                        "--compact",
                        "jobs",
                        "list",
                        "--requester",
                        requester,
                        "--state",
                        state,
                        "--limit",
                        "1",
                    ]),
                    &BTreeMap::new(),
                    60,
                )?;
                let value: Value = serde_json::from_slice(&bytes)?;
                require(
                    value["version"] == 1 && value["jobs"] == json!([]),
                    "durable Krisis requester work has not drained; holds remain",
                )?;
            }
        }
        Ok(json!({"drained":true}))
    }
    fn restore_controls(&self) -> Result<Value> {
        let observed = self.controls()?;
        if let Some(controls) = self
            .context
            .prior()?
            .get("controls")
            .and_then(Value::as_object)
        {
            for (key, prior) in controls {
                let before = prior["enabled"]
                    .as_bool()
                    .ok_or_else(|| Error::new("invalid captured schedule state"))?;
                let actual = observed[key]["enabled"]
                    .as_bool()
                    .ok_or_else(|| Error::new("owned schedule disappeared during deployment"))?;
                if before != actual {
                    require(
                        !before,
                        "previously enabled observer is unexpectedly disabled",
                    )?;
                    let selected = binding(&self.paths, &self.clockwork, key)?;
                    disable(&self.paths, &self.clockwork, key, &selected)?;
                }
            }
        }
        self.controls()
    }
    fn readiness(&self, candidate: bool) -> Result<()> {
        let info = self
            .snapshot()?
            .current
            .ok_or_else(|| Error::new("Krisis is absent"))?;
        let pins = lifecycle::pins_from_receipt(&self.paths, &self.clockwork, &info)?;
        if candidate {
            require(
                pins == self.candidate_pins()?,
                "candidate did not adopt exact Annals and Codex pins",
            )?;
        }
        doctor(
            &self.paths,
            &package::root(&self.paths, &info).join("libexec/krisis"),
            &pins,
        )?;
        Ok(())
    }
    fn verify(&self) -> Result<Value> {
        self.clean()?;
        let info = self
            .snapshot()?
            .current
            .ok_or_else(|| Error::new("installed Krisis is absent"))?;
        package::matches_candidate(&self.paths, &info, &self.options()?)?;
        for flag in ["--version", "--help"] {
            checked(
                &self.paths,
                &self.payload()?,
                &args(&[flag]),
                &BTreeMap::new(),
                30,
            )?;
        }
        let mut result = inspect_result(Some(&info));
        result["controls"] = self.restore_controls()?;
        self.readiness(true)?;
        Ok(result)
    }
    fn recover(&self) -> Result<Value> {
        self.clean()?;
        let observed = inspect_result(self.snapshot()?.current.as_ref());
        let prior = observed["current"] == self.context.prior()?["current"];
        if prior {
            self.check_prior()?;
        } else {
            let info = self
                .snapshot()?
                .current
                .ok_or_else(|| Error::new("candidate installation is absent"))?;
            package::matches_candidate(&self.paths, &info, &self.options()?)?;
        }
        self.restore_controls()?;
        let recovery = self
            .context
            .request
            .recovery
            .as_ref()
            .unwrap_or(&Value::Null);
        if prior && recovery["any_apply_started"] == false {
            return Ok(json!({"safe_to_release":true,"installed":"prior"}));
        }
        let status = self.status("status")?;
        let released = recovery["verified"] == true
            && (status["holds"] == json!([])
                || status["holds"] == json!([self.context.request.run_id]));
        if !released {
            self.owned_status()?;
        }
        self.readiness(!prior)?;
        Ok(json!({"safe_to_release":true,"installed":if prior {"prior"}else{"candidate"}}))
    }
}

pub fn execute(operation: Operation) -> Result<Value> {
    let context = Context::read(
        "krisis",
        "decisions",
        "krisis-install",
        env!("CARGO_PKG_VERSION"),
    )?;
    let paths = Paths::new(context.home.clone())?;
    let adapter = Adapter {
        clockwork: paths.home.join(".local/bin/clockwork"),
        context,
        paths,
    };
    let (status, detail, data) = match operation {
        Operation::Inspect => (
            "ready",
            "Krisis installation and admission inspected",
            adapter.inspect()?,
        ),
        Operation::Hold => (
            "held",
            "run-owned Krisis admission hold persisted",
            json!({"targets":[adapter.status("hold")?]}),
        ),
        Operation::Drain => (
            "drained",
            "Krisis admission and durable requester work drained",
            adapter.drain()?,
        ),
        Operation::Apply => {
            adapter.clean()?;
            adapter.check_prior()?;
            adapter.drain()?;
            let mut options = adapter.options()?;
            lifecycle::install(&options)?;
            options.final_cutover = true;
            lifecycle::install(&options)?;
            adapter.restore_controls()?;
            (
                "applied",
                "Krisis installation and operator controls restored",
                inspect_result(adapter.snapshot()?.current.as_ref()),
            )
        }
        Operation::Verify => (
            "verified",
            "Krisis candidate, controls and readiness verified",
            adapter.verify()?,
        ),
        Operation::Release => {
            adapter.clean()?;
            adapter.restore_controls()?;
            (
                "released",
                "only this run's admission hold released",
                json!({"targets":[adapter.status("release")?]}),
            )
        }
        Operation::Recover => (
            "recovered",
            "coherent Krisis generation verified without inferred database rollback",
            adapter.recover()?,
        ),
    };
    Ok(reply(status, detail, data))
}
