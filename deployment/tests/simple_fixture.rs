use cell_install::simple::Spec;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    binary: PathBuf,
    reader: PathBuf,
    spec: Spec,
}

fn write_executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let home = root.join("home");
        fs::create_dir(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        let binary = root.join("payload");
        let reader = root.join("reader");
        write_executable(&reader, "#!/bin/sh\nexit 0\n");
        let value = Self {
            _temp: temp,
            home,
            binary,
            reader,
            spec: specification(),
        };
        value.payload("first", false);
        value
    }
    fn payload(&self, revision: &str, fail_help: bool) {
        write_executable(
            &self.binary,
            &format!(
                "#!/bin/sh\n# {revision}\ncase \"${{1-}}\" in\n--version) printf '%s\\n' '{} {}';;\n--help) exit {};;\necho-input) /bin/cat;;\n*) exit 0;;\nesac\n",
                self.spec.product,
                env!("CARGO_PKG_VERSION"),
                if fail_help { 7 } else { 0 }
            ),
        );
    }
    fn run(&self, operation: &str, extra: &[&str]) -> Output {
        let mut command = Command::new(installer());
        command
            .env("HOME", &self.home)
            .arg(operation)
            .arg("--home")
            .arg(&self.home);
        if matches!(operation, "install" | "verify") {
            command
                .arg("--binary")
                .arg(&self.binary)
                .arg("--bundle")
                .arg(source_root().join(self.spec.provider_source));
        }
        if self.spec.product == "clockwork" {
            command.arg("--chancery").arg(&self.reader);
        }
        command.args(extra).output().unwrap()
    }
    fn success(&self, operation: &str, extra: &[&str]) -> Value {
        let output = self.run(operation, extra);
        assert!(
            output.status.success(),
            "{}: {} {}",
            operation,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn root(&self) -> PathBuf {
        self.home
            .join("Library/Application Support")
            .join(self.spec.application)
            .join("install")
    }
}

#[test]
fn installs_exact_payload_and_provider_and_is_idempotent() {
    let fixture = Fixture::new();
    let first = fixture.success("install", &[]);
    assert_eq!(first["data"]["format"], "cell-install-v2");
    assert_eq!(fixture.success("install", &[])["data"], first["data"]);
    fixture.success("verify", &[]);
    let release = fixture
        .root()
        .join("releases")
        .join(first["data"]["release_id"].as_str().unwrap());
    let output = Command::new(installer())
        .arg("verify-release")
        .arg(&release)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!fixture.root().join("previous").exists());
    fixture.payload("different", false);
    assert!(!fixture.run("verify", &[]).status.success());
    let second = fixture.success("install", &[]);
    assert_ne!(first["data"]["release_id"], second["data"]["release_id"]);
    assert_eq!(
        fs::read_link(fixture.root().join("previous")).unwrap(),
        PathBuf::from(format!(
            "releases/{}",
            first["data"]["release_id"].as_str().unwrap()
        ))
    );
}

#[test]
fn publication_failure_restores_prior_and_stale_selection_refuses_change() {
    let fixture = Fixture::new();
    let first = fixture.success("install", &[]);
    fixture.payload("broken-help", true);
    let failed = fixture.run("install", &[]);
    assert!(!failed.status.success());
    let detail: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(detail["error"]["disposition"], "restored");
    assert_eq!(
        fixture.success("inspect", &[])["data"]["current"]["release_id"],
        first["data"]["release_id"]
    );
    fixture.payload("valid-next", false);
    assert!(
        !fixture
            .run("install", &["--expected-current", "absent"])
            .status
            .success()
    );
    assert_eq!(
        fixture.success("inspect", &[])["data"]["current"]["release_id"],
        first["data"]["release_id"]
    );
}

#[test]
fn foreign_public_paths_are_preserved() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.home.join(".local/bin")).unwrap();
    let public = fixture.home.join(".local/bin").join(fixture.spec.product);
    symlink("/foreign/program", &public).unwrap();
    assert!(!fixture.run("install", &[]).status.success());
    assert_eq!(
        fs::read_link(&public).unwrap(),
        Path::new("/foreign/program")
    );
    assert!(!fixture.root().join("current").exists());
}

#[test]
fn public_frontend_preserves_standard_input() {
    let fixture = Fixture::new();
    fixture.success("install", &[]);
    let mut child = Command::new(fixture.home.join(".local/bin").join(fixture.spec.product))
        .arg("echo-input")
        .env("HOME", &fixture.home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"fixture body\nwith two lines\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"fixture body\nwith two lines\n");
}
