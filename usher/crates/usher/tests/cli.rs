use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn Error>>;

fn write(root: &Path, path: &str, text: &str) -> TestResult {
    let path = root.join(path);
    fs::create_dir_all(path.parent().ok_or("missing parent")?)?;
    fs::write(path, text)?;
    Ok(())
}

fn descriptor(root: &Path, id: &str, product_root: &str, providers: &str) -> TestResult {
    write(
        root,
        &format!("pipeline/products/{id}.sh"),
        &format!(
            "PIPELINE_SCHEMA=1\nPRODUCT_ID={id}\nPRODUCT_NAME={id}\nPRODUCT_DIR={product_root}\nPROVIDERS='{providers}'\n"
        ),
    )
}

fn provider(root: &Path, product: &str, id: &str) -> TestResult {
    let base = format!("{product}/chancery/{id}");
    write(
        root,
        &format!("{base}/provider.json"),
        &json!({
            "schema_version": 3,
            "provider": {"id": id, "name": id, "release": "0.1.0"},
            "entries": ["entries/read.json"]
        })
        .to_string(),
    )?;
    write(
        root,
        &format!("{base}/entries/read.json"),
        &json!({
            "id": format!("{id}.records.read"), "contract_version": 1,
            "manual": "manuals/read.md"
        })
        .to_string(),
    )?;
    write(
        root,
        &format!("{base}/manuals/read.md"),
        "# A deliberately minimal manual\n",
    )
}

fn product(root: &Path, id: &str) -> TestResult {
    descriptor(root, id, id, &format!("{id}|{id}|{id}/chancery/{id}|1"))?;
    write(
        root,
        &format!("{id}/AGENTS.md"),
        &format!("Semantics-Project: {id}\n"),
    )?;
    provider(root, id, id)
}

fn fixture() -> Result<TempDir, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    product(temp.path(), "alpha")?;
    Ok(temp)
}

fn run(root: &Path, verb: &str, selection: Option<&str>) -> Result<(i32, Value), Box<dyn Error>> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_usher"));
    // A recognition command must not need installed programs, HOME, or registries.
    command.env_clear().args(["--json", verb]).arg(root);
    if let Some(selection) = selection {
        command.args(["--product", selection]);
    }
    let output = command.output()?;
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok((
        output.status.code().ok_or("terminated by signal")?,
        serde_json::from_slice(&output.stdout)?,
    ))
}

#[test]
fn declarations_pass_without_installed_services_or_quality_requirements() -> TestResult {
    let temp = fixture()?;
    // No vocabulary or service registry exists, and the provider has no normalized
    // promise. Complete Chancery validity remains Chancery's separate concern.
    let (status, first) = run(temp.path(), "check", None)?;
    assert_eq!(status, 0);
    assert_eq!(first["scope"], "repository_declarations");
    assert_eq!(first["complete"], 1);
    assert_eq!(first["incomplete"], 0);
    assert_eq!(
        first["products"][0]["semantics"]["identities"],
        json!(["alpha"])
    );
    let (_, second) = run(temp.path(), "report", None)?;
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn product_missing_both_introductions_is_reported_and_fails_check() -> TestResult {
    let temp = fixture()?;
    descriptor(temp.path(), "beta", "beta", "")?;
    fs::create_dir(temp.path().join("beta"))?;
    let (status, report) = run(temp.path(), "report", None)?;
    assert_eq!(status, 0);
    assert_eq!(report["incomplete"], 1);
    assert_eq!(report["products"][1]["id"], "beta");
    assert_eq!(report["products"][1]["semantics"]["status"], "missing");
    assert_eq!(report["products"][1]["chancery"]["status"], "missing");
    let (status, checked) = run(temp.path(), "check", None)?;
    assert_eq!(status, 1);
    assert_eq!(checked, report);
    Ok(())
}

#[test]
fn markers_are_exact_local_and_unambiguous() -> TestResult {
    let temp = fixture()?;
    for marker in [
        " Semantics-Project: alpha\n",
        "Semantics-Project: Alpha\n",
        "Semantics-Project: alpha \n",
        "Semantics-Project: alpha\nSemantics-Project: beta\n",
    ] {
        write(temp.path(), "alpha/AGENTS.md", marker)?;
        let (status, report) = run(temp.path(), "check", None)?;
        assert_eq!(status, 1);
        assert_eq!(report["products"][0]["semantics"]["status"], "invalid");
    }
    write(temp.path(), "AGENTS.md", "Semantics-Project: alpha\n")?;
    write(
        temp.path(),
        "alpha/nested/AGENTS.md",
        "Semantics-Project: alpha\n",
    )?;
    fs::remove_file(temp.path().join("alpha/AGENTS.md"))?;
    let (_, report) = run(temp.path(), "check", None)?;
    assert_eq!(report["products"][0]["semantics"]["status"], "missing");
    Ok(())
}

#[test]
fn multiple_providers_and_renamed_product_aliases_are_preserved() -> TestResult {
    let temp = fixture()?;
    provider(temp.path(), "alpha", "alpha-usage")?;
    descriptor(
        temp.path(),
        "alpha",
        "alpha",
        "alpha|alpha|alpha/chancery/alpha|1\nalpha-usage|alpha-usage|alpha/chancery/alpha-usage|1",
    )?;
    let path = temp.path().join("pipeline/products/alpha.sh");
    let source = fs::read_to_string(&path)? + "PRODUCT_ALIASES='new-name alpha'\n";
    fs::write(&path, source)?;
    let (status, report) = run(temp.path(), "check", Some("new-name"))?;
    assert_eq!(status, 0);
    assert_eq!(
        report["products"][0]["chancery"]["identities"],
        json!(["alpha", "alpha-usage"])
    );
    assert_eq!(report["products"][0]["id"], "alpha");
    assert_eq!(run(temp.path(), "check", Some("unknown"))?.0, 2);
    Ok(())
}

#[test]
fn collisions_remain_visible_when_selecting_one_product() -> TestResult {
    let temp = fixture()?;
    product(temp.path(), "beta")?;
    write(temp.path(), "beta/AGENTS.md", "Semantics-Project: alpha\n")?;
    provider(temp.path(), "beta", "alpha")?;
    descriptor(
        temp.path(),
        "beta",
        "beta",
        "beta|alpha|beta/chancery/alpha|1",
    )?;
    let (status, report) = run(temp.path(), "check", Some("alpha"))?;
    assert_eq!(status, 1);
    assert_eq!(report["products"][0]["semantics"]["status"], "invalid");
    assert_eq!(report["products"][0]["chancery"]["status"], "invalid");
    assert!(report.to_string().contains("beta"));
    Ok(())
}

#[test]
fn root_and_alias_collisions_are_identity_failures() -> TestResult {
    let temp = fixture()?;
    descriptor(temp.path(), "beta", "alpha/", "")?;
    let path = temp.path().join("pipeline/products/beta.sh");
    fs::write(
        &path,
        fs::read_to_string(&path)? + "PRODUCT_ALIASES='alpha'\n",
    )?;
    let (status, report) = run(temp.path(), "check", None)?;
    assert_eq!(status, 1);
    for index in 0..2 {
        assert_eq!(report["products"][index]["identity"]["status"], "invalid");
    }
    assert!(report.to_string().contains("duplicate product root"));
    assert_eq!(run(temp.path(), "report", Some("alpha"))?.0, 2);
    Ok(())
}

#[test]
fn broken_provider_material_cannot_satisfy_an_introduction() -> TestResult {
    let temp = fixture()?;
    let manifest = "alpha/chancery/alpha/provider.json";
    write(temp.path(), manifest, "{}")?;
    let (_, report) = run(temp.path(), "check", None)?;
    assert_eq!(report["products"][0]["chancery"]["status"], "invalid");
    provider(temp.path(), "alpha", "alpha")?;
    fs::remove_file(temp.path().join("alpha/chancery/alpha/entries/read.json"))?;
    assert_eq!(
        run(temp.path(), "check", None)?.1["products"][0]["chancery"]["status"],
        "missing"
    );
    provider(temp.path(), "alpha", "alpha")?;
    write(temp.path(), "alpha/chancery/alpha/manuals/read.md", " \n")?;
    assert_eq!(
        run(temp.path(), "check", None)?.1["products"][0]["chancery"]["status"],
        "invalid"
    );
    Ok(())
}

#[test]
fn future_provider_format_is_unassessed_and_other_products_survive() -> TestResult {
    let temp = fixture()?;
    product(temp.path(), "beta")?;
    let path = temp.path().join("alpha/chancery/alpha/provider.json");
    let mut manifest: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    manifest["schema_version"] = json!(99);
    fs::write(path, manifest.to_string())?;
    let (status, report) = run(temp.path(), "check", None)?;
    assert_eq!(status, 1);
    assert_eq!(report["products"][0]["chancery"]["status"], "unassessed");
    assert_eq!(report["products"][1]["complete"], true);
    Ok(())
}

#[test]
fn executable_descriptor_syntax_is_never_run() -> TestResult {
    let temp = fixture()?;
    let path = temp.path().join("pipeline/products/alpha.sh");
    let sentinel = temp.path().join("must-not-exist");
    fs::write(
        &path,
        format!(
            "{}\nDANGER=$(touch '{}')\n",
            fs::read_to_string(&path)?,
            sentinel.display()
        ),
    )?;
    let (status, report) = run(temp.path(), "check", None)?;
    assert_eq!(status, 1);
    assert!(!sentinel.exists());
    assert_eq!(report["products"][0]["identity"]["status"], "unassessed");
    Ok(())
}

#[test]
fn empty_or_missing_inventory_never_reports_a_pass() -> TestResult {
    let temp = tempfile::tempdir()?;
    assert_eq!(run(temp.path(), "report", None)?.0, 2);
    fs::create_dir_all(temp.path().join("pipeline/products"))?;
    assert_eq!(run(temp.path(), "check", None)?.0, 2);
    Ok(())
}

#[test]
fn provider_identity_and_entry_identity_must_agree_with_the_inventory() -> TestResult {
    let temp = fixture()?;
    write(
        temp.path(),
        "alpha/chancery/alpha/entries/read.json",
        &json!({
            "id": "foreign.records.read", "contract_version": 1, "manual": "manuals/read.md"
        })
        .to_string(),
    )?;
    assert_eq!(run(temp.path(), "check", None)?.0, 1);
    provider(temp.path(), "alpha", "alpha")?;
    descriptor(
        temp.path(),
        "alpha",
        "alpha",
        "alpha|foreign|alpha/chancery/alpha|1",
    )?;
    assert_eq!(run(temp.path(), "check", None)?.0, 1);
    Ok(())
}

#[test]
fn parent_paths_and_foreign_bundles_are_rejected() -> TestResult {
    let temp = fixture()?;
    product(temp.path(), "beta")?;
    descriptor(
        temp.path(),
        "alpha",
        "alpha",
        "alpha|beta|beta/chancery/beta|1",
    )?;
    assert_eq!(run(temp.path(), "check", Some("alpha"))?.0, 1);
    descriptor(
        temp.path(),
        "alpha",
        "alpha",
        "alpha|alpha|alpha/chancery/alpha|1",
    )?;
    write(
        temp.path(),
        "alpha/chancery/alpha/entries/read.json",
        &json!({
            "id": "alpha.records.read", "contract_version": 1, "manual": "../../../AGENTS.md"
        })
        .to_string(),
    )?;
    assert_eq!(run(temp.path(), "check", Some("alpha"))?.0, 1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn symbolic_evidence_is_invalid_even_when_the_target_is_valid() -> TestResult {
    use std::os::unix::fs::symlink;
    let temp = fixture()?;
    write(temp.path(), "marker.md", "Semantics-Project: alpha\n")?;
    let marker = temp.path().join("alpha/AGENTS.md");
    fs::remove_file(&marker)?;
    symlink(temp.path().join("marker.md"), marker)?;
    let (status, report) = run(temp.path(), "check", None)?;
    assert_eq!(status, 1);
    assert_eq!(report["products"][0]["semantics"]["status"], "invalid");
    Ok(())
}
