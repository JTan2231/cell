use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn list_and_show_present_the_complete_semantic_catalog_without_mutation() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.write_provider("nucleus", vec![execution_entry()])?;
    fixture.write_provider("todo", vec![todo_entry()])?;
    let before = tree_snapshot(fixture.registry())?;

    let listed = fixture.run_json(&["list"])?;
    assert!(listed.status.success());
    let listed_json = stdout_json(&listed)?;
    assert_eq!(listed_json["schema_version"], 2);
    assert_eq!(listed_json["data"]["entries"][0]["id"], "nucleus.execution");
    assert_eq!(
        listed_json["data"]["entries"][1]["id"],
        "todo.concern.capture-and-route"
    );
    let todo = &listed_json["data"]["entries"][1];
    assert_eq!(todo["title"], "Use todo.concern.capture-and-route");
    assert_eq!(todo["summary"], "Perform todo.concern.capture-and-route");
    assert_eq!(todo["kind"], "capability");
    assert_eq!(todo["mode"], "use");
    assert_eq!(todo["provider"]["release"], "1.0.0");
    assert_eq!(todo["provider_release"], "1.0.0");
    assert_eq!(todo["support"], "supported");
    assert_eq!(todo["availability"], "installed");
    assert_eq!(todo["compatibility"], "compatible");
    assert_eq!(todo["readiness"], "not_checked");
    assert!(todo.get("routing").is_none());
    assert!(todo.get("routable").is_none());

    let listed_human = fixture.run_human(&["list"])?;
    assert!(listed_human.status.success());
    let human = String::from_utf8(listed_human.stdout)?;
    assert!(human.contains("USE — ordinary outcome work"));
    assert!(human.contains("Use todo.concern.capture-and-route"));
    assert!(human.contains("Perform todo.concern.capture-and-route"));
    assert!(human.contains("supported · installed · compatible · not_checked"));

    let shown = fixture.run_json(&["show", "todo.concern.capture-and-route"])?;
    assert!(shown.status.success());
    let shown = stdout_json(&shown)?;
    assert_eq!(shown["data"]["availability"], "installed");
    assert_eq!(shown["data"]["compatibility"], "compatible");
    assert_eq!(shown["data"]["readiness"], "not_checked");
    assert!(shown["data"]["entry"].get("routing").is_none());
    assert!(shown["data"]["entry"].get("routable").is_none());
    assert!(
        shown["data"]["manual"]
            .as_str()
            .is_some_and(|manual| manual.contains("Todo is authoritative"))
    );

    assert_eq!(before, tree_snapshot(fixture.registry())?);
    Ok(())
}

#[test]
fn legacy_v1_bundles_are_read_with_routing_metadata_ignored() -> TestResult {
    let fixture = Fixture::new()?;
    let root = fixture.write_legacy_provider(
        "alpha",
        vec![legacy_capability("alpha.run", "run alpha", &json!([]))],
    )?;

    let validated = run_validate(&root)?;
    assert!(validated.status.success());

    let listed = fixture.run_json(&["list"])?;
    assert!(listed.status.success());
    let listed = stdout_json(&listed)?;
    assert_eq!(listed["data"]["entries"][0]["id"], "alpha.run");
    assert!(listed["data"]["entries"][0].get("routing").is_none());
    assert!(listed["data"]["entries"][0].get("routable").is_none());

    let shown = fixture.run_json(&["show", "alpha.run"])?;
    assert!(shown.status.success());
    let shown = stdout_json(&shown)?;
    assert!(shown["data"]["entry"].get("routing").is_none());
    assert!(shown["data"]["entry"].get("routable").is_none());
    Ok(())
}

#[test]
fn schema_v2_rejects_removed_and_v3_fields_and_newer_schemas() -> TestResult {
    let fixture = Fixture::new()?;
    let mut routed = capability("alpha.run", &json!([]));
    routed["routable"] = json!(true);
    routed["routing"] = json!({"triggers": ["run alpha"], "exclusions": []});
    let routed_root = fixture.write_provider("alpha", vec![routed])?;
    let routed_report = run_validate(&routed_root)?;
    assert_eq!(routed_report.status.code(), Some(1));
    assert!(has_issue(&stdout_json(&routed_report)?, "invalid_entry"));

    let promised = fixture.write_provider(
        "promised",
        vec![normalized_capability("promised.run", &json!([]))],
    )?;
    let promised_report = run_validate(&promised)?;
    assert_eq!(promised_report.status.code(), Some(1));
    assert!(has_issue(
        &stdout_json(&promised_report)?,
        "unexpected_promise_declaration"
    ));

    let null_scope = fixture.write_provider_named(
        "null-scope",
        "null-scope",
        2,
        vec![capability("null-scope.run", &json!([]))],
    )?;
    let manifest_path = null_scope.join("provider.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    manifest["promise_scope"] = Value::Null;
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    let null_scope_report = run_validate(&null_scope)?;
    assert_eq!(null_scope_report.status.code(), Some(1));
    assert!(has_issue(
        &stdout_json(&null_scope_report)?,
        "unexpected_promise_scope"
    ));

    let mut null_promise_entry = capability("null-promise.run", &json!([]));
    null_promise_entry["promise"] = Value::Null;
    let null_promise = fixture.write_provider_named(
        "null-promise",
        "null-promise",
        2,
        vec![null_promise_entry],
    )?;
    let null_promise_report = run_validate(&null_promise)?;
    assert_eq!(null_promise_report.status.code(), Some(1));
    assert!(has_issue(
        &stdout_json(&null_promise_report)?,
        "unexpected_promise_declaration"
    ));

    let newer = fixture.write_provider_named(
        "newer",
        "beta",
        4,
        vec![capability("beta.run", &json!([]))],
    )?;
    let newer_report = run_validate(&newer)?;
    assert_eq!(newer_report.status.code(), Some(1));
    assert!(has_issue(
        &stdout_json(&newer_report)?,
        "unsupported_schema"
    ));
    Ok(())
}

#[test]
fn schema_v3_requires_and_validates_provider_scope_and_promise_claims() -> TestResult {
    let fixture = Fixture::new()?;
    let missing_scope = fixture.write_provider_named(
        "missing-scope",
        "alpha",
        3,
        vec![normalized_capability("alpha.run", &json!([]))],
    )?;
    let missing_report = run_validate(&missing_scope)?;
    assert_eq!(missing_report.status.code(), Some(1));
    assert!(has_issue(
        &stdout_json(&missing_report)?,
        "missing_promise_scope"
    ));

    let mut blank_promise = normalized_capability("beta.run", &json!([]));
    blank_promise["promise"]["outputs"] = json!([]);
    let blank_root = fixture.write_v3_provider("beta", vec![blank_promise])?;
    let blank_report = run_validate(&blank_root)?;
    assert_eq!(blank_report.status.code(), Some(1));
    assert!(has_issue(&stdout_json(&blank_report)?, "empty_field"));

    let mut null_promise = normalized_capability("null-v3.run", &json!([]));
    null_promise["promise"] = Value::Null;
    let null_root = fixture.write_v3_provider("null-v3", vec![null_promise])?;
    let null_report = run_validate(&null_root)?;
    assert_eq!(null_report.status.code(), Some(1));
    assert!(has_issue(
        &stdout_json(&null_report)?,
        "invalid_promise_declaration"
    ));

    let mut unbounded_reliance = normalized_capability("reliance.run", &json!([]));
    unbounded_reliance["promise"]["reliances"] = json!([{
        "status": "declared",
        "statement": "The result relies on an upstream contract.",
        "target": "upstream",
        "kind": "data",
        "contract": "upstream.read"
    }]);
    let reliance_root = fixture.write_v3_provider("reliance", vec![unbounded_reliance])?;
    let reliance_report = run_validate(&reliance_root)?;
    assert_eq!(reliance_report.status.code(), Some(1));
    assert!(has_issue(
        &stdout_json(&reliance_report)?,
        "unbounded_reliance_contract"
    ));

    let valid = fixture.write_v3_provider(
        "gamma",
        vec![normalized_capability("gamma.run", &json!([]))],
    )?;
    assert!(run_validate(&valid)?.status.success());
    Ok(())
}

#[test]
fn doctor_reports_dependencies_and_default_list_exposes_unavailable_entries() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.write_provider("todo", vec![todo_entry()])?;

    let doctor = fixture.run_json(&["doctor"])?;
    assert_eq!(doctor.status.code(), Some(1));
    let doctor = stdout_json(&doctor)?;
    assert_eq!(doctor["ok"], false);
    assert!(has_issue(&doctor, "missing_dependency"));

    let listed = fixture.run_json(&["list"])?;
    assert!(listed.status.success());
    let listed = stdout_json(&listed)?;
    assert_eq!(listed["data"]["entries"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["data"]["entries"][0]["compatibility"], "unavailable");

    fixture.write_provider("nucleus", vec![execution_entry()])?;
    let healthy = fixture.run_json(&["doctor"])?;
    assert!(healthy.status.success());
    assert_eq!(stdout_json(&healthy)?["data"]["valid"], true);
    Ok(())
}

#[test]
fn dependency_unavailability_propagates_through_installed_entries() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.write_provider("alpha", vec![dependent_entry("alpha.run", "beta.run")])?;
    fixture.write_provider("beta", vec![dependent_entry("beta.run", "missing.run")])?;

    let doctor = fixture.run_json(&["doctor"])?;
    assert_eq!(doctor.status.code(), Some(1));
    let doctor = stdout_json(&doctor)?;
    assert!(has_issue(&doctor, "missing_dependency"));
    assert!(has_issue(&doctor, "unavailable_dependency"));

    let shown = fixture.run_json(&["show", "alpha.run"])?;
    assert!(shown.status.success());
    let shown_json = stdout_json(&shown)?;
    assert_eq!(shown_json["data"]["compatibility"], "unavailable");
    assert_eq!(
        shown_json["data"]["dependency_statuses"][0]["state"],
        "unavailable"
    );
    let shown_human = fixture.run_human(&["show", "alpha.run"])?;
    assert!(shown_human.status.success());
    assert!(String::from_utf8(shown_human.stdout)?.contains("beta.run: unavailable"));
    Ok(())
}

#[test]
fn standalone_validation_checks_internal_versions_but_not_external_dependencies() -> TestResult {
    let fixture = Fixture::new()?;
    let external =
        fixture.write_provider("alpha", vec![dependent_entry("alpha.run", "external.run")])?;
    let external_report = run_validate(&external)?;
    assert!(external_report.status.success());
    assert_eq!(
        stdout_json(&external_report)?["data"]["external_dependencies"],
        "not_checked"
    );

    let mismatched = fixture.write_provider_named(
        "mismatched",
        "beta",
        2,
        vec![
            capability("beta.base", &json!([])),
            capability(
                "beta.run",
                &json!([{
                    "id": "beta.base",
                    "min_contract": 2,
                    "max_contract_exclusive": 3
                }]),
            ),
        ],
    )?;
    let mismatch_report = run_validate(&mismatched)?;
    assert_eq!(mismatch_report.status.code(), Some(1));
    assert!(has_issue(
        &stdout_json(&mismatch_report)?,
        "incompatible_dependency"
    ));
    Ok(())
}

#[test]
fn standalone_validation_rejects_internal_dependency_cycles() -> TestResult {
    let fixture = Fixture::new()?;
    let cyclic = fixture.write_provider(
        "alpha",
        vec![
            dependent_entry("alpha.one", "alpha.two"),
            dependent_entry("alpha.two", "alpha.one"),
        ],
    )?;

    let report = run_validate(&cyclic)?;
    assert_eq!(report.status.code(), Some(1));
    assert!(has_issue(&stdout_json(&report)?, "dependency_cycle"));

    let fixture = Fixture::new()?;
    fixture.write_provider(
        "alpha",
        vec![
            capability(
                "alpha.one",
                &json!([
                    {"id": "alpha.two", "min_contract": 1, "max_contract_exclusive": 2},
                    {"id": "alpha.three", "min_contract": 1, "max_contract_exclusive": 2}
                ]),
            ),
            dependent_entry("alpha.two", "alpha.one"),
            dependent_entry("alpha.three", "alpha.four"),
            dependent_entry("alpha.four", "alpha.three"),
        ],
    )?;
    let shown = stdout_json(&fixture.run_json(&["show", "alpha.one"])?)?;
    assert_eq!(shown["data"]["dependency_statuses"][0]["state"], "cycle");
    assert_eq!(
        shown["data"]["dependency_statuses"][1]["state"],
        "unavailable"
    );
    Ok(())
}

#[test]
fn list_is_complete_and_filters_only_when_requested() -> TestResult {
    let fixture = Fixture::new()?;
    let mut deprecated = capability("alpha.old", &json!([]));
    deprecated["support"] = json!("deprecated");
    fixture.write_provider("alpha", vec![deprecated])?;
    fixture.write_provider("career", vec![operation_entry()])?;

    let listed = stdout_json(&fixture.run_json(&["list"])?)?;
    assert_eq!(listed["data"]["entries"].as_array().map(Vec::len), Some(2));
    assert_eq!(listed["data"]["entries"][0]["support"], "deprecated");
    assert_eq!(
        listed["data"]["entries"][1]["readiness"],
        "session_dependent"
    );

    let operations = stdout_json(&fixture.run_json(&["list", "--kind", "operation"])?)?;
    assert_eq!(
        operations["data"]["entries"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        operations["data"]["entries"][0]["id"],
        "career.jobs.line-up"
    );

    let develop = stdout_json(&fixture.run_json(&["list", "--mode", "develop"])?)?;
    assert_eq!(develop["data"]["entries"].as_array().map(Vec::len), Some(0));
    Ok(())
}

#[test]
fn malformed_provider_is_isolated_and_visible_in_catalog_results() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.write_provider("nucleus", vec![execution_entry()])?;
    let broken = fixture.registry().join("broken");
    fs::create_dir_all(&broken)?;
    fs::write(broken.join("provider.json"), b"{not json")?;

    let listed = fixture.run_json(&["list"])?;
    assert!(listed.status.success());
    let listed_json = stdout_json(&listed)?;
    assert_eq!(
        listed_json["data"]["entries"].as_array().map(Vec::len),
        Some(1)
    );
    assert!(has_issue(&listed_json, "invalid_manifest"));

    let listed_human = fixture.run_human(&["list"])?;
    assert!(String::from_utf8(listed_human.stdout)?.contains("ISSUES"));

    let doctor = fixture.run_json(&["doctor"])?;
    assert_eq!(doctor.status.code(), Some(1));
    assert!(has_issue(&stdout_json(&doctor)?, "invalid_manifest"));
    Ok(())
}

#[test]
fn validation_rejects_inner_symlinks_but_ignores_unindexed_artifacts() -> TestResult {
    let fixture = Fixture::new()?;
    let valid = fixture.write_provider("alpha", vec![capability("alpha.run", &json!([]))])?;
    fs::write(valid.join("entries/old-draft.json"), b"{broken stale draft")?;

    let validated = run_validate(&valid)?;
    assert!(validated.status.success());
    assert_eq!(stdout_json(&validated)?["data"]["valid"], true);

    let linked = fixture.temporary().path().join("linked-bundle");
    fs::create_dir_all(&linked)?;
    let outside = fixture.temporary().path().join("outside-provider.json");
    fs::write(&outside, fs::read(valid.join("provider.json"))?)?;
    std::os::unix::fs::symlink(&outside, linked.join("provider.json"))?;
    let rejected = run_validate(&linked)?;
    assert_eq!(rejected.status.code(), Some(1));
    assert!(has_issue(
        &stdout_json(&rejected)?,
        "bundle_symlink_rejected"
    ));
    Ok(())
}

#[test]
fn operations_require_session_surfaces_and_non_authorizations() -> TestResult {
    let fixture = Fixture::new()?;
    let mut operation = operation_entry();
    operation["session_surfaces"] = json!([]);
    operation["does_not_authorize"] = json!([]);
    let invalid = fixture.write_provider("career", vec![operation])?;
    let report = run_validate(&invalid)?;
    assert_eq!(report.status.code(), Some(1));
    assert!(has_issue(&stdout_json(&report)?, "empty_field"));

    let valid_root =
        fixture.write_provider_named("career-valid", "career", 2, vec![operation_entry()])?;
    let valid = run_validate(&valid_root)?;
    assert!(valid.status.success());

    let operation_fixture = Fixture::new()?;
    let mut normalized_operation = operation_entry();
    normalized_operation["promise"] = normalized_promise();
    operation_fixture.write_v3_provider("career", vec![normalized_operation])?;
    let resolved = stdout_json(&operation_fixture.run_json(&["resolve", "career.jobs.line-up"])?)?;
    assert_eq!(resolved["data"]["status"], "resolved_not_ready");
    assert_eq!(resolved["data"]["readiness"], "session_dependent");
    assert_eq!(
        resolved["data"]["root"]["facet_coverage"]["interfaces"]["state"],
        "declared"
    );
    Ok(())
}

#[test]
fn resolve_assembles_an_exact_dossier_and_reports_legacy_dependency_gaps() -> TestResult {
    let fixture = Fixture::new()?;
    let alpha_root = fixture.write_v3_provider(
        "alpha",
        vec![normalized_capability(
            "alpha.run",
            &json!([{
                "id": "beta.run",
                "min_contract": 1,
                "max_contract_exclusive": 2
            }]),
        )],
    )?;
    fixture.write_v3_provider(
        "beta",
        vec![normalized_capability(
            "beta.run",
            &json!([{
                "id": "gamma.run",
                "min_contract": 1,
                "max_contract_exclusive": 2
            }]),
        )],
    )?;
    fixture.write_provider("gamma", vec![capability("gamma.run", &json!([]))])?;
    let before = tree_snapshot(fixture.registry())?;

    let resolved = fixture.run_json(&[
        "resolve",
        "alpha.run",
        "--min-contract",
        "1",
        "--max-contract-exclusive",
        "2",
        "--require",
        "data_semantics",
    ])?;
    assert_eq!(resolved.status.code(), Some(1));
    let resolved_json = stdout_json(&resolved)?;
    assert_eq!(resolved_json["ok"], false);
    assert_eq!(resolved_json["data"]["status"], "incomplete_declaration");
    assert_eq!(
        resolved_json["data"]["contract_requirement"]["satisfied"],
        true
    );
    assert_eq!(
        resolved_json["data"]["facet_requirements"]["unsatisfied"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        resolved_json["data"]["root"]["provider_promise_scope"]["inventory"]["completeness"],
        "complete"
    );
    assert_eq!(
        resolved_json["data"]["root"]["facet_coverage"]["data_semantics"]["state"],
        "declared"
    );
    assert_eq!(
        resolved_json["data"]["dependency_closure"][0]["entry"]["id"],
        "beta.run"
    );
    assert_eq!(
        resolved_json["data"]["dependency_closure"][1]["entry"]["id"],
        "gamma.run"
    );
    assert!(has_gap(&resolved_json, "provider_scope_undeclared"));
    assert!(has_gap(&resolved_json, "facet_undeclared"));
    assert_eq!(
        resolved_json["data"]["root"]["basis"]["provider_manifest"]["sha256"],
        sha256_file(&alpha_root.join("provider.json"))?
    );
    assert_eq!(
        resolved_json["data"]["root"]["basis"]["entry_contract"]["sha256"],
        sha256_file(&alpha_root.join("entries/alpha-run.json"))?
    );
    assert_eq!(
        resolved_json["data"]["root"]["basis"]["manual"]["sha256"],
        sha256_file(&alpha_root.join("manuals/alpha-run.md"))?
    );

    let human = fixture.run_human(&["resolve", "alpha.run"])?;
    assert_eq!(human.status.code(), Some(1));
    let human = String::from_utf8(human.stdout)?;
    assert!(human.contains("Resolved outward promise"));
    assert!(human.contains("DEPENDENCY CONTRACT"));
    assert!(human.contains("RESOLUTION GAPS"));
    assert!(human.contains("Readiness:           not_checked"));
    assert_eq!(before, tree_snapshot(fixture.registry())?);
    Ok(())
}

#[test]
fn fully_declared_resolution_succeeds_without_claiming_readiness() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.write_v3_provider(
        "alpha",
        vec![normalized_capability("alpha.run", &json!([]))],
    )?;

    let resolved = fixture.run_json(&[
        "resolve",
        "alpha.run",
        "--require",
        "outputs",
        "--require",
        "data_semantics",
    ])?;
    assert!(resolved.status.success());
    let resolved = stdout_json(&resolved)?;
    assert_eq!(resolved["ok"], true);
    assert_eq!(resolved["data"]["status"], "resolved_not_ready");
    assert_eq!(resolved["data"]["readiness"], "not_checked");
    assert_eq!(resolved["data"]["dependency_closure_status"], "complete");
    assert_eq!(
        resolved["data"]["dependency_closure"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(resolved["data"]["gaps"].as_array().map(Vec::len), Some(0));

    let unsatisfied = fixture.run_json(&["resolve", "alpha.run", "--require", "reliances"])?;
    assert_eq!(unsatisfied.status.code(), Some(1));
    let unsatisfied = stdout_json(&unsatisfied)?;
    assert_eq!(unsatisfied["data"]["status"], "incomplete_declaration");
    assert!(has_gap(&unsatisfied, "required_facet_unsatisfied"));
    Ok(())
}

#[test]
fn partial_provider_inventory_remains_an_explicit_resolution_gap() -> TestResult {
    let fixture = Fixture::new()?;
    let root = fixture.write_v3_provider(
        "alpha",
        vec![normalized_capability("alpha.run", &json!([]))],
    )?;
    let manifest_path = root.join("provider.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    manifest["promise_scope"]["inventory"]["completeness"] = json!("partial");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;

    let resolved = fixture.run_json(&["resolve", "alpha.run"])?;
    assert_eq!(resolved.status.code(), Some(1));
    let resolved = stdout_json(&resolved)?;
    assert_eq!(resolved["data"]["status"], "incomplete_declaration");
    assert!(has_gap(&resolved, "provider_inventory_partial"));
    Ok(())
}

#[test]
fn resolve_reports_dependency_and_uncontracted_reliance_gaps() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.write_v3_provider(
        "alpha",
        vec![normalized_capability(
            "alpha.run",
            &json!([{
                "id": "missing.run",
                "min_contract": 1,
                "max_contract_exclusive": 2
            }]),
        )],
    )?;
    let unavailable = fixture.run_json(&["resolve", "alpha.run"])?;
    assert_eq!(unavailable.status.code(), Some(1));
    let unavailable = stdout_json(&unavailable)?;
    assert_eq!(unavailable["data"]["status"], "dependency_unavailable");
    assert_eq!(
        unavailable["data"]["dependency_closure_status"],
        "unavailable"
    );
    assert_eq!(unavailable["data"]["root"]["entry"]["id"], "alpha.run");
    assert!(has_gap(&unavailable, "dependency_unavailable"));

    let mut reliant = normalized_capability("reliant.run", &json!([]));
    reliant["promise"]["reliances"] = json!([{
        "status": "declared",
        "statement": "The result reads upstream records without a published data contract.",
        "target": "upstream",
        "kind": "data"
    }]);
    fixture.write_v3_provider("reliant", vec![reliant])?;
    let unresolved = fixture.run_json(&["resolve", "reliant.run"])?;
    assert_eq!(unresolved.status.code(), Some(1));
    let unresolved = stdout_json(&unresolved)?;
    assert_eq!(unresolved["data"]["status"], "incomplete_declaration");
    assert!(has_gap(&unresolved, "uncontracted_reliance"));
    Ok(())
}

#[test]
fn resolve_requires_one_exact_id_and_valid_contract_bounds() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.write_v3_provider(
        "alpha",
        vec![normalized_capability("alpha.run", &json!([]))],
    )?;

    let semantic_text = fixture.run_json(&["resolve", "alpha.run", "extra words"])?;
    assert_eq!(semantic_text.status.code(), Some(2));
    let error = stderr_json(&semantic_text)?;
    assert_eq!(error["schema_version"], 2);
    assert_eq!(error["error"]["code"], "invalid_command");

    let removed_all = fixture.run_json(&["list", "--all"])?;
    assert_eq!(removed_all.status.code(), Some(2));
    assert_eq!(
        stderr_json(&removed_all)?["error"]["code"],
        "invalid_command"
    );

    let help = fixture.run_human(&["--help"])?;
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout)?.contains("  resolve"));

    let missing = fixture.run_json(&["resolve", "missing.entry"])?;
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(stderr_json(&missing)?["error"]["code"], "entry_not_found");

    let invalid_range = fixture.run_json(&[
        "resolve",
        "alpha.run",
        "--min-contract",
        "2",
        "--max-contract-exclusive",
        "2",
    ])?;
    assert_eq!(invalid_range.status.code(), Some(1));
    assert_eq!(
        stderr_json(&invalid_range)?["error"]["code"],
        "invalid_contract_range"
    );

    let incompatible = fixture.run_json(&["resolve", "alpha.run", "--min-contract", "2"])?;
    assert_eq!(incompatible.status.code(), Some(1));
    assert_eq!(
        stdout_json(&incompatible)?["data"]["status"],
        "contract_incompatible"
    );
    Ok(())
}

struct Fixture {
    temporary: TempDir,
    registry: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, io::Error> {
        let temporary = tempfile::tempdir()?;
        let registry = temporary.path().join("registry");
        fs::create_dir(&registry)?;
        Ok(Self {
            temporary,
            registry,
        })
    }

    fn temporary(&self) -> &TempDir {
        &self.temporary
    }

    fn registry(&self) -> &Path {
        &self.registry
    }

    fn write_provider(&self, id: &str, entries: Vec<Value>) -> Result<PathBuf, Box<dyn Error>> {
        self.write_provider_named(id, id, 2, entries)
    }

    fn write_legacy_provider(
        &self,
        id: &str,
        entries: Vec<Value>,
    ) -> Result<PathBuf, Box<dyn Error>> {
        self.write_provider_named(id, id, 1, entries)
    }

    fn write_v3_provider(&self, id: &str, entries: Vec<Value>) -> Result<PathBuf, Box<dyn Error>> {
        let root = self.write_provider_named(id, id, 3, entries)?;
        let manifest_path = root.join("provider.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        manifest["promise_scope"] = provider_promise_scope();
        fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
        Ok(root)
    }

    fn write_provider_named(
        &self,
        directory: &str,
        id: &str,
        schema_version: u32,
        entries: Vec<Value>,
    ) -> Result<PathBuf, Box<dyn Error>> {
        let root = self.registry.join(directory);
        fs::create_dir_all(root.join("entries"))?;
        fs::create_dir_all(root.join("manuals"))?;
        let mut indexed = Vec::new();
        for entry in entries {
            let entry_id = required_text(&entry, "id")?;
            let manual = required_text(&entry, "manual")?;
            let filename = format!("{}.json", entry_id.replace('.', "-"));
            let indexed_path = format!("entries/{filename}");
            fs::write(root.join(&indexed_path), serde_json::to_vec_pretty(&entry)?)?;
            fs::write(
                root.join(manual),
                format!("# {entry_id}\n\nTodo is authoritative for its domain result.\n"),
            )?;
            indexed.push(indexed_path);
        }
        let manifest = json!({
            "schema_version": schema_version,
            "provider": {
                "id": id,
                "name": title(id),
                "release": "1.0.0"
            },
            "entries": indexed
        });
        fs::write(
            root.join("provider.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        Ok(root)
    }

    fn run_json(&self, arguments: &[&str]) -> Result<Output, io::Error> {
        run_command(self.registry(), arguments)
    }

    fn run_human(&self, arguments: &[&str]) -> Result<Output, io::Error> {
        Command::new(env!("CARGO_BIN_EXE_chancery"))
            .arg("--registry")
            .arg(self.registry())
            .args(arguments)
            .output()
    }
}

fn run_command(registry: &Path, arguments: &[&str]) -> Result<Output, io::Error> {
    Command::new(env!("CARGO_BIN_EXE_chancery"))
        .arg("--registry")
        .arg(registry)
        .arg("--json")
        .args(arguments)
        .output()
}

fn run_validate(bundle: &Path) -> Result<Output, io::Error> {
    Command::new(env!("CARGO_BIN_EXE_chancery"))
        .arg("--json")
        .arg("validate")
        .arg(bundle)
        .output()
}

fn capability(id: &str, dependencies: &Value) -> Value {
    json!({
        "id": id,
        "contract_version": 1,
        "kind": "capability",
        "mode": "use",
        "support": "supported",
        "title": format!("Use {id}"),
        "summary": format!("Perform {id}"),
        "use_when": ["The durable outcome is requested."],
        "do_not_use_when": ["Immediate unrelated work is requested."],
        "outcome": "The provider establishes its documented domain result.",
        "effects": ["May write provider-owned state when separately invoked."],
        "authority": ["Provider state is authoritative."],
        "success": ["The provider reports domain success."],
        "failure_and_recovery": ["Inspect provider state before retrying."],
        "privacy": ["Inputs may enter provider-owned state."],
        "interfaces": [{
            "label": "Run",
            "invocation": format!("/Users/joey/.local/bin/{} run", id.split('.').next().unwrap_or("provider"))
        }],
        "dependencies": dependencies,
        "does_not_authorize": [],
        "manual": format!("manuals/{}.md", id.replace('.', "-"))
    })
}

fn normalized_capability(id: &str, dependencies: &Value) -> Value {
    let mut entry = capability(id, dependencies);
    entry["promise"] = normalized_promise();
    entry
}

fn normalized_promise() -> Value {
    json!({
        "consumers": [claim("declared", "Local callers may use this capability.")],
        "preconditions": [claim("declared", "The installed provider is available.")],
        "inputs": [claim("declared", "The documented input is accepted.")],
        "outputs": [claim("declared", "The documented output is returned.")],
        "data_semantics": [claim("declared", "Output fields retain their documented meaning.")],
        "identity_and_units": [claim("declared", "Stable IDs identify records.")],
        "completeness_and_freshness": [claim("declared", "The output states its current coverage.")],
        "access": [claim("declared", "Access is local and read-only.")],
        "lifecycle_and_consistency": [claim("declared", "One invocation observes one provider view.")],
        "operational_limits": [claim("declared", "The documented bounds apply.")],
        "compatibility_and_evolution": [claim("declared", "Contract version identifies compatibility.")],
        "reliances": [claim("not_applicable", "No substantive cross-system reliance exists.")]
    })
}

fn claim(status: &str, statement: &str) -> Value {
    json!({"status": status, "statement": statement})
}

fn provider_promise_scope() -> Value {
    json!({
        "authoritative_for": ["The provider owns its domain result."],
        "not_authoritative_for": ["The caller owns its domain use."],
        "inventory": {
            "covers": ["All supported public CLI outcomes in this fixture."],
            "completeness": "complete",
            "excludes": ["Implementation helpers and live readiness."]
        },
        "shared_access_and_trust": ["Interfaces are local."],
        "shared_privacy_and_retention": ["The provider retains no resolver state."],
        "compatibility_and_retirement": ["Contract versions identify compatibility."],
        "operational_limits": ["Per-entry bounds apply."]
    })
}

fn legacy_capability(id: &str, trigger: &str, dependencies: &Value) -> Value {
    let mut entry = capability(id, dependencies);
    entry["routable"] = json!(true);
    entry["routing"] = json!({
        "triggers": [trigger],
        "exclusions": ["do this now"]
    });
    entry
}

fn execution_entry() -> Value {
    capability("nucleus.execution", &json!([]))
}

fn todo_entry() -> Value {
    capability(
        "todo.concern.capture-and-route",
        &json!([{
            "id": "nucleus.execution",
            "min_contract": 1,
            "max_contract_exclusive": 2
        }]),
    )
}

fn dependent_entry(id: &str, dependency: &str) -> Value {
    capability(
        id,
        &json!([{
            "id": dependency,
            "min_contract": 1,
            "max_contract_exclusive": 2
        }]),
    )
}

fn operation_entry() -> Value {
    json!({
        "id": "career.jobs.line-up",
        "contract_version": 1,
        "kind": "operation",
        "mode": "operate",
        "support": "supported",
        "title": "Line up jobs",
        "summary": "Find and verify strong live roles.",
        "use_when": ["Live job discovery is requested."],
        "do_not_use_when": ["An application submission is requested."],
        "outcome": "A verified conversational shortlist.",
        "effects": ["Reads live postings."],
        "authority": ["The canonical posting establishes availability."],
        "success": ["Every shortlisted role was verified live."],
        "failure_and_recovery": ["Stop when canonical status cannot be verified."],
        "privacy": ["No persistent application record is created."],
        "dependencies": [],
        "session_surfaces": ["browser"],
        "does_not_authorize": ["Submitting an application."],
        "runtime": "interactive_agent",
        "automation": "none",
        "steps": ["Search live postings."],
        "checkpoints": ["Open the canonical posting."],
        "adaptation": ["Reobserve current semantic controls."],
        "stop_when": ["Live access is unavailable."],
        "manual": "manuals/career-jobs-line-up.md"
    })
}

fn required_text<'a>(value: &'a Value, key: &str) -> Result<&'a str, io::Error> {
    value[key]
        .as_str()
        .ok_or_else(|| io::Error::other(format!("fixture field {key} must be text")))
}

fn title(value: &str) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

fn stdout_json(output: &Output) -> Result<Value, serde_json::Error> {
    serde_json::from_slice(&output.stdout)
}

fn stderr_json(output: &Output) -> Result<Value, serde_json::Error> {
    serde_json::from_slice(&output.stderr)
}

fn has_issue(value: &Value, code: &str) -> bool {
    value["data"]["issues"]
        .as_array()
        .is_some_and(|issues| issues.iter().any(|issue| issue["code"] == code))
}

fn has_gap(value: &Value, code: &str) -> bool {
    value["data"]["gaps"]
        .as_array()
        .is_some_and(|gaps| gaps.iter().any(|gap| gap["code"] == code))
}

fn tree_snapshot(
    root: &Path,
) -> Result<Vec<(PathBuf, u64, Option<std::time::SystemTime>)>, io::Error> {
    fn walk(
        root: &Path,
        current: &Path,
        values: &mut Vec<(PathBuf, u64, Option<std::time::SystemTime>)>,
    ) -> Result<(), io::Error> {
        let mut children = fs::read_dir(current)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        children.sort();
        for path in children {
            let metadata = fs::symlink_metadata(&path)?;
            values.push((
                path.strip_prefix(root)
                    .map_err(|error| io::Error::other(error.to_string()))?
                    .to_path_buf(),
                metadata.len(),
                metadata.modified().ok(),
            ));
            if metadata.is_dir() {
                walk(root, &path, values)?;
            }
        }
        Ok(())
    }

    let mut values = Vec::new();
    walk(root, root, &mut values)?;
    Ok(values)
}

fn sha256_file(path: &Path) -> Result<String, io::Error> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}
