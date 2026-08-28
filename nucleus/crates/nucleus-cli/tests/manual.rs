use std::process::Command;

const OPERATOR_MANUAL: &str = include_str!("../../../docs/operator-manual.md");

#[test]
fn manual_prints_canonical_markdown_without_a_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let missing_socket = temporary.path().join("missing.sock");

    let output = Command::new(env!("CARGO_BIN_EXE_nucleus"))
        .arg("--socket")
        .arg(missing_socket)
        .arg("manual")
        .output()?;

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, OPERATOR_MANUAL.as_bytes());
    Ok(())
}
