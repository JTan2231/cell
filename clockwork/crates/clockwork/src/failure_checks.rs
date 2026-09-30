//! Pending scheduling failures share the alert policy without closing admission.
use std::collections::BTreeMap;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;

use clockwork::api::NotificationCheck;
use serde::{Deserialize, Serialize};

use crate::error::{Context as _, Error, Result};
use crate::notification_checks::{Checks, condition, read_report};
use crate::paths::Layout;
use crate::store::Store;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FailureEvent {
    pub cursor: i64,
    pub key: String,
    pub code: String,
    pub occurrence: String,
    pub activation_id: Option<String>,
    pub recorded_at: i64,
    pub definition_digest: Option<String>,
    pub email_cli: String,
    pub delayed: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Episode {
    event: FailureEvent,
    check: NotificationCheck,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FailureChecks {
    version: u32,
    cursor: i64,
    pub(crate) pending: BTreeMap<String, Episode>,
}

impl FailureChecks {
    pub(crate) fn load(layout: &Layout) -> Result<Self> {
        match std::fs::read(layout.state_root().join("failure-checks.json")) {
            Ok(bytes) => {
                let state: Self = serde_json::from_slice(&bytes)
                    .context("failure_checks_invalid", "read pending failure checks")?;
                if state.version != 1
                    || state.cursor < 0
                    || state.pending.iter().any(|(key, episode)| {
                        key != &episode.event.key
                            || episode.event.cursor > state.cursor
                            || !episode.event.delayed
                            || episode.check.failure_threshold == 0
                    })
                {
                    return Err(Error::new(
                        "failure_checks_invalid",
                        "invalid pending failure state",
                    ));
                }
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                version: 1,
                cursor: 0,
                pending: BTreeMap::new(),
            }),
            Err(error) => Err(Error::new("failure_checks_unavailable", error.to_string())),
        }
    }

    pub(crate) fn refresh(&mut self, store: &Store, checks: &Checks) -> Result<()> {
        for event in store.failure_events(self.cursor)? {
            self.cursor = event.cursor;
            if event.delayed && store.active_incident(&event.key)?.is_none() {
                self.pending
                    .entry(event.key.clone())
                    .or_insert_with(|| Episode {
                        event,
                        check: NotificationCheck {
                            failure_threshold: checks.failure_threshold,
                            consecutive_failures: 0,
                            last_checked_at: None,
                            condition: "unchecked".into(),
                            eligible_at: None,
                        },
                    });
            }
        }
        Ok(())
    }

    pub(crate) fn save(&self, layout: &Layout) -> Result<()> {
        let path = layout.state_root().join("failure-checks.json");
        let temporary = layout
            .state_root()
            .join(format!(".failure-checks-{}", uuid::Uuid::now_v7()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .context("failure_checks_write_failed", "prepare failure checks")?;
        file.write_all(
            &serde_json::to_vec(self).context("failure_checks_invalid", "encode failure checks")?,
        )
        .context("failure_checks_write_failed", "write failure checks")?;
        file.sync_all()
            .context("failure_checks_write_failed", "sync failure checks")?;
        std::fs::rename(&temporary, path)
            .context("failure_checks_write_failed", "commit failure checks")?;
        std::fs::File::open(layout.state_root())
            .and_then(|file| file.sync_all())
            .context(
                "failure_checks_write_failed",
                "sync failure check directory",
            )
    }

    pub(crate) async fn observe(
        &mut self,
        store: &mut Store,
        layout: &Layout,
        checks: &mut Checks,
        now: i64,
    ) -> Result<()> {
        self.refresh(store, checks)?;
        let mut due = Vec::new();
        for (key, episode) in &self.pending {
            if episode
                .check
                .last_checked_at
                .is_none_or(|at| now >= at.saturating_add(i64::from(checks.interval_seconds)))
                || store
                    .confirmed_occurrence(key, &episode.event.occurrence)?
                    .is_some()
            {
                due.push(key.clone());
            }
        }
        let report = if due.is_empty() {
            None
        } else {
            read_report(layout, &checks.cell_root).await
        };
        for key in due {
            let Some(episode) = self.pending.get_mut(&key) else {
                continue;
            };
            // A committed confirmation can precede sidecar publication or approval.
            let confirmed = store.confirmed_occurrence(&key, &episode.event.occurrence)?;
            if let Some(incident) = confirmed {
                if incident.resumed_at.is_none() {
                    episode.check.consecutive_failures = episode
                        .check
                        .consecutive_failures
                        .max(checks.failure_threshold);
                    episode.check.eligible_at.get_or_insert(incident.created_at);
                    checks.seed(&incident, &episode.check);
                }
                self.pending.remove(&key);
                continue;
            }
            let binding = store.binding(&key)?;
            if binding.halted_incident.is_some() {
                self.pending.remove(&key);
                continue;
            }
            let selected = binding
                .definition_digest
                .as_deref()
                .map(|digest| store.definition(digest))
                .transpose()?;
            let state = if !binding.enabled
                || selected.as_ref().is_none_or(|selected| {
                    selected.manifest.schema_version < 2
                        || selected.manifest.failure.on_abend
                            != clockwork::api::AbendPolicy::HaltUntilApproved
                }) {
                "inactive"
            } else {
                condition(report.as_ref(), &key, Some(&episode.event), store)?
            };
            episode.check.failure_threshold = checks.failure_threshold;
            episode.check.last_checked_at = Some(now);
            episode.check.condition = state.into();
            if matches!(state, "healthy" | "inactive") {
                self.pending.remove(&key);
                continue;
            }
            episode.check.consecutive_failures =
                episode.check.consecutive_failures.saturating_add(1);
            if episode.check.consecutive_failures >= checks.failure_threshold {
                episode.check.eligible_at = Some(now);
                let incident = store.confirm_failure(&episode.event, now)?;
                checks.seed(&incident, &episode.check);
                self.pending.remove(&key);
            }
        }
        // The halt is durable before eligibility, and eligibility before clearing
        // the pending episode. Recovery can repeat either publication safely.
        checks.save(layout)?;
        self.save(layout)
    }
}

pub(crate) fn pending_keys(store: &Store, layout: &Layout) -> Result<Vec<String>> {
    let mut state = FailureChecks::load(layout)?;
    state.refresh(store, &Checks::load(layout)?)?;
    Ok(state.pending.keys().cloned().collect())
}
