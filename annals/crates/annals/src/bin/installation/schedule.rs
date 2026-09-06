use super::{
    Error, MINUTE, OsString, Path, Process, Result, Value, call, environment, install_root, json,
    release, write_private,
};
use cell_install::ReleaseInfo;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct Control {
    pub present: bool,
    pub enabled: bool,
    pub digest: Option<String>,
}

fn checked_hash(value: &str) -> Result<String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::new("Clockwork returned an invalid digest"));
    }
    Ok(value.to_owned())
}

pub(super) fn inspect(home: &Path, clockwork: &Path, key: &str) -> Result<Control> {
    let output = cell_install::command::run(
        clockwork,
        &["--json".into(), "binding".into(), "show".into(), key.into()],
        &environment(home, None),
        MINUTE,
    )?;
    if !output.status.success() {
        let value: Value = serde_json::from_slice(if output.stdout.is_empty() {
            &output.stderr
        } else {
            &output.stdout
        })?;
        if value.pointer("/error/code").and_then(Value::as_str) == Some("binding_not_found") {
            return Ok(Control {
                present: false,
                enabled: false,
                digest: None,
            });
        }
        return Err(Error::new("Clockwork binding inspection failed"));
    }
    let value: Value = serde_json::from_slice(&output.stdout)?;
    let data = value
        .get("data")
        .ok_or_else(|| Error::new("Clockwork response has no data"))?;
    if value.get("ok") != Some(&json!(true)) || data.get("key") != Some(&json!(key)) {
        return Err(Error::new("Clockwork returned a foreign binding"));
    }
    let enabled = data
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| Error::new("invalid Clockwork enabled state"))?;
    let digest = match data.get("definition_digest") {
        Some(Value::Null) => None,
        Some(Value::String(digest)) => Some(checked_hash(digest)?),
        _ => return Err(Error::new("invalid Clockwork selection")),
    };
    if enabled && digest.is_none() {
        return Err(Error::new("enabled Clockwork binding has no definition"));
    }
    Ok(Control {
        present: true,
        enabled,
        digest,
    })
}

pub(super) fn definition(
    home: &Path,
    key: &str,
    library: &Path,
    info: &ReleaseInfo,
) -> Result<Value> {
    let root = release::root(home, info);
    let user = Process::new("/usr/bin/id").arg("-un").output()?;
    if !user.status.success() {
        return Err(Error::new("operator name unavailable"));
    }
    let user = String::from_utf8_lossy(&user.stdout).trim().to_owned();
    let runner = root.join("bin/annals-inbox");
    let launch = if info.format == cell_install::TRANSACTION_FORMAT {
        json!({"kind":"direct","program":runner,"sha256":cell_install::file_digest(&runner)?})
    } else {
        json!({"kind":"interpreted","interpreter":"/bin/sh","interpreter_sha256":cell_install::file_digest(Path::new("/bin/sh"))?,"script":runner,"script_sha256":cell_install::file_digest(&runner)?})
    };
    Ok(json!({
        "schema_version":1,"key":key,"release_id":info.release_id,"release_root":root,
        "authority":"current-user-background","overlap":"skip","arguments":[],"cwd":library,
        "schedule":{"kind":"interval","seconds":300,"run_at_load":true},"launch":launch,
        "environment":{"HOME":home,"USER":user,"LOGNAME":user,"ANNALS_CONFIG":library.join("config.toml")},
        "output":{"stdout":library.join("log/inbox.stdout.log"),"stderr":library.join("log/inbox.stderr.log")}
    }))
}

pub(super) fn prove(
    home: &Path,
    clockwork: &Path,
    key: &str,
    library: &Path,
    control: &Control,
    info: Option<&ReleaseInfo>,
) -> Result<()> {
    if let Some(digest) = &control.digest {
        let info = info.ok_or_else(|| Error::new("Annals binding has no proved release"))?;
        cell_install::verify_release_at(
            &release::layout(),
            &release::root(home, info),
            &release::legacy,
        )?;
        let value = call(
            clockwork,
            &[
                "--json".into(),
                "definition".into(),
                "show".into(),
                digest.into(),
            ],
            home,
            None,
        )?;
        let expected = definition(home, key, library, info)?;
        if value.pointer("/data/key") != Some(&json!(key))
            || value.pointer("/data/digest") != Some(&json!(digest))
            || value.pointer("/data/manifest") != Some(&expected)
        {
            return Err(Error::new(
                "Clockwork definition differs from complete Annals release definition",
            ));
        }
    }
    Ok(())
}

pub(super) fn selected_release(
    home: &Path,
    clockwork: &Path,
    control: &Control,
) -> Result<Option<ReleaseInfo>> {
    let Some(digest) = &control.digest else {
        return Ok(None);
    };
    let value = call(
        clockwork,
        &[
            "--json".into(),
            "definition".into(),
            "show".into(),
            digest.into(),
        ],
        home,
        None,
    )?;
    let id = value
        .pointer("/data/manifest/release_id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("selected Annals definition has no release"))?;
    checked_hash(id)?;
    let root = install_root(home).join("releases").join(id);
    if value.pointer("/data/manifest/release_root") != Some(&json!(root)) {
        return Err(Error::new(
            "selected Annals definition names a foreign release root",
        ));
    }
    Ok(Some(cell_install::verify_release_at(
        &release::layout(),
        &root,
        &release::legacy,
    )?))
}

pub(super) fn render(path: &Path, value: &Value) -> Result<()> {
    let text = toml::to_string(value)
        .map_err(|_| Error::new("cannot render Annals Clockwork definition"))?;
    write_private(path, text.as_bytes(), false)
}

pub(super) fn register(
    home: &Path,
    clockwork: &Path,
    key: &str,
    definition: &Path,
) -> Result<String> {
    let value = call(
        clockwork,
        &[
            "--json".into(),
            "definition".into(),
            "register".into(),
            definition.as_os_str().to_owned(),
        ],
        home,
        None,
    )?;
    if value.pointer("/data/key") != Some(&json!(key)) {
        return Err(Error::new("Clockwork registered another binding"));
    }
    checked_hash(
        value
            .pointer("/data/digest")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("Clockwork registration returned no digest"))?,
    )
}

pub(super) fn disable(
    home: &Path,
    clockwork: &Path,
    key: &str,
    expected: &Control,
) -> Result<Control> {
    if inspect(home, clockwork, key)? != *expected {
        return Err(Error::new("Annals binding changed before disable"));
    }
    if !expected.enabled {
        return Ok(expected.clone());
    }
    call(
        clockwork,
        &[
            "--json".into(),
            "binding".into(),
            "disable".into(),
            key.into(),
        ],
        home,
        None,
    )?;
    let actual = inspect(home, clockwork, key)?;
    if actual.enabled || actual.digest != expected.digest {
        return Err(Error::new("Annals binding did not disable coherently"));
    }
    Ok(actual)
}

pub(super) fn select(
    home: &Path,
    clockwork: &Path,
    key: &str,
    expected: &Control,
    digest: &str,
    enabled: bool,
) -> Result<Control> {
    if inspect(home, clockwork, key)? != *expected {
        return Err(Error::new("Annals binding changed before selection"));
    }
    let mut args: Vec<OsString> = [
        "--json",
        "binding",
        if enabled { "switch" } else { "disable" },
        key,
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    if !enabled {
        args.push("--select".into());
    }
    args.push(digest.into());
    call(clockwork, &args, home, None)?;
    let actual = inspect(home, clockwork, key)?;
    if actual.enabled != enabled || actual.digest.as_deref() != Some(digest) {
        return Err(Error::new("Annals binding did not select exact candidate"));
    }
    Ok(actual)
}

pub(super) fn restore(
    home: &Path,
    clockwork: &Path,
    key: &str,
    prior: &Control,
    candidate: Option<&str>,
    exact_absence: bool,
) -> Result<()> {
    let actual = inspect(home, clockwork, key)?;
    if actual.digest.is_some()
        && actual.digest != prior.digest
        && actual.digest.as_deref() != candidate
    {
        return Err(Error::new("unattributable Annals binding during recovery"));
    }
    if actual == *prior {
        return Ok(());
    }
    if let Some(digest) = &prior.digest {
        select(home, clockwork, key, &actual, digest, prior.enabled)?;
    } else {
        if exact_absence && actual.digest.is_some() {
            return Err(Error::new(
                "Clockwork cannot erase a committed first selection; Annals maintenance retained",
            ));
        }
        disable(home, clockwork, key, &actual)?;
    }
    Ok(())
}
