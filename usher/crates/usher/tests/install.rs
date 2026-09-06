#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture {
    _temporary: TempDir,
    root: PathBuf,
    home: PathBuf,
    source: PathBuf,
    bundle: PathBuf,
    candidate: PathBuf,
    binary: PathBuf,
    installer: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().canonicalize()?;
        let home = root.join("Operator Home");
        let source = root.join("source");
        let bundle = source.join("usher/chancery");
        let candidate = root.join("candidate");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&bundle)?;
        fs::create_dir_all(bundle.join("entries"))?;
        fs::create_dir_all(bundle.join("manuals"))?;
        fs::write(bundle.join("entries/recognition.json"), "{}")?;
        fs::write(bundle.join("manuals/recognition.md"), "# Recognition\n")?;
        fs::create_dir_all(candidate.join("bin"))?;
        fs::write(
            bundle.join("provider.json"),
            serde_json::to_vec(&json!({
                "schema_version": 3,
                "provider": {"id": "usher", "name": "Usher", "release": env!("CARGO_PKG_VERSION")},
                "entries": []
            }))?,
        )?;
        let binary = candidate.join("bin/usher");
        let installer = candidate.join("bin/usher-install");
        fs::copy(env!("CARGO_BIN_EXE_usher"), &binary)?;
        fs::copy(env!("CARGO_BIN_EXE_usher-install"), &installer)?;
        Ok(Self {
            _temporary: temporary,
            root,
            home,
            source,
            bundle,
            candidate,
            binary,
            installer,
        })
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.installer);
        command.env("HOME", &self.home);
        command
    }

    fn install(&self) -> Result<Value, Box<dyn Error>> {
        let output = self
            .command()
            .arg("install")
            .arg("--binary")
            .arg(&self.binary)
            .arg("--bundle")
            .arg(&self.bundle)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    fn request(&self) -> Result<Value, Box<dyn Error>> {
        let mut binaries = BTreeMap::new();
        for (name, path) in [("usher", &self.binary), ("usher-install", &self.installer)] {
            binaries.insert(name, json!({"path": format!("bin/{name}"), "sha256": cell_install::file_digest(path)?, "version": format!("{name} {}", env!("CARGO_PKG_VERSION"))}));
        }
        let source_inputs: BTreeMap<_, _> = cell_install::provider_inventory(
            &self.bundle,
            &cell_install::InstallSpec {
                product: "usher",
                application: "Usher",
                commands: &["usher", "usher-install"],
                provider: "usher",
            },
        )?
        .into_iter()
        .map(|(path, entry)| (format!("usher/chancery/{path}"), entry.sha256))
        .collect();
        let mut candidate = json!({
            "schema": 1, "product": "usher", "source_commit": "a".repeat(40),
            "source_key": "source:test", "source_inputs": source_inputs, "binaries": binaries
        });
        let mut bytes = serde_json::to_vec(&candidate)?;
        bytes.push(b'\n');
        candidate["candidate_id"] = json!(format!("sha256:{:x}", Sha256::digest(bytes)));
        Ok(
            json!({"schema":1,"product":"usher","run_id":"test-run", "run_dir":self.root,
            "source_root":self.source,"candidate_dir":self.candidate,"candidate":candidate,
            "prior":null,"selected_products":["usher"],"recovery":null}),
        )
    }

    fn adapter(&self, operation: &str, request: &Value) -> Result<(bool, Value), Box<dyn Error>> {
        let mut child = self
            .command()
            .args(["adapter", operation])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or("missing child stdin")?
            .write_all(&serde_json::to_vec(request)?)?;
        let output = child.wait_with_output()?;
        Ok((
            output.status.success(),
            serde_json::from_slice(&output.stdout)?,
        ))
    }
}

#[test]
fn installs_both_commands_and_verifies_exact_candidate_without_state() -> TestResult {
    let fixture = Fixture::new()?;
    let reply = fixture.install()?;
    let release_id = reply["data"]["release_id"]
        .as_str()
        .ok_or("missing release")?;
    let release = fixture
        .home
        .join("Library/Application Support/Usher/install/releases")
        .join(release_id);
    assert!(release.join("package/install").is_file());
    assert!(fixture.home.join(".local/bin/usher-install").is_symlink());
    let output = fixture
        .command()
        .arg("verify")
        .arg("--binary")
        .arg(&fixture.binary)
        .arg("--bundle")
        .arg(&fixture.bundle)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let output = fixture
        .command()
        .arg("verify-release")
        .arg(&release)
        .output()?;
    assert!(output.status.success());
    assert!(
        !fixture
            .home
            .join("Library/Application Support/Usher/usher.db")
            .exists()
    );
    let repeated = fixture.install()?;
    assert_eq!(reply["data"], repeated["data"]);
    Ok(())
}

#[test]
fn adapter_runs_complete_lifecycle_and_recovers_lost_apply_reply() -> TestResult {
    let fixture = Fixture::new()?;
    let mut request = fixture.request()?;
    let (success, inspected) = fixture.adapter("inspect", &request)?;
    assert!(success, "{inspected}");
    request["prior"] = inspected["data"].clone();
    for (operation, status) in [
        ("hold", "held"),
        ("drain", "drained"),
        ("apply", "applied"),
        ("verify", "verified"),
    ] {
        let (success, reply) = fixture.adapter(operation, &request)?;
        assert!(success, "{operation}: {reply}");
        assert_eq!(reply["status"], status);
    }
    request["recovery"] = json!({"apply_started":true,"applied":false,"any_apply_started":true});
    let (success, recovered) = fixture.adapter("recover", &request)?;
    assert!(success, "{recovered}");
    assert_eq!(recovered["data"]["installed"], "candidate");
    assert_eq!(recovered["data"]["safe_to_release"], true);
    assert!(fixture.adapter("release", &request)?.0);
    Ok(())
}

#[test]
fn adapter_rejects_forged_evidence_and_stale_baseline() -> TestResult {
    let fixture = Fixture::new()?;
    let mut request = fixture.request()?;
    let (_, inspected) = fixture.adapter("inspect", &request)?;
    request["prior"] = inspected["data"].clone();
    fixture.install()?;
    let (success, reply) = fixture.adapter("apply", &request)?;
    assert!(!success);
    assert_eq!(reply["status"], "stopped");
    request["candidate"]["candidate_id"] = json!("sha256:forged");
    assert!(!fixture.adapter("inspect", &request)?.0);
    let mut request = fixture.request()?;
    request["product"] = json!("foreign");
    assert!(!fixture.adapter("inspect", &request)?.0);
    Ok(())
}

#[test]
fn adapter_accepts_python_candidate_hashes_with_unicode_source_paths() -> TestResult {
    let fixture = Fixture::new()?;
    let filename = "café-🙂.md";
    let mut request = fixture.request()?;
    fs::write(
        fixture.bundle.join("manuals").join(filename),
        "Unicode provider material",
    )?;
    let candidate = &mut request["candidate"];
    candidate["source_inputs"][format!("usher/chancery/manuals/{filename}")] = json!(
        cell_install::file_digest(&fixture.bundle.join("manuals").join(filename))?
    );
    candidate
        .as_object_mut()
        .ok_or("missing candidate")?
        .remove("candidate_id");
    let encoded =
        serde_json::to_string(candidate)?.replace(filename, "caf\\u00e9-\\ud83d\\ude42.md") + "\n";
    candidate["candidate_id"] = json!(format!("sha256:{:x}", Sha256::digest(encoded.as_bytes())));
    let (success, reply) = fixture.adapter("inspect", &request)?;
    assert!(success, "{reply}");
    Ok(())
}

#[test]
fn adapter_rejects_unlisted_provider_files() -> TestResult {
    let fixture = Fixture::new()?;
    let request = fixture.request()?;
    fs::write(
        fixture.bundle.join("manuals/unlisted.md"),
        "not in passed candidate",
    )?;
    let (success, reply) = fixture.adapter("inspect", &request)?;
    assert!(!success);
    assert_eq!(reply["status"], "stopped");
    Ok(())
}

#[test]
fn interrupted_first_publication_can_recover_to_captured_absence() -> TestResult {
    let fixture = Fixture::new()?;
    let mut request = fixture.request()?;
    let (success, inspected) = fixture.adapter("inspect", &request)?;
    assert!(success, "{inspected}");
    request["prior"] = inspected["data"].clone();
    fixture.install()?;
    // The process can die after publishing public links but before current.
    fs::remove_file(
        fixture
            .home
            .join("Library/Application Support/Usher/install/current"),
    )?;
    request["recovery"] = json!({"apply_started":true,"applied":false,"any_apply_started":true});
    let (success, reply) = fixture.adapter("recover", &request)?;
    assert!(success, "{reply}");
    assert_eq!(reply["data"]["installed"], "prior");
    assert_eq!(reply["data"]["safe_to_release"], true);
    let output = fixture.command().arg("inspect").output()?;
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout)?["data"],
        Value::Null
    );
    Ok(())
}

#[test]
fn exact_provider_changes_invalidate_candidate_verification() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.install()?;
    fs::write(
        fixture.bundle.join("manuals/added.md"),
        "changed provider bytes",
    )?;
    let output = fixture
        .command()
        .arg("verify")
        .arg("--binary")
        .arg(&fixture.binary)
        .arg("--bundle")
        .arg(&fixture.bundle)
        .output()?;
    assert!(!output.status.success());
    Ok(())
}

#[test]
fn foreign_recovery_path_and_stale_selection_are_rejected() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.install()?;
    let output = fixture
        .command()
        .arg("install")
        .arg("--binary")
        .arg(&fixture.binary)
        .arg("--bundle")
        .arg(&fixture.bundle)
        .args(["--expected-current", "absent"])
        .output()?;
    assert!(!output.status.success());
    let output = fixture
        .command()
        .arg("recover")
        .arg("--release")
        .arg(Path::new("/tmp/foreign"))
        .output()?;
    assert!(!output.status.success());
    Ok(())
}

#[test]
fn direct_install_rejects_an_installer_from_another_product_release() -> TestResult {
    let fixture = Fixture::new()?;
    let path = fixture.bundle.join("provider.json");
    let mut provider: Value = serde_json::from_slice(&fs::read(&path)?)?;
    provider["provider"]["release"] = json!("99.0.0");
    fs::write(path, serde_json::to_vec(&provider)?)?;
    let output = fixture
        .command()
        .arg("install")
        .arg("--binary")
        .arg(&fixture.binary)
        .arg("--bundle")
        .arg(&fixture.bundle)
        .output()?;
    assert!(!output.status.success());
    assert!(!fixture.home.join(".local/bin/usher").exists());
    Ok(())
}
