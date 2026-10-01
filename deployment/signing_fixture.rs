//! Copy test executables before applying the fixed current-user signing policy.

use std::io;
use std::path::Path;

pub(super) fn copy_signed(
    product: &str,
    artifact: &str,
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> io::Result<()> {
    let source = source.as_ref();
    let destination = destination.as_ref();
    if destination.exists() && source.canonicalize()? == destination.canonicalize()? {
        return Err(io::Error::other("fixture copy must differ from its source"));
    }
    if destination.exists() || destination.is_symlink() {
        std::fs::remove_file(destination)?;
    }
    std::fs::copy(source, destination)?;
    #[cfg(target_os = "macos")]
    {
        let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        while !root.join("deployment/signing_fixture.py").is_file() {
            if !root.pop() {
                return Err(io::Error::other("Cell fixture signing helper is missing"));
            }
        }
        let output = std::process::Command::new("python3")
            .arg(root.join("deployment/signing_fixture.py"))
            .args([product, artifact])
            .arg(destination)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "fixture signing failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (product, artifact);
    Ok(())
}
