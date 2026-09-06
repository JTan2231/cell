use super::{
    BTreeMap, Duration, Error, InstallArgs, Path, PathBuf, Result, VERSION, environment,
    install_root,
};
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::{
    InstallLayout, LockKind, LockSpec, PreparedRelease, ProviderSpec, PublicEntry, PublicKind,
    ReleaseInfo, ReleasePlan, SourceFile,
};

pub(super) fn public(installer: bool) -> Vec<PublicEntry> {
    let mut entries = vec![
        PublicEntry {
            path: ".local/bin/annals".into(),
            artifact: "bin/annals".into(),
            kind: PublicKind::Symlink,
            mode: 0o755,
        },
        PublicEntry {
            path: ".local/bin/annals-usage".into(),
            artifact: "libexec/annals-usage".into(),
            kind: PublicKind::Symlink,
            mode: 0o755,
        },
    ];
    for name in ["annals", "annals-usage"] {
        entries.push(PublicEntry {
            path: format!("Library/Application Support/Chancery/providers/{name}").into(),
            artifact: format!("share/chancery/{name}"),
            kind: PublicKind::Symlink,
            mode: 0o755,
        });
    }
    if installer {
        entries.push(PublicEntry {
            path: ".local/bin/annals-install".into(),
            artifact: "bin/annals-install".into(),
            kind: PublicKind::Symlink,
            mode: 0o755,
        });
    }
    entries
}

pub(super) fn layout() -> InstallLayout {
    InstallLayout {
        product: "annals".into(),
        application: "Annals".into(),
        public: public(true),
        product_lock: LockSpec {
            path: "Library/Application Support/Annals/install/.update-lock".into(),
            kind: LockKind::Directory,
        },
        catalog_lock: Some(LockSpec {
            path: "Library/Application Support/Chancery/.catalog-update-lock".into(),
            kind: LockKind::Shlock,
        }),
    }
}

const LEGACY_PROOFS: &[LegacyProof] = &[
    LegacyProof {
        key: "binary_sha256",
        paths: &["libexec/annals"],
    },
    LegacyProof {
        key: "usage_binary_sha256",
        paths: &["libexec/annals-usage"],
    },
    LegacyProof {
        key: "frontend_sha256",
        paths: &["bin/annals", "package/annals-user"],
    },
    LegacyProof {
        key: "runner_sha256",
        paths: &["bin/annals-inbox", "package/annals-inbox"],
    },
    LegacyProof {
        key: "clockwork_template_sha256",
        paths: &["package/annals-inbox.clockwork.toml.in"],
    },
    LegacyProof {
        key: "decisions_config_template_sha256",
        paths: &["package/annals-decisions.toml.in"],
    },
    LegacyProof {
        key: "decisions_clockwork_template_sha256",
        paths: &["package/annals-decisions-inbox.clockwork.toml.in"],
    },
    LegacyProof {
        key: "decisions_provisioner_sha256",
        paths: &["package/provision-decisions-user.sh"],
    },
    LegacyProof {
        key: "legacy_agent_plist_sha256",
        paths: &["package/org.annals.inbox.agent.plist"],
    },
    LegacyProof {
        key: "updater_sha256",
        paths: &["package/deploy-user.sh"],
    },
];
const LEGACY_PROVIDERS: &[LegacyProvider] = &[
    LegacyProvider {
        key: "chancery_annals_sha256",
        provider: "annals",
        path: "share/chancery/annals",
        version_key: "",
    },
    LegacyProvider {
        key: "chancery_usage_sha256",
        provider: "annals-usage",
        path: "share/chancery/annals-usage",
        version_key: "",
    },
];

pub(super) fn legacy(root: &Path) -> Result<ReleaseInfo> {
    let manifest = cell_install::legacy::manifest(&root.join("manifest.json"))?;
    let format = manifest
        .get("format")
        .ok_or_else(|| Error::new("legacy Annals format missing"))?;
    if format == "4" {
        cell_install::legacy::verify(
            root,
            &LegacySpec {
                format: "4",
                manifest: "manifest.json",
                metadata: &["source_revision", "source_dirty"],
                proofs: LEGACY_PROOFS,
                providers: LEGACY_PROVIDERS,
                hash_path_lines: true,
            },
            public(false),
        )
    } else if format == "3" {
        const PROOFS: &[LegacyProof] = &[
            LegacyProof {
                key: "binary_sha256",
                paths: &["libexec/annals"],
            },
            LegacyProof {
                key: "usage_binary_sha256",
                paths: &["libexec/annals-usage"],
            },
            LegacyProof {
                key: "frontend_sha256",
                paths: &["bin/annals", "package/annals-user"],
            },
            LegacyProof {
                key: "runner_sha256",
                paths: &["bin/annals-inbox", "package/annals-inbox"],
            },
            LegacyProof {
                key: "clockwork_template_sha256",
                paths: &["package/annals-inbox.clockwork.toml.in"],
            },
            LegacyProof {
                key: "legacy_agent_plist_sha256",
                paths: &["package/org.annals.inbox.agent.plist"],
            },
            LegacyProof {
                key: "updater_sha256",
                paths: &["package/deploy-user.sh"],
            },
        ];
        cell_install::legacy::verify(
            root,
            &LegacySpec {
                format: "3",
                manifest: "manifest.json",
                metadata: &["source_revision", "source_dirty"],
                proofs: PROOFS,
                providers: LEGACY_PROVIDERS,
                hash_path_lines: true,
            },
            public(false),
        )
    } else {
        Err(Error::new("unsupported legacy Annals release"))
    }
}

fn version(binary: &Path, name: &str, home: &Path) -> Result<String> {
    let output = cell_install::command::checked(
        binary,
        &["--version".into()],
        &environment(home, None),
        Duration::from_secs(30),
    )?;
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text
        .trim()
        .strip_prefix(&format!("{name} "))
        .ok_or_else(|| Error::new("invalid Annals program version"))?;
    if name == "annals" && version != VERSION {
        return Err(Error::new("Annals installer and candidate versions differ"));
    }
    cell_install::command::checked(
        binary,
        &["--help".into()],
        &environment(home, None),
        Duration::from_secs(30),
    )?;
    Ok(version.to_owned())
}

pub(super) fn prepare(args: &InstallArgs, home: &Path) -> Result<PreparedRelease> {
    for path in [
        &args.binary,
        &args.usage_binary,
        &args.bundle,
        &args.usage_bundle,
        &args.nucleus,
        &args.nucleus_socket,
        &args.clockwork,
    ] {
        if !path.is_absolute() {
            return Err(Error::new("Annals deployment paths must be absolute"));
        }
    }
    let versions = BTreeMap::from([
        ("annals".into(), version(&args.binary, "annals", home)?),
        (
            "annals-usage".into(),
            version(&args.usage_binary, "annals-usage", home)?,
        ),
    ]);
    let installer = std::env::current_exe()?;
    let mut files = BTreeMap::from([
        (
            "libexec/annals".into(),
            SourceFile {
                source: args.binary.clone(),
                mode: 0o755,
            },
        ),
        (
            "libexec/annals-usage".into(),
            SourceFile {
                source: args.usage_binary.clone(),
                mode: 0o755,
            },
        ),
    ]);
    for path in [
        "bin/annals",
        "bin/annals-inbox",
        "bin/annals-install",
        "package/install",
    ] {
        files.insert(
            path.into(),
            SourceFile {
                source: installer.clone(),
                mode: 0o755,
            },
        );
    }
    let mut providers = BTreeMap::new();
    for (id, bundle) in [
        ("annals", &args.bundle),
        ("annals-usage", &args.usage_bundle),
    ] {
        let spec = cell_install::InstallSpec {
            product: id,
            application: "Annals",
            commands: &["annals"],
            provider: id,
        };
        for (path, entry) in cell_install::provider_inventory(bundle, &spec)? {
            files.insert(
                format!("share/chancery/{id}/{path}"),
                SourceFile {
                    source: bundle.join(path),
                    mode: entry.mode,
                },
            );
        }
        providers.insert(
            id.into(),
            ProviderSpec {
                path: format!("share/chancery/{id}"),
                version: versions
                    .get(id)
                    .cloned()
                    .ok_or_else(|| Error::new("missing Annals version"))?,
            },
        );
    }
    cell_install::prepare_release(
        &layout(),
        home,
        &ReleasePlan {
            files,
            versions,
            providers,
        },
    )
}

pub(super) fn root(home: &Path, info: &ReleaseInfo) -> PathBuf {
    install_root(home).join("releases").join(&info.release_id)
}

pub(super) fn exact_candidate(
    info: &ReleaseInfo,
    root: &Path,
    binary: &Path,
    usage: &Path,
) -> Result<()> {
    for (relative, source) in [
        ("libexec/annals", binary),
        ("libexec/annals-usage", usage),
        ("bin/annals-install", std::env::current_exe()?.as_path()),
    ] {
        if info.files.get(relative).map(|entry| &entry.sha256)
            != Some(&cell_install::file_digest(source)?)
            || cell_install::file_digest(&root.join(relative))?
                != cell_install::file_digest(source)?
        {
            return Err(Error::new(
                "installed Annals program differs from admitted candidate",
            ));
        }
    }
    Ok(())
}
