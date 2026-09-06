use super::*;
use crate::artifact::{hash_bytes, identity};
use crate::{FileEntry, Manifest};
use std::fmt::Write as _;

const SPEC: InstallSpec = InstallSpec {
    product: "usher",
    application: "Usher",
    commands: &["usher", "usher-install"],
    provider: "usher",
};

struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    input: ReleaseInput,
}

impl Fixture {
    fn new(version: &str) -> Result<Self> {
        let root = tempfile::tempdir()?;
        let directory = root.path().canonicalize()?;
        let home = directory.join("home");
        fs::create_dir(&home)?;
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700))?;
        let sources = directory.join("source");
        fs::create_dir(&sources)?;
        let mut binaries = BTreeMap::new();
        for name in SPEC.commands {
            let path = sources.join(name);
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\ncase \"$1\" in\n--version) printf '%s\\n' '{name} {version}';;\n--help) case \"$0\" in */releases/*) [ ! -f \"$HOME/fail-smoke\" ] || exit 1;; esac;;\n*) exit 1;;\nesac\n"
                ),
            )?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
            binaries.insert((*name).to_owned(), path);
        }
        let provider_dir = sources.join("provider");
        fs::create_dir_all(provider_dir.join("entries"))?;
        fs::create_dir(provider_dir.join("manuals"))?;
        fs::write(
            provider_dir.join("provider.json"),
            format!("{{\"provider\":{{\"id\":\"usher\",\"release\":\"{version}\"}}}}"),
        )?;
        fs::write(provider_dir.join("entries/install.json"), "{}\n")?;
        fs::write(provider_dir.join("manuals/install.md"), "Install Usher.\n")?;
        let installer = binaries["usher-install"].clone();
        Ok(Self {
            _root: root,
            home,
            input: ReleaseInput {
                binaries,
                provider_dir,
                installer,
            },
        })
    }

    fn installed(&self) -> Result<Installation> {
        install(&SPEC, &self.home, &self.input, None)
    }
    fn root(&self, release: &Installation) -> PathBuf {
        self.home
            .join("Library/Application Support/Usher/install")
            .join(&release.current)
    }
}

#[test]
fn install_redeploy_and_verify_exact_source() -> Result<()> {
    let f = Fixture::new("0.3.0")?;
    assert_eq!(inspect(&SPEC, &f.home)?, None);
    let installed = f.installed()?;
    assert_eq!(installed.format, FORMAT);
    assert_eq!(verify_candidate(&SPEC, &f.home, &f.input)?, installed);
    assert_eq!(f.installed()?, installed);
    let provider = f
        .home
        .join("Library/Application Support/Chancery/providers/usher");
    assert_eq!(
        fs::read_link(provider)?,
        f.home
            .join("Library/Application Support/Usher/install/current/share/chancery/usher")
    );
    fs::write(
        f.input.provider_dir.join("manuals/install.md"),
        "Changed source.\n",
    )?;
    assert!(verify_candidate(&SPEC, &f.home, &f.input).is_err());
    assert_eq!(inspect(&SPEC, &f.home)?, Some(installed));
    Ok(())
}

#[test]
fn stale_selection_and_foreign_public_paths_are_refused() -> Result<()> {
    let f = Fixture::new("0.3.0")?;
    let installed = f.installed()?;
    assert!(
        install(&SPEC, &f.home, &f.input, Some("absent"))
            .err()
            .ok_or_else(|| Error::new("expected failure"))?
            .message
            .contains("stale")
    );
    assert_eq!(inspect(&SPEC, &f.home)?, Some(installed));
    let cli = f.home.join(".local/bin/usher");
    fs::remove_file(&cli)?;
    symlink("/bin/sh", &cli)?;
    assert!(install(&SPEC, &f.home, &f.input, None).is_err());
    assert_eq!(fs::read_link(cli)?, Path::new("/bin/sh"));
    Ok(())
}

#[test]
fn altered_inventory_and_hardlinked_inputs_are_refused() -> Result<()> {
    let f = Fixture::new("0.3.0")?;
    let installed = f.installed()?;
    let extra = f.root(&installed).join("unmanifested");
    fs::write(&extra, "not part of release")?;
    assert!(inspect(&SPEC, &f.home).is_err());
    fs::remove_file(extra)?;
    let bin_dir = f.root(&installed).join("bin");
    fs::set_permissions(&bin_dir, fs::Permissions::from_mode(0o777))?;
    assert!(inspect(&SPEC, &f.home).is_err());
    fs::set_permissions(&bin_dir, fs::Permissions::from_mode(0o755))?;
    let alias = f.input.provider_dir.join("manuals/alias.md");
    fs::hard_link(f.input.provider_dir.join("manuals/install.md"), alias)?;
    assert!(install(&SPEC, &f.home, &f.input, None).is_err());
    assert_eq!(inspect(&SPEC, &f.home)?, Some(installed));
    Ok(())
}

#[test]
fn candidate_verification_requires_installed_smoke() -> Result<()> {
    let f = Fixture::new("0.3.0")?;
    let installed = f.installed()?;
    fs::write(f.home.join("fail-smoke"), "")?;
    assert_eq!(inspect(&SPEC, &f.home)?, Some(installed));
    assert!(verify_candidate(&SPEC, &f.home, &f.input).is_err());
    Ok(())
}

#[test]
fn failed_first_install_restores_absence_and_failed_update_restores_prior() -> Result<()> {
    let first = Fixture::new("0.3.0")?;
    fs::write(first.home.join("fail-smoke"), "")?;
    let error = install(&SPEC, &first.home, &first.input, None)
        .err()
        .ok_or_else(|| Error::new("expected failure"))?;
    assert_eq!(error.disposition, Disposition::Restored);
    assert_eq!(inspect(&SPEC, &first.home)?, None);

    fs::remove_file(first.home.join("fail-smoke"))?;
    let before = first.installed()?;
    let next = Fixture::new("0.4.0")?;
    fs::write(first.home.join("fail-smoke"), "")?;
    let error = install(&SPEC, &first.home, &next.input, None)
        .err()
        .ok_or_else(|| Error::new("expected failure"))?;
    assert_eq!(error.disposition, Disposition::Restored);
    assert_eq!(inspect(&SPEC, &first.home)?, Some(before));
    Ok(())
}

#[test]
fn legacy_restore_detaches_extra_command_and_new_install_can_follow() -> Result<()> {
    let f = Fixture::new("0.3.0")?;
    let new = f.installed()?;
    let paths = Paths::new(&SPEC, &f.home)?;
    let stage = tempfile::tempdir_in(&paths.releases)?;
    fs::create_dir(stage.path().join("bin"))?;
    fs::create_dir(stage.path().join("package"))?;
    copy_file(
        &f.input.binaries["usher"],
        &stage.path().join("bin/usher"),
        0o755,
    )?;
    copy_file(
        &f.input.installer,
        &stage.path().join("package/deploy-user.sh"),
        0o755,
    )?;
    let bundle = stage.path().join("share/chancery/usher");
    fs::create_dir_all(bundle.join("entries"))?;
    fs::create_dir(bundle.join("manuals"))?;
    let provider = provider_files(&f.input.provider_dir, &SPEC)?;
    let mut recipe = String::new();
    for (relative, file) in provider {
        copy_file(
            &f.input.provider_dir.join(&relative),
            &bundle.join(&relative),
            0o444,
        )?;
        let _ = write!(recipe, "path=./{relative}\n{}  ./{relative}\n", file.sha256);
    }
    let bh = digest(&stage.path().join("bin/usher"))?;
    let dh = digest(&stage.path().join("package/deploy-user.sh"))?;
    let ch = hash_bytes(recipe.as_bytes());
    let id = hash_bytes(format!("{bh}\n{dh}\n{ch}\n").as_bytes());
    fs::write(
        stage.path().join("manifest.txt"),
        format!(
            "format=1\nproduct=usher\nrelease_id={id}\nversion=0.3.0\nbinary_sha256={bh}\ndeployer_sha256={dh}\nchancery_sha256={ch}\n"
        ),
    )?;
    let release = paths.releases.join(&id);
    fs::rename(stage.path(), &release)?;
    let legacy = verify_release(&SPEC, &release)?;
    assert_eq!(legacy.format, "legacy-usher-v1");
    assert_eq!(
        restore(&SPEC, &f.home, &legacy.current, Some(&new.current))?,
        legacy
    );
    assert!(
        f.home
            .join(".local/bin/usher-install")
            .symlink_metadata()
            .is_err()
    );
    let installer_link = f.home.join(".local/bin/usher-install");
    symlink(
        paths.install.join("current/bin/usher-install"),
        &installer_link,
    )?;
    assert!(inspect(&SPEC, &f.home).is_err());
    assert_eq!(
        recover_installation(&SPEC, &f.home, &f.input, Some(&legacy))?,
        Some(legacy.clone())
    );
    assert!(installer_link.symlink_metadata().is_err());
    assert_eq!(f.installed()?, new);
    Ok(())
}

#[test]
fn partial_first_publication_has_bounded_absence_and_explicit_release_recovery() -> Result<()> {
    let f = Fixture::new("0.3.0")?;
    let paths = Paths::new(&SPEC, &f.home)?;
    paths.directories(true)?;
    let cli = f.home.join(".local/bin/usher");
    symlink(paths.install.join("current/bin/usher"), &cli)?;
    assert!(inspect(&SPEC, &f.home).is_err());
    assert_eq!(recover_installation(&SPEC, &f.home, &f.input, None)?, None);
    assert!(cli.symlink_metadata().is_err());

    let installed = f.installed()?;
    fs::remove_file(paths.install.join("current"))?;
    assert!(inspect(&SPEC, &f.home).is_err());
    assert_eq!(
        restore(&SPEC, &f.home, &installed.current, Some("absent"))?,
        installed
    );
    fs::remove_file(&cli)?;
    symlink("/bin/sh", &cli)?;
    assert!(restore(&SPEC, &f.home, &installed.current, None).is_err());
    assert_eq!(fs::read_link(cli)?, Path::new("/bin/sh"));
    Ok(())
}

#[test]
fn format_identity_is_fixed_and_includes_mode_and_installer_bytes() -> Result<()> {
    let mut manifest = Manifest {
        format: FORMAT.to_owned(),
        product: "usher".to_owned(),
        provider: "usher".to_owned(),
        versions: BTreeMap::from([("usher".to_owned(), "0.3.0".to_owned())]),
        files: BTreeMap::from([
            (
                "bin/usher".to_owned(),
                FileEntry {
                    sha256: "a".repeat(64),
                    mode: 0o755,
                },
            ),
            (
                "package/install".to_owned(),
                FileEntry {
                    sha256: "b".repeat(64),
                    mode: 0o755,
                },
            ),
        ]),
        release_id: String::new(),
    };
    let expected = identity(&manifest)?;
    assert_eq!(
        expected,
        "e3a8c050047a100fcf24207aee08788fa37be27a1f99cb938a11739217063e20"
    );
    assert_eq!(
        serde_json::to_string(&manifest)?,
        format!(
            "{{\"format\":\"cell-install-v1\",\"product\":\"usher\",\"provider\":\"usher\",\"versions\":{{\"usher\":\"0.3.0\"}},\"files\":{{\"bin/usher\":{{\"sha256\":\"{}\",\"mode\":493}},\"package/install\":{{\"sha256\":\"{}\",\"mode\":493}}}},\"release_id\":\"\"}}",
            "a".repeat(64),
            "b".repeat(64)
        )
    );
    manifest
        .files
        .get_mut("package/install")
        .ok_or_else(|| Error::new("missing installer entry"))?
        .mode = 0o444;
    assert_ne!(identity(&manifest)?, expected);
    manifest
        .files
        .get_mut("package/install")
        .ok_or_else(|| Error::new("missing installer entry"))?
        .mode = 0o755;
    manifest
        .files
        .get_mut("package/install")
        .ok_or_else(|| Error::new("missing installer entry"))?
        .sha256 = "c".repeat(64);
    assert_ne!(identity(&manifest)?, expected);
    Ok(())
}

#[test]
fn abandoned_private_scratch_is_removed_and_live_scratch_is_retained() -> Result<()> {
    let f = Fixture::new("0.3.0")?;
    let paths = Paths::new(&SPEC, &f.home)?;
    paths.directories(true)?;
    let mut child = Command::new("/usr/bin/true").spawn()?;
    let dead_pid = child.id();
    child.wait()?;
    let dead = paths
        .releases
        .join(format!(".cell-install-stage-usher-{dead_pid}-deadxx"));
    let live = paths.releases.join(format!(
        ".cell-install-stage-usher-{}-livexx",
        std::process::id()
    ));
    let selector = paths
        .install
        .join(format!(".cell-install-selector-usher-{dead_pid}-deadxx"));
    for path in [&dead, &live, &selector] {
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    symlink(
        "releases/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        selector.join("link"),
    )?;
    let _product = Lock::acquire(paths.install.join(".update-lock"), paths.uid)?;
    clean_scratch(&paths, false)?;
    let _catalog = Lock::acquire(paths.catalog.join(".catalog-update-lock"), paths.uid)?;
    clean_scratch(&paths, true)?;
    assert!(!dead.exists());
    assert!(live.is_dir());
    assert!(!selector.exists());
    Ok(())
}
