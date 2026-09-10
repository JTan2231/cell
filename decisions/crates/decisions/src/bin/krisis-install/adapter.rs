//! Fixed coordinator protocol with Krisis-owned admission and readiness.
use super::{
    Install, lifecycle, package,
    support::{
        ACTIVE, Paths, Pins, args, binding, binding_receipt, checked, doctor, exists,
        inspect_result, maintenance, require, switch,
    },
};
use cell_install::adapter::{Context, Operation, reply};
use cell_install::transaction::InstallSnapshot;
use cell_install::{Error, Result};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

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
        if let Some(value) = self
            .context
            .request
            .settings
            .as_ref()
            .and_then(|settings| settings.get("codex_bin"))
            .or_else(|| {
                self.context
                    .request
                    .dependency_settings
                    .get("nucleus")
                    .and_then(|settings| settings.get("codex_bin"))
            })
        {
            let path = value
                .as_str()
                .map(PathBuf::from)
                .ok_or_else(|| Error::new("Krisis codex_bin must be an absolute path"))?;
            require(path.is_absolute(), "Krisis codex_bin must be absolute")?;
            cell_install::file_digest(&path)?;
            return Ok(path);
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
            let active = binding(&self.paths, &self.clockwork, ACTIVE)?;
            require(
                !active.enabled && active.definition_digest.is_none(),
                "Krisis binding has no owned installation",
            )?;
            return Ok(if active.exists {
                json!({ACTIVE:active})
            } else {
                json!({})
            });
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
        if snapshot.current.is_some() {
            self.candidate_pins()?;
        } else {
            self.codex()?;
        }
        let mut result = inspect_result(snapshot.current.as_ref());
        result["selection"] = serde_json::to_value(&snapshot)?;
        result["controls"] = self.controls()?;
        result["dependencies"] = json!(["annals", "nucleus", "clockwork"]);
        result["maintenance_products"] = json!([]);
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
        {
            let value = self.status("status")?;
            require(
                value["holds"] == json!([self.context.request.run_id]),
                "drain requires this run's sole recorded admission hold",
            )?;
            if value["drained"] != true {
                return Ok(json!({"drained":false}));
            }
        }
        if !self.paths.home.join(".local/bin/nucleus").exists()
            && !self
                .paths
                .home
                .join("Library/Application Support/Nucleus/nucleus.db")
                .exists()
        {
            return Ok(json!({"drained":true}));
        }
        for requester in ["krisis", "decisions"] {
            let mut after: Option<nucleus_core::JobId> = None;
            loop {
                let mut arguments = args(&[
                    "--compact",
                    "jobs",
                    "list",
                    "--requester",
                    requester,
                    "--limit",
                    "1000",
                ]);
                if let Some(cursor) = &after {
                    arguments.extend(["--after".into(), cursor.to_string().into()]);
                }
                let bytes = checked(
                    &self.paths,
                    &self.paths.home.join(".local/bin/nucleus"),
                    &arguments,
                    &BTreeMap::new(),
                    60,
                )?;
                let page: nucleus_core::ListJobsResponseV1 = serde_json::from_slice(&bytes)?;
                require(page.version == 1, "unsupported Krisis Nucleus job response")?;
                if page.jobs.iter().any(|job| !job.state.is_terminal()) {
                    return Ok(json!({"drained":false}));
                }
                let Some(next) = page.next else {
                    break;
                };
                require(
                    after.as_ref() != Some(&next),
                    "Krisis Nucleus job cursor did not advance",
                )?;
                after = Some(next);
            }
        }
        Ok(json!({"drained":true}))
    }
    fn restore_controls(&self, activate: bool) -> Result<Value> {
        let observed = self.controls()?;
        for (key, actual) in observed
            .as_object()
            .ok_or_else(|| Error::new("invalid Krisis controls"))?
        {
            let prior = &self.context.prior()?["controls"][key];
            let enabled = activate
                && self
                    .context
                    .activation_enabled(prior["enabled"].as_bool().unwrap_or(true))?;
            if let Some(digest) = actual["definition_digest"].as_str() {
                switch(&self.paths, &self.clockwork, key, digest, enabled)?;
            }
        }
        self.controls()
    }
    fn require_disabled(&self) -> Result<Value> {
        let value = self.controls()?;
        require(
            value.as_object().is_some_and(|controls| {
                controls.values().all(|control| control["enabled"] != true)
            }),
            "Krisis observer activated before final activation",
        )?;
        Ok(value)
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
        let selected = self.context.selected();
        let info = self
            .snapshot()?
            .current
            .ok_or_else(|| Error::new("installed Krisis is absent"))?;
        if selected {
            package::matches_candidate(&self.paths, &info, &self.options()?)?;
        } else {
            self.check_prior()?;
        }
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
        result["controls"] = self.require_disabled()?;
        self.readiness(selected)?;
        Ok(result)
    }
    fn recover(&self) -> Result<Value> {
        lifecycle::recover_lock(&self.paths)?;
        let recovered_candidate = if lifecycle::no_unfinished_transaction(&self.paths).is_err() {
            lifecycle::recover_owned(&self.options()?, &self.context.request.run_id)?
        } else {
            None
        };
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
        self.restore_controls(false)?;
        let recovery = self
            .context
            .request
            .recovery
            .as_ref()
            .unwrap_or(&Value::Null);
        let candidate = recovered_candidate.unwrap_or(
            !prior || recovery["configured"] == true || recovery["installed"] == "candidate",
        );
        if prior && recovery["configure_started"] != true {
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
        Ok(json!({"safe_to_release":true,"installed":if candidate {"candidate"}else{"prior"}}))
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep the fixed protocol phases beside their admission and recovery evidence"
)]
pub fn execute(operation: Operation) -> Result<Value> {
    let context = Context::read(
        "krisis",
        "decisions",
        "krisis-install",
        env!("CARGO_PKG_VERSION"),
    )?;
    context.validate_settings(&["codex_bin"], &["enabled"])?;
    let mut paths = Paths::new(context.home.clone())?;
    paths.deployment_run_id = Some(context.request.run_id.clone().into());
    let adapter = Adapter {
        clockwork: context.dependency_binary("clockwork")?,
        context,
        paths,
    };
    let (status, detail, data) = match operation {
        Operation::Apply => {
            adapter.check_prior()?;
            let prepared = package::stage(
                &adapter.paths,
                &adapter.context.binary("krisis")?,
                &adapter.context.request.source_root.join("decisions"),
            )?;
            (
                "applied",
                "Krisis immutable release staged; configuration and publication pending",
                json!({"staged":prepared.info}),
            )
        }
        Operation::Activate => {
            require(
                adapter.status("status")?["holds"] == json!([]),
                "Krisis activation requires released admission",
            )?;
            (
                "activated",
                "Krisis captured scheduling intent restored",
                adapter.restore_controls(true)?,
            )
        }
        Operation::Inspect => (
            "ready",
            "Krisis installation and admission inspected",
            adapter.inspect()?,
        ),
        Operation::Hold => {
            let status = adapter.status("hold")?;
            adapter.restore_controls(false)?;
            (
                "held",
                "run-owned Krisis admission hold persisted",
                json!({"targets":[status]}),
            )
        }
        Operation::Drain => {
            let data = adapter.drain()?;
            (
                if data["drained"] == true {
                    "drained"
                } else {
                    "waiting"
                },
                "Krisis admission and durable requester work",
                data,
            )
        }
        Operation::Configure => {
            if !adapter.context.selected() {
                adapter.check_prior()?;
                adapter.readiness(false)?;
                return Ok(reply(
                    "configured",
                    "Krisis retained configuration verified",
                    json!({}),
                ));
            }
            adapter.clean()?;
            adapter.check_prior()?;
            require(
                adapter.drain()?["drained"] == true,
                "Krisis apply requires settled durable work",
            )?;
            let mut options = adapter.options()?;
            options.final_cutover = true;
            lifecycle::install(&options, Some(&adapter.context.request.run_id))?;
            adapter.restore_controls(false)?;
            (
                "configured",
                "Krisis pins configured and candidate published with scheduling disabled",
                inspect_result(adapter.snapshot()?.current.as_ref()),
            )
        }
        Operation::Verify => (
            "verified",
            "Krisis installation, controls and readiness verified",
            adapter.verify()?,
        ),
        Operation::Release => {
            adapter.clean()?;
            adapter.require_disabled()?;
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
