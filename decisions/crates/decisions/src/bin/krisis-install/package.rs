use super::{
    Install, legacy,
    support::{Paths, require},
};
use cell_install::transaction::{
    self, InstallLayout, InstallSnapshot, LockKind, LockSpec, PreparedRelease, ProviderSpec,
    PublicEntry, PublicKind, ReleaseInfo, ReleasePlan, SourceFile,
};
use cell_install::{Error, FileEntry, Result, file_digest};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

fn entry(path: &str, artifact: &str) -> PublicEntry {
    PublicEntry {
        path: path.into(),
        artifact: artifact.into(),
        kind: PublicKind::Symlink,
        mode: 0o755,
    }
}

pub fn layout() -> InstallLayout {
    InstallLayout {
        product: "krisis".into(),
        application: "Decisions".into(),
        product_lock: LockSpec {
            path: "Library/Application Support/Decisions/install/.update-lock".into(),
            kind: LockKind::Directory,
        },
        catalog_lock: Some(LockSpec {
            path: "Library/Application Support/Chancery/.catalog-update-lock".into(),
            kind: LockKind::Shlock,
        }),
        public: vec![
            entry(".local/bin/krisis", "bin/krisis"),
            entry(".local/bin/krisis-install", "bin/krisis-install"),
            entry(
                "Library/Application Support/Chancery/providers/krisis",
                "share/chancery/krisis",
            ),
            entry(
                "Library/Application Support/Chancery/providers/decisions",
                "share/chancery/decisions",
            ),
        ],
    }
}

pub fn legacy_info(root: &Path, uid: u32) -> Result<ReleaseInfo> {
    let old = legacy::read(root, uid)?;
    let public = if old.format == 4 {
        layout()
            .public
            .into_iter()
            .filter(|item| item.artifact != "bin/krisis-install")
            .collect()
    } else {
        vec![
            entry(".local/bin/decisions", "bin/decisions"),
            entry(
                "Library/Application Support/Chancery/providers/decisions",
                "share/chancery/decisions",
            ),
        ]
    };
    let files = old
        .files
        .into_iter()
        .map(|(name, sha256)| {
            let mode = fs::symlink_metadata(root.join(&name))?.mode() & 0o7777;
            Ok((name, FileEntry { sha256, mode }))
        })
        .collect::<Result<_>>()?;
    Ok(ReleaseInfo {
        release_id: old.id,
        format: format!("legacy-{}", old.format),
        versions: BTreeMap::from([("krisis".into(), old.version)]),
        files,
        public,
    })
}

pub fn inspect(paths: &Paths) -> Result<InstallSnapshot> {
    transaction::inspect_installation(&layout(), &paths.home, &|root| legacy_info(root, paths.uid))
}

pub fn verify(root: &Path, uid: u32) -> Result<ReleaseInfo> {
    require(root.is_absolute(), "release path must be absolute")?;
    transaction::verify_release_at(&layout(), root, &|root| legacy_info(root, uid))
}

pub fn root(paths: &Paths, info: &ReleaseInfo) -> PathBuf {
    paths.install.join("releases").join(&info.release_id)
}

fn add_bundle(files: &mut BTreeMap<String, SourceFile>, source: &Path, prefix: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    require(
        metadata.is_dir() && metadata.mode() & 0o022 == 0,
        "provider source directory is unsafe",
    )?;
    for item in fs::read_dir(source)? {
        let item = item?;
        let path = item.path();
        let name = item
            .file_name()
            .into_string()
            .map_err(|_| Error::new("provider name is not UTF-8"))?;
        let key = format!("{prefix}/{name}");
        if fs::symlink_metadata(&path)?.is_dir() {
            add_bundle(files, &path, &key)?;
        } else {
            file_digest(&path)?;
            files.insert(
                key,
                SourceFile {
                    source: path,
                    mode: 0o644,
                },
            );
        }
    }
    Ok(())
}

// Keep the complete release inventory and its source/recovery selection in one place.
#[allow(clippy::too_many_lines)]
pub fn plan(options: &Install) -> Result<ReleasePlan> {
    let installer = std::env::current_exe()?;
    let source = options
        .source_root
        .clone()
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let (package, providers) = if options.source_root.is_none()
        && installer
            .parent()
            .is_some_and(|parent| parent.file_name().is_some_and(|name| name == "package"))
    {
        let package = installer
            .parent()
            .ok_or_else(|| Error::new("installer has no package"))?
            .to_owned();
        (
            package.clone(),
            vec![
                ("krisis", package.join("../share/chancery/krisis")),
                ("decisions", package.join("../share/chancery/decisions")),
            ],
        )
    } else {
        (
            source.join("packaging/macos"),
            vec![
                ("krisis", source.join("chancery")),
                ("decisions", source.join("chancery-legacy")),
            ],
        )
    };
    require(
        options.binary.is_absolute(),
        "candidate binary must be absolute",
    )?;
    let mut files = BTreeMap::from([
        (
            "libexec/krisis".into(),
            SourceFile {
                source: options.binary.clone(),
                mode: 0o755,
            },
        ),
        (
            "bin/krisis-install".into(),
            SourceFile {
                source: installer.clone(),
                mode: 0o755,
            },
        ),
        (
            "package/install".into(),
            SourceFile {
                source: installer,
                mode: 0o755,
            },
        ),
    ]);
    for (name, mode) in [
        ("krisis", 0o755),
        ("krisis-observer", 0o755),
        ("krisis-observer.clockwork.toml.in", 0o644),
        ("hooks.json", 0o600),
    ] {
        file_digest(&package.join(name))?;
        let source = fs::canonicalize(package.join(name))?;
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
    let mut bundle_specs = BTreeMap::new();
    for (name, source) in providers {
        let source = fs::canonicalize(source)?;
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(source.join("provider.json"))?)?;
        require(
            manifest["provider"]["id"] == name
                && manifest["provider"]["release"] == env!("CARGO_PKG_VERSION"),
            "provider and installer versions differ",
        )?;
        let prefix = format!("share/chancery/{name}");
        add_bundle(&mut files, &source, &prefix)?;
        bundle_specs.insert(
            name.into(),
            ProviderSpec {
                path: prefix,
                version: env!("CARGO_PKG_VERSION").into(),
            },
        );
    }
    Ok(ReleasePlan {
        files,
        providers: bundle_specs,
        versions: BTreeMap::from([
            ("krisis".into(), env!("CARGO_PKG_VERSION").into()),
            ("krisis-install".into(), env!("CARGO_PKG_VERSION").into()),
        ]),
    })
}

pub fn prepare(paths: &Paths, options: &Install) -> Result<PreparedRelease> {
    let output = super::support::checked(
        paths,
        &options.binary,
        &super::support::args(&["--version"]),
        &BTreeMap::new(),
        30,
    )?;
    require(
        String::from_utf8_lossy(&output).trim() == format!("krisis {}", env!("CARGO_PKG_VERSION")),
        "candidate and installer versions differ",
    )?;
    transaction::prepare_release(&layout(), &paths.home, &plan(options)?)
}

pub fn matches_candidate(paths: &Paths, info: &ReleaseInfo, options: &Install) -> Result<()> {
    require(
        info.format == transaction::TRANSACTION_FORMAT,
        "selected release is not a Rust candidate",
    )?;
    let plan = plan(options)?;
    require(
        info.files.len() == plan.files.len(),
        "candidate artifact inventory differs",
    )?;
    for (path, source) in plan.files {
        let actual = info
            .files
            .get(&path)
            .ok_or_else(|| Error::new("candidate artifact is absent"))?;
        require(
            actual.sha256 == file_digest(&source.source)? && actual.mode == source.mode,
            "selected artifact differs from candidate",
        )?;
    }
    verify(&root(paths, info), paths.uid)?;
    Ok(())
}
