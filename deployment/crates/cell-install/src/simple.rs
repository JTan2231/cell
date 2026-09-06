//! Shared installer entry point for products whose installation selects program
//! bytes without owning a database or service lifecycle.

use crate::adapter::{self, Context, Operation};
use crate::legacy::{self, LegacySpec};
use crate::transaction::{
    self as tx, InstallLayout, InstallSnapshot, LockKind, LockSpec, PreparedRelease, ProviderSpec,
    PublicEntry, PublicKind, ReleaseInfo, ReleasePlan, SourceFile,
};
use crate::{Error, Result, command, file_digest};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

pub struct Spec {
    pub product: &'static str,
    pub application: &'static str,
    pub source_directory: &'static str,
    pub provider_source: &'static str,
    pub legacy_provider_path: &'static str,
    pub legacy: &'static LegacySpec,
    pub wrapper: Option<&'static [u8]>,
    pub lock_kind: LockKind,
    pub lock_at_state: bool,
    pub maintained: bool,
}

impl Spec {
    #[must_use]
    pub fn layout(&self) -> InstallLayout {
        let state = format!("Library/Application Support/{}", self.application);
        InstallLayout {
            product: self.product.to_owned(),
            application: self.application.to_owned(),
            product_lock: LockSpec {
                path: PathBuf::from(format!(
                    "{state}/{suffix}.update-lock",
                    suffix = if self.lock_at_state { "" } else { "install/" }
                )),
                kind: self.lock_kind.clone(),
            },
            catalog_lock: Some(LockSpec {
                path: PathBuf::from("Library/Application Support/Chancery/.catalog-update-lock"),
                kind: LockKind::Shlock,
            }),
            public: vec![
                self.command_entry(false),
                self.installer_entry(),
                self.provider_entry(false),
            ],
        }
    }

    fn command_entry(&self, _legacy: bool) -> PublicEntry {
        PublicEntry {
            path: PathBuf::from(format!(".local/bin/{}", self.product)),
            artifact: format!("bin/{}", self.product),
            kind: PublicKind::Symlink,
            mode: 0o555,
        }
    }
    fn installer_entry(&self) -> PublicEntry {
        PublicEntry {
            path: PathBuf::from(format!(".local/bin/{}-install", self.product)),
            artifact: format!("bin/{}-install", self.product),
            kind: PublicKind::Symlink,
            mode: 0o555,
        }
    }
    fn provider_entry(&self, old: bool) -> PublicEntry {
        PublicEntry {
            path: PathBuf::from(format!(
                "Library/Application Support/Chancery/providers/{}",
                self.product
            )),
            artifact: if old {
                self.legacy_provider_path.to_owned()
            } else {
                format!("share/chancery/{}", self.product)
            },
            kind: PublicKind::Symlink,
            mode: 0o555,
        }
    }

    /// Read only this product's supported predecessor release.
    ///
    /// # Errors
    /// Returns an error for foreign or unproved predecessor content.
    pub fn legacy(&self, root: &Path) -> Result<ReleaseInfo> {
        let values = legacy::manifest(&root.join(self.legacy.manifest))?;
        if values
            .get("product")
            .is_some_and(|product| product != self.product)
        {
            return Err(Error::new("foreign legacy product"));
        }
        legacy::verify(
            root,
            self.legacy,
            vec![self.command_entry(true), self.provider_entry(true)],
        )
    }

    fn install_root(&self, home: &Path) -> PathBuf {
        home.join("Library/Application Support")
            .join(self.application)
            .join("install")
    }
}

struct Input {
    home: PathBuf,
    binary: Option<PathBuf>,
    bundle: Option<PathBuf>,
    reader: Option<PathBuf>,
    release: Option<PathBuf>,
    expected: Option<String>,
}

fn input(args: &[String]) -> Result<Input> {
    let mut value = Input {
        home: std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| Error::new("HOME is required"))?,
        binary: None,
        bundle: None,
        reader: None,
        release: None,
        expected: None,
    };
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let argument = args
            .next()
            .ok_or_else(|| Error::new("installer flag requires a value"))?;
        match flag.as_str() {
            "--home" => value.home = PathBuf::from(argument),
            "--binary" => value.binary = Some(PathBuf::from(argument)),
            "--bundle" => value.bundle = Some(PathBuf::from(argument)),
            "--chancery" => value.reader = Some(PathBuf::from(argument)),
            "--release" => value.release = Some(PathBuf::from(argument)),
            "--expected-current" => value.expected = Some(argument.clone()),
            _ => return Err(Error::new("unknown installer flag")),
        }
    }
    if !value.home.is_absolute()
        || [&value.binary, &value.bundle, &value.reader, &value.release]
            .into_iter()
            .flatten()
            .any(|path| !path.is_absolute())
    {
        return Err(Error::new("installer paths must be absolute"));
    }
    Ok(value)
}

fn expected(snapshot: &InstallSnapshot, value: Option<&str>) -> Result<()> {
    let selected = snapshot.current.as_ref().map_or_else(
        || "absent".to_owned(),
        |release| format!("releases/{}", release.release_id),
    );
    if value.is_some_and(|value| value != selected) {
        return Err(Error::new("stale deployment: installed selection changed"));
    }
    Ok(())
}

fn version(binary: &Path, name: &str, expected: &str, home: &Path) -> Result<()> {
    let output = command::checked(
        binary,
        &["--version".into()],
        &BTreeMap::from([("HOME".into(), home.as_os_str().to_owned())]),
        Duration::from_secs(30),
    )?;
    if String::from_utf8_lossy(&output.stdout).trim() != format!("{name} {expected}") {
        return Err(Error::new(
            "candidate version does not match provider and installer",
        ));
    }
    Ok(())
}

fn plan(spec: &Spec, release_version: &str, args: &Input, scratch: &Path) -> Result<ReleasePlan> {
    let binary = args
        .binary
        .as_ref()
        .ok_or_else(|| Error::new("--binary is required"))?;
    let bundle = args
        .bundle
        .as_ref()
        .ok_or_else(|| Error::new("--bundle is required"))?;
    version(binary, spec.product, release_version, &args.home)?;
    let installer = std::env::current_exe()?;
    let mut files = BTreeMap::from([
        (
            format!("bin/{}-install", spec.product),
            SourceFile {
                source: installer.clone(),
                mode: 0o555,
            },
        ),
        (
            "package/install".to_owned(),
            SourceFile {
                source: installer,
                mode: 0o555,
            },
        ),
    ]);
    if let Some(wrapper) = spec.wrapper {
        let path = scratch.join("frontend");
        std::fs::write(&path, wrapper)?;
        for relative in [
            format!("bin/{}", spec.product),
            format!("package/{}", spec.product),
        ] {
            files.insert(
                relative,
                SourceFile {
                    source: path.clone(),
                    mode: 0o555,
                },
            );
        }
        files.insert(
            format!("libexec/{}", spec.product),
            SourceFile {
                source: binary.clone(),
                mode: 0o555,
            },
        );
    } else {
        files.insert(
            format!("bin/{}", spec.product),
            SourceFile {
                source: binary.clone(),
                mode: 0o555,
            },
        );
    }
    let inventory = crate::provider_inventory(
        bundle,
        &crate::InstallSpec {
            product: spec.product,
            application: spec.application,
            commands: &[],
            provider: spec.product,
        },
    )?;
    for relative in inventory.keys() {
        files.insert(
            format!("share/chancery/{}/{relative}", spec.product),
            SourceFile {
                source: bundle.join(relative),
                mode: 0o444,
            },
        );
    }
    Ok(ReleasePlan {
        files,
        versions: BTreeMap::from([
            (spec.product.to_owned(), release_version.to_owned()),
            (
                format!("{}-install", spec.product),
                release_version.to_owned(),
            ),
        ]),
        providers: BTreeMap::from([(
            spec.product.to_owned(),
            ProviderSpec {
                path: format!("share/chancery/{}", spec.product),
                version: release_version.to_owned(),
            },
        )]),
    })
}

fn smoke(spec: &Spec, home: &Path, release: &ReleaseInfo, reader: Option<&Path>) -> Result<()> {
    let environment = BTreeMap::from([("HOME".into(), home.as_os_str().to_owned())]);
    let expected = release
        .versions
        .get(spec.product)
        .ok_or_else(|| Error::new("installed version is missing"))?;
    let public = home.join(".local/bin").join(spec.product);
    version(&public, spec.product, expected, home)?;
    command::checked(
        &public,
        &["--help".into()],
        &environment,
        Duration::from_secs(30),
    )?;
    if release
        .files
        .contains_key(&format!("bin/{}-install", spec.product))
    {
        version(
            &home.join(format!(".local/bin/{}-install", spec.product)),
            &format!("{}-install", spec.product),
            expected,
            home,
        )?;
    }
    if spec.product == "chancery" {
        command::checked(
            &public,
            &["list".into()],
            &environment,
            Duration::from_secs(30),
        )?;
    }
    if spec.product == "clockwork" {
        let reader =
            reader.ok_or_else(|| Error::new("Clockwork installation requires --chancery"))?;
        for entry in [
            "clockwork.schedule.operate",
            "clockwork.install.operate",
            "clockwork.develop.change",
        ] {
            command::checked(
                reader,
                &[
                    "--registry".into(),
                    home.join("Library/Application Support/Chancery/providers")
                        .into_os_string(),
                    "show".into(),
                    entry.into(),
                ],
                &environment,
                Duration::from_secs(30),
            )?;
        }
    }
    Ok(())
}

fn prove(plan: &ReleasePlan, release: &ReleaseInfo) -> Result<()> {
    if release.format != tx::TRANSACTION_FORMAT
        || release.versions != plan.versions
        || release.files.len() != plan.files.len()
    {
        return Err(Error::new("installed release differs from candidate"));
    }
    for (relative, source) in &plan.files {
        if release
            .files
            .get(relative)
            .is_none_or(|file| file.mode != source.mode)
            || release.files[relative].sha256 != file_digest(&source.source)?
        {
            return Err(Error::new(
                "installed artifact differs from exact candidate",
            ));
        }
    }
    Ok(())
}

fn install(spec: &Spec, release_version: &str, args: &Input) -> Result<ReleaseInfo> {
    let layout = spec.layout();
    let legacy = |root: &Path| spec.legacy(root);
    let before = tx::inspect_installation(&layout, &args.home, &legacy)?;
    expected(&before, args.expected.as_deref())?;
    let scratch = tempfile::tempdir()?;
    let plan = plan(spec, release_version, args, scratch.path())?;
    let prepared = tx::prepare_release(&layout, &args.home, &plan)?;
    if spec.product == "clockwork" || spec.product == "chancery" {
        let reader = if spec.product == "chancery" {
            args.binary.as_deref()
        } else {
            args.reader.as_deref()
        }
        .ok_or_else(|| Error::new("--chancery is required"))?;
        file_digest(reader)?;
        command::checked(
            reader,
            &[
                "validate".into(),
                prepared
                    .root
                    .join(format!("share/chancery/{}", spec.product))
                    .into_os_string(),
            ],
            &BTreeMap::new(),
            Duration::from_secs(30),
        )?;
    }
    let mut transaction = tx::lock_installation(&layout, &args.home, &legacy)?;
    transaction.recheck(&before)?;
    transaction.publish(&prepared, &before, |release| {
        prove(&plan, release)?;
        smoke(spec, &args.home, release, args.reader.as_deref())
    })?;
    Ok(prepared.info)
}

fn current(spec: &Spec, home: &Path) -> Result<InstallSnapshot> {
    tx::inspect_installation(&spec.layout(), home, &|root| spec.legacy(root))
}

fn maintained(spec: &Spec, context: &Context, operation: &str, owner: bool) -> Result<Value> {
    maintained_at(
        spec,
        context,
        operation,
        owner,
        &context.home.join(".local/bin").join(spec.product),
    )
}

fn maintained_at(
    _spec: &Spec,
    context: &Context,
    operation: &str,
    owner: bool,
    binary: &Path,
) -> Result<Value> {
    let mut args: Vec<OsString> = vec!["--json".into(), "maintenance".into(), operation.into()];
    if owner {
        args.push(context.request.run_id.clone().into());
    }
    let env = BTreeMap::from([(
        "CELL_DEPLOYMENT_RUN_ID".into(),
        context.request.run_id.clone().into(),
    )]);
    let result = command::json(binary, &args, &env, Duration::from_secs(660))?;
    let status = command::maintenance(&result)?;
    if status.get("protocol_version") != Some(&json!(1)) {
        return Err(Error::new("unsupported maintenance protocol"));
    }
    Ok(status.clone())
}

#[allow(clippy::too_many_lines)] // Keep fixed protocol operations and their shared baseline together.
fn adapter_run(spec: &Spec, release_version: &str, operation: Operation) -> Result<Value> {
    let context = Context::read(
        spec.product,
        spec.source_directory,
        &format!("{}-install", spec.product),
        release_version,
    )?;
    let mut args = Input {
        home: context.home.clone(),
        binary: Some(context.binary(spec.product)?),
        bundle: Some(context.request.source_root.join(spec.provider_source)),
        reader: None,
        release: None,
        expected: None,
    };
    if spec.product == "clockwork" {
        args.reader = Some(std::fs::canonicalize(
            context.home.join(".local/bin/chancery"),
        )?);
    }
    if operation == Operation::Inspect {
        let scratch = tempfile::tempdir()?;
        plan(spec, release_version, &args, scratch.path())?;
        let snapshot = current(spec, &context.home)?;
        let runtime = if spec.maintained {
            let status = maintained(spec, &context, "status", false)?;
            if status["holds"] != json!([]) {
                return Err(Error::new("another operation holds product maintenance"));
            }
            status
        } else {
            Value::Null
        };
        return Ok(adapter::reply(
            "ready",
            "owned installation inspected",
            json!({"installation":snapshot,"runtime":runtime,"maintenance_products":if spec.maintained {vec!["nucleus"]} else {vec![]}}),
        ));
    }
    let prior: InstallSnapshot = serde_json::from_value(
        context
            .prior()?
            .get("installation")
            .cloned()
            .ok_or_else(|| Error::new("prior installation evidence missing"))?,
    )?;
    let observed = match current(spec, &context.home) {
        Ok(value) => value,
        Err(_) if operation == Operation::Recover && context.selected() => {
            let forward = context
                .request
                .recovery
                .as_ref()
                .and_then(|value| value.get("any_apply_started"))
                == Some(&Value::Bool(true));
            let scratch = tempfile::tempdir()?;
            let plan = plan(spec, release_version, &args, scratch.path())?;
            let prepared = tx::prepare_release(&spec.layout(), &context.home, &plan)?;
            if spec.maintained && forward {
                let binary = context.binary(spec.product)?;
                let status = maintained_at(spec, &context, "status", false, &binary)?;
                if status["holds"] != json!([context.request.run_id]) || status["drained"] != true {
                    return Err(Error::new(
                        "partial installation recovery requires drained run-owned hold",
                    ));
                }
                command::json(
                    &binary,
                    &["--json".into(), "doctor".into()],
                    &BTreeMap::from([(
                        "CELL_DEPLOYMENT_RUN_ID".into(),
                        context.request.run_id.clone().into(),
                    )]),
                    Duration::from_secs(180),
                )?;
            }
            let layout = spec.layout();
            let legacy = |root: &Path| spec.legacy(root);
            let mut transaction = tx::lock_installation(&layout, &context.home, &legacy)?;
            transaction
                .recover(&prior, &prepared, forward, |release| {
                    smoke(spec, &context.home, release, args.reader.as_deref())
                })?
                .after
        }
        Err(error) => return Err(error),
    };
    let before_apply = matches!(
        operation,
        Operation::Hold | Operation::Drain | Operation::Apply
    );
    if before_apply && observed != prior {
        return Err(Error::new("installation changed since inspection"));
    }
    let data = match operation {
        Operation::Hold if spec.maintained => maintained(spec, &context, "hold", true)?,
        Operation::Drain if spec.maintained => {
            let start = Instant::now();
            loop {
                let status = maintained(spec, &context, "drain", false)?;
                if status["holds"] != json!([context.request.run_id]) {
                    return Err(Error::new("drain requires sole run-owned hold"));
                }
                if status["drained"] == true {
                    break status;
                }
                if start.elapsed() > Duration::from_secs(600) {
                    return Err(Error::new("product work did not drain; hold retained"));
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        Operation::Apply => {
            if !context.selected() {
                return Err(Error::new("cannot install an affected-only product"));
            }
            if spec.maintained {
                let status = maintained(spec, &context, "status", false)?;
                if status["holds"] != json!([context.request.run_id]) || status["drained"] != true {
                    return Err(Error::new("installation requires drained run-owned hold"));
                }
                command::json(
                    &context.binary(spec.product)?,
                    &[
                        "--json".into(),
                        "migrate".into(),
                        "--backup".into(),
                        spec.install_root(&context.home)
                            .parent()
                            .ok_or_else(|| Error::new("invalid state root"))?
                            .join(format!(
                                "crm-pre-migration-{}.sqlite",
                                context.request.run_id
                            ))
                            .into_os_string(),
                    ],
                    &BTreeMap::from([(
                        "CELL_DEPLOYMENT_RUN_ID".into(),
                        context.request.run_id.clone().into(),
                    )]),
                    Duration::from_secs(600),
                )?;
            }
            args.expected = Some(prior.current.as_ref().map_or_else(
                || "absent".to_owned(),
                |release| format!("releases/{}", release.release_id),
            ));
            json!(install(spec, release_version, &args)?)
        }
        Operation::Verify | Operation::Recover => {
            let is_prior = observed == prior;
            if !is_prior || operation == Operation::Verify && context.selected() {
                let scratch = tempfile::tempdir()?;
                let plan = plan(spec, release_version, &args, scratch.path())?;
                prove(
                    &plan,
                    observed
                        .current
                        .as_ref()
                        .ok_or_else(|| Error::new("installed candidate absent"))?,
                )?;
            }
            if let Some(release) = &observed.current {
                smoke(spec, &context.home, release, args.reader.as_deref())?;
            }
            if spec.maintained {
                let status = maintained(spec, &context, "status", false)?;
                let verified = context
                    .request
                    .recovery
                    .as_ref()
                    .and_then(|value| value.get("verified"))
                    == Some(&Value::Bool(true));
                let before = context
                    .request
                    .recovery
                    .as_ref()
                    .and_then(|value| value.get("any_apply_started"))
                    == Some(&Value::Bool(false));
                if !(is_prior && before || verified && status["holds"] == json!([]))
                    && (status["holds"] != json!([context.request.run_id])
                        || status["drained"] != true)
                {
                    return Err(Error::new(
                        "maintained recovery needs exact run hold or captured verification",
                    ));
                }
                if !(is_prior && before) {
                    command::json(
                        &context.home.join(".local/bin").join(spec.product),
                        &["--json".into(), "doctor".into()],
                        &BTreeMap::from([(
                            "CELL_DEPLOYMENT_RUN_ID".into(),
                            context.request.run_id.clone().into(),
                        )]),
                        Duration::from_secs(180),
                    )?;
                }
            }
            json!({"installation":observed,"safe_to_release":true,"installed":if is_prior {"prior"} else {"candidate"}})
        }
        Operation::Release if spec.maintained => maintained(spec, &context, "release", true)?,
        Operation::Hold | Operation::Drain | Operation::Release => json!({"drained":true}),
        Operation::Inspect => return Err(Error::new("invalid adapter dispatch")),
    };
    let status = match operation {
        Operation::Inspect => "ready",
        Operation::Hold => "held",
        Operation::Drain => "drained",
        Operation::Apply => "applied",
        Operation::Verify => "verified",
        Operation::Recover => "recovered",
        Operation::Release => "released",
    };
    Ok(adapter::reply(
        status,
        "product installation operation completed",
        data,
    ))
}

fn execute(spec: &Spec, release_version: &str, arguments: &[String]) -> Result<Value> {
    let (operation, remaining) = arguments
        .split_first()
        .ok_or_else(|| Error::new("installer operation is required"))?;
    if operation == "adapter" {
        if remaining.len() != 1 {
            return Err(Error::new("adapter requires one operation"));
        }
        return adapter_run(spec, release_version, remaining[0].parse()?);
    }
    if operation == "verify-release" {
        if remaining.len() != 1 {
            return Err(Error::new("verify-release requires one release directory"));
        }
        return Ok(
            json!({"ok":true,"data":tx::verify_release_at(&spec.layout(),Path::new(&remaining[0]),&|root|spec.legacy(root))?}),
        );
    }
    let args = input(remaining)?;
    let data = match operation.as_str() {
        "install" => json!(install(spec, release_version, &args)?),
        "inspect" => json!(current(spec, &args.home)?),
        "verify" => {
            let snapshot = current(spec, &args.home)?;
            let scratch = tempfile::tempdir()?;
            let plan = plan(spec, release_version, &args, scratch.path())?;
            let release = snapshot
                .current
                .ok_or_else(|| Error::new("installation absent"))?;
            prove(&plan, &release)?;
            smoke(spec, &args.home, &release, args.reader.as_deref())?;
            json!(release)
        }
        "recover" => {
            let release = args
                .release
                .as_ref()
                .ok_or_else(|| Error::new("--release is required"))?;
            if release.parent() != Some(spec.install_root(&args.home).join("releases").as_path()) {
                return Err(Error::new("recovery release is outside owned installation"));
            }
            let layout = spec.layout();
            let legacy = |root: &Path| spec.legacy(root);
            let info = tx::verify_release_at(&layout, release, &legacy)?;
            let before = tx::inspect_detached_installation(&layout, &args.home, &legacy)?;
            expected(&before, args.expected.as_deref())?;
            let mut transaction = tx::lock_installation(&layout, &args.home, &legacy)?;
            transaction.publish(
                &PreparedRelease {
                    root: release.clone(),
                    info: info.clone(),
                },
                &before,
                |release| smoke(spec, &args.home, release, args.reader.as_deref()),
            )?;
            json!(info)
        }
        "uninstall" if spec.product == "clockwork" => {
            let layout = spec.layout();
            let legacy = |root: &Path| spec.legacy(root);
            let before = current(spec, &args.home)?;
            let mut transaction = tx::lock_installation(&layout, &args.home, &legacy)?;
            for (path, transition) in [
                (
                    args.home
                        .join("Library/Application Support/Clockwork/locks"),
                    true,
                ),
                (args.home.join("Library/LaunchAgents"), false),
            ] {
                match std::fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.is_dir() => {
                        for entry in std::fs::read_dir(&path)? {
                            let name = entry?.file_name();
                            let name = name.to_string_lossy();
                            if transition && name.ends_with(".transition.json")
                                || !transition
                                    && name.starts_with("org.clockwork.")
                                    && name.ends_with(".plist")
                            {
                                return Err(Error::new(
                                    "Clockwork binding transitions must be recovered and bindings disabled before uninstall",
                                ));
                            }
                        }
                    }
                    Ok(_) => {
                        return Err(Error::new(
                            "Clockwork locks or LaunchAgents path is not a regular directory",
                        ));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            transaction.detach(&before)?;
            json!({"detached":true})
        }
        _ => return Err(Error::new("unsupported installer operation")),
    };
    Ok(json!({"ok":true,"data":data}))
}

/// Run a product's stateless installation boundary; direct runtime state remains
/// outside this entry point. The maintained coordinator route is explicit.
#[must_use]
pub fn main(spec: &Spec, version: &str) -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments == ["--version"] || arguments == ["-V"] {
        println!("{}-install {version}", spec.product);
        return ExitCode::SUCCESS;
    }
    if arguments.is_empty() || arguments == ["--help"] || arguments == ["-h"] {
        println!(
            "{}-install {version}\n\ninstall --binary ABS --bundle ABS [--home ABS] [--expected-current absent|releases/HASH]\ninspect [--home ABS]\nverify --binary ABS --bundle ABS [--home ABS]\nverify-release ABS\nrecover --release ABS [--home ABS] [--expected-current absent|releases/HASH]\nClockwork requires --chancery ABS for installation/verification/recovery; uninstall detaches only owned selectors.",
            spec.product
        );
        return ExitCode::SUCCESS;
    }
    let adapter = arguments.first().is_some_and(|value| value == "adapter");
    let result = execute(spec, version, &arguments);
    if adapter {
        return adapter::finish(result);
    }
    match result {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!(
                "{}",
                json!({"ok":false,"error":{"detail":error.message,"disposition":error.disposition}})
            );
            ExitCode::FAILURE
        }
    }
}
