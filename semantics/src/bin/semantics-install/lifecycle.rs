use super::{Candidate, HomeArgs, Result, fail};
use cell_install::transaction::{
    InstallLayout, LockKind, LockSpec, PreparedRelease, ProviderSpec, PublicEntry, PublicKind,
    ReleaseInfo, ReleasePlan, SourceFile,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub(super) const KEY: &str = "semantics/worker";
const FRONTEND: &str = include_str!("../../../packaging/macos/semantics");
const RUNNER: &str = include_str!("../../../packaging/macos/semantics-worker");
const TEMPLATE: &str = include_str!("../../../packaging/macos/semantics-worker.clockwork.toml.in");

pub(super) fn layout() -> InstallLayout {
    InstallLayout {
        product: "semantics".into(),
        application: "Semantics".into(),
        product_lock: LockSpec {
            path: "Library/Application Support/Semantics/install/.update-lock".into(),
            kind: LockKind::Directory,
        },
        catalog_lock: Some(LockSpec {
            path: "Library/Application Support/Chancery/.catalog-update-lock".into(),
            kind: LockKind::Shlock,
        }),
        public: vec![
            PublicEntry {
                path: ".local/bin/semantics".into(),
                artifact: "bin/semantics".into(),
                kind: PublicKind::Symlink,
                mode: 0o755,
            },
            PublicEntry {
                path: ".local/bin/semantics-install".into(),
                artifact: "package/install".into(),
                kind: PublicKind::Symlink,
                mode: 0o755,
            },
            PublicEntry {
                path: "Library/Application Support/Chancery/providers/semantics".into(),
                artifact: "share/chancery/semantics".into(),
                kind: PublicKind::Symlink,
                mode: 0o755,
            },
        ],
    }
}

pub(super) fn legacy(root: &Path) -> cell_install::Result<ReleaseInfo> {
    use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
    const FORMAT_ONE: &[LegacyProof] = &[
        LegacyProof {
            key: "binary_sha256",
            paths: &["libexec/semantics"],
        },
        LegacyProof {
            key: "frontend_sha256",
            paths: &["bin/semantics", "package/semantics"],
        },
        LegacyProof {
            key: "runner_sha256",
            paths: &["bin/semantics-worker", "package/semantics-worker"],
        },
        LegacyProof {
            key: "plist_sha256",
            paths: &["package/org.semantics.worker.plist"],
        },
        LegacyProof {
            key: "deployer_sha256",
            paths: &["package/deploy-user.sh"],
        },
        LegacyProof {
            key: "uninstaller_sha256",
            paths: &["package/uninstall-user.sh"],
        },
    ];
    const FORMAT_TWO: &[LegacyProof] = &[
        LegacyProof {
            key: "binary_sha256",
            paths: &["libexec/semantics"],
        },
        LegacyProof {
            key: "frontend_sha256",
            paths: &["bin/semantics", "package/semantics"],
        },
        LegacyProof {
            key: "runner_sha256",
            paths: &["bin/semantics-worker", "package/semantics-worker"],
        },
        LegacyProof {
            key: "clockwork_template_sha256",
            paths: &["package/semantics-worker.clockwork.toml.in"],
        },
        LegacyProof {
            key: "deployer_sha256",
            paths: &["package/deploy-user.sh"],
        },
        LegacyProof {
            key: "uninstaller_sha256",
            paths: &["package/uninstall-user.sh"],
        },
    ];
    let text = fs::read_to_string(root.join("manifest.txt"))?;
    let old = text.starts_with("format=1\n");
    let values = cell_install::legacy::manifest(&root.join("manifest.txt"))?;
    let keys = [
        "format",
        "release_id",
        "version",
        "binary_sha256",
        "frontend_sha256",
        "runner_sha256",
        if old {
            "plist_sha256"
        } else {
            "clockwork_template_sha256"
        },
        "deployer_sha256",
        "uninstaller_sha256",
        "chancery_sha256",
    ];
    let mut expected = String::new();
    for key in keys {
        expected.push_str(key);
        expected.push('=');
        expected.push_str(values.get(key).map_or("", String::as_str));
        expected.push('\n');
    }
    let version = values.get("version").map_or("", String::as_str);
    let base = version.split(['+', '-']).next().unwrap_or("");
    let valid_version = base.split('.').count() == 3
        && base
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if text != expected || !valid_version {
        return Err(cell_install::Error {
            message: "legacy Semantics manifest is not canonical".into(),
            disposition: cell_install::Disposition::Unchanged,
        });
    }
    let spec = cell_install::InstallSpec {
        product: "semantics",
        application: "Semantics",
        commands: &["semantics"],
        provider: "semantics",
    };
    let files = cell_install::provider_inventory(&root.join("share/chancery/semantics"), &spec)?;
    if files.len() != 7 {
        return Err(cell_install::Error {
            message: "legacy Semantics provider inventory differs".into(),
            disposition: cell_install::Disposition::Unchanged,
        });
    }
    let mut public = layout().public;
    public.retain(|entry| entry.path != Path::new(".local/bin/semantics-install"));
    cell_install::legacy::verify(
        root,
        &LegacySpec {
            format: if old { "1" } else { "2" },
            manifest: "manifest.txt",
            metadata: &["version"],
            proofs: if old { FORMAT_ONE } else { FORMAT_TWO },
            providers: &[LegacyProvider {
                key: "chancery_sha256",
                provider: "semantics",
                path: "share/chancery/semantics",
                version_key: "version",
            }],
            hash_path_lines: true,
        },
        public,
    )
}

pub(super) fn verify_release(release: &Path) -> Result<ReleaseInfo> {
    Ok(cell_install::transaction::verify_release_at(
        &layout(),
        release,
        &legacy,
    )?)
}

pub(super) fn prepare(args: &Candidate) -> Result<PreparedRelease> {
    if !args.binary.is_absolute() || !args.bundle.is_absolute() {
        return fail("candidate binary and bundle must be absolute");
    }
    digest(&args.binary)?;
    if fs::metadata(&args.binary)?.mode() & 0o111 == 0 {
        return fail("Semantics candidate is not executable");
    }
    let version = Command::new(&args.binary).arg("--version").output()?;
    if !version.status.success()
        || String::from_utf8_lossy(&version.stdout).trim()
            != format!("semantics {}", env!("CARGO_PKG_VERSION"))
    {
        return fail("candidate and installer versions disagree");
    }
    let inputs = tempfile::tempdir()?;
    let mut files = BTreeMap::from([
        (
            "libexec/semantics".into(),
            SourceFile {
                source: args.binary.clone(),
                mode: 0o755,
            },
        ),
        (
            "package/install".into(),
            SourceFile {
                source: std::env::current_exe()?,
                mode: 0o755,
            },
        ),
    ]);
    for (name, bytes, mode) in [
        ("semantics", FRONTEND, 0o755),
        ("semantics-worker", RUNNER, 0o755),
        ("semantics-worker.clockwork.toml.in", TEMPLATE, 0o444),
    ] {
        let source = inputs.path().join(name);
        fs::write(&source, bytes)?;
        files.insert(
            format!("package/{name}"),
            SourceFile {
                source: source.clone(),
                mode,
            },
        );
        if mode == 0o755 {
            files.insert(format!("bin/{name}"), SourceFile { source, mode });
        }
    }
    let spec = cell_install::InstallSpec {
        product: "semantics",
        application: "Semantics",
        commands: &["semantics"],
        provider: "semantics",
    };
    for (relative, _) in cell_install::provider_inventory(&args.bundle, &spec)? {
        files.insert(
            format!("share/chancery/semantics/{relative}"),
            SourceFile {
                source: args.bundle.join(relative),
                mode: 0o444,
            },
        );
    }
    let version = env!("CARGO_PKG_VERSION").to_owned();
    let plan = ReleasePlan {
        files,
        versions: BTreeMap::from([
            ("semantics".into(), version.clone()),
            ("semantics-install".into(), version.clone()),
        ]),
        providers: BTreeMap::from([(
            "semantics".into(),
            ProviderSpec {
                path: "share/chancery/semantics".into(),
                version,
            },
        )]),
    };
    Ok(cell_install::transaction::prepare_release(
        &layout(),
        &args.home.home,
        &plan,
    )?)
}

fn current(paths: &Paths) -> Result<Option<String>> {
    read_link(&paths.install.join("current"))?
        .map(|path| {
            let value = path.to_str().ok_or("invalid current selector")?;
            if !value.strip_prefix("releases/").is_some_and(valid_hash) {
                return fail("invalid Semantics current selector");
            }
            verify_release(&paths.install.join(&path))?;
            Ok(value.to_owned())
        })
        .transpose()
}

pub(super) fn inspect(home: &HomeArgs) -> Result<Value> {
    let paths = Paths::new(home)?;
    let installed =
        cell_install::transaction::inspect_installation(&layout(), &paths.home, &legacy)?;
    let current = current(&paths)?;
    let binding = binding(&paths, current.as_deref())?;
    let controls = if current.is_some() {
        json!({KEY:binding})
    } else {
        json!({})
    };
    Ok(
        json!({"installed":installed,"current":current.as_deref().unwrap_or("absent"),"controls":controls}),
    )
}

pub(super) fn verify(args: &Candidate) -> Result<Value> {
    let prepared = prepare(args)?;
    let value = inspect(&args.home)?;
    if value["installed"]["current"]["release_id"] != prepared.info.release_id {
        return fail("installed Semantics release differs from the sealed candidate");
    }
    let paths = Paths::new(&args.home)?;
    paths.doctor(&paths.payload(), None, None)?;
    Ok(value)
}

pub(super) struct Paths {
    pub home: PathBuf,
    pub state: PathBuf,
    pub install: PathBuf,
    pub database: PathBuf,
    pub clockwork: PathBuf,
    launchctl: PathBuf,
    uid: u32,
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
fn digest(path: &Path) -> Result<String> {
    Ok(cell_install::file_digest(path)?)
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn private_file(path: &Path, uid: u32, exact: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.nlink() != 1
        || (exact && metadata.mode() & 0o7777 != 0o600)
    {
        return fail("Semantics private file has unsafe ownership, links or permissions");
    }
    Ok(())
}

fn directory(path: &Path, uid: u32) -> Result<()> {
    if !exists(path) {
        if let Some(parent) = path.parent() {
            directory(parent, uid)?;
        }
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || (metadata.uid() != uid && metadata.uid() != 0)
        || metadata.mode() & 0o022 != 0
    {
        return fail("Semantics directory has unsafe ownership or permissions");
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or("missing destination parent")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn read_link(path: &Path) -> Result<Option<PathBuf>> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Ok(Some(fs::read_link(path)?)),
        Ok(_) => fail("Semantics selector is occupied by a foreign file"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn select(path: &Path, target: Option<&Path>) -> Result<()> {
    read_link(path)?;
    if let Some(target) = target {
        let temp = tempfile::tempdir_in(path.parent().ok_or("missing selector parent")?)?;
        symlink(target, temp.path().join("link"))?;
        fs::rename(temp.path().join("link"), path)?;
    } else if exists(path) {
        fs::remove_file(path)?;
    }
    Ok(())
}

impl Paths {
    pub fn new(home: &HomeArgs) -> Result<Self> {
        if !home.home.is_absolute() || fs::canonicalize(&home.home)? != home.home {
            return fail("home must be absolute and non-symbolic");
        }
        let uid: u32 = String::from_utf8(Command::new("/usr/bin/id").arg("-u").output()?.stdout)?
            .trim()
            .parse()?;
        if uid == 0 || fs::metadata(&home.home)?.uid() != uid {
            return fail("Semantics installation requires the non-root home owner");
        }
        let state = home.home.join("Library/Application Support/Semantics");
        Ok(Self {
            home: home.home.clone(),
            install: state.join("install"),
            database: state.join("semantics.db"),
            state,
            clockwork: home
                .clockwork
                .clone()
                .unwrap_or_else(|| home.home.join(".local/bin/clockwork")),
            launchctl: home.launchctl.clone(),
            uid,
        })
    }
    fn prepare(&self) -> Result<()> {
        for path in [
            self.install.clone(),
            self.install.join("releases"),
            self.state.join("backups/deployments"),
            self.logs(),
            self.home.join(".local/bin"),
            self.home.join("Library/LaunchAgents"),
            self.home
                .join("Library/Application Support/Chancery/providers"),
        ] {
            directory(&path, self.uid)?;
        }
        self.database_files()?;
        Ok(())
    }
    fn logs(&self) -> PathBuf {
        self.home.join("Library/Logs/Semantics")
    }
    fn marker(&self) -> PathBuf {
        self.state.join(".clockwork-maintenance")
    }
    fn receipt(&self) -> PathBuf {
        self.state.join(".deployment-maintenance.json")
    }
    fn plist(&self) -> PathBuf {
        self.home
            .join("Library/LaunchAgents/org.semantics.worker.plist")
    }
    fn service(&self) -> String {
        format!("gui/{}/org.semantics.worker", self.uid)
    }
    pub fn payload(&self) -> PathBuf {
        self.install.join("current/libexec/semantics")
    }
    fn selectors(&self) -> Vec<PathBuf> {
        vec![
            self.install.join("current"),
            self.install.join("previous"),
            self.home.join(".local/bin/semantics"),
            self.home.join(".local/bin/semantics-install"),
            self.home
                .join("Library/Application Support/Chancery/providers/semantics"),
        ]
    }
    fn database_files(&self) -> Result<()> {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let path = self.state.join(format!("semantics.db{suffix}"));
            if exists(&path) {
                private_file(&path, self.uid, true)?;
                if !exists(&self.database) {
                    return fail("Semantics sidecar exists without its database");
                }
            }
        }
        if exists(&self.receipt()) {
            private_file(&self.receipt(), self.uid, true)?;
            if !exists(&self.marker()) {
                return fail("Semantics maintenance receipt has no gate");
            }
        }
        if exists(&self.marker()) {
            private_file(&self.marker(), self.uid, true)?;
        }
        Ok(())
    }
    fn quiescent(&self) -> Result<()> {
        self.database_files()?;
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let path = self.state.join(format!("semantics.db{suffix}"));
            if exists(&path)
                && Command::new("/usr/sbin/lsof")
                    .args(["-t", "--"])
                    .arg(path)
                    .output()?
                    .status
                    .code()
                    != Some(1)
            {
                return fail("Semantics database or sidecar is open, or quiescence is unproved");
            }
        }
        Ok(())
    }
    fn gate(&self) -> Result<bool> {
        if exists(&self.marker()) {
            private_file(&self.marker(), self.uid, true)?;
            return Ok(false);
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.marker())?;
        file.sync_all()?;
        Ok(true)
    }
    pub fn command(&self, program: &Path, args: &[&str], run: Option<&str>) -> Result<Output> {
        let mut command = Command::new(program);
        command
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
        if let Some(run) = run {
            command.env("CELL_DEPLOYMENT_RUN_ID", run);
        }
        Ok(command.output()?)
    }
    pub fn clock(&self, args: &[&str]) -> Result<Value> {
        let output = self.command(&self.clockwork, &[&["--json"][..], args].concat(), None)?;
        let result: Value = serde_json::from_slice(&output.stdout)
            .or_else(|_| serde_json::from_slice(&output.stderr))?;
        if !output.status.success() || result["ok"] != true {
            return fail("Clockwork did not complete the requested Semantics operation");
        }
        Ok(result["data"].clone())
    }
    pub fn doctor(&self, payload: &Path, watermark: Option<&str>, run: Option<&str>) -> Result<()> {
        let annals = self.home.join(".local/bin/annals");
        if !fs::metadata(&annals).is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0) {
            return fail("Annals executable is unavailable");
        }
        let config = self
            .home
            .join("Library/Application Support/Annals/decisions/config.toml");
        if !fs::symlink_metadata(&config)?.is_file() {
            return fail("Annals decisions config is unavailable");
        }
        let execute = |args: &[&str]| -> Result<Value> {
            let output = Command::new(payload)
                .args(["--database"])
                .arg(&self.database)
                .arg("--json")
                .args(args)
                .env_clear()
                .env("HOME", &self.home)
                .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                .env(
                    "CELL_DEPLOYMENT_RUN_ID",
                    run.map_or_else(
                        || std::env::var("CELL_DEPLOYMENT_RUN_ID").unwrap_or_default(),
                        str::to_owned,
                    ),
                )
                .env("SEMANTICS_ANNALS", &annals)
                .env("SEMANTICS_ANNALS_CONFIG", &config)
                .output()?;
            if !output.status.success() {
                return fail("Semantics candidate readiness failed");
            }
            Ok(serde_json::from_slice(&output.stdout)?)
        };
        if let Some(watermark) = watermark {
            if watermark.is_empty() {
                return fail("final Decisions watermark is empty");
            }
            execute(&[
                "project",
                "activate-annals",
                "--final-decisions-watermark",
                watermark,
            ])?;
        }
        let doctor = execute(&["doctor"])?;
        let checks = doctor["checks"]
            .as_array()
            .ok_or("Semantics doctor omitted checks")?;
        if doctor["ok"] != true
            || [
                "database",
                "participation_markers",
                "annals_decision_feed",
                "nucleus_reconciliation",
            ]
            .iter()
            .any(|name| {
                !checks.iter().any(|c| {
                    c["name"] == *name
                        && c["ok"] == true
                        && (*name != "database"
                            || c["detail"]
                                .as_str()
                                .is_some_and(|s| s.starts_with("schema 3 at")))
                })
            })
        {
            return fail("Semantics doctor did not prove schema 3 and every required dependency");
        }
        Ok(())
    }
}

fn worker_lock(paths: &Paths) -> Result<File> {
    let path = paths.state.join("semantics.db.worker.lock");
    if exists(&path) {
        private_file(&path, paths.uid, false)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    file.try_lock_exclusive()
        .map_err(|_| "another Semantics worker is active")?;
    Ok(file)
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub enabled: bool,
    pub definition_digest: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hold {
    version: u32,
    key: String,
    release_id: String,
    definition_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
// These are independent durable facts needed for crash recovery.
#[allow(clippy::struct_excessive_bools)]
struct Transaction {
    version: u32,
    committed: bool,
    baseline: cell_install::transaction::InstallSnapshot,
    backup_files: BTreeMap<String, String>,
    home: PathBuf,
    selectors: BTreeMap<PathBuf, Option<PathBuf>>,
    binding: Binding,
    legacy_loaded: bool,
    legacy_plist: bool,
    database_absent: bool,
    database_backed_up: bool,
    database_touched: bool,
    scheduler_changed: bool,
    candidate_selected: bool,
    candidate_release: Option<String>,
    candidate_digest: Option<String>,
    marker_created: bool,
    prior_receipt: Option<Hold>,
    hold_owned: bool,
    suspension: Option<cell_install::transaction::SelectionReceipt>,
    publication: Option<cell_install::transaction::SelectionReceipt>,
}

fn save(transaction: &Transaction, directory: &Path) -> Result<()> {
    write_private(
        &directory.join("transaction.json"),
        &serde_json::to_vec(transaction)?,
    )
}

pub(super) fn definition(paths: &Paths, release: &Path) -> Result<Value> {
    let template = fs::read_to_string(release.join("package/semantics-worker.clockwork.toml.in"))?;
    let mut rendered = template;
    let substitutions = BTreeMap::from([
        (
            "RELEASE_ID",
            release
                .file_name()
                .ok_or("missing release ID")?
                .to_string_lossy()
                .into_owned(),
        ),
        ("RELEASE_ROOT", release.display().to_string()),
        ("SEMANTICS_STATE", paths.state.display().to_string()),
        ("SEMANTICS_HOME", paths.home.display().to_string()),
        ("SEMANTICS_LOGS", paths.logs().display().to_string()),
        ("INTERPRETER_SHA256", digest(Path::new("/bin/sh"))?),
        (
            "RUNNER_SHA256",
            digest(&release.join("bin/semantics-worker"))?,
        ),
    ]);
    for (key, value) in substitutions {
        let escaped = serde_json::to_string(&value)?;
        rendered = rendered.replace(&format!("__{key}__"), &escaped[1..escaped.len() - 1]);
    }
    let value: toml::Value = toml::from_str(&rendered)?;
    Ok(serde_json::to_value(value)?)
}

pub(super) fn binding(paths: &Paths, current: Option<&str>) -> Result<Binding> {
    let output = paths.command(&paths.clockwork, &["--json", "binding", "show", KEY], None)?;
    let value: Value = serde_json::from_slice(&output.stdout)
        .or_else(|_| serde_json::from_slice(&output.stderr))?;
    if !output.status.success() {
        if value.pointer("/error/code").and_then(Value::as_str) == Some("binding_not_found") {
            return Ok(Binding::default());
        }
        return fail("unable to inspect Semantics Clockwork binding");
    }
    let data = &value["data"];
    let enabled = data["enabled"]
        .as_bool()
        .ok_or("invalid Clockwork enabled state")?;
    let digest = match &data["definition_digest"] {
        Value::Null => None,
        Value::String(v) if valid_hash(v) => Some(v.clone()),
        _ => return fail("invalid Clockwork definition digest"),
    };
    if data["key"] != KEY || (enabled && digest.is_none()) {
        return fail("invalid Semantics Clockwork binding");
    }
    if let Some(digest) = &digest {
        let release = paths
            .install
            .join(current.ok_or("selected Clockwork binding has no current release")?);
        verify_release(&release)?;
        let actual = paths.clock(&["definition", "show", digest])?;
        if actual["digest"] != *digest
            || actual["key"] != KEY
            || actual["manifest"] != definition(paths, &release)?
        {
            return fail(
                "Clockwork definition differs from the complete Semantics-owned definition",
            );
        }
    }
    Ok(Binding {
        enabled,
        definition_digest: digest,
    })
}

fn legacy_plist(paths: &Paths, selected: Option<&str>) -> Result<(bool, bool)> {
    let plist = paths.plist();
    let owned = exists(&plist);
    if owned {
        private_file(&plist, paths.uid, false)?;
        if fs::metadata(&plist)?.mode() & 0o7777 != 0o644 {
            return fail("legacy Semantics LaunchAgent has foreign permissions");
        }
        let release = paths
            .install
            .join(selected.ok_or("legacy LaunchAgent has no current release")?);
        verify_release(&release)?;
        let mut template = fs::read_to_string(release.join("package/org.semantics.worker.plist"))?;
        for (key, path) in [
            (
                "SEMANTICS_WORKER_RUNNER",
                paths.install.join("current/bin/semantics-worker"),
            ),
            ("SEMANTICS_STATE_DIR", paths.state.clone()),
            ("SEMANTICS_HOME", paths.home.clone()),
            (
                "SEMANTICS_WORKER_STDOUT",
                paths.logs().join("worker.stdout.log"),
            ),
            (
                "SEMANTICS_WORKER_STDERR",
                paths.logs().join("worker.stderr.log"),
            ),
        ] {
            template = template.replace(&format!("__{key}__"), &path.to_string_lossy());
        }
        if fs::read(&plist)? != template.as_bytes() {
            return fail("legacy LaunchAgent bytes differ from the owned Semantics template");
        }
    }
    let loaded = paths
        .command(&paths.launchctl, &["print", &paths.service()], None)?
        .status
        .success();
    if loaded && !owned {
        return fail("loaded Semantics label has no owned recoverable plist");
    }
    Ok((owned, loaded))
}

fn public_paths() -> Vec<PathBuf> {
    layout()
        .public
        .into_iter()
        .map(|entry| entry.path)
        .collect()
}

fn restore_database(paths: &Paths, transaction: &Transaction, backup: &Path) -> Result<()> {
    if !transaction.database_touched {
        return Ok(());
    }
    if !transaction.database_backed_up {
        return fail("database rollback has no complete backup");
    }
    let expected_database = transaction.backup_files.contains_key("semantics.db");
    if expected_database == transaction.database_absent {
        return fail("Semantics backup database presence differs from its transaction");
    }
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let name = format!("semantics.db{suffix}");
        let saved = backup.join(&name);
        match transaction.backup_files.get(&name) {
            Some(expected) => {
                private_file(&saved, paths.uid, true)?;
                if digest(&saved)? != *expected {
                    return fail("Semantics retained database backup changed");
                }
            }
            None if exists(&saved) => {
                return fail("Semantics retained backup has an unexpected sidecar");
            }
            None => {}
        }
    }
    paths.quiescent()?;
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let live = paths.state.join(format!("semantics.db{suffix}"));
        let saved = backup.join(format!("semantics.db{suffix}"));
        if exists(&saved) {
            private_file(&saved, paths.uid, true)?;
        }
        if exists(&live) {
            fs::remove_file(&live)?;
        }
        if exists(&saved) {
            write_private(&live, &fs::read(&saved)?)?;
        }
    }
    Ok(())
}

fn recovery_binding(paths: &Paths, transaction: &Transaction) -> Result<Binding> {
    let output = paths.command(&paths.clockwork, &["--json", "binding", "show", KEY], None)?;
    let value: Value = serde_json::from_slice(&output.stdout)
        .or_else(|_| serde_json::from_slice(&output.stderr))?;
    if !output.status.success() {
        if value.pointer("/error/code").and_then(Value::as_str) == Some("binding_not_found") {
            return Ok(Binding::default());
        }
        return fail("unable to inspect Semantics recovery binding");
    }
    let digest = value["data"]["definition_digest"].as_str();
    let selected = if digest.is_none() && value["data"]["definition_digest"].is_null() {
        None
    } else if digest == transaction.candidate_digest.as_deref() {
        transaction
            .candidate_release
            .as_ref()
            .map(|id| format!("releases/{id}"))
    } else if digest == transaction.binding.definition_digest.as_deref() {
        transaction
            .baseline
            .current
            .as_ref()
            .map(|release| format!("releases/{}", release.release_id))
    } else {
        return fail("Semantics recovery binding selects a foreign definition");
    };
    binding(paths, selected.as_deref())
}

fn rollback(
    paths: &Paths,
    transaction: &Transaction,
    backup: &Path,
    tx: &mut cell_install::transaction::InstallTransaction<'_>,
) -> Result<()> {
    paths.gate()?;
    if transaction.scheduler_changed {
        recovery_binding(paths, transaction)?;
        paths.clock(&["binding", "disable", KEY])?;
    }
    if transaction.committed {
        return fail("committed Semantics installation requires forward recovery");
    }
    let actual =
        cell_install::transaction::inspect_detached_installation(&layout(), &paths.home, &legacy)?;
    let candidate_id = transaction.candidate_release.as_deref();
    if actual.current.as_ref().is_some_and(|r| {
        Some(r.release_id.as_str()) != candidate_id
            && transaction.baseline.current.as_ref() != Some(r)
    }) {
        return fail("Semantics recovery found a foreign release generation");
    }
    tx.suspend(&actual, &public_paths())?;
    if transaction.candidate_selected && transaction.binding.definition_digest.is_none() {
        return fail(
            "Clockwork cannot clear the candidate to the prior null selection; private release and authenticated maintenance remain for forward recovery",
        );
    }
    restore_database(paths, transaction, backup)?;
    if transaction.legacy_plist {
        write_private(
            &paths.plist(),
            &fs::read(backup.join("prior-worker.plist"))?,
        )?;
        fs::set_permissions(paths.plist(), fs::Permissions::from_mode(0o644))?;
    }
    if transaction.scheduler_changed {
        if let Some(digest) = &transaction.binding.definition_digest {
            if transaction.binding.enabled {
                paths.clock(&["binding", "switch", KEY, digest])?;
            } else {
                paths.clock(&["binding", "disable", KEY, "--select", digest])?;
            }
        } else {
            paths.clock(&["binding", "disable", KEY])?;
        }
    }
    if transaction.legacy_loaded {
        if transaction.binding.enabled {
            return fail("recovery refuses dual active schedulers");
        }
        if !paths
            .command(
                &paths.launchctl,
                &[
                    "bootstrap",
                    &format!("gui/{}", paths.uid),
                    paths.plist().to_str().ok_or("invalid plist path")?,
                ],
                None,
            )?
            .status
            .success()
        {
            return fail("could not restore the owned legacy Semantics scheduler");
        }
    }
    let release = paths.install.join("releases").join(
        transaction
            .candidate_release
            .as_ref()
            .ok_or("missing recovery candidate")?,
    );
    let prepared = PreparedRelease {
        info: verify_release(&release)?,
        root: release,
    };
    tx.recover(&transaction.baseline, &prepared, false, |_| Ok(()))?;
    if let Some(hold) = &transaction.prior_receipt {
        write_private(&paths.receipt(), &serde_json::to_vec(hold)?)?;
    } else if transaction.hold_owned && exists(&paths.receipt()) {
        private_file(&paths.receipt(), paths.uid, true)?;
        fs::remove_file(paths.receipt())?;
    }
    if transaction.marker_created {
        private_file(&paths.marker(), paths.uid, true)?;
        fs::remove_file(paths.marker())?;
    }
    Ok(())
}

fn retain_failure(
    paths: &Paths,
    transaction: &Transaction,
    tx: &mut cell_install::transaction::InstallTransaction<'_>,
) {
    let _ = paths.gate();
    if recovery_binding(paths, transaction).is_ok() {
        let _ = paths.clock(&["binding", "disable", KEY]);
    }
    let _ = paths.command(&paths.launchctl, &["bootout", &paths.service()], None);
    if let Ok(actual) =
        cell_install::transaction::inspect_detached_installation(&layout(), &paths.home, &legacy)
    {
        let _ = tx.suspend(&actual, &public_paths());
    }
    let _ = select(&paths.install.join("previous"), None);
    if transaction.candidate_selected
        && let Some(release) = &transaction.candidate_release
    {
        let _ = select(
            &paths.install.join("current"),
            Some(Path::new(&format!("releases/{release}"))),
        );
    }
    if exists(&paths.plist()) {
        let _ = fs::remove_file(paths.plist());
    }
}

pub(super) fn install(args: &Candidate) -> Result<Value> {
    if args.final_decisions_watermark.is_some() && !args.keep_maintenance {
        return fail("legacy Annals activation requires retained maintenance");
    }
    let paths = Paths::new(&args.home)?;
    paths.prepare()?;
    let prepared = prepare(args)?;
    let mut tx = cell_install::transaction::lock_installation(&layout(), &paths.home, &legacy)?;
    for entry in fs::read_dir(&paths.install)? {
        if entry?
            .file_name()
            .to_string_lossy()
            .starts_with(".transaction.")
        {
            return fail("Semantics has retained an unfinished transaction; use explicit recovery");
        }
    }
    let installed =
        cell_install::transaction::inspect_detached_installation(&layout(), &paths.home, &legacy)?;
    let old_current = current(&paths)?;
    if args
        .expected_current
        .as_deref()
        .is_some_and(|v| v != old_current.as_deref().unwrap_or("absent"))
    {
        return fail("Semantics installation changed since inspection");
    }
    let binding = binding(&paths, old_current.as_deref())?;
    let (legacy_plist, legacy_loaded) = legacy_plist(&paths, old_current.as_deref())?;
    if legacy_loaded && binding.enabled {
        return fail("legacy Semantics and Clockwork schedules are both active");
    }
    tx.recheck(&installed)?;
    let mut transaction = Transaction {
        version: 1,
        committed: false,
        baseline: installed.clone(),
        backup_files: BTreeMap::new(),
        home: paths.home.clone(),
        selectors: paths
            .selectors()
            .into_iter()
            .map(|p| Ok((p.clone(), read_link(&p)?)))
            .collect::<Result<_>>()?,
        binding,
        legacy_loaded,
        legacy_plist,
        database_absent: !exists(&paths.database),
        database_backed_up: false,
        database_touched: false,
        scheduler_changed: false,
        candidate_selected: false,
        candidate_release: Some(prepared.info.release_id.clone()),
        candidate_digest: None,
        marker_created: false,
        prior_receipt: None,
        hold_owned: false,
        suspension: None,
        publication: None,
    };
    if exists(&paths.receipt()) {
        private_file(&paths.receipt(), paths.uid, true)?;
        let receipt: Hold = serde_json::from_slice(&fs::read(paths.receipt())?)?;
        if receipt.version != 1
            || receipt.key != KEY
            || Some(format!("releases/{}", receipt.release_id)).as_deref() != old_current.as_deref()
            || Some(&receipt.definition_digest) != transaction.binding.definition_digest.as_ref()
        {
            return fail("maintenance hold receipt does not match current owned state");
        }
        transaction.prior_receipt = Some(receipt);
    }
    let backup = tempfile::Builder::new()
        .prefix(".transaction.")
        .tempdir_in(&paths.install)?;
    let backup = backup.keep();
    save(&transaction, &backup)?;
    if transaction.legacy_plist {
        write_private(
            &backup.join("prior-worker.plist"),
            &fs::read(paths.plist())?,
        )?;
    }
    let retained_backup = paths.state.join("backups/deployments").join(format!(
        "pre-{}-{}",
        prepared.info.release_id,
        uuid::Uuid::now_v7()
    ));
    let mut worker = None;
    let result = (|| -> Result<Value> {
        transaction.marker_created = paths.gate()?;
        transaction.hold_owned = transaction.marker_created || transaction.prior_receipt.is_some();
        save(&transaction, &backup)?;
        for name in ["worker.stdout.log", "worker.stderr.log"] {
            let log = paths.logs().join(name);
            if exists(&log) {
                private_file(&log, paths.uid, false)?;
                fs::set_permissions(log, fs::Permissions::from_mode(0o600))?;
            }
        }
        let definition = definition(&paths, &prepared.root)?;
        let toml = toml::to_string(&definition)?;
        let definition_path = backup.join("worker.toml");
        write_private(&definition_path, toml.as_bytes())?;
        let registered = paths.clock(&[
            "definition",
            "register",
            definition_path.to_str().ok_or("invalid definition path")?,
        ])?;
        let digest = registered["digest"]
            .as_str()
            .filter(|v| valid_hash(v))
            .ok_or("Clockwork omitted registered digest")?
            .to_owned();
        transaction.candidate_digest = Some(digest.clone());
        transaction.scheduler_changed = true;
        save(&transaction, &backup)?;
        paths.clock(&["binding", "disable", KEY])?;
        if transaction.legacy_loaded
            && !paths
                .command(&paths.launchctl, &["bootout", &paths.service()], None)?
                .status
                .success()
        {
            return fail("could not stop the owned legacy Semantics scheduler");
        }
        if transaction.legacy_plist {
            fs::remove_file(paths.plist())?;
        }
        transaction.suspension = Some(tx.suspend(&installed, &public_paths())?);
        save(&transaction, &backup)?;
        worker = Some(worker_lock(&paths)?);
        paths.quiescent()?;
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let live = paths.state.join(format!("semantics.db{suffix}"));
            if exists(&live) {
                write_private(
                    &backup.join(format!("semantics.db{suffix}")),
                    &fs::read(&live)?,
                )?;
                let name = format!("semantics.db{suffix}");
                transaction
                    .backup_files
                    .insert(name.clone(), cell_install::file_digest(&backup.join(name))?);
            }
        }
        paths.quiescent()?;
        transaction.database_backed_up = true;
        transaction.database_touched = true;
        save(&transaction, &backup)?;
        paths.doctor(
            &prepared.root.join("libexec/semantics"),
            args.final_decisions_watermark.as_deref(),
            None,
        )?;
        if transaction.hold_owned {
            let hold = Hold {
                version: 1,
                key: KEY.into(),
                release_id: prepared.info.release_id.clone(),
                definition_digest: digest.clone(),
            };
            write_private(&paths.receipt(), &serde_json::to_vec(&hold)?)?;
        }
        let suspended = transaction
            .suspension
            .as_ref()
            .ok_or("missing suspended installation")?;
        transaction.publication = Some(tx.publish(&prepared, &suspended.after, |_| Ok(()))?);
        transaction.candidate_selected = true;
        save(&transaction, &backup)?;
        paths.clock(&["binding", "switch", KEY, &digest])?;
        private_file(&paths.marker(), paths.uid, true)?;
        let receipt = json!({"version":1,"release_id":prepared.info.release_id,"previous":installed,"clockwork_definition":digest,"maintenance_retained":args.keep_maintenance || !transaction.hold_owned,"rollback_snapshot":retained_backup});
        write_private(
            &paths.install.join("last-update.json"),
            &serde_json::to_vec(&receipt)?,
        )?;
        transaction.committed = true;
        save(&transaction, &backup)?;
        Ok(receipt)
    })();
    match result {
        Ok(value) => {
            drop(worker);
            if transaction.hold_owned && !args.keep_maintenance {
                private_file(&paths.marker(), paths.uid, true)?;
                fs::remove_file(paths.receipt())?;
                fs::remove_file(paths.marker())?;
            }
            fs::rename(&backup, &retained_backup)?;
            Ok(value)
        }
        Err(error) => {
            if rollback(&paths, &transaction, &backup, &mut tx).is_ok() {
                drop(worker);
                fs::remove_dir_all(&backup)?;
                Err(error)
            } else {
                retain_failure(&paths, &transaction, &mut tx);
                drop(worker);
                Err(format!("{error}; Semantics remains maintenance-gated; private recovery transaction: {}", backup.display()).into())
            }
        }
    }
}

pub(super) fn recover(home: &HomeArgs, backup: &Path, forward: bool) -> Result<()> {
    let paths = Paths::new(home)?;
    if backup.parent() != Some(paths.install.as_path())
        || !backup
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(".transaction."))
    {
        return fail("recovery requires an exact retained Semantics transaction");
    }
    if !exists(backup) {
        return fail("Semantics recovery transaction is absent");
    }
    directory(backup, paths.uid)?;
    private_file(&backup.join("transaction.json"), paths.uid, true)?;
    let mut transaction: Transaction =
        serde_json::from_slice(&fs::read(backup.join("transaction.json"))?)?;
    if transaction.version != 1 || transaction.home != paths.home {
        return fail("recovery transaction belongs to another installation");
    }
    let mut tx = cell_install::transaction::lock_installation(&layout(), &paths.home, &legacy)?;
    recovery_binding(&paths, &transaction)?;
    paths.gate()?;
    paths.clock(&["binding", "disable", KEY])?;
    let _worker = worker_lock(&paths)?;
    if forward || transaction.committed {
        let actual = cell_install::transaction::inspect_detached_installation(
            &layout(),
            &paths.home,
            &legacy,
        )?;
        if actual.current.as_ref().is_some_and(|r| {
            transaction.candidate_release.as_ref() != Some(&r.release_id)
                && transaction.baseline.current.as_ref() != Some(r)
        }) {
            return fail("Semantics forward recovery found a foreign generation");
        }
        tx.suspend(&actual, &public_paths())?;
        paths.quiescent()?;
        let release = paths.install.join("releases").join(
            transaction
                .candidate_release
                .as_ref()
                .ok_or("missing candidate recovery release")?,
        );
        let prepared = PreparedRelease {
            info: verify_release(&release)?,
            root: release,
        };
        let digest = transaction
            .candidate_digest
            .as_ref()
            .ok_or("missing candidate recovery definition")?;
        if exists(&paths.receipt()) {
            private_file(&paths.receipt(), paths.uid, true)?;
            let receipt: Hold = serde_json::from_slice(&fs::read(paths.receipt())?)?;
            if receipt.version != 1
                || receipt.key != KEY
                || receipt.release_id != prepared.info.release_id
                || receipt.definition_digest != *digest
            {
                return fail(
                    "forward recovery requires the authenticated candidate maintenance hold",
                );
            }
        } else if !transaction.committed || !transaction.hold_owned {
            return fail("forward recovery has no authenticated candidate hold");
        }
        let definition = paths.clock(&["definition", "show", digest])?;
        if definition["manifest"] != self::definition(&paths, &prepared.root)? {
            return fail("forward recovery definition differs from exact retained candidate");
        }
        paths.doctor(&prepared.root.join("libexec/semantics"), None, None)?;
        tx.recover(&transaction.baseline, &prepared, true, |_| Ok(()))?;
        if let Err(error) = paths.clock(&["binding", "switch", KEY, digest]) {
            retain_failure(&paths, &transaction, &mut tx);
            return Err(error);
        }
        transaction.committed = true;
        save(&transaction, backup)?;
        private_file(&paths.marker(), paths.uid, true)?;
        if transaction.hold_owned {
            if exists(&paths.receipt()) {
                fs::remove_file(paths.receipt())?;
            }
            fs::remove_file(paths.marker())?;
        }
        fs::rename(
            backup,
            paths
                .state
                .join("backups/deployments")
                .join(format!("recovered-{}", uuid::Uuid::now_v7())),
        )?;
        return Ok(());
    }
    match rollback(&paths, &transaction, backup, &mut tx) {
        Ok(()) => {
            fs::remove_dir_all(backup)?;
            Ok(())
        }
        Err(error) => {
            retain_failure(&paths, &transaction, &mut tx);
            Err(error)
        }
    }
}

pub(super) fn uninstall(home: &HomeArgs) -> Result<()> {
    let paths = Paths::new(home)?;
    paths.prepare()?;
    let mut tx = cell_install::transaction::lock_installation(&layout(), &paths.home, &legacy)?;
    let installed =
        cell_install::transaction::inspect_detached_installation(&layout(), &paths.home, &legacy)?;
    let selected = current(&paths)?;
    // Public selectors may already be absent after a previous uninstall.
    for entry in layout().public {
        if let Some(target) = read_link(&paths.home.join(entry.path))?
            && target != paths.install.join("current").join(entry.artifact)
        {
            return fail("Semantics public selector has foreign ownership");
        }
    }
    let binding = binding(&paths, selected.as_deref())?;
    let (owned_plist, loaded) = legacy_plist(&paths, selected.as_deref())?;
    if selected.is_none() {
        return Ok(());
    }
    paths.gate()?;
    paths.clock(&["binding", "disable", KEY])?;
    if loaded
        && !paths
            .command(&paths.launchctl, &["bootout", &paths.service()], None)?
            .status
            .success()
    {
        return fail("could not stop legacy Semantics service; maintenance remains");
    }
    let _worker = worker_lock(&paths)?;
    paths.quiescent()?;
    if owned_plist {
        fs::remove_file(paths.plist())?;
    }
    if binding.enabled && loaded {
        return fail("Semantics had dual scheduler admission; maintenance remains");
    }
    tx.suspend(&installed, &public_paths())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backup_fixture() -> Result<(tempfile::TempDir, Paths, PathBuf, Transaction)> {
        let root = tempfile::tempdir()?;
        let state = root.path().join("state");
        let backup = root.path().join("backup");
        fs::create_dir(&state)?;
        fs::create_dir(&backup)?;
        let paths = Paths {
            home: root.path().to_owned(),
            install: state.join("install"),
            database: state.join("semantics.db"),
            state,
            clockwork: root.path().join("must-not-run"),
            launchctl: root.path().join("must-not-run"),
            uid: fs::metadata(root.path())?.uid(),
        };
        write_private(&paths.database, b"candidate database")?;
        write_private(&paths.state.join("semantics.db-wal"), b"candidate WAL")?;
        let mut backup_files = BTreeMap::new();
        for (name, bytes) in [
            ("semantics.db", "prior database"),
            ("semantics.db-wal", "prior WAL"),
        ] {
            let path = backup.join(name);
            write_private(&path, bytes.as_bytes())?;
            backup_files.insert(name.into(), digest(&path)?);
        }
        let transaction = Transaction {
            version: 1,
            committed: false,
            baseline: serde_json::from_value(json!({"current":null,"previous":null,"entries":{}}))?,
            backup_files,
            home: paths.home.clone(),
            selectors: BTreeMap::new(),
            binding: Binding {
                enabled: false,
                definition_digest: None,
            },
            legacy_loaded: false,
            legacy_plist: false,
            database_absent: false,
            database_backed_up: true,
            database_touched: true,
            scheduler_changed: false,
            candidate_selected: false,
            candidate_release: None,
            candidate_digest: None,
            marker_created: false,
            prior_receipt: None,
            hold_owned: false,
            suspension: None,
            publication: None,
        };
        Ok((root, paths, backup, transaction))
    }

    #[test]
    fn missing_backup_database_does_not_remove_any_live_file() -> Result<()> {
        let (_root, paths, backup, transaction) = backup_fixture()?;
        fs::remove_file(backup.join("semantics.db"))?;
        assert!(restore_database(&paths, &transaction, &backup).is_err());
        assert_eq!(fs::read(&paths.database)?, b"candidate database");
        assert_eq!(
            fs::read(paths.state.join("semantics.db-wal"))?,
            b"candidate WAL"
        );
        Ok(())
    }

    #[test]
    fn changed_backup_sidecar_does_not_replace_the_live_database() -> Result<()> {
        let (_root, paths, backup, transaction) = backup_fixture()?;
        write_private(&backup.join("semantics.db-wal"), b"changed saved WAL")?;
        let error = restore_database(&paths, &transaction, &backup)
            .err()
            .ok_or("expected database recovery to fail")?;
        assert!(
            error
                .to_string()
                .contains("retained database backup changed")
        );
        assert_eq!(fs::read(&paths.database)?, b"candidate database");
        assert_eq!(
            fs::read(paths.state.join("semantics.db-wal"))?,
            b"candidate WAL"
        );
        Ok(())
    }
}
