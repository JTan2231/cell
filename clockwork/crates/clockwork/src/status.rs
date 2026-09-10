use iatreion_api::{
    Activity, Admission, AdmissionState, Count, Evidence, InspectionReference, Intent,
    OperationalUnit, Outcome, OutcomeKind, Readiness, ReadinessState, Reason, SchedulerObservation,
    StatusSnapshot,
};

use crate::error::Result;
use crate::model::ActivationState;
use crate::paths::Layout;
use crate::store::Store;

#[allow(clippy::too_many_lines)]
pub(crate) fn snapshot(layout: &Layout) -> Result<StatusSnapshot> {
    let started = iatreion_api::now_unix_seconds();
    let store = Store::open_read_only(layout)?;
    let bindings = store.bindings()?;
    let history = store.history(None, 10_000)?;
    let running = history
        .iter()
        .filter(|activation| activation.state == ActivationState::Running)
        .count();
    let latest = history.first().cloned();
    let halted = bindings
        .iter()
        .filter(|binding| binding.halted_incident.is_some())
        .collect::<Vec<_>>();
    let mut reasons = Vec::new();
    let mut inspection = vec![InspectionReference {
        capability_id: "clockwork.schedule.operate".to_owned(),
        record_id: None,
    }];
    for binding in &halted {
        let Some(incident_id) = binding.halted_incident.as_deref() else {
            continue;
        };
        let incident = store.incident(incident_id)?;
        reasons.push(Reason {
            code: "failure_halted".to_owned(),
            summary: format!(
                "{} is halted by incident {} ({})",
                binding.key, incident.id, incident.code
            ),
        });
        inspection.push(InspectionReference {
            capability_id: "clockwork.schedule.operate".to_owned(),
            record_id: Some(incident.id),
        });
    }
    let enabled = bindings.iter().filter(|binding| binding.enabled).count();
    let evidence = Evidence {
        counts: vec![
            count(
                "bindings",
                bindings.len(),
                "binding",
                "all retained Clockwork bindings",
            ),
            count(
                "enabled",
                enabled,
                "binding",
                "bindings configured for launchd admission",
            ),
            count(
                "failure_halted",
                halted.len(),
                "binding",
                "bindings with one current open incident",
            ),
            count(
                "recorded_running",
                running,
                "activation",
                "activation rows recorded as running without process verification",
            ),
        ],
        latest_runtime_outcome: latest.map(runtime_outcome),
        latest_domain_outcome: None,
        latest_domain_success: None,
    };
    let finished = iatreion_api::now_unix_seconds();
    let scheduler_observations = bindings
        .iter()
        .map(|binding| {
            let latest = history
                .iter()
                .find(|activation| activation.key == binding.key)
                .cloned();
            SchedulerObservation {
                key: binding.key.clone(),
                enabled: binding.enabled,
                failure_halted: binding.halted_incident.is_some(),
                recorded_running: history.iter().any(|activation| {
                    activation.key == binding.key && activation.state == ActivationState::Running
                }),
                incident_id: binding.halted_incident.clone(),
                latest_runtime_outcome: latest.map(runtime_outcome),
            }
        })
        .collect();
    Ok(StatusSnapshot {
        schema_version: iatreion_api::STATUS_SCHEMA_VERSION,
        product_id: "clockwork".to_owned(),
        provider_release: env!("CARGO_PKG_VERSION").to_owned(),
        observed_at_start: started,
        observed_at_end: finished,
        complete: true,
        units: vec![OperationalUnit {
            id: "clockwork/schedules".to_owned(),
            owning_product_id: "clockwork".to_owned(),
            clockwork_key: None,
            intent: Intent::Active,
            admission: Admission {
                state: AdmissionState::Open,
                reasons: Vec::new(),
            },
            activity: if running == 0 {
                Activity::Idle
            } else {
                Activity::Running
            },
            readiness: Readiness {
                state: if halted.is_empty() {
                    ReadinessState::Ready
                } else {
                    ReadinessState::Degraded
                },
                scope: "Clockwork database, bindings, and retained activation records".to_owned(),
                reasons,
            },
            evidence,
            inspection,
        }],
        scheduler_observations,
        diagnostics: if running == 0 {
            Vec::new()
        } else {
            vec![iatreion_api::Diagnostic {
                code: "recorded_activity_unverified".to_owned(),
                summary: "status does not repair or verify recorded running process identities"
                    .to_owned(),
            }]
        },
    })
}

fn count(name: &str, value: usize, unit: &str, scope: &str) -> Count {
    Count {
        name: name.to_owned(),
        value: u64::try_from(value).unwrap_or(u64::MAX),
        unit: unit.to_owned(),
        scope: scope.to_owned(),
    }
}

fn runtime_outcome(activation: crate::model::ActivationRecord) -> Outcome {
    let kind = match (activation.state, activation.exit_code) {
        (ActivationState::Exited, Some(0)) => OutcomeKind::Succeeded,
        (ActivationState::Running, _) => OutcomeKind::Uncertain,
        _ => OutcomeKind::Failed,
    };
    Outcome {
        kind,
        occurred_at: activation.finished_at.unwrap_or(activation.admitted_at),
        reference: Some(activation.id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_status_database_stays_missing() {
        let directory = tempfile::tempdir().unwrap();
        let layout = Layout::discover(Some(directory.path().to_path_buf())).unwrap();
        let before = std::fs::read_dir(directory.path()).unwrap().count();
        assert!(snapshot(&layout).is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), before);
    }
}
