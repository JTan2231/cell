//! Exact readers for retained Decisions and Krisis shell releases.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

use cell_install::{Error, Result, file_digest};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug)]
pub struct Release {
    pub id: String,
    pub version: String,
    pub format: u32,
    pub files: BTreeMap<String, String>,
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::new(message))
    }
}

pub fn hash_lines(values: &[String]) -> String {
    let mut hash = Sha256::new();
    for value in values {
        hash.update(value.as_bytes());
        hash.update(b"\n");
    }
    format!("{:x}", hash.finalize())
}

pub fn hexadecimal(value: &str, size: usize) -> bool {
    value.len() == size
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn pairs(path: &Path) -> Result<Vec<(String, String)>> {
    file_digest(path)?;
    let text = fs::read_to_string(path)?;
    require(text.ends_with('\n'), "release receipt is not canonical")?;
    let mut seen = BTreeSet::new();
    text.lines()
        .map(|line| {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| Error::new("invalid release receipt"))?;
            require(
                seen.insert(key.to_owned()),
                "duplicate release receipt field",
            )?;
            Ok((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn inventory(root: &Path, directory: &Path, uid: u32, output: &mut BTreeSet<String>) -> Result<()> {
    let info = fs::symlink_metadata(directory)?;
    require(
        info.is_dir() && info.uid() == uid && info.mode() & 0o022 == 0,
        "release directory is foreign or writable",
    )?;
    let entries: Vec<_> = fs::read_dir(directory)?.collect::<std::io::Result<_>>()?;
    require(
        !entries.is_empty(),
        "release contains an empty uncommitted directory",
    )?;
    for entry in entries {
        let path = entry.path();
        let info = fs::symlink_metadata(&path)?;
        require(
            info.uid() == uid && info.mode() & 0o022 == 0 && !info.file_type().is_symlink(),
            "release contains unsafe ownership or a symbolic entry",
        )?;
        if info.is_dir() {
            inventory(root, &path, uid, output)?;
        } else {
            require(
                info.is_file() && info.nlink() == 1,
                "release contains a shared or special file",
            )?;
            let name = path
                .strip_prefix(root)
                .map_err(|_| Error::new("release path escaped its root"))?
                .to_str()
                .ok_or_else(|| Error::new("release path is not UTF-8"))?
                .to_owned();
            require(
                !name.contains(['\r', '\n']),
                "release path contains a line break",
            )?;
            output.insert(name);
        }
    }
    Ok(())
}

pub fn bundle_hash(root: &Path) -> Result<String> {
    let mut files = BTreeSet::new();
    inventory(root, root, fs::symlink_metadata(root)?.uid(), &mut files)?;
    let mut hash = Sha256::new();
    for name in files {
        hash.update(format!(
            "path=./{name}\n{}  ./{name}\n",
            file_digest(&root.join(&name))?
        ));
    }
    Ok(format!("{:x}", hash.finalize()))
}

// Keep the ordered legacy manifest recipe and its complete inventory proof together.
#[allow(clippy::too_many_lines)]
pub fn read(root: &Path, uid: u32) -> Result<Release> {
    let id = root
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| Error::new("invalid release identity"))?;
    require(hexadecimal(id, 64), "invalid retained release identity")?;
    let fields = pairs(&root.join("manifest.txt"))?;
    require(
        fields.len() >= 3
            && fields[0].0 == "format"
            && fields[1] == ("release_id".into(), id.into())
            && fields[2].0 == "version",
        "invalid retained release manifest header",
    )?;
    let format = fields[0]
        .1
        .parse::<u32>()
        .map_err(|_| Error::new("invalid retained release format"))?;
    let version = fields[2].1.clone();
    let mut expected = vec![
        "format",
        "release_id",
        "version",
        "binary_sha256",
        "frontend_sha256",
    ];
    let mut proofs: Vec<(&str, Vec<String>)> = vec![];
    if format == 4 {
        expected.extend([
            "observer_runner_sha256",
            "observer_clockwork_definition_sha256",
            "hooks_sha256",
            "deployer_sha256",
            "uninstaller_sha256",
            "krisis_chancery_sha256",
            "decisions_chancery_sha256",
        ]);
        proofs.extend([
            ("binary_sha256", vec!["libexec/krisis".into()]),
            ("frontend_sha256", vec!["bin/krisis".into()]),
            ("observer_runner_sha256", vec!["bin/krisis-observer".into()]),
            (
                "observer_clockwork_definition_sha256",
                vec!["package/krisis-observer.clockwork.toml.in".into()],
            ),
        ]);
        let frontend = root.join("package/krisis");
        let runner = root.join("package/krisis-observer");
        if frontend.try_exists()? && runner.try_exists()? {
            proofs[1].1.push("package/krisis".into());
            proofs[2].1.push("package/krisis-observer".into());
        } else {
            require(
                !frontend.try_exists()?
                    && !runner.try_exists()?
                    && matches!(version.as_str(), "0.4.0" | "0.4.1"),
                "retained Krisis packaged executables are missing",
            )?;
        }
    } else {
        require(
            matches!(format, 2 | 3),
            "unsupported retained Decisions release format",
        )?;
        let (daily_field, observer_field, daily, observer) = if format == 2 {
            (
                "daily_plist_sha256",
                "observer_plist_sha256",
                "package/org.decisions.daily-email.plist",
                "package/org.decisions.observer.plist",
            )
        } else {
            (
                "daily_clockwork_definition_sha256",
                "observer_clockwork_definition_sha256",
                "package/decisions-daily-email.clockwork.toml.in",
                "package/decisions-observer.clockwork.toml.in",
            )
        };
        expected.extend([
            "daily_runner_sha256",
            "observer_runner_sha256",
            daily_field,
            observer_field,
            "hooks_sha256",
            "deployer_sha256",
            "uninstaller_sha256",
            "chancery_sha256",
        ]);
        proofs.extend([
            ("binary_sha256", vec!["libexec/decisions".into()]),
            ("frontend_sha256", vec!["bin/decisions".into()]),
            (
                "daily_runner_sha256",
                vec!["bin/decisions-daily-email".into()],
            ),
            (
                "observer_runner_sha256",
                vec!["bin/decisions-observer".into()],
            ),
            (daily_field, vec![daily.into()]),
            (observer_field, vec![observer.into()]),
        ]);
    }
    if matches!(format, 2 | 3) {
        for (position, name) in [
            (1, "decisions"),
            (2, "decisions-daily-email"),
            (3, "decisions-observer"),
        ] {
            let relative = format!("package/{name}");
            if root.join(&relative).symlink_metadata().is_ok() {
                proofs[position].1.push(relative);
            }
        }
    }
    proofs.extend([
        ("hooks_sha256", vec!["package/hooks.json".into()]),
        ("deployer_sha256", vec!["package/deploy-user.sh".into()]),
        (
            "uninstaller_sha256",
            vec!["package/uninstall-user.sh".into()],
        ),
    ]);
    require(
        fields.iter().map(|(k, _)| k.as_str()).eq(expected),
        "retained release manifest field order differs",
    )?;
    let values: BTreeMap<_, _> = fields.into_iter().collect();
    let mut files = BTreeMap::new();
    let mut identity = vec![];
    for (key, paths) in proofs {
        let digest = &values[key];
        require(
            hexadecimal(digest, 64),
            "retained release digest is invalid",
        )?;
        for path in paths {
            require(
                file_digest(&root.join(&path))? == *digest,
                "retained release artifact is tampered",
            )?;
            files.insert(path, digest.clone());
        }
        identity.push(digest.clone());
    }
    let providers = if format == 4 {
        vec![
            ("krisis_chancery_sha256", "krisis"),
            ("decisions_chancery_sha256", "decisions"),
        ]
    } else {
        vec![("chancery_sha256", "decisions")]
    };
    for (key, provider) in providers {
        let directory = root.join("share/chancery").join(provider);
        let bundle = bundle_hash(&directory)?;
        require(
            bundle == values[key],
            "retained provider bundle is tampered",
        )?;
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("provider.json"))?)?;
        require(
            manifest
                .pointer("/provider/id")
                .and_then(serde_json::Value::as_str)
                == Some(provider)
                && manifest
                    .pointer("/provider/release")
                    .and_then(serde_json::Value::as_str)
                    == Some(version.as_str()),
            "retained provider identity or version differs",
        )?;
        let mut inventory_names = BTreeSet::new();
        inventory(&directory, &directory, uid, &mut inventory_names)?;
        for name in inventory_names {
            files.insert(
                format!("share/chancery/{provider}/{name}"),
                file_digest(&directory.join(name))?,
            );
        }
        identity.push(bundle);
    }
    require(
        hash_lines(&identity) == id,
        "retained release content identity differs",
    )?;
    files.insert(
        "manifest.txt".into(),
        file_digest(&root.join("manifest.txt"))?,
    );
    let mut actual = BTreeSet::new();
    inventory(root, root, uid, &mut actual)?;
    require(
        actual == files.keys().cloned().collect(),
        "retained release contains unmanifested files",
    )?;
    Ok(Release {
        id: id.into(),
        version,
        format,
        files,
    })
}
