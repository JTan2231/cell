//! Read-only service observations gate initial alerts, never product admission.
use std::collections::BTreeMap;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use clockwork::api::{IncidentRecord, NotificationCheck};
use iatreion_api::{Activity, AdmissionState, Intent, ProbeState, ReadinessState, Report};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt as _;

use crate::error::{Context as _, Error, Result};
use crate::paths::Layout;
use crate::store::Store;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checks {
    version: u32,
    pub(crate) failure_threshold: u32,
    pub(crate) interval_seconds: u32,
    pub(crate) cell_root: PathBuf,
    incidents: BTreeMap<String, NotificationCheck>,
}

impl Checks {
    pub(crate) fn load(layout: &Layout) -> Result<Self> {
        match std::fs::read(layout.state_root().join("notification-checks.json")) {
            Ok(bytes) => {
                let checks: Self = serde_json::from_slice(&bytes)
                    .context("notification_checks_invalid", "read notification checks")?;
                if checks.version != 1
                    || checks.failure_threshold == 0
                    || checks.interval_seconds == 0
                    || !checks.cell_root.is_absolute()
                {
                    return Err(Error::new(
                        "notification_checks_invalid",
                        "invalid notification check policy",
                    ));
                }
                Ok(checks)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                version: 1,
                failure_threshold: 5,
                interval_seconds: 60,
                cell_root: layout.home().join("rust/cell"),
                incidents: BTreeMap::new(),
            }),
            Err(error) => Err(Error::new(
                "notification_checks_unavailable",
                error.to_string(),
            )),
        }
    }

    pub(crate) fn save(&self, layout: &Layout) -> Result<()> {
        let temporary = layout
            .state_root()
            .join(format!(".notification-checks-{}", uuid::Uuid::now_v7()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .context(
                "notification_checks_write_failed",
                "prepare notification checks",
            )?;
        file.write_all(
            &serde_json::to_vec(self)
                .context("notification_checks_invalid", "encode notification checks")?,
        )
        .context(
            "notification_checks_write_failed",
            "write notification checks",
        )?;
        file.sync_all().context(
            "notification_checks_write_failed",
            "sync notification checks",
        )?;
        std::fs::rename(
            &temporary,
            layout.state_root().join("notification-checks.json"),
        )
        .context(
            "notification_checks_write_failed",
            "commit notification checks",
        )?;
        std::fs::File::open(layout.state_root())
            .and_then(|file| file.sync_all())
            .context(
                "notification_checks_write_failed",
                "sync notification check directory",
            )
    }

    pub(crate) fn view(&self, incident: &IncidentRecord) -> NotificationCheck {
        self.incidents
            .get(&incident.id)
            .cloned()
            .unwrap_or(NotificationCheck {
                failure_threshold: self.failure_threshold,
                consecutive_failures: 0,
                last_checked_at: None,
                condition: "unchecked".into(),
                eligible_at: None,
            })
    }

    pub(crate) fn due(&self, incident: &IncidentRecord, now: i64) -> bool {
        let check = self.view(incident);
        check
            .last_checked_at
            .is_none_or(|at| now >= at.saturating_add(i64::from(self.interval_seconds)))
    }

    pub(crate) fn record(&mut self, incident: &IncidentRecord, condition: &str, now: i64) {
        let mut check = self.view(incident);
        check.failure_threshold = self.failure_threshold;
        check.last_checked_at = Some(now);
        check.condition = condition.into();
        if matches!(condition, "healthy" | "inactive") {
            check.consecutive_failures = 0;
            check.eligible_at = None;
        } else {
            check.consecutive_failures = check.consecutive_failures.saturating_add(1);
            if check.consecutive_failures >= check.failure_threshold {
                check.eligible_at.get_or_insert(now);
            }
        }
        self.incidents.insert(incident.id.clone(), check);
    }

    pub(crate) async fn observe(
        &mut self,
        store: &Store,
        layout: &Layout,
        incidents: &[IncidentRecord],
        now: i64,
    ) -> Result<()> {
        let due = incidents
            .iter()
            .filter(|incident| self.due(incident, now))
            .collect::<Vec<_>>();
        if due.is_empty() {
            return Ok(());
        }
        let report = read_report(layout, &self.cell_root).await;
        for incident in due {
            let binding = store.binding(&incident.key)?;
            let condition = if incident.resumed_at.is_some()
                || binding.halted_incident.as_deref() != Some(&incident.id)
            {
                "healthy"
            } else if !binding.enabled {
                "inactive"
            } else {
                condition(report.as_ref(), &incident.key)
            };
            self.record(incident, condition, now);
        }
        self.save(layout)
    }
}

fn condition(report: Option<&Report>, key: &str) -> &'static str {
    let Some((product, unit)) = report.and_then(|report| {
        report.products.iter().find_map(|product| {
            product
                .units
                .iter()
                .find(|unit| unit.observation.clockwork_key.as_deref() == Some(key))
                .map(|unit| (product, &unit.observation))
        })
    }) else {
        return "unknown";
    };
    if matches!(unit.intent, Intent::Disabled | Intent::Retired)
        || unit.admission.reasons.iter().any(|reason| {
            reason.code.starts_with("operator_") || reason.code.starts_with("maintenance")
        })
    {
        return "inactive";
    }
    if product.probe_state != ProbeState::Observed {
        return "unknown";
    }
    if matches!(
        unit.readiness.state,
        ReadinessState::Blocked | ReadinessState::Degraded
    ) || unit
        .admission
        .reasons
        .iter()
        .any(|reason| reason.code == "failure_halted")
        || unit.activity == Activity::Stopped
    {
        return "unhealthy";
    }
    if unit.readiness.state == ReadinessState::Ready && unit.admission.state == AdmissionState::Open
    {
        return "healthy";
    }
    "unknown"
}

async fn read_report(layout: &Layout, root: &std::path::Path) -> Option<Report> {
    let mut child = tokio::process::Command::new(layout.status_report_cli())
        .args(["report"])
        .arg(root)
        .arg("--json")
        .env("CHANCERY_USAGE_INTERNAL", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        let mut bytes = Vec::new();
        stdout
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await
            .ok()?;
        if bytes.len() > 2 * 1024 * 1024 || !child.wait().await.ok()?.success() {
            return None;
        }
        let report: Report = serde_json::from_slice(&bytes).ok()?;
        let now = crate::store::now_unix().ok()?;
        (report.schema_version == iatreion_api::REPORT_SCHEMA_VERSION
            && report.observed_at_end >= now.saturating_sub(30)
            && report.observed_at_start <= report.observed_at_end
            && report.observed_at_end <= now.saturating_add(5))
        .then_some(report)
    })
    .await
    .ok()
    .flatten();
    if result.is_none() {
        let _ = child.kill().await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn five_consecutive_checks_reset_persist_and_release_only_once() {
        let directory = tempfile::tempdir().unwrap();
        let layout = Layout::isolated(directory.path());
        let mut store = Store::open(&layout).unwrap();
        let incident = store
            .import_halt("example/worker", "failed", "job/one")
            .unwrap();
        let mut checks = Checks::load(&layout).unwrap();
        for at in [60, 120, 180] {
            checks.record(&incident, "unhealthy", at);
        }
        assert_eq!(checks.view(&incident).consecutive_failures, 3);
        assert!(!checks.due(&incident, 181));
        checks.record(&incident, "healthy", 240);
        assert_eq!(checks.view(&incident).consecutive_failures, 0);
        for at in [300, 360, 420, 480] {
            checks.record(&incident, "unhealthy", at);
        }
        checks.save(&layout).unwrap();
        let mut checks = Checks::load(&layout).unwrap();
        assert_eq!(checks.view(&incident).eligible_at, None);
        checks.record(&incident, "unhealthy", 540);
        assert_eq!(checks.view(&incident).eligible_at, Some(540));
        checks.record(&incident, "unhealthy", 600);
        assert_eq!(checks.view(&incident).eligible_at, Some(540));
        checks.record(&incident, "inactive", 660);
        assert_eq!(checks.view(&incident).consecutive_failures, 0);
        assert_eq!(checks.view(&incident).eligible_at, None);
        assert!(store.require_unhalted("example/worker").is_err());
    }
}
