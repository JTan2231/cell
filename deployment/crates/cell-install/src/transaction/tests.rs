use super::*;

struct Fixture {
    _temporary: tempfile::TempDir,
    home: PathBuf,
    inputs: PathBuf,
    layout: InstallLayout,
}

fn no_legacy(_: &Path) -> Result<ReleaseInfo> {
    Err(Error::new("legacy release not supported in this fixture"))
}

impl Fixture {
    fn new() -> Result<Self> {
        let temporary = tempfile::tempdir()?;
        let home = fs::canonicalize(temporary.path())?.join("home");
        let inputs = fs::canonicalize(temporary.path())?.join("inputs");
        fs::create_dir(&home)?;
        fs::create_dir(&inputs)?;
        let layout = InstallLayout {
            product: "fixture".to_owned(),
            application: "Fixture".to_owned(),
            product_lock: LockSpec {
                path: "Library/Application Support/Fixture/install/.update-lock".into(),
                kind: LockKind::Directory,
            },
            catalog_lock: Some(LockSpec {
                path: "Library/Application Support/Chancery/.catalog-lock".into(),
                kind: LockKind::Shlock,
            }),
            public: vec![
                PublicEntry {
                    path: ".local/bin/fixture".into(),
                    artifact: "bin/fixture".to_owned(),
                    kind: PublicKind::Symlink,
                    mode: 0o755,
                },
                PublicEntry {
                    path: ".local/bin/copied".into(),
                    artifact: "bin/copied".to_owned(),
                    kind: PublicKind::Copy,
                    mode: 0o755,
                },
                PublicEntry {
                    path: "Library/Application Support/Chancery/providers/fixture".into(),
                    artifact: "share/chancery/fixture".to_owned(),
                    kind: PublicKind::Symlink,
                    mode: 0o755,
                },
            ],
        };
        Ok(Self {
            _temporary: temporary,
            home,
            inputs,
            layout,
        })
    }

    fn prepare(&self, revision: &str) -> Result<PreparedRelease> {
        let content = [
            ("bin/fixture", revision.to_owned(), 0o755),
            ("bin/copied", format!("copied {revision}"), 0o755),
            (
                "share/chancery/fixture/provider.json",
                "{\"provider\":{\"id\":\"fixture\",\"release\":\"1.2.3\"}}".to_owned(),
                0o644,
            ),
            (
                "share/chancery/companion/provider.json",
                "{\"provider\":{\"id\":\"companion\",\"release\":\"4.5.6\"}}".to_owned(),
                0o644,
            ),
        ];
        let mut files = BTreeMap::new();
        for (index, (path, bytes, mode)) in content.into_iter().enumerate() {
            let source = self.inputs.join(format!("{revision}-{index}"));
            fs::write(&source, bytes)?;
            files.insert(path.to_owned(), SourceFile { source, mode });
        }
        prepare_release(
            &self.layout,
            &self.home,
            &ReleasePlan {
                files,
                versions: BTreeMap::from([
                    ("fixture".to_owned(), "1.2.3".to_owned()),
                    ("companion".to_owned(), "4.5.6".to_owned()),
                ]),
                providers: BTreeMap::from([
                    (
                        "fixture".to_owned(),
                        ProviderSpec {
                            path: "share/chancery/fixture".to_owned(),
                            version: "1.2.3".to_owned(),
                        },
                    ),
                    (
                        "companion".to_owned(),
                        ProviderSpec {
                            path: "share/chancery/companion".to_owned(),
                            version: "4.5.6".to_owned(),
                        },
                    ),
                ]),
            },
        )
    }
}

#[test]
fn independent_versions_exact_inventory_and_directory_safety() -> Result<()> {
    let fixture = Fixture::new()?;
    let prepared = fixture.prepare("first")?;
    assert_eq!(fixture.prepare("first")?.info, prepared.info);
    assert_eq!(
        prepared.info.versions.get("companion").map(String::as_str),
        Some("4.5.6")
    );
    fs::set_permissions(
        prepared.root.join("share/chancery"),
        fs::Permissions::from_mode(0o777),
    )?;
    assert!(verify_prepared(&prepared).is_err());
    fs::set_permissions(
        prepared.root.join("share/chancery"),
        fs::Permissions::from_mode(0o755),
    )?;
    fs::write(prepared.root.join("unlisted"), "extra")?;
    assert!(verify_prepared(&prepared).is_err());
    Ok(())
}

#[test]
fn suspension_compensates_to_suspended_view_then_restores_prior() -> Result<()> {
    let fixture = Fixture::new()?;
    let first = fixture.prepare("first")?;
    let second = fixture.prepare("second")?;
    let empty = inspect_installation(&fixture.layout, &fixture.home, &no_legacy)?;
    let mut transaction = lock_installation(&fixture.layout, &fixture.home, &no_legacy)?;
    let installed = transaction.publish(&first, &empty, |_| Ok(()))?;
    let suspended = transaction.suspend(
        &installed.after,
        &[".local/bin/fixture".into(), ".local/bin/copied".into()],
    )?;
    assert!(inspect_installation(&fixture.layout, &fixture.home, &no_legacy).is_err());
    assert_eq!(
        inspect_detached_installation(&fixture.layout, &fixture.home, &no_legacy)?,
        suspended.after
    );
    let failed = transaction
        .publish(&second, &suspended.after, |_| {
            Err(Error::new("installed smoke failed"))
        })
        .err()
        .ok_or_else(|| Error::new("publication unexpectedly succeeded"))?;
    assert_eq!(failed.disposition, Disposition::Restored);
    transaction.recheck(&suspended.after)?;
    assert!(fs::symlink_metadata(fixture.home.join(".local/bin/fixture")).is_err());
    transaction.restore(&suspended, |_| Ok(()))?;
    assert_eq!(
        fs::read_to_string(fixture.home.join(".local/bin/copied"))?,
        "copied first"
    );
    let published = transaction.publish(&second, &installed.after, |_| Ok(()))?;
    transaction.restore(&published, |_| Ok(()))?;
    transaction.recheck(&installed.after)?;
    let serialized = serde_json::to_vec(&published)?;
    assert_eq!(
        serde_json::from_slice::<SelectionReceipt>(&serialized)?,
        published
    );
    Ok(())
}

#[test]
fn recovery_repairs_partial_fresh_publication_and_refuses_foreign_path() -> Result<()> {
    let fixture = Fixture::new()?;
    let candidate = fixture.prepare("first")?;
    let prior = inspect_installation(&fixture.layout, &fixture.home, &no_legacy)?;
    let mut transaction = lock_installation(&fixture.layout, &fixture.home, &no_legacy)?;
    let public = fixture.home.join(".local/bin/fixture");
    fs::create_dir_all(
        public
            .parent()
            .ok_or_else(|| Error::new("missing parent"))?,
    )?;
    symlink(
        install_root(&fixture.layout, &fixture.home).join("current/bin/fixture"),
        &public,
    )?;
    assert!(inspect_installation(&fixture.layout, &fixture.home, &no_legacy).is_err());
    transaction.recover(&prior, &candidate, false, |_| Ok(()))?;
    transaction.recheck(&prior)?;
    symlink("/unowned", &public)?;
    assert!(
        transaction
            .recover(&prior, &candidate, true, |_| Ok(()))
            .is_err()
    );
    assert_eq!(fs::read_link(&public)?, PathBuf::from("/unowned"));
    fs::remove_file(&public)?;
    transaction.recover(&prior, &candidate, true, |_| Ok(()))?;
    let installed = inspect_installation(&fixture.layout, &fixture.home, &no_legacy)?;
    let detached = transaction.detach(&installed)?;
    assert!(candidate.root.is_dir());
    assert!(detached.after.current.is_none());
    transaction.restore(&detached, |_| Ok(()))?;
    transaction.recheck(&installed)?;
    Ok(())
}

#[test]
fn directory_lock_preserves_existing_protocol() -> Result<()> {
    let fixture = Fixture::new()?;
    let transaction = lock_installation(&fixture.layout, &fixture.home, &no_legacy)?;
    assert!(
        fixture
            .home
            .join(&fixture.layout.product_lock.path)
            .is_dir()
    );
    assert!(lock_installation(&fixture.layout, &fixture.home, &no_legacy).is_err());
    drop(transaction);
    assert!(
        !fixture
            .home
            .join(&fixture.layout.product_lock.path)
            .exists()
    );
    drop(lock_installation(
        &fixture.layout,
        &fixture.home,
        &no_legacy,
    )?);
    Ok(())
}

#[test]
fn explicit_legacy_proof_restores_predecessor_without_new_public_entries() -> Result<()> {
    let fixture = Fixture::new()?;
    let candidate = fixture.prepare("second")?;
    let release_id = hash_bytes(b"legacy payload");
    let root = install_root(&fixture.layout, &fixture.home);
    let legacy_root = root.join("releases").join(&release_id);
    fs::create_dir_all(legacy_root.join("bin"))?;
    fs::write(legacy_root.join("bin/fixture"), "legacy payload")?;
    fs::set_permissions(
        legacy_root.join("bin/fixture"),
        fs::Permissions::from_mode(0o755),
    )?;
    let public = vec![fixture.layout.public[0].clone()];
    let proof = |path: &Path| {
        let (files, _) = inventory(path)?;
        if files.len() != 1
            || files
                .get("bin/fixture")
                .is_none_or(|file| file.sha256 != release_id)
        {
            return Err(Error::new("invalid predecessor fixture"));
        }
        Ok(ReleaseInfo {
            release_id: release_id.clone(),
            format: "legacy-fixture".to_owned(),
            versions: BTreeMap::new(),
            files,
            public: public.clone(),
        })
    };
    symlink(format!("releases/{release_id}"), root.join("current"))?;
    fs::create_dir_all(fixture.home.join(".local/bin"))?;
    symlink(
        root.join("current/bin/fixture"),
        fixture.home.join(".local/bin/fixture"),
    )?;
    let prior = inspect_installation(&fixture.layout, &fixture.home, &proof)?;
    let mut transaction = lock_installation(&fixture.layout, &fixture.home, &proof)?;
    let published = transaction.publish(&candidate, &prior, |_| Ok(()))?;
    transaction.restore(&published, |_| Ok(()))?;
    transaction.recheck(&prior)?;
    assert!(fs::symlink_metadata(fixture.home.join(".local/bin/copied")).is_err());
    let retained = PreparedRelease {
        info: verify_release_at(&fixture.layout, &legacy_root, &proof)?,
        root: legacy_root,
    };
    transaction.publish(&retained, &prior, |_| Ok(()))?;
    Ok(())
}

#[test]
fn directory_lock_recovers_only_recognized_dead_owner() -> Result<()> {
    let fixture = Fixture::new()?;
    drop(lock_installation(
        &fixture.layout,
        &fixture.home,
        &no_legacy,
    )?);
    let path = fixture.home.join(&fixture.layout.product_lock.path);
    fs::create_dir(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    assert!(lock_installation(&fixture.layout, &fixture.home, &no_legacy).is_err());
    // The child has exited and was reaped, so kill -0 can prove this PID dead.
    let mut child = std::process::Command::new("/usr/bin/true").spawn()?;
    let pid = child.id();
    child.wait()?;
    fs::write(
        path.join(LOCK_OWNER),
        format!("cell-install-lock-v1\n{pid}\n"),
    )?;
    fs::set_permissions(path.join(LOCK_OWNER), fs::Permissions::from_mode(0o600))?;
    fs::write(path.join("foreign"), "retained")?;
    assert!(lock_installation(&fixture.layout, &fixture.home, &no_legacy).is_err());
    assert!(path.join("foreign").is_file());
    fs::remove_file(path.join("foreign"))?;
    drop(lock_installation(
        &fixture.layout,
        &fixture.home,
        &no_legacy,
    )?);
    assert!(!path.exists());
    Ok(())
}
