use crate::artifact::identity;
use crate::{Error, FORMAT, FileEntry, Manifest, Result};
use std::collections::BTreeMap;

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
