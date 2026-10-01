use crate::artifact::{inventory, inventory_matches, write_manifest};
use crate::{FORMAT, InstallSpec, Manifest, Result};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn new_releases_use_opaque_ids_and_record_paths_and_modes() -> Result<()> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("bin"))?;
    fs::write(root.path().join("bin/usher"), "a non-native fixture")?;
    fs::set_permissions(
        root.path().join("bin/usher"),
        fs::Permissions::from_mode(0o555),
    )?;
    let spec = InstallSpec {
        product: "usher",
        application: "Usher",
        commands: &["usher"],
        provider: "usher",
    };
    let manifest = write_manifest(root.path(), &spec, BTreeMap::new())?;
    assert_eq!(manifest.format, FORMAT);
    assert!(uuid::Uuid::parse_str(&manifest.release_id).is_ok());
    let serialized = serde_json::to_value(&manifest)?;
    assert!(serialized["files"]["bin/usher"].get("sha256").is_none());
    fs::set_permissions(
        root.path().join("bin/usher"),
        fs::Permissions::from_mode(0o755),
    )?;
    fs::write(root.path().join("bin/usher"), "different bytes")?;
    fs::set_permissions(
        root.path().join("bin/usher"),
        fs::Permissions::from_mode(0o555),
    )?;
    let (mut files, _) = inventory(root.path())?;
    files.remove("manifest.json");
    assert!(inventory_matches(&files, &manifest.files));
    fs::set_permissions(
        root.path().join("bin/usher"),
        fs::Permissions::from_mode(0o444),
    )?;
    let (mut files, _) = inventory(root.path())?;
    files.remove("manifest.json");
    assert!(!inventory_matches(&files, &manifest.files));
    fs::remove_file(root.path().join("manifest.json"))?;
    let other = write_manifest(root.path(), &spec, BTreeMap::new())?;
    assert_ne!(manifest.release_id, other.release_id);
    Ok(())
}

#[test]
fn predecessor_digests_are_read_as_metadata() -> Result<()> {
    let manifest: Manifest = serde_json::from_value(serde_json::json!({
        "format": "cell-install-v1", "product": "usher", "provider": "usher",
        "versions": {"usher": "0.3.0"},
        "files": {"bin/usher": {"sha256": "a".repeat(64), "mode": 493}},
        "release_id": "b".repeat(64),
    }))?;
    assert!(manifest.files["bin/usher"].code_identifier.is_none());
    assert_eq!(manifest.files["bin/usher"].sha256, "a".repeat(64));
    Ok(())
}
