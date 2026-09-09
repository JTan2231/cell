//! Attended system-to-user migration. A committed state root is never moved back.
use cell_install::{Disposition, Error, Result};
use clap::Args as ClapArgs;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

const SYSTEM_TARGET: &str = "system/org.annals.inbox";
const KEY: &str = "annals/inbox";
const DAEMON: &str = include_str!("../../../../../packaging/launchd/org.annals.inbox.plist");
const AGENT: &str = include_str!("../../../../../packaging/launchd/org.annals.inbox.agent.plist");

#[derive(ClapArgs)]
pub(super) struct Args {
    #[arg(long)]
    binary: PathBuf,
    #[arg(long)]
    usage_binary: PathBuf,
    #[arg(long)]
    bundle: PathBuf,
    #[arg(long)]
    usage_bundle: PathBuf,
    #[arg(long)]
    nucleus: PathBuf,
    #[arg(long)]
    nucleus_socket: PathBuf,
    #[arg(long)]
    clockwork: PathBuf,
    #[arg(long, env = "ANNALS_MIGRATION_LEGACY_PREFIX", hide = true)]
    legacy_prefix: Option<PathBuf>,
    #[arg(long, env = "ANNALS_MIGRATION_LEGACY_STATE", hide = true)]
    legacy_state: Option<PathBuf>,
    #[arg(
        long,
        default_value = "/bin/launchctl",
        env = "ANNALS_MIGRATION_LAUNCHCTL"
    )]
    launchctl: PathBuf,
    #[arg(
        long,
        default_value = "/usr/bin/dscl",
        env = "ANNALS_MIGRATION_DSCL",
        hide = true
    )]
    dscl: PathBuf,
    #[arg(
        long,
        default_value = "/usr/bin/sudo",
        env = "ANNALS_MIGRATION_OPERATOR_RUNNER",
        hide = true
    )]
    operator_runner: PathBuf,
    #[arg(long, env = "ANNALS_MIGRATION_DEPLOY", hide = true)]
    deploy: Option<PathBuf>,
    #[arg(long, default_value_t = 3900, env = "ANNALS_UPDATE_WAIT_SECONDS")]
    wait_seconds: u64,
}

fn failure(message: &str) -> Error {
    Error {
        message: message.into(),
        disposition: Disposition::Unchanged,
    }
}
fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
fn hash(path: &Path) -> Result<String> {
    cell_install::file_digest(path)
}
fn file(path: &Path, owner: u32, mode: Option<u32>) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.uid() != owner
        || metadata.nlink() != 1
        || mode.is_some_and(|mode| metadata.mode() & 0o7777 != mode)
    {
        return Err(failure(
            "migration file ownership, links or mode differ from the exact expected state",
        ));
    }
    Ok(())
}
fn dir(path: &Path, owner: u32) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != owner || metadata.mode() & 0o022 != 0 {
        return Err(failure("migration directory ownership or mode is unsafe"));
    }
    Ok(())
}
// Keep the timeout constructors compatible with the Rust 1.89 MSRV.
#[allow(clippy::duration_suboptimal_units)]
fn direct(program: &Path, args: &[OsString]) -> Result<Output> {
    cell_install::command::run(program, args, &BTreeMap::new(), Duration::from_secs(120))
}
fn checked(program: &Path, args: &[OsString]) -> Result<Output> {
    let output = direct(program, args)?;
    if !output.status.success() {
        return Err(failure("migration child command did not complete"));
    }
    Ok(output)
}
fn words(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}
fn text(output: Output) -> Result<String> {
    String::from_utf8(output.stdout)
        .map(|v| v.trim().to_owned())
        .map_err(|_| failure("migration command returned invalid text"))
}
fn id(args: &[&str]) -> Result<String> {
    text(checked(Path::new("/usr/bin/id"), &words(args))?)
}

struct State<'a> {
    args: &'a Args,
    invoking_uid: u32,
    operator: String,
    uid: u32,
    group: String,
    home: PathBuf,
    legacy: PathBuf,
    transaction: PathBuf,
    target: PathBuf,
    frontend: PathBuf,
    payload: PathBuf,
    daemon: PathBuf,
}

impl State<'_> {
    // Duration::from_hours is newer than the Rust 1.89 MSRV.
    #[allow(clippy::duration_suboptimal_units)]
    fn user(&self, program: &Path, args: &[OsString]) -> Result<Output> {
        let mut command = words(&["-u", &self.operator, "/usr/bin/env", "-i"]);
        command.extend([
            OsString::from(format!("HOME={}", self.home.display())),
            "PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin".into(),
            format!("USER={}", self.operator).into(),
            format!("LOGNAME={}", self.operator).into(),
            program.as_os_str().to_owned(),
        ]);
        command.extend_from_slice(args);
        cell_install::command::run(
            &self.args.operator_runner,
            &command,
            &BTreeMap::new(),
            Duration::from_secs(7200),
        )
    }
    fn user_checked(&self, program: &Path, args: &[OsString]) -> Result<Output> {
        let result = self.user(program, args)?;
        if !result.status.success() {
            return Err(failure(
                "operator migration command did not complete; retained phase determines recovery",
            ));
        }
        Ok(result)
    }
    fn owner_write(&self, path: &Path, bytes: &[u8], owner: u32) -> Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| failure("migration output has no parent"))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        if owner != self.invoking_uid {
            checked(
                Path::new("/usr/sbin/chown"),
                &[
                    owner.to_string().into(),
                    temporary.path().as_os_str().to_owned(),
                ],
            )?;
        }
        temporary
            .persist(path)
            .map_err(|_| failure("cannot commit migration control file"))?;
        File::open(parent)?.sync_all()?;
        Ok(())
    }
    fn phase(&self) -> Result<Option<String>> {
        if !exists(&self.transaction) {
            return Ok(None);
        }
        dir(&self.transaction, self.invoking_uid)?;
        file(
            &self.transaction.join("phase"),
            self.invoking_uid,
            Some(0o600),
        )?;
        Ok(Some(
            fs::read_to_string(self.transaction.join("phase"))?
                .trim()
                .to_owned(),
        ))
    }
    fn write_phase(&self, phase: &str) -> Result<()> {
        self.owner_write(
            &self.transaction.join("phase"),
            format!("{phase}\n").as_bytes(),
            self.invoking_uid,
        )
    }
    fn record(&self, name: &str) -> Result<String> {
        let path = self.transaction.join(name);
        file(&path, self.invoking_uid, Some(0o600))?;
        Ok(fs::read_to_string(path)?.trim().to_owned())
    }
    fn marker(&self) -> PathBuf {
        self.target.join("spool/.maintenance")
    }
    fn handoff(&self) -> Result<PathBuf> {
        let new = self.target.join("install/clockwork-handoff.toml");
        let old = self
            .target
            .join("install/.migration-annals-inbox.clockwork.toml");
        if exists(&new) && exists(&old) {
            return Err(failure("migration has conflicting Clockwork handoff files"));
        }
        Ok(if exists(&old) { old } else { new })
    }
    fn gate(&self, root: &Path) -> Result<()> {
        let marker = root.join("spool/.maintenance");
        if exists(&marker) {
            return file(&marker, self.uid, Some(0o600));
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&marker)?;
        file.sync_all()?;
        if self.uid != self.invoking_uid {
            checked(
                Path::new("/usr/sbin/chown"),
                &[self.uid.to_string().into(), marker.into_os_string()],
            )?;
        }
        Ok(())
    }
    fn launch(&self, args: &[&str]) -> Result<Output> {
        direct(&self.args.launchctl, &words(args))
    }
    fn system_loaded(&self) -> Result<bool> {
        let output = self.launch(&["print", SYSTEM_TARGET])?;
        if output.status.success() {
            return Ok(true);
        }
        if [output.stdout, output.stderr]
            .concat()
            .windows(b"Could not find service".len())
            .any(|window| window == b"Could not find service")
        {
            return Ok(false);
        }
        Err(failure("legacy system service absence is unproved"))
    }
    fn clock(&self, args: &[&str]) -> Result<Value> {
        let output = self.user(
            &self.args.clockwork,
            &words(&[&["--json"][..], args].concat()),
        )?;
        let value: Value = serde_json::from_slice(&output.stdout)
            .or_else(|_| serde_json::from_slice(&output.stderr))?;
        if !output.status.success() || value["ok"] != true {
            return Err(failure("Clockwork migration operation did not complete"));
        }
        Ok(value["data"].clone())
    }
    fn binding(&self) -> Result<Option<(bool, Option<String>)>> {
        let output = self.user(
            &self.args.clockwork,
            &words(&["--json", "binding", "show", KEY]),
        )?;
        let value: Value = serde_json::from_slice(&output.stdout)
            .or_else(|_| serde_json::from_slice(&output.stderr))?;
        if !output.status.success() {
            if value.pointer("/error/code").and_then(Value::as_str) == Some("binding_not_found") {
                return Ok(None);
            }
            return Err(failure("cannot inspect migration Clockwork binding"));
        }
        let enabled = value["data"]["enabled"]
            .as_bool()
            .ok_or_else(|| failure("invalid binding enabled state"))?;
        let digest = match &value["data"]["definition_digest"] {
            Value::Null => None,
            Value::String(value) if valid_hash(value) => Some(value.clone()),
            _ => return Err(failure("invalid binding definition digest")),
        };
        if value["ok"] != true || value["data"]["key"] != KEY || enabled && digest.is_none() {
            return Err(failure("foreign or invalid migration Clockwork binding"));
        }
        Ok(Some((enabled, digest)))
    }
    fn empty_binding(&self) -> Result<()> {
        if self
            .binding()?
            .is_some_and(|(enabled, digest)| enabled || digest.is_some())
        {
            return Err(failure(
                "migration requires an absent or disabled unselected Clockwork binding",
            ));
        }
        Ok(())
    }
    fn exact_daemon(&self) -> Result<()> {
        file(&self.daemon, self.invoking_uid, Some(0o644))?;
        let expected = tempfile::NamedTempFile::new()?;
        fs::write(expected.path(), DAEMON)?;
        for (key, value) in [("UserName", &self.operator), ("GroupName", &self.group)] {
            checked(
                Path::new("/usr/bin/plutil"),
                &[
                    "-replace".into(),
                    key.into(),
                    "-string".into(),
                    value.into(),
                    expected.path().as_os_str().to_owned(),
                ],
            )?;
        }
        if fs::read(&self.daemon)? != fs::read(expected.path())? {
            return Err(failure(
                "legacy LaunchDaemon differs from the complete rendered Annals template",
            ));
        }
        Ok(())
    }
    fn exact_user_agent(&self, path: &Path) -> Result<()> {
        file(path, self.uid, Some(0o600))?;
        let expected = tempfile::NamedTempFile::new()?;
        fs::write(expected.path(), AGENT)?;
        checked(
            Path::new("/usr/bin/plutil"),
            &[
                "-remove".into(),
                "ProgramArguments.0".into(),
                expected.path().as_os_str().to_owned(),
            ],
        )?;
        checked(
            Path::new("/usr/bin/plutil"),
            &[
                "-insert".into(),
                "ProgramArguments.0".into(),
                "-string".into(),
                self.home.join(".local/bin/annals").into_os_string(),
                expected.path().as_os_str().to_owned(),
            ],
        )?;
        for (key, value) in [
            ("WorkingDirectory", self.target.clone()),
            ("EnvironmentVariables.HOME", self.home.clone()),
            ("StandardOutPath", self.target.join("log/inbox.stdout.log")),
            (
                "StandardErrorPath",
                self.target.join("log/inbox.stderr.log"),
            ),
        ] {
            checked(
                Path::new("/usr/bin/plutil"),
                &[
                    "-replace".into(),
                    key.into(),
                    "-string".into(),
                    value.into_os_string(),
                    expected.path().as_os_str().to_owned(),
                ],
            )?;
        }
        if fs::read(path)? != fs::read(expected.path())? {
            return Err(failure(
                "user LaunchAgent differs from the exact legacy Annals template",
            ));
        }
        Ok(())
    }
    fn verify_handoff(&self) -> Result<Value> {
        let path = self.handoff()?;
        file(&path, self.uid, Some(0o600))?;
        file(&self.marker(), self.uid, Some(0o600))?;
        let actual: toml::Value = toml::from_str(&fs::read_to_string(&path)?)
            .map_err(|_| failure("invalid inert migration definition"))?;
        let actual = serde_json::to_value(actual)?;
        let selected = fs::read_link(self.target.join("install/current"))?;
        let selected = selected
            .to_str()
            .ok_or_else(|| failure("invalid migration release selector"))?;
        let release_id = selected
            .strip_prefix("releases/")
            .filter(|v| valid_hash(v))
            .ok_or_else(|| failure("invalid migration release identity"))?;
        let release = self.target.join("install").join(selected);
        dir(&release, self.uid)?;
        let verified = self.user_checked(
            &std::env::current_exe()?,
            &["verify-release".into(), release.as_os_str().to_owned()],
        )?;
        let verified: Value = serde_json::from_slice(&verified.stdout)?;
        if verified["ok"] != true || verified["data"]["release_id"] != release_id {
            return Err(failure("migration release identity was not verified"));
        }
        let runner = release.join("bin/annals-inbox");
        let launch = if verified["data"]["format"] == cell_install::TRANSACTION_FORMAT {
            json!({"kind":"direct","program":runner,"sha256":hash(&runner)?})
        } else {
            json!({"kind":"interpreted","interpreter":"/bin/sh","interpreter_sha256":hash(Path::new("/bin/sh"))?,"script":runner,"script_sha256":hash(&runner)?})
        };
        let mut expected = json!({"schema_version":2,"key":KEY,"release_id":release_id,"release_root":release,"authority":"current-user-background","overlap":"skip","arguments":[],"cwd":self.target,
            "schedule":{"kind":"interval","seconds":300,"run_at_load":true},
            "launch":launch,
            "environment":{"HOME":self.home,"USER":self.operator,"LOGNAME":self.operator,"ANNALS_CONFIG":self.target.join("config.toml")},
            "output":{"stdout":self.target.join("log/inbox.stdout.log"),"stderr":self.target.join("log/inbox.stderr.log")}});
        if actual["schema_version"] == 1 {
            expected["schema_version"] = json!(1);
        }
        if actual != expected {
            return Err(failure(
                "migration handoff differs from the complete release-owned Clockwork definition",
            ));
        }
        Ok(json!({"release_id":release_id,"handoff_sha256":hash(&path)?}))
    }
    fn child_transactions(&self) -> Result<Vec<PathBuf>> {
        let install = self.target.join("install");
        if !exists(&install) {
            return Ok(Vec::new());
        }
        dir(&install, self.uid)?;
        let mut pending = Vec::new();
        for entry in fs::read_dir(install)? {
            let entry = entry?;
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("transaction.")
            {
                pending.push(entry.path());
            }
        }
        Ok(pending)
    }
    fn child_may_have_run(&self) -> Result<bool> {
        Ok(exists(&self.target.join("install/current"))
            || exists(&self.handoff()?)
            || !self.child_transactions()?.is_empty())
    }
    fn child_archives(&self) -> Result<Vec<String>> {
        let parent = self.target.join("backups/deployments");
        if !exists(&parent) {
            return Ok(Vec::new());
        }
        dir(&parent, self.uid)?;
        let mut names = Vec::new();
        for entry in fs::read_dir(parent)? {
            let name = entry?
                .file_name()
                .into_string()
                .map_err(|_| failure("invalid Annals recovery archive name"))?;
            if name.starts_with("transaction.primary.") {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }
    fn child_reply_data(&self, reply: &Value, proof: &Value) -> Result<Value> {
        let data = &reply["data"];
        if reply["ok"] != true
            || data["release_id"] != proof["release_id"]
            || data["clockwork_handoff"] != json!(self.handoff()?)
            || !data["clockwork_definition"].is_null()
        {
            return Err(failure(
                "child response does not prove the exact inert handoff",
            ));
        }
        Ok(data.clone())
    }
    fn child_receipt(&self, proof: &Value) -> Result<()> {
        let receipt = self.target.join("install/last-update.json");
        if exists(&receipt) {
            file(&receipt, self.uid, Some(0o600))?;
            return Ok(());
        }
        let reply = self.transaction.join("child-reply.json");
        let data = if exists(&reply) {
            file(&reply, self.invoking_uid, Some(0o600))?;
            self.child_reply_data(&serde_json::from_slice(&fs::read(reply)?)?, proof)?
        } else {
            // A completed child may lose its stdout before the parent saves it.
            // Its newly archived committed journal proves completion without
            // replaying --fresh-state against the already-moved generation.
            let baseline = self.transaction.join("child-archives.json");
            file(&baseline, self.invoking_uid, Some(0o600))?;
            let baseline: Vec<String> = serde_json::from_slice(&fs::read(baseline)?)?;
            let mut completed = 0;
            for name in self.child_archives()? {
                if baseline.contains(&name) {
                    continue;
                }
                let root = self.target.join("backups/deployments").join(name);
                dir(&root, self.uid)?;
                let path = root.join("journal.json");
                file(&path, self.uid, Some(0o600))?;
                let journal: Value = serde_json::from_slice(&fs::read(path)?)?;
                if journal["schema"] == 1
                    && journal["home"] == json!(self.home)
                    && journal["key"] == KEY
                    && journal["committed"] == true
                    && journal["fresh_state"] == true
                    && journal["keep_maintenance"] == true
                    && journal["candidate"]["release_id"] == proof["release_id"]
                {
                    completed += 1;
                }
            }
            if completed != 1 {
                return Err(failure(
                    "completed child installation is unproved; retain state and recover its exact transaction",
                ));
            }
            json!({"release_id":proof["release_id"],"clockwork_key":KEY,"clockwork_definition":null,"clockwork_handoff":self.handoff()?})
        };
        self.owner_write(&receipt, &serde_json::to_vec(&data)?, self.uid)
    }
    fn commit_child_handoff(&self) -> Result<()> {
        self.gate(&self.target)?;
        if let Some(pending) = self.child_transactions()?.first() {
            return Err(failure(&format!(
                "recover the retained Annals child transaction first: {}; migration state and gate remain",
                pending.display()
            )));
        }
        self.empty_binding()?;
        let proof = self.verify_handoff()?;
        self.child_receipt(&proof)?;
        let receipt = self.target.join("install/last-update.json");
        file(&receipt, self.uid, Some(0o600))?;
        let receipt: Value = serde_json::from_slice(&fs::read(receipt)?)?;
        if receipt["release_id"] != proof["release_id"]
            || !receipt["clockwork_definition"].is_null()
        {
            return Err(failure(
                "child receipt does not prove the inert migration handoff",
            ));
        }
        self.owner_write(
            &self.transaction.join("handoff.json"),
            &serde_json::to_vec(&proof)?,
            self.invoking_uid,
        )?;
        self.write_phase("committed")
    }
    fn finish(&self) -> Result<Value> {
        self.gate(&self.target)?;
        let proof = self.verify_handoff()?;
        let recorded = self.transaction.join("handoff.json");
        if exists(&recorded) {
            file(&recorded, self.invoking_uid, Some(0o600))?;
            if serde_json::from_slice::<Value>(&fs::read(&recorded)?)? != proof {
                return Err(failure("committed migration handoff changed"));
            }
        } else {
            self.owner_write(&recorded, &serde_json::to_vec(&proof)?, self.invoking_uid)?;
        }
        let definition = self.clock(&[
            "definition",
            "register",
            self.handoff()?
                .to_str()
                .ok_or_else(|| failure("invalid handoff path"))?,
        ])?;
        let digest = definition["digest"]
            .as_str()
            .filter(|value| valid_hash(value))
            .ok_or_else(|| failure("Clockwork omitted the committed definition digest"))?;
        let receipt_path = self.target.join("install/last-update.json");
        file(&receipt_path, self.uid, Some(0o600))?;
        let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
        match receipt.get("clockwork_definition") {
            Some(Value::Null) => receipt["clockwork_definition"] = digest.into(),
            Some(Value::String(previous)) if previous == digest => {}
            _ => {
                return Err(failure(
                    "deployment receipt selects a different Clockwork definition",
                ));
            }
        }
        self.owner_write(
            &receipt_path,
            &serde_json::to_vec_pretty(&receipt)?,
            self.uid,
        )?;
        if let Some((_, Some(selected))) = self.binding()?
            && selected != digest
        {
            return Err(failure(
                "committed migration encountered a foreign Clockwork selection",
            ));
        }
        if !self
            .binding()?
            .is_some_and(|(enabled, selected)| enabled && selected.as_deref() == Some(digest))
        {
            self.clock(&["binding", "switch", KEY, digest])?;
        }
        self.retire()?;
        file(&self.marker(), self.uid, Some(0o600))?;
        fs::remove_file(self.marker())?;
        fs::remove_dir_all(&self.transaction)?;
        Ok(
            json!({"operator":self.operator,"home":self.home,"state":self.target,"release_id":proof["release_id"],"clockwork_definition":digest,"committed":true}),
        )
    }
    fn retire(&self) -> Result<()> {
        if exists(&self.daemon) {
            self.exact_daemon()?;
        }
        if self.system_loaded()? && !self.launch(&["bootout", SYSTEM_TARGET])?.status.success() {
            return Err(failure(
                "legacy system service could not be stopped; committed transaction and maintenance remain",
            ));
        }
        if self.system_loaded()? {
            return Err(failure(
                "legacy system service remains loaded; committed transaction and maintenance remain",
            ));
        }
        if exists(&self.daemon) {
            self.exact_daemon()?;
            fs::remove_file(&self.daemon)?;
        }
        for (name, path) in [
            ("frontend-sha256", &self.frontend),
            ("payload-sha256", &self.payload),
        ] {
            if exists(path) {
                file(path, self.invoking_uid, None)?;
                let proof = self.transaction.join(name);
                if exists(&proof) && hash(path)? != self.record(name)? {
                    return Err(failure("legacy program changed during migration"));
                }
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }
    fn rollback(&self) -> Result<()> {
        let phase = self.phase()?;
        if phase.as_deref() == Some("committed") {
            return Err(failure(
                "committed migration cannot roll back the state root",
            ));
        }
        if phase.as_deref() == Some("installing")
            || phase.as_deref() == Some("rewritten") && self.child_may_have_run()?
        {
            return Err(failure(
                "child installation may have committed; migration state and backups must remain for recovery",
            ));
        }
        self.empty_binding()?;
        let agent = self
            .home
            .join("Library/LaunchAgents/org.annals.inbox.plist");
        if exists(&agent) {
            self.exact_user_agent(&agent)?;
        }
        let _ = self.launch(&["bootout", &format!("gui/{}/org.annals.inbox", self.uid)]);
        if exists(&agent) {
            self.exact_user_agent(&agent)?;
            fs::remove_file(agent)?;
        }
        for (name, artifact) in [
            ("annals", "bin/annals"),
            ("annals-usage", "libexec/annals-usage"),
            ("annals-install", "bin/annals-install"),
        ] {
            let public = self.home.join(".local/bin").join(name);
            if exists(&public) {
                let target = fs::read_link(&public)?;
                if target != self.target.join("install/current").join(artifact) {
                    return Err(failure(
                        "migration rollback refuses a foreign public command",
                    ));
                }
                fs::remove_file(public)?;
            }
        }
        if !exists(&self.legacy) && exists(&self.target) {
            fs::rename(&self.target, &self.legacy)?;
        }
        if exists(&self.legacy) {
            dir(&self.legacy, self.uid)?;
            self.owner_write(
                &self.legacy.join("config.toml"),
                &fs::read(self.transaction.join("config.toml"))?,
                self.uid,
            )?;
            let marker = self.legacy.join("spool/.maintenance");
            if exists(&marker) {
                file(&marker, self.uid, Some(0o600))?;
                fs::remove_file(marker)?;
            }
            // Only directories proved absent before this transaction are its disposable staging.
            for (name, flag) in [("install", "had-install"), ("backups", "had-backups")] {
                if self.record(flag)? == "0" && exists(&self.legacy.join(name)) {
                    fs::remove_dir_all(self.legacy.join(name))?;
                }
            }
        }
        if self.record("was-loaded")? == "1" {
            self.exact_daemon()?;
            let _ = self.launch(&["enable", SYSTEM_TARGET]);
            if !self.system_loaded()?
                && !self
                    .launch(&[
                        "bootstrap",
                        "system",
                        self.daemon
                            .to_str()
                            .ok_or_else(|| failure("invalid daemon path"))?,
                    ])?
                    .status
                    .success()
            {
                return Err(failure("legacy system service restoration failed"));
            }
            let _ = self.launch(&["kickstart", SYSTEM_TARGET]);
        }
        fs::remove_dir_all(&self.transaction)?;
        Ok(())
    }
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// Keep the complete operator and retained-state authority checks together.
#[allow(clippy::too_many_lines)]
fn state(args: &Args) -> Result<State<'_>> {
    let invoking_uid: u32 = id(&["-u"])?
        .parse()
        .map_err(|_| failure("cannot determine invoking user"))?;
    if invoking_uid != 0 && args.legacy_prefix.is_none() {
        return Err(failure("attended system migration requires root"));
    }
    if args
        .legacy_prefix
        .as_ref()
        .is_some_and(|p| !p.is_absolute() || p == Path::new("/"))
    {
        return Err(failure(
            "fixture legacy prefix must be an absolute directory other than root",
        ));
    }
    if args.deploy.is_some() && args.legacy_prefix.is_none() {
        return Err(failure(
            "a replacement child installer is restricted to an isolated fixture prefix",
        ));
    }
    for path in [
        &args.binary,
        &args.usage_binary,
        &args.bundle,
        &args.usage_bundle,
        &args.nucleus,
        &args.nucleus_socket,
        &args.clockwork,
        &args.launchctl,
        &args.dscl,
        &args.operator_runner,
    ] {
        if !path.is_absolute() {
            return Err(failure("migration arguments must be absolute"));
        }
    }
    let prefix = args.legacy_prefix.as_deref().unwrap_or(Path::new("/"));
    let legacy = args
        .legacy_state
        .clone()
        .unwrap_or_else(|| prefix.join("Library/Application Support/Annals"));
    if !legacy.is_absolute() {
        return Err(failure("legacy state must be absolute"));
    }
    let transaction = PathBuf::from(format!("{}.migrate-to-user", legacy.display()));
    let daemon = prefix.join("Library/LaunchDaemons/org.annals.inbox.plist");
    let recovering = exists(&transaction);
    if recovering {
        dir(&transaction, invoking_uid)?;
    }
    let read_record = |name: &str| -> Result<String> {
        let path = transaction.join(name);
        file(&path, invoking_uid, Some(0o600))?;
        Ok(fs::read_to_string(path)?.trim().to_owned())
    };
    let operator = if recovering {
        read_record("operator")?
    } else {
        text(checked(
            Path::new("/usr/bin/plutil"),
            &[
                "-extract".into(),
                "UserName".into(),
                "raw".into(),
                "-o".into(),
                "-".into(),
                daemon.as_os_str().to_owned(),
            ],
        )?)?
    };
    if operator.is_empty()
        || !operator
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(failure("invalid legacy operator identity"));
    }
    let uid: u32 = id(&["-u", &operator])?
        .parse()
        .map_err(|_| failure("legacy operator UID is invalid"))?;
    if uid == 0 {
        return Err(failure("legacy operator must not be root"));
    }
    let group = id(&["-gn", &operator])?;
    let home = if recovering {
        PathBuf::from(read_record("home")?)
    } else {
        let record = text(checked(
            &args.dscl,
            &words(&[
                ".",
                "-read",
                &format!("/Users/{operator}"),
                "NFSHomeDirectory",
            ]),
        )?)?;
        PathBuf::from(
            record
                .strip_prefix("NFSHomeDirectory:")
                .ok_or_else(|| failure("operator home record is invalid"))?
                .trim(),
        )
    };
    if !home.is_absolute() {
        return Err(failure("operator home must be absolute"));
    }
    dir(&home, uid)?;
    Ok(State {
        args,
        invoking_uid,
        operator,
        uid,
        group,
        target: home.join("Library/Application Support/Annals"),
        home,
        legacy,
        transaction,
        daemon,
        frontend: prefix.join("usr/local/bin/annals"),
        payload: prefix.join("usr/local/libexec/annals/annals"),
    })
}

// Keep the ordered durable phases and their compensation in one explicit flow.
#[allow(clippy::too_many_lines)]
pub(super) fn run(args: &Args) -> Result<Value> {
    let state = state(args)?;
    match state.phase()?.as_deref() {
        Some("committed") => return state.finish(),
        Some("installing") => {
            state.commit_child_handoff()?;
            return state.finish();
        }
        Some("rewritten") if state.child_may_have_run()? => {
            state.commit_child_handoff()?;
            return state.finish();
        }
        Some("prepared" | "stopped" | "moved" | "rewritten") => state.rollback()?,
        None => {}
        Some(_) => return Err(failure("unknown retained migration phase")),
    }
    state.exact_daemon()?;
    state.empty_binding()?;
    dir(&state.legacy, state.uid)?;
    for name in ["config.toml", "annals.db", "codex-home", "spool", "log"] {
        let path = state.legacy.join(name);
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(failure("legacy state is incomplete or symbolic"));
        }
    }
    for path in [
        state.target.clone(),
        state
            .home
            .join("Library/LaunchAgents/org.annals.inbox.plist"),
        state.home.join(".local/bin/annals"),
        state.home.join(".local/bin/annals-usage"),
        state.home.join(".local/bin/annals-install"),
    ] {
        if exists(&path) {
            return Err(failure("user Annals state already exists"));
        }
    }
    let config = fs::read_to_string(state.legacy.join("config.toml"))?;
    if !config
        .lines()
        .any(|l| l == "library = \"/Library/Application Support/Annals/annals.db\"")
        || !config
            .lines()
            .any(|l| l == "root = \"/Library/Application Support/Annals/spool\"")
    {
        return Err(failure("legacy config has nonstandard state paths"));
    }
    for path in [&state.frontend, &state.payload] {
        file(path, state.invoking_uid, None)?;
    }
    state.user_checked(&args.binary, &words(&["--version"]))?;
    state.user_checked(&args.usage_binary, &words(&["--version"]))?;
    state.user_checked(&state.frontend, &words(&["stats"]))?;
    fs::create_dir(&state.transaction)?;
    fs::set_permissions(&state.transaction, fs::Permissions::from_mode(0o700))?;
    for (name, value) in [
        ("operator", state.operator.clone()),
        ("home", state.home.display().to_string()),
        ("was-loaded", u8::from(state.system_loaded()?).to_string()),
        (
            "had-install",
            u8::from(exists(&state.legacy.join("install"))).to_string(),
        ),
        (
            "had-backups",
            u8::from(exists(&state.legacy.join("backups"))).to_string(),
        ),
        ("frontend-sha256", hash(&state.frontend)?),
        ("payload-sha256", hash(&state.payload)?),
    ] {
        state.owner_write(
            &state.transaction.join(name),
            format!("{value}\n").as_bytes(),
            state.invoking_uid,
        )?;
    }
    state.owner_write(
        &state.transaction.join("config.toml"),
        config.as_bytes(),
        state.invoking_uid,
    )?;
    state.write_phase("prepared")?;
    let result = (|| -> Result<Value> {
        let _ = state.launch(&["disable", SYSTEM_TARGET]);
        state.gate(&state.legacy)?;
        let deadline = Instant::now() + Duration::from_secs(args.wait_seconds);
        loop {
            let status =
                state.user_checked(&state.frontend, &words(&["--json", "inbox", "status"]))?;
            let status: Value = serde_json::from_slice(&status.stdout)?;
            if status
                .pointer("/data/locked")
                .or_else(|| status.get("locked"))
                == Some(&Value::Bool(false))
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err(failure(
                    "legacy inbox did not drain; maintenance remains until recovery",
                ));
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        let _ = state.launch(&["bootout", SYSTEM_TARGET]);
        state.write_phase("stopped")?;
        let parent = state
            .target
            .parent()
            .ok_or_else(|| failure("target state has no parent"))?;
        if !exists(parent) {
            state.user_checked(
                Path::new("/bin/mkdir"),
                &["-p".into(), parent.as_os_str().to_owned()],
            )?;
            state.user_checked(
                Path::new("/bin/chmod"),
                &["0700".into(), parent.as_os_str().to_owned()],
            )?;
        }
        dir(parent, state.uid)?;
        if fs::metadata(&state.legacy)?.dev() != fs::metadata(parent)?.dev() {
            return Err(failure(
                "migration requires legacy and user state on one filesystem",
            ));
        }
        fs::rename(&state.legacy, &state.target)?;
        state.write_phase("moved")?;
        let rewritten = config
            .lines()
            .map(|line| match line {
                "library = \"/Library/Application Support/Annals/annals.db\"" => {
                    "library = \"annals.db\""
                }
                "root = \"/Library/Application Support/Annals/spool\"" => "root = \"spool\"",
                other => other,
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        state.owner_write(
            &state.target.join("config.toml"),
            rewritten.as_bytes(),
            state.uid,
        )?;
        state.gate(&state.target)?;
        state.write_phase("rewritten")?;
        let installer = args.deploy.clone().unwrap_or(std::env::current_exe()?);
        let mut child = vec![OsString::from("install")];
        for (flag, value) in [
            ("--binary", &args.binary),
            ("--usage-binary", &args.usage_binary),
            ("--bundle", &args.bundle),
            ("--usage-bundle", &args.usage_bundle),
            ("--nucleus", &args.nucleus),
            ("--nucleus-socket", &args.nucleus_socket),
            ("--clockwork", &args.clockwork),
            ("--home", &state.home),
            ("--launchctl", &args.launchctl),
        ] {
            child.push(flag.into());
            child.push(value.as_os_str().to_owned());
        }
        child.extend(words(&["--fresh-state", "--migration-clockwork-handoff"]));
        state.owner_write(
            &state.transaction.join("child-archives.json"),
            &serde_json::to_vec(&state.child_archives()?)?,
            state.invoking_uid,
        )?;
        // The child can archive the original generation before returning. A
        // missing response never authorizes deleting those recovery backups.
        state.write_phase("installing")?;
        let response = state.user_checked(&installer, &child)?;
        let response: Value = serde_json::from_slice(&response.stdout)?;
        let proof = state.verify_handoff()?;
        state.child_reply_data(&response, &proof)?;
        state.owner_write(
            &state.transaction.join("child-reply.json"),
            &serde_json::to_vec(&response)?,
            state.invoking_uid,
        )?;
        // State cannot move after this fsynced phase, even if registration fails.
        state.commit_child_handoff()?;
        state.finish()
    })();
    match result {
        Ok(value) => Ok(value),
        Err(mut error) => {
            if state.phase()?.as_deref() == Some("committed") {
                error.disposition = Disposition::Uncertain;
                error.message.push_str(
                    "; committed migration requires forward resumption of the same handoff",
                );
            } else if state.rollback().is_ok() {
                error.disposition = Disposition::Restored;
            } else {
                error.disposition = Disposition::Uncertain;
                error.message.push_str(
                    "; legacy restoration is unproved; transaction and maintenance remain",
                );
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(launch: &str) -> Result<(tempfile::TempDir, Args)> {
        let root = tempfile::tempdir()?;
        let launchctl = root.path().join("launchctl");
        fs::write(&launchctl, format!("#!/bin/sh\n{launch}\n"))?;
        fs::set_permissions(&launchctl, fs::Permissions::from_mode(0o700))?;
        let unused = root.path().join("must-not-run");
        let args = Args {
            binary: unused.clone(),
            usage_binary: unused.clone(),
            bundle: unused.clone(),
            usage_bundle: unused.clone(),
            nucleus: unused.clone(),
            nucleus_socket: unused.clone(),
            clockwork: unused.clone(),
            legacy_prefix: Some(root.path().to_owned()),
            legacy_state: None,
            launchctl,
            dscl: unused.clone(),
            operator_runner: unused,
            deploy: None,
            wait_seconds: 0,
        };
        Ok((root, args))
    }

    fn state<'a>(args: &'a Args, root: &Path) -> Result<State<'a>> {
        let uid = fs::metadata(root)?.uid();
        let state = State {
            args,
            invoking_uid: uid,
            operator: "fixture".into(),
            uid,
            group: "fixture".into(),
            home: root.join("home"),
            legacy: root.join("legacy"),
            transaction: root.join("transaction"),
            target: root.join("state"),
            frontend: root.join("annals"),
            payload: root.join("annals-core"),
            daemon: root.join("absent-daemon.plist"),
        };
        for path in [&state.transaction, &state.target.join("spool")] {
            fs::create_dir_all(path)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        state.write_phase("committed")?;
        state.gate(&state.target)?;
        for path in [&state.frontend, &state.payload] {
            fs::write(path, b"retained legacy program")?;
        }
        Ok(state)
    }

    #[test]
    fn committed_rollback_keeps_the_state_root_and_gate_without_calling_children() -> Result<()> {
        let (root, args) = fixture("exit 91")?;
        let state = state(&args, root.path())?;
        let error = state
            .rollback()
            .err()
            .ok_or_else(|| failure("expected operation to fail"))?;
        assert!(
            error
                .message
                .contains("committed migration cannot roll back")
        );
        assert!(state.target.is_dir());
        assert!(!state.legacy.exists());
        assert!(state.marker().is_file());
        assert_eq!(state.phase()?.as_deref(), Some("committed"));
        Ok(())
    }

    #[test]
    fn uncertain_child_commit_preserves_the_original_generation_in_backups() -> Result<()> {
        let (root, args) = fixture("exit 91")?;
        let state = state(&args, root.path())?;
        state.write_phase("installing")?;
        let original = state
            .target
            .join("backups/deployments/child/original/annals.db");
        fs::create_dir_all(
            original
                .parent()
                .ok_or_else(|| failure("fixture archive has no parent"))?,
        )?;
        fs::write(&original, b"original database")?;
        let error = state
            .rollback()
            .err()
            .ok_or_else(|| failure("expected operation to fail"))?;
        assert!(
            error
                .message
                .contains("child installation may have committed")
        );
        assert_eq!(fs::read(original)?, b"original database");
        assert!(state.target.is_dir());
        assert!(!state.legacy.exists());
        assert!(state.marker().is_file());
        Ok(())
    }

    #[test]
    fn legacy_rewritten_phase_with_a_child_selector_cannot_delete_child_backups() -> Result<()> {
        let (root, args) = fixture("exit 91")?;
        let state = state(&args, root.path())?;
        state.write_phase("rewritten")?;
        fs::create_dir(state.target.join("install"))?;
        std::os::unix::fs::symlink(
            format!("releases/{}", "a".repeat(64)),
            state.target.join("install/current"),
        )?;
        let error = state
            .rollback()
            .err()
            .ok_or_else(|| failure("expected operation to fail"))?;
        assert!(
            error
                .message
                .contains("child installation may have committed")
        );
        assert!(state.target.is_dir());
        assert!(!state.legacy.exists());
        assert!(state.marker().is_file());
        Ok(())
    }

    #[test]
    fn a_new_committed_child_archive_recovers_a_lost_success_response() -> Result<()> {
        let (root, args) = fixture("exit 91")?;
        let state = state(&args, root.path())?;
        fs::create_dir(state.target.join("install"))?;
        let proof = json!({"release_id":"a".repeat(64)});
        state.owner_write(
            &state.transaction.join("child-archives.json"),
            b"[]",
            state.invoking_uid,
        )?;
        let archive = state
            .target
            .join("backups/deployments/transaction.primary.fixture");
        fs::create_dir_all(&archive)?;
        state.owner_write(
            &archive.join("journal.json"),
            &serde_json::to_vec(&json!({"schema":1,"home":state.home,"key":KEY,"committed":true,"fresh_state":true,"keep_maintenance":true,"candidate":{"release_id":proof["release_id"]}}))?,
            state.uid,
        )?;
        state.child_receipt(&proof)?;
        let receipt: Value =
            serde_json::from_slice(&fs::read(state.target.join("install/last-update.json"))?)?;
        assert_eq!(receipt["release_id"], proof["release_id"]);
        assert!(receipt["clockwork_definition"].is_null());
        assert!(state.marker().is_file());
        Ok(())
    }

    #[test]
    fn an_archive_that_predates_the_child_does_not_prove_its_success() -> Result<()> {
        let (root, args) = fixture("exit 91")?;
        let state = state(&args, root.path())?;
        fs::create_dir(state.target.join("install"))?;
        let archive = state
            .target
            .join("backups/deployments/transaction.primary.prior");
        fs::create_dir_all(&archive)?;
        state.owner_write(
            &state.transaction.join("child-archives.json"),
            br#"["transaction.primary.prior"]"#,
            state.invoking_uid,
        )?;
        let error = state
            .child_receipt(&json!({"release_id":"a".repeat(64)}))
            .err()
            .ok_or_else(|| failure("expected operation to fail"))?;
        assert!(
            error
                .message
                .contains("completed child installation is unproved")
        );
        assert!(!state.target.join("install/last-update.json").exists());
        assert!(state.marker().is_file());
        Ok(())
    }

    #[test]
    fn unsuccessful_bootout_retains_every_legacy_artifact_and_the_gate() -> Result<()> {
        let (root, args) =
            fixture("case \"$1\" in print) exit 0;; bootout) exit 1;; *) exit 92;; esac")?;
        let state = state(&args, root.path())?;
        let error = state
            .retire()
            .err()
            .ok_or_else(|| failure("expected operation to fail"))?;
        assert!(error.message.contains("could not be stopped"));
        assert!(state.frontend.is_file());
        assert!(state.payload.is_file());
        assert!(state.marker().is_file());
        assert_eq!(state.phase()?.as_deref(), Some("committed"));
        Ok(())
    }

    #[test]
    fn successful_bootout_still_requires_a_fresh_absence_proof() -> Result<()> {
        let (root, args) = fixture("case \"$1\" in print|bootout) exit 0;; *) exit 92;; esac")?;
        let state = state(&args, root.path())?;
        let error = state
            .retire()
            .err()
            .ok_or_else(|| failure("expected operation to fail"))?;
        assert!(error.message.contains("remains loaded"));
        assert!(state.frontend.is_file());
        assert!(state.payload.is_file());
        assert!(state.marker().is_file());
        Ok(())
    }

    #[test]
    fn an_inspection_error_is_not_proof_that_the_service_is_absent() -> Result<()> {
        let (root, args) = fixture("echo 'inspection unavailable' >&2; exit 1")?;
        let state = state(&args, root.path())?;
        let error = state
            .retire()
            .err()
            .ok_or_else(|| failure("expected operation to fail"))?;
        assert!(error.message.contains("absence is unproved"));
        assert!(state.frontend.is_file());
        assert!(state.payload.is_file());
        assert!(state.marker().is_file());
        Ok(())
    }

    #[test]
    fn migration_id_is_only_an_exact_lowercase_sha256() {
        assert!(valid_hash(&"a".repeat(64)));
        for invalid in [
            "a".repeat(63),
            "A".repeat(64),
            format!("{}g", "a".repeat(63)),
            format!("../{}", "a".repeat(64)),
        ] {
            assert!(!valid_hash(&invalid));
        }
    }
}
