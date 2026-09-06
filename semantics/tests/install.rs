#[test]
#[cfg(target_os = "macos")]
fn rust_installer_preserves_packaging_and_recovery_boundaries() -> std::io::Result<()> {
    let result = std::process::Command::new("/bin/sh")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packaging/macos/test-deploy-user.sh"
        ))
        .env(
            "SEMANTICS_INSTALL_TEST_BINARY",
            env!("CARGO_BIN_EXE_semantics-install"),
        )
        .output()?;
    assert!(
        result.status.success(),
        "packaging fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
