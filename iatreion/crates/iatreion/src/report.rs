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
    }
    unit.activity = if scheduler.recorded_running {
        Activity::Running
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
    if unit.evidence.latest_runtime_outcome.is_none() {
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
