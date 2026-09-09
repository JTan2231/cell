//! Todo's schedule definition and guarded Clockwork handoff.

use super::{
    BTreeMap, CommandExt, Error, OsString, Path, PathBuf, PermissionsExt, Process, Result, call,
    fs, home, install_root, layout, legacy, owned, state, strings, write_atomic,
};
use clockwork::api::{
    Authority, BindingRecord, DefinitionRecord, FailurePolicy, LaunchImage, Manifest,
    Output as LogOutput, OverlapPolicy, Schedule as Trigger, SelectionPage,
};

pub(super) const KEY: &str = "todo/daily-email";

fn invoke<T: serde::de::DeserializeOwned>(home: &Path, args: &[OsString]) -> Result<T> {
    let mut arguments = vec!["--json".into()];
    arguments.extend_from_slice(args);
    let output = call(
        &home.join(".local/bin/clockwork"),
        &arguments,
        home,
        None,
        180,
    )?;
    clockwork::api::decode(&output.stdout).map_err(|error| Error::new(error.to_string()))
}

pub(super) fn binding(home: &Path) -> Result<Option<BindingRecord>> {
    let mut limit = 100_usize;
    loop {
        let page: SelectionPage<BindingRecord> = invoke(
            home,
            &strings(&["binding", "list", "--limit", &limit.to_string()]),
        )?;
        if !page.has_more {
            return Ok(page.items.into_iter().find(|item| item.key == KEY));
        }
        limit = limit
            .checked_mul(2)
            .ok_or_else(|| Error::new("Clockwork binding list is incomplete"))?;
    }
}

pub(super) fn definition(home: &Path, root: &Path) -> Result<Manifest> {
    let info = cell_install::transaction::verify_release_at(&layout(), root, &legacy)?;
    let runner = root.join("bin/todo-daily-email");
    Ok(Manifest {
        schema_version: 2,
        key: KEY.into(),
        release_id: info.release_id,
        release_root: root.to_string_lossy().into_owned(),
        authority: Authority::CurrentUserBackground,
        overlap: OverlapPolicy::Skip,
        failure: FailurePolicy::default(),
        timeout_seconds: Some(180),
        arguments: vec![],
        cwd: state(home).to_string_lossy().into_owned(),
        schedule: Trigger::LocalCalendar {
            hour: 9,
            minute: 0,
            run_at_load: false,
        },
        launch: LaunchImage::Direct {
            program: runner.to_string_lossy().into_owned(),
            sha256: cell_install::file_digest(&runner)?,
        },
        environment: BTreeMap::from([("HOME".into(), home.to_string_lossy().into_owned())]),
        output: LogOutput {
            stdout: home
                .join("Library/Logs/Todo/email.stdout.log")
                .to_string_lossy()
                .into_owned(),
            stderr: home
                .join("Library/Logs/Todo/email.stderr.log")
                .to_string_lossy()
                .into_owned(),
        },
    })
}

pub(super) fn verify(
    home: &Path,
    binding: &BindingRecord,
    expected_root: Option<&Path>,
) -> Result<()> {
    let Some(digest) = &binding.definition_digest else {
        if binding.enabled {
            return Err(Error::new(
                "Todo Clockwork binding has no selected definition",
            ));
        }
        return Ok(());
    };
    let saved: DefinitionRecord = invoke(home, &strings(&["definition", "show", digest]))?;
    let root = PathBuf::from(&saved.manifest.release_root);
    if root.parent() != Some(install_root(home).join("releases").as_path())
        || expected_root.is_some_and(|expected| expected != root)
        || saved.manifest != definition(home, &root)?
    {
        return Err(Error::new(
            "Todo Clockwork definition is foreign or changed",
        ));
    }
    Ok(())
}

pub(super) fn disable(home: &Path, selection: Option<&str>) -> Result<BindingRecord> {
    let mut args = strings(&["binding", "disable", KEY]);
    if let Some(digest) = selection {
        args.extend(strings(&["--select", digest]));
    }
    invoke(home, &args)
}

pub(super) fn select(home: &Path, digest: &str, enabled: bool) -> Result<BindingRecord> {
    if enabled {
        invoke(home, &strings(&["binding", "switch", KEY, digest]))
    } else {
        disable(home, Some(digest))
    }
}

pub(super) fn register(home: &Path, root: &Path, staging: &Path) -> Result<DefinitionRecord> {
    let manifest = definition(home, root)?;
    // Definitions and log files contain no credential or email body.
    for output in [&manifest.output.stdout, &manifest.output.stderr] {
        let path = Path::new(output);
        match owned(path, home)? {
            Some(_) => fs::set_permissions(path, fs::Permissions::from_mode(0o600))?,
            None => write_atomic(path, &[], 0o600)?,
        }
    }
    let file = staging.join("clockwork.toml");
    write_atomic(
        &file,
        manifest
            .to_toml()
            .map_err(|error| Error::new(error.to_string()))?
            .as_bytes(),
        0o600,
    )?;
    invoke(
        home,
        &[
            "definition".into(),
            "register".into(),
            file.into_os_string(),
        ],
    )
}

pub(super) fn frontend() -> Result<()> {
    let home = home(None)?;
    let binary = fs::canonicalize(std::env::current_exe()?)?;
    let root = binary
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| Error::new("invalid Todo runner"))?;
    let mut command = Process::new("/bin/zsh");
    command
        .arg("--no-rcs")
        .arg(root.join("package/todo-daily-email"))
        .arg(root.join("libexec/todo"))
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    Err(command.exec().into())
}
