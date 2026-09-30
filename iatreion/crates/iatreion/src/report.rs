use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iatreion_api::{
    Activity, AdmissionState, Count, InspectionReference, ProbeState, REPORT_SCHEMA_VERSION,
    ReadinessState, Reason, Report, SchedulerObservation, now_unix_seconds,
};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use usher::api::inspect_operations;

use crate::probe;

#[derive(Debug, Clone)]
pub struct CollectionOptions {
    pub root: PathBuf,
    pub command_dir: PathBuf,
    pub product: Option<String>,
    pub unit: Option<String>,
    pub total_timeout: Duration,
    pub probe_timeout: Duration,
    pub concurrency: usize,
}

pub struct Collection {
    pub report: Report,
    pub selected_unit_found: bool,
}

/// Collect one bounded, in-memory operational report.
///
/// # Errors
/// Returns a diagnostic when the source inventory cannot be established or an
/// exact unit selection does not exist.
#[allow(clippy::too_many_lines)]
pub async fn collect(options: CollectionOptions) -> Result<Collection, String> {
    if options.concurrency == 0 {
        return Err("probe concurrency must be positive".to_owned());
    }
    let root = options
        .root
        .canonicalize()
        .map_err(|error| format!("cannot open checkout root: {error}"))?;
    validate_command_dir(&options.command_dir)?;
    let mut inventory = inspect_operations(&root, None)?;
    let selected_product = if let Some(selection) = options.product.as_deref() {
        let matches = inventory
            .products
            .iter()
            .filter(|product| {
                product.id == selection || product.aliases.iter().any(|alias| alias == selection)
            })
            .map(|product| product.id.clone())
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [id] => Some(id.clone()),
            [] => return Err(format!("product selection {selection:?} is unknown")),
            _ => return Err(format!("product selection {selection:?} is ambiguous")),
        }
    } else {
        None
    };
    if let Some(selected) = selected_product.as_deref() {
        inventory
            .products
            .retain(|product| product.id == selected || product.id == "clockwork");
    }
    let observed_at_start = now_unix_seconds();
    let semaphore = Arc::new(Semaphore::new(options.concurrency));
    let mut tasks = JoinSet::new();
    let mut pending = std::collections::BTreeMap::new();
    for product in inventory.products {
        pending.insert(product.id.clone(), product.clone());
        let semaphore = Arc::clone(&semaphore);
        let command_dir = options.command_dir.clone();
        let probe_timeout = options.probe_timeout;
        let product_id = product.id.clone();
        tasks.spawn(async move {
            let Ok(_permit) = semaphore.acquire_owned().await else {
                return (
                    product_id,
                    probe::unknown_product(
                        product,
                        ProbeState::Failed,
                        vec![iatreion_api::Diagnostic {
                            code: "probe_admission_failed".to_owned(),
                            summary: "the report probe semaphore closed".to_owned(),
                        }],
                    ),
                );
            };
            (
                product_id,
                probe::observe(product, &command_dir, probe_timeout, observed_at_start).await,
            )
        });
    }

    let deadline = tokio::time::Instant::now() + options.total_timeout;
    let mut products = Vec::new();
    let mut deadline_reached = false;
    loop {
        match tokio::time::timeout_at(deadline, tasks.join_next()).await {
            Ok(Some(Ok((product_id, status)))) => {
                pending.remove(&product_id);
                products.push(status);
            }
            Ok(Some(Err(_))) => {}
            Ok(None) => break,
            Err(_) => {
                deadline_reached = true;
                break;
            }
        }
    }
    tasks.abort_all();
    drop(tasks);
    for product in pending.into_values() {
        let (state, code, summary) = if deadline_reached {
            (
                ProbeState::TimedOut,
                "collection_deadline_exceeded",
                "the probe did not finish within the total report budget",
            )
        } else {
            (
                ProbeState::Failed,
                "probe_task_failed",
                "the probe task ended without an observation",
            )
        };
        products.push(probe::unknown_product(
            product,
            state,
            vec![iatreion_api::Diagnostic {
                code: code.to_owned(),
                summary: summary.to_owned(),
            }],
        ));
    }
    products.sort_by(|left, right| left.id.cmp(&right.id));
    join_scheduler_observations(&mut products);
    if let Some(selected) = selected_product.as_deref() {
        products.retain(|product| product.id == selected);
    }
    let selected_unit_found = if let Some(unit_id) = options.unit.as_deref() {
        let mut found = false;
        for product in &mut products {
            product.units.retain(|unit| {
                let selected = unit.observation.id == unit_id;
                found |= selected;
                selected
            });
        }
        products.retain(|product| !product.units.is_empty());
        found
    } else {
        true
    };
    if !selected_unit_found {
        return Err(format!(
            "operational unit selection {:?} is unknown",
            options.unit.as_deref().unwrap_or_default()
        ));
    }
    let observed_at_end = now_unix_seconds();
    Ok(Collection {
        report: Report {
            schema_version: REPORT_SCHEMA_VERSION,
            scope: "cell_operational_status".to_owned(),
            root: root.to_string_lossy().into_owned(),
            observed_at_start,
            observed_at_end,
            products,
        },
        selected_unit_found,
    })
}

fn join_scheduler_observations(products: &mut [iatreion_api::ProductStatus]) {
    let observations = products
        .iter()
        .flat_map(|product| product.scheduler_observations.iter().cloned())
        .map(|observation| (observation.key.clone(), observation))
        .collect::<std::collections::BTreeMap<_, _>>();
    for product in products {
        for reported in &mut product.units {
            let Some(key) = reported.observation.clockwork_key.clone() else {
                continue;
            };
            let Some(scheduler) = observations.get(&key) else {
                reported.observation.admission.reasons.push(Reason {
                    code: "clockwork_binding_unobserved".to_owned(),
                    summary: format!("Clockwork did not report the declared binding {key}"),
                });
                reported.group = probe::classify(&reported.observation);
                continue;
            };
            apply_scheduler(reported, scheduler);
        }
    }
}

fn apply_scheduler(reported: &mut iatreion_api::ReportedUnit, scheduler: &SchedulerObservation) {
    let unit = &mut reported.observation;
    if matches!(
        unit.intent,
        iatreion_api::Intent::Disabled | iatreion_api::Intent::Retired
    ) {
        reported.group = probe::classify(unit);
        return;
    }
    unit.admission.state = if scheduler.enabled && !scheduler.failure_halted {
        AdmissionState::Open
    } else {
        AdmissionState::Closed
    };
    if !scheduler.enabled {
        unit.admission.reasons.push(Reason {
            code: "operator_disabled_schedule".to_owned(),
            summary: format!("Clockwork binding {} is disabled", scheduler.key),
        });
    }
    if scheduler.failure_halted {
        unit.admission.reasons.push(Reason {
            code: "failure_halted".to_owned(),
            summary: scheduler.incident_id.as_ref().map_or_else(
                || format!("Clockwork binding {} is failure-halted", scheduler.key),
                |incident| {
                    format!(
                        "Clockwork binding {} is halted by incident {incident}",
                        scheduler.key
                    )
                },
            ),
        });
        unit.readiness.state = ReadinessState::Blocked;
        unit.readiness.reasons.push(Reason {
            code: "clockwork_failure_halt".to_owned(),
            summary: "the scheduler will not admit another activation until recovery".to_owned(),
        });
    } else if scheduler.failure_pending {
        unit.admission.reasons.push(Reason {
            code: "failure_pending".to_owned(),
            summary: format!(
                "Clockwork binding {} has a failure episode under service checks; scheduling is not failure-halted",
                scheduler.key
            ),
        });
    }
    unit.activity = if scheduler.recorded_running || unit.activity == Activity::Running {
        Activity::Running
    } else if unit.activity == Activity::Stopped {
        Activity::Stopped
    } else {
        Activity::Idle
    };
    unit.evidence.counts.extend([
        Count {
            name: "schedule_enabled".to_owned(),
            value: u64::from(scheduler.enabled),
            unit: "boolean".to_owned(),
            scope: format!("Clockwork binding {}", scheduler.key),
        },
        Count {
            name: "recorded_running".to_owned(),
            value: u64::from(scheduler.recorded_running),
            unit: "boolean".to_owned(),
            scope: format!("Clockwork binding {}", scheduler.key),
        },
    ]);
    if scheduler
        .latest_runtime_outcome
        .as_ref()
        .is_some_and(|scheduler_outcome| {
            unit.evidence
                .latest_runtime_outcome
                .as_ref()
                .is_none_or(|product_outcome| {
                    scheduler_outcome.occurred_at >= product_outcome.occurred_at
                })
        })
    {
        unit.evidence
            .latest_runtime_outcome
            .clone_from(&scheduler.latest_runtime_outcome);
    }
    unit.inspection.push(InspectionReference {
        capability_id: "clockwork.schedule.operate".to_owned(),
        record_id: scheduler.incident_id.clone(),
    });
    reported.group = probe::classify(unit);
}

fn validate_command_dir(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("installed command directory must be absolute".to_owned());
    }
    if !path.is_dir() {
        return Err("installed command directory is unavailable".to_owned());
    }
    Ok(())
}

#[must_use]
pub fn has_unknown_coverage(report: &Report) -> bool {
    report.products.iter().any(|product| {
        product.probe_state != ProbeState::Observed
            || product
                .units
                .iter()
                .any(|unit| unit.group == iatreion_api::Group::Unknown)
    })
}

#[cfg(test)]
mod tests {
    use iatreion_api::{Evidence, Group, Intent, Outcome, OutcomeKind};

    use super::*;

    fn reported_unit() -> iatreion_api::ReportedUnit {
        let mut unit = iatreion_api::declared_unit(
            "sample",
            "sample/worker",
            Some("sample/worker"),
            Intent::Active,
            "sample.status.inspect",
        );
        unit.readiness.state = ReadinessState::Ready;
        unit.activity = Activity::Idle;
        iatreion_api::ReportedUnit {
            group: Group::Unknown,
            source_product_id: "sample".to_owned(),
            observation: unit,
        }
    }

    fn scheduler() -> SchedulerObservation {
        SchedulerObservation {
            key: "sample/worker".to_owned(),
            enabled: true,
            failure_halted: false,
            failure_pending: false,
            recorded_running: false,
            incident_id: None,
            latest_runtime_outcome: None,
        }
    }

    fn outcome(kind: OutcomeKind, occurred_at: i64) -> Outcome {
        Outcome {
            kind,
            occurred_at,
            reference: None,
        }
    }

    #[test]
    fn pending_failure_keeps_admission_open_and_product_readiness() {
        let mut reported = reported_unit();
        let mut scheduler = scheduler();
        scheduler.failure_pending = true;
        apply_scheduler(&mut reported, &scheduler);
        assert_eq!(reported.observation.admission.state, AdmissionState::Open);
        assert_eq!(reported.observation.readiness.state, ReadinessState::Ready);
        assert_eq!(reported.group, Group::NeedsAttention);
        assert!(
            reported
                .observation
                .admission
                .reasons
                .iter()
                .any(|reason| reason.code == "failure_pending")
        );
        assert!(
            !reported
                .observation
                .admission
                .reasons
                .iter()
                .any(|reason| reason.code == "failure_halted")
        );
    }

    #[test]
    fn actual_failure_halt_closes_admission() {
        let mut reported = reported_unit();
        let mut scheduler = scheduler();
        scheduler.failure_halted = true;
        scheduler.incident_id = Some("exact-incident".to_owned());
        apply_scheduler(&mut reported, &scheduler);
        assert_eq!(reported.observation.admission.state, AdmissionState::Closed);
        assert_eq!(
            reported.observation.readiness.state,
            ReadinessState::Blocked
        );
        assert!(
            reported
                .observation
                .admission
                .reasons
                .iter()
                .any(|reason| reason.code == "failure_halted")
        );
    }

    #[test]
    fn keeps_product_running_evidence_after_a_scheduler_failure() {
        let mut reported = reported_unit();
        reported.observation.activity = Activity::Running;
        let mut scheduler = scheduler();
        scheduler.failure_pending = true;
        scheduler.latest_runtime_outcome = Some(outcome(OutcomeKind::Failed, 200));
        apply_scheduler(&mut reported, &scheduler);
        assert_eq!(reported.observation.activity, Activity::Running);
        assert_eq!(reported.observation.readiness.state, ReadinessState::Ready);
        assert_eq!(
            reported.observation.evidence.latest_runtime_outcome,
            scheduler.latest_runtime_outcome
        );
    }

    #[test]
    fn keeps_product_stopped_evidence_without_a_running_activation() {
        let mut reported = reported_unit();
        reported.observation.activity = Activity::Stopped;
        apply_scheduler(&mut reported, &scheduler());
        assert_eq!(reported.observation.activity, Activity::Stopped);
        assert_eq!(reported.group, Group::NeedsAttention);
    }

    #[test]
    fn joins_newest_runtime_outcome_without_overwriting_domain_evidence() {
        let mut reported = reported_unit();
        let domain_success = outcome(OutcomeKind::Succeeded, 150);
        reported.observation.evidence = Evidence {
            latest_runtime_outcome: Some(outcome(OutcomeKind::Succeeded, 100)),
            latest_domain_outcome: Some(domain_success.clone()),
            latest_domain_success: Some(domain_success.clone()),
            ..Evidence::default()
        };
        let mut scheduler = scheduler();
        scheduler.latest_runtime_outcome = Some(outcome(OutcomeKind::Failed, 200));
        apply_scheduler(&mut reported, &scheduler);
        assert_eq!(
            reported.observation.evidence.latest_runtime_outcome,
            scheduler.latest_runtime_outcome
        );
        assert_eq!(
            reported.observation.evidence.latest_domain_outcome,
            Some(domain_success.clone())
        );
        assert_eq!(
            reported.observation.evidence.latest_domain_success,
            Some(domain_success)
        );

        reported.observation.evidence.latest_runtime_outcome =
            Some(outcome(OutcomeKind::Succeeded, 200));
        apply_scheduler(&mut reported, &scheduler);
        assert_eq!(
            reported.observation.evidence.latest_runtime_outcome,
            scheduler.latest_runtime_outcome
        );

        reported.observation.evidence.latest_runtime_outcome =
            Some(outcome(OutcomeKind::Succeeded, 300));
        apply_scheduler(&mut reported, &scheduler);
        assert_eq!(
            reported.observation.evidence.latest_runtime_outcome,
            Some(outcome(OutcomeKind::Succeeded, 300))
        );
    }
}
