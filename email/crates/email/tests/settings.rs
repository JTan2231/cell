use std::os::unix::fs::PermissionsExt as _;
use std::process::Command;

#[test]
fn setup_and_receiving_selection_remain_private_and_repeatable()
-> Result<(), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    let key = home.path().join("supplied-key");
    std::fs::write(&key, "synthetic-private-key")?;
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600))?;
    let invoke = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_email"))
            .args(arguments)
            .env("HOME", home.path())
            .env_remove("RESEND_API_KEY")
            .output()
    };
    let key_path = key.to_str().ok_or("key path encoding")?;
    let output = invoke(&[
        "setup",
        "--credential-file",
        key_path,
        "--receiving-domain",
        "fixture.resend.app",
    ])?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-private-key"));
    let selected = home
        .path()
        .join("Library/Application Support/Email/settings/resend-api-key");
    assert_eq!(
        std::fs::metadata(&selected)?.permissions().mode() & 0o777,
        0o600
    );
    assert!(invoke(&["setup"])?.status.success());
    let output = invoke(&["receive", "settings"])?;
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(value, serde_json::json!({"domains":["fixture.resend.app"]}));
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644))?;
    assert!(
        !invoke(&["setup", "--credential-file", key_path])?
            .status
            .success()
    );
    assert_eq!(std::fs::read_to_string(selected)?, "synthetic-private-key");
    Ok(())
}
