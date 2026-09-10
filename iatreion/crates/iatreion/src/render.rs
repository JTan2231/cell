use std::io::{self, Write};

use iatreion_api::{Activity, AdmissionState, Group, Intent, ReadinessState, Report};

/// Render the compact human view without hiding any unknown unit.
///
/// # Errors
/// Returns an output error from the selected writer.
pub fn render(report: &Report, output: &mut impl Write) -> io::Result<()> {
    let mut counts = [0_usize; 4];
    for unit in report.products.iter().flat_map(|product| &product.units) {
        counts[group_index(unit.group)] += 1;
    }
    writeln!(output, "Iatreion operational report")?;
    writeln!(
        output,
        "Snapshot: {}..{} (Unix seconds)",
        report.observed_at_start, report.observed_at_end
    )?;
    writeln!(
        output,
        "Products: {}; units: {} operating, {} need attention, {} intentionally inactive, {} unknown.\n",
        report.products.len(),
        counts[1],
        counts[0],
        counts[2],
        counts[3]
    )?;
    for group in [
        Group::NeedsAttention,
        Group::Operating,
        Group::IntentionallyInactive,
        Group::Unknown,
    ] {
        let units = report
            .products
            .iter()
            .flat_map(|product| product.units.iter().map(move |unit| (product, unit)))
            .filter(|(_, unit)| unit.group == group)
            .collect::<Vec<_>>();
        if units.is_empty() {
            continue;
        }
        writeln!(output, "{}", group_label(group))?;
        for (product, unit) in units {
            let observation = &unit.observation;
            writeln!(
                output,
                "  {:28} intent={}; admission={}; activity={}; readiness={}",
                observation.id,
                intent_label(observation.intent),
                admission_label(observation.admission.state),
                activity_label(observation.activity),
                readiness_label(observation.readiness.state),
            )?;
            for reason in observation
                .admission
                .reasons
                .iter()
                .chain(&observation.readiness.reasons)
            {
                writeln!(output, "    {}: {}", reason.code, reason.summary)?;
            }
            for count in &observation.evidence.counts {
                writeln!(
                    output,
                    "    {}={} {} ({})",
                    count.name, count.value, count.unit, count.scope
                )?;
            }
            for reference in &observation.inspection {
                if let Some(record_id) = &reference.record_id {
                    writeln!(
                        output,
                        "    inspect: {} ({})",
                        reference.capability_id, record_id
                    )?;
                } else {
                    writeln!(output, "    inspect: {}", reference.capability_id)?;
                }
            }
            if !product.complete {
                for diagnostic in &product.diagnostics {
                    writeln!(output, "    {}: {}", diagnostic.code, diagnostic.summary)?;
                }
            }
        }
        writeln!(output)?;
    }
    Ok(())
}

fn group_index(group: Group) -> usize {
    match group {
        Group::NeedsAttention => 0,
        Group::Operating => 1,
        Group::IntentionallyInactive => 2,
        Group::Unknown => 3,
    }
}

fn group_label(group: Group) -> &'static str {
    match group {
        Group::NeedsAttention => "NEEDS ATTENTION",
        Group::Operating => "OPERATING",
        Group::IntentionallyInactive => "INTENTIONALLY INACTIVE",
        Group::Unknown => "UNKNOWN",
    }
}

fn intent_label(value: Intent) -> &'static str {
    match value {
        Intent::Active => "active",
        Intent::OnDemand => "on-demand",
        Intent::Disabled => "disabled",
        Intent::Retired => "retired",
        Intent::Unknown => "unknown",
    }
}

fn admission_label(value: AdmissionState) -> &'static str {
    match value {
        AdmissionState::Open => "open",
        AdmissionState::Closed => "closed",
        AdmissionState::Unknown => "unknown",
        AdmissionState::NotApplicable => "n/a",
    }
}

fn activity_label(value: Activity) -> &'static str {
    match value {
        Activity::Running => "running",
        Activity::Idle => "idle",
        Activity::Stopped => "stopped",
        Activity::Unknown => "unknown",
        Activity::NotApplicable => "n/a",
    }
}

fn readiness_label(value: ReadinessState) -> &'static str {
    match value {
        ReadinessState::Ready => "ready",
        ReadinessState::Degraded => "degraded",
        ReadinessState::Blocked => "blocked",
        ReadinessState::Unknown => "unknown",
        ReadinessState::NotApplicable => "n/a",
    }
}
