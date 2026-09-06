use super::{
    Duration, Error, MINUTE, OsString, Path, PathBuf, Result, environment, fs, install_root,
    optional_private, private_file, state, uid, write_private,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Agent {
    pub bytes: Option<Vec<u8>>,
    pub loaded: bool,
}

fn plist(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents/org.annals.inbox.plist")
}
fn target() -> Result<String> {
    Ok(format!("gui/{}/org.annals.inbox", uid()?))
}

fn execute(home: &Path, launchctl: &Path, args: &[OsString]) -> Result<std::process::Output> {
    cell_install::command::run(launchctl, args, &environment(home, None), MINUTE)
}

fn loaded(home: &Path, launchctl: &Path) -> Result<bool> {
    Ok(
        execute(home, launchctl, &["print".into(), target()?.into()])?
            .status
            .success(),
    )
}

pub(super) fn capture(
    home: &Path,
    launchctl: &Path,
    no_start: bool,
    clockwork_enabled: bool,
) -> Result<Agent> {
    let path = plist(home);
    let bytes = if optional_private(&path)? {
        let actual = fs::read(&path)?;
        let temporary = tempfile::tempdir_in(install_root(home))?;
        let expected = temporary.path().join("agent.plist");
        write_private(
            &expected,
            include_bytes!("../../../../../packaging/launchd/org.annals.inbox.agent.plist"),
            false,
        )?;
        let fields = [
            ("ProgramArguments.0", home.join(".local/bin/annals")),
            ("WorkingDirectory", state(home)),
            ("EnvironmentVariables.HOME", home.to_owned()),
            ("StandardOutPath", state(home).join("log/inbox.stdout.log")),
            (
                "StandardErrorPath",
                state(home).join("log/inbox.stderr.log"),
            ),
        ];
        for (key, value) in fields {
            cell_install::command::checked(
                Path::new("/usr/bin/plutil"),
                &[
                    "-replace".into(),
                    key.into(),
                    "-string".into(),
                    value.into_os_string(),
                    expected.as_os_str().to_owned(),
                ],
                &environment(home, None),
                Duration::from_secs(30),
            )?;
        }
        if fs::read(&expected)? != actual {
            return Err(Error::new(
                "legacy Annals LaunchAgent is not the exact owned template",
            ));
        }
        Some(actual)
    } else {
        None
    };
    let loaded = !no_start && loaded(home, launchctl)?;
    if loaded && (bytes.is_none() || clockwork_enabled) {
        return Err(Error::new(
            "Annals has an unowned or competing active legacy service",
        ));
    }
    Ok(Agent { bytes, loaded })
}

pub(super) fn retire(home: &Path, launchctl: &Path, before: &Agent) -> Result<()> {
    let Some(bytes) = &before.bytes else {
        return Ok(());
    };
    private_file(&plist(home))?;
    if fs::read(plist(home))? != *bytes {
        return Err(Error::new(
            "legacy Annals LaunchAgent changed before retirement",
        ));
    }
    if loaded(home, launchctl)? != before.loaded {
        return Err(Error::new(
            "legacy Annals service changed before retirement",
        ));
    }
    cell_install::command::checked(
        launchctl,
        &["disable".into(), target()?.into()],
        &environment(home, None),
        MINUTE,
    )?;
    if before.loaded {
        cell_install::command::checked(
            launchctl,
            &["bootout".into(), "--wait".into(), target()?.into()],
            &environment(home, None),
            MINUTE,
        )?;
    }
    if loaded(home, launchctl)? {
        return Err(Error::new("legacy Annals service is still loaded"));
    }
    if fs::read(plist(home))? != *bytes {
        return Err(Error::new(
            "legacy Annals LaunchAgent changed during retirement",
        ));
    }
    fs::remove_file(plist(home))?;
    Ok(())
}

pub(super) fn restore(home: &Path, launchctl: &Path, before: &Agent) -> Result<()> {
    let Some(bytes) = &before.bytes else {
        return Ok(());
    };
    if optional_private(&plist(home))? {
        if fs::read(plist(home))? != *bytes {
            return Err(Error::new("foreign Annals LaunchAgent blocks restoration"));
        }
    } else {
        write_private(&plist(home), bytes, false)?;
    }
    if before.loaded {
        cell_install::command::checked(
            launchctl,
            &["enable".into(), target()?.into()],
            &environment(home, None),
            MINUTE,
        )?;
        if !loaded(home, launchctl)? {
            cell_install::command::checked(
                launchctl,
                &[
                    "bootstrap".into(),
                    format!("gui/{}", uid()?).into(),
                    plist(home).into_os_string(),
                ],
                &environment(home, None),
                MINUTE,
            )?;
        }
        if !loaded(home, launchctl)? {
            return Err(Error::new(
                "legacy Annals service restoration could not be proved",
            ));
        }
    }
    Ok(())
}
