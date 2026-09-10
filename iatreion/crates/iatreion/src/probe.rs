use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use iatreion_api::{
    Activity, Admission, AdmissionState, Diagnostic, Evidence, Group, InspectionReference, Intent,
    OperationalUnit, ProbeState, ProductStatus, Readiness, ReadinessState, ReportedUnit,
    STATUS_SCHEMA_VERSION, StatusSnapshot, validate_snapshot,
};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use usher::api::{OperationalProduct, OperationalUnitDeclaration};

const OUTPUT_LIMIT: usize = 1024 * 1024;

#[allow(clippy::too_many_lines)]
pub(crate) async fn observe(
    product: OperationalProduct,
    command_dir: &Path,
    timeout: Duration,
    report_started_at: i64,
) -> ProductStatus {
    if !product.complete {
        return unknown_product(product, ProbeState::Undeclared, Vec::new());
    }
    if product.id == "iatreion" {
        return self_observation(product);
    }
    let Some(command) = product.status_command.as_deref() else {
        return unknown_product(product, ProbeState::Undeclared, Vec::new());
    };
    let executable = command_dir.join(command);
    let mut child = match Command::new(&executable)
        .args(["status-snapshot", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return unknown_product(
                product,
                ProbeState::Unavailable,
                vec![diagnostic(
                    "probe_unavailable",
                    &format!("status probe could not start: {}", error.kind()),
                )],
            );
        }
    };

    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return unknown_product(
            product,
            ProbeState::Failed,
            vec![diagnostic(
                "probe_observation_failed",
                "status probe output pipes were unavailable",
            )],
        );
    };
    let stdout_task = tokio::spawn(read_bounded(stdout));
    let stderr_task = tokio::spawn(read_bounded(stderr));
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            stdout_task.abort();
            stderr_task.abort();
            return unknown_product(
                product,
                ProbeState::Failed,
                vec![diagnostic(
                    "probe_observation_failed",
                    &format!("status probe could not be observed: {}", error.kind()),
                )],
            );
        }
        Err(_) => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
            stdout_task.abort();
            stderr_task.abort();
            return unknown_product(
                product,
                ProbeState::TimedOut,
                vec![diagnostic(
                    "probe_timed_out",
                    "status probe exceeded its collection budget",
                )],
            );
        }
    };
    let Ok(Ok(stdout)) = stdout_task.await else {
        stderr_task.abort();
        return unknown_product(
            product,
            ProbeState::Failed,
            vec![diagnostic(
                "probe_observation_failed",
                "status probe stdout could not be observed",
            )],
        );
    };
    let Ok(Ok(stderr)) = stderr_task.await else {
        return unknown_product(
            product,
            ProbeState::Failed,
            vec![diagnostic(
                "probe_observation_failed",
                "status probe stderr could not be observed",
            )],
        );
    };
    if stdout.overflow || stderr.overflow {
        return unknown_product(
            product,
            ProbeState::OutputOverflow,
            vec![diagnostic(
                "probe_output_overflow",
                "status probe exceeded its output limit",
            )],
        );
    }
    if !status.success() {
        return unknown_product(
            product,
            ProbeState::Failed,
            vec![diagnostic(
                "probe_failed",
                &format!(
                    "status probe exited unsuccessfully{}",
                    status
                        .code()
                        .map_or_else(String::new, |code| format!(" with code {code}"))
                ),
            )],
        );
    }
    let snapshot: StatusSnapshot = match serde_json::from_slice(&stdout.bytes) {
        Ok(snapshot) => snapshot,
        Err(_) => {
            return unknown_product(
                product,
                ProbeState::Invalid,
                vec![diagnostic(
                    "invalid_probe_response",
                    "status probe did not return one valid JSON snapshot",
                )],
            );
        }
    };
    if snapshot.schema_version != STATUS_SCHEMA_VERSION {
        return unknown_product(
            product,
            ProbeState::UnsupportedSchema,
            vec![diagnostic(
                "unsupported_probe_schema",
                "status probe returned an unsupported schema version",
            )],
        );
    }
    if let Err(message) = validate_snapshot(&snapshot) {
        return unknown_product(
            product,
            ProbeState::Invalid,
            vec![diagnostic("invalid_probe_response", message)],
        );
    }
    if snapshot.product_id != product.id {
        return unknown_product(
            product,
            ProbeState::Invalid,
            vec![diagnostic(
                "probe_identity_mismatch",
                "status probe returned a different product identity",
            )],
        );
    }
    if snapshot.observed_at_end < report_started_at.saturating_sub(30) {
        return unknown_product(
            product,
            ProbeState::Invalid,
            vec![diagnostic(
                "stale_observation",
                "status probe returned an observation older than the freshness limit",
            )],
        );
    }

    merge_snapshot(product, snapshot)
}

struct BoundedOutput {
    bytes: Vec<u8>,
    overflow: bool,
}

async fn read_bounded(mut input: impl AsyncRead + Unpin) -> std::io::Result<BoundedOutput> {
    let mut bytes = Vec::new();
    let mut overflow = false;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = input.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = OUTPUT_LIMIT.saturating_sub(bytes.len());
        bytes.extend_from_slice(&buffer[..read.min(remaining)]);
        overflow |= read > remaining;
    }
    Ok(BoundedOutput { bytes, overflow })
}

fn merge_snapshot(product: OperationalProduct, snapshot: StatusSnapshot) -> ProductStatus {
    let scheduler_observations = snapshot.scheduler_observations;
    let mut observed = snapshot
        .units
        .into_iter()
        .map(|unit| (unit.id.clone(), unit))
        .collect::<BTreeMap<_, _>>();
    let mut diagnostics = snapshot.diagnostics;
    let mut complete = snapshot.complete;
    let mut units = Vec::new();
    for declaration in &product.units {
        let Some(unit) = observed.remove(&declaration.id) else {
            complete = false;
            diagnostics.push(diagnostic(
                "missing_unit_observation",
                &format!("{} has no observation", declaration.id),
            ));
            units.push(reported_unknown(&product.id, declaration));
            continue;
        };
        if unit.owning_product_id != product.id
            || unit.clockwork_key != declaration.clockwork_key
            || intent_name(unit.intent) != declaration.intent
        {
            complete = false;
            diagnostics.push(diagnostic(
                "unit_declaration_mismatch",
                &format!("{} differs from its Cell declaration", declaration.id),
            ));
            units.push(reported_unknown(&product.id, declaration));
            continue;
        }
        units.push(ReportedUnit {
            group: classify(&unit),
            source_product_id: product.id.clone(),
            observation: unit,
        });
    }
    if !observed.is_empty() {
        complete = false;
        diagnostics.push(diagnostic(
            "undeclared_unit_observation",
            "status probe returned an operational unit not declared by Cell",
        ));
    }
    ProductStatus {
        id: product.id,
        name: product.name,
        aliases: product.aliases,
        probe_state: ProbeState::Observed,
        complete,
        diagnostics,
        units,
        scheduler_observations,
    }
}

fn self_observation(product: OperationalProduct) -> ProductStatus {
    let units = product
        .units
        .iter()
        .map(|declaration| {
            let mut unit = unknown_unit(&product.id, declaration);
            unit.intent = Intent::OnDemand;
            unit.admission.state = AdmissionState::NotApplicable;
            unit.activity = Activity::Running;
            unit.readiness = Readiness {
                state: ReadinessState::Ready,
                scope: "current report process".to_owned(),
                reasons: Vec::new(),
            };
            ReportedUnit {
                group: Group::Operating,
                source_product_id: product.id.clone(),
                observation: unit,
            }
        })
        .collect();
    ProductStatus {
        id: product.id,
        name: product.name,
        aliases: product.aliases,
        probe_state: ProbeState::Observed,
        complete: true,
        diagnostics: Vec::new(),
        units,
        scheduler_observations: Vec::new(),
    }
}

pub(crate) fn unknown_product(
    product: OperationalProduct,
    state: ProbeState,
    mut diagnostics: Vec<Diagnostic>,
) -> ProductStatus {
    diagnostics.extend(product.issues.iter().map(|issue| {
        let (code, summary) = issue
            .split_once(": ")
            .unwrap_or(("operational_declaration_invalid", issue.as_str()));
        diagnostic(code, summary)
    }));
    let units = product
        .units
        .iter()
        .map(|unit| reported_unknown(&product.id, unit))
        .collect();
    ProductStatus {
        id: product.id,
        name: product.name,
        aliases: product.aliases,
        probe_state: state,
        complete: false,
        diagnostics,
        units,
        scheduler_observations: Vec::new(),
    }
}

fn reported_unknown(product_id: &str, declaration: &OperationalUnitDeclaration) -> ReportedUnit {
    ReportedUnit {
        group: Group::Unknown,
        source_product_id: product_id.to_owned(),
        observation: unknown_unit(product_id, declaration),
    }
}

fn unknown_unit(product_id: &str, declaration: &OperationalUnitDeclaration) -> OperationalUnit {
    OperationalUnit {
        id: declaration.id.clone(),
        owning_product_id: product_id.to_owned(),
        clockwork_key: declaration.clockwork_key.clone(),
        intent: parse_intent(&declaration.intent),
        admission: Admission {
            state: AdmissionState::Unknown,
            reasons: Vec::new(),
        },
        activity: Activity::Unknown,
        readiness: Readiness {
            state: ReadinessState::Unknown,
            scope: "local operational prerequisites".to_owned(),
            reasons: Vec::new(),
        },
        evidence: Evidence::default(),
        inspection: vec![InspectionReference {
            capability_id: declaration.inspection_capability.clone(),
            record_id: None,
        }],
    }
}

pub(crate) fn classify(unit: &OperationalUnit) -> Group {
    let reasons = unit
        .admission
        .reasons
        .iter()
        .chain(&unit.readiness.reasons)
        .map(|reason| reason.code.as_str())
        .collect::<Vec<_>>();
    let known_failure = reasons.iter().any(|code| {
        code.contains("failure")
            || code.contains("halt")
            || code.contains("blocked")
            || code.contains("prerequisite")
    });
    if known_failure
        || matches!(
            unit.readiness.state,
            ReadinessState::Blocked | ReadinessState::Degraded
        )
        || (unit.intent == Intent::Active && unit.activity == Activity::Stopped)
    {
        return Group::NeedsAttention;
    }
    if matches!(unit.intent, Intent::Disabled | Intent::Retired)
        || (unit.admission.state == AdmissionState::Closed
            && reasons
                .iter()
                .any(|code| code.contains("operator") || code.contains("maintenance")))
    {
        return Group::IntentionallyInactive;
    }
    if unit.intent == Intent::Unknown
        || unit.admission.state == AdmissionState::Unknown
        || unit.activity == Activity::Unknown
        || unit.readiness.state == ReadinessState::Unknown
    {
        return Group::Unknown;
    }
    Group::Operating
}

fn parse_intent(value: &str) -> Intent {
    match value {
        "active" => Intent::Active,
        "on_demand" => Intent::OnDemand,
        "disabled" => Intent::Disabled,
        "retired" => Intent::Retired,
        _ => Intent::Unknown,
    }
}

fn intent_name(value: Intent) -> &'static str {
    match value {
        Intent::Active => "active",
        Intent::OnDemand => "on_demand",
        Intent::Disabled => "disabled",
        Intent::Retired => "retired",
        Intent::Unknown => "unknown",
    }
}

fn diagnostic(code: &str, summary: &str) -> Diagnostic {
    Diagnostic {
        code: code.to_owned(),
        summary: summary.to_owned(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn unit(
        intent: Intent,
        admission: AdmissionState,
        activity: Activity,
        readiness: ReadinessState,
    ) -> OperationalUnit {
        OperationalUnit {
            id: "sample/unit".to_owned(),
            owning_product_id: "sample".to_owned(),
            clockwork_key: None,
            intent,
            admission: Admission {
                state: admission,
                reasons: Vec::new(),
            },
            activity,
            readiness: Readiness {
                state: readiness,
                scope: "local".to_owned(),
                reasons: Vec::new(),
            },
            evidence: Evidence::default(),
            inspection: Vec::new(),
        }
    }

    #[test]
    fn classifies_independent_dimensions() {
        assert_eq!(
            classify(&unit(
                Intent::OnDemand,
                AdmissionState::NotApplicable,
                Activity::NotApplicable,
                ReadinessState::Ready
            )),
            Group::Operating
        );
        assert_eq!(
            classify(&unit(
                Intent::Retired,
                AdmissionState::NotApplicable,
                Activity::NotApplicable,
                ReadinessState::NotApplicable
            )),
            Group::IntentionallyInactive
        );
        assert_eq!(
            classify(&unit(
                Intent::Active,
                AdmissionState::Open,
                Activity::Stopped,
                ReadinessState::Ready
            )),
            Group::NeedsAttention
        );
        assert_eq!(
            classify(&unit(
                Intent::Active,
                AdmissionState::Unknown,
                Activity::Unknown,
                ReadinessState::Unknown
            )),
            Group::Unknown
        );
    }

    fn declared_product(command: &str) -> OperationalProduct {
        OperationalProduct {
            id: "sample".to_owned(),
            name: "Sample".to_owned(),
            aliases: Vec::new(),
            descriptor: "pipeline/products/sample.sh".to_owned(),
            complete: true,
            status_schema: Some(1),
            status_command: Some(command.to_owned()),
            units: vec![OperationalUnitDeclaration {
                id: "sample/unit".to_owned(),
                intent: "on_demand".to_owned(),
                clockwork_key: None,
                inspection_capability: "sample.status.inspect".to_owned(),
            }],
            issues: Vec::new(),
        }
    }

    fn executable(directory: &std::path::Path, name: &str, body: &str) {
        let path = directory.join(name);
        std::fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    #[tokio::test]
    async fn accepts_one_exact_bounded_probe_response() {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = iatreion_api::static_snapshot(
            "sample",
            "1.0.0",
            vec![iatreion_api::on_demand_unit(
                "sample",
                "sample/unit",
                "sample.status.inspect",
            )],
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        executable(
            directory.path(),
            "sample",
            &format!(
                "test \"$1\" = status-snapshot\ntest \"$2\" = --json\nprintf '%s\\n' '{json}'"
            ),
        );
        let status = observe(
            declared_product("sample"),
            directory.path(),
            Duration::from_secs(1),
            iatreion_api::now_unix_seconds(),
        )
        .await;
        assert_eq!(status.probe_state, ProbeState::Observed);
        assert!(status.complete);
        assert_eq!(status.units[0].group, Group::Operating);
    }

    #[tokio::test]
    async fn timeout_and_overflow_are_explicit_unknown_coverage() {
        let directory = tempfile::tempdir().unwrap();
        executable(directory.path(), "slow", "sleep 5");
        let timed_out = observe(
            declared_product("slow"),
            directory.path(),
            Duration::from_millis(25),
            iatreion_api::now_unix_seconds(),
        )
        .await;
        assert_eq!(timed_out.probe_state, ProbeState::TimedOut);
        assert_eq!(timed_out.units[0].group, Group::Unknown);

        executable(
            directory.path(),
            "large",
            "head -c 1048577 /dev/zero | tr '\\000' x",
        );
        let overflow = observe(
            declared_product("large"),
            directory.path(),
            Duration::from_secs(2),
            iatreion_api::now_unix_seconds(),
        )
        .await;
        assert_eq!(overflow.probe_state, ProbeState::OutputOverflow);
        assert_eq!(overflow.units[0].group, Group::Unknown);
    }
}
