//! One bounded renderer execution followed by one Email submission.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt as _};

pub const RENDER_TIMEOUT_SECONDS: u64 = 1_200;
pub const MAX_BODY_BYTES: usize = 64_000;
pub const MAX_DIAGNOSTIC_BYTES: usize = 4_096;

pub fn executable(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "executable path must be absolute");
    let metadata = path.metadata().context("cannot inspect executable")?;
    ensure!(metadata.is_file(), "executable must be a regular file");
    ensure!(
        metadata.permissions().mode() & 0o111 != 0,
        "executable must have execute permission"
    );
    Ok(())
}

/// Executes trusted current-user code. Only HOME, fixed PATH and usage marking
/// enter the renderer environment. Its executable's parent is the working directory.
pub async fn render(argv: &[String]) -> Result<String> {
    crate::manifest::validate_render(argv)?;
    let program = Path::new(&argv[0]);
    executable(program)?;
    let home = crate::home()?;
    let mut command = tokio::process::Command::new(program);
    command
        .args(&argv[1..])
        .current_dir(
            program
                .parent()
                .context("renderer working directory missing")?,
        )
        .env_clear()
        .env("HOME", &home)
        .env(
            "PATH",
            format!(
                "/usr/bin:/bin:/usr/sbin:/sbin:{}",
                home.join(".local/bin").display()
            ),
        )
        .env("CHANCERY_USAGE_INTERNAL", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command.as_std_mut().process_group(0);
    let mut child = command.spawn().context("cannot start renderer")?;
    let group = RendererGroup(child.id().context("renderer PID missing")?);
    let stdout = child.stdout.take().context("renderer stdout missing")?;
    let stderr = child.stderr.take().context("renderer stderr missing")?;
    let operation = async {
        let (body, diagnostic, status) = tokio::try_join!(
            capture_body(stdout),
            capture_diagnostic(stderr),
            child.wait(),
        )?;
        if !diagnostic.bytes.is_empty() {
            eprintln!("{}", String::from_utf8_lossy(&diagnostic.bytes));
        }
        if diagnostic.truncated {
            eprintln!("renderer stderr truncated after {MAX_DIAGNOSTIC_BYTES} bytes");
        }
        ensure!(
            status.success(),
            "renderer failed ({status}); no email submitted"
        );
        decode_body(body)
    };
    let result = tokio::time::timeout(Duration::from_secs(RENDER_TIMEOUT_SECONDS), operation)
        .await
        .context("renderer timed out; no email submitted")?;
    // End remaining members of the renderer group on every completion path.
    drop(group);
    if result.is_err() {
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
    }
    result
}

pub async fn run(id: &str, subject: &str, argv: &[String], email_binary: &Path) -> Result<Value> {
    crate::manifest::validate_id(id)?;
    crate::manifest::validate_subject(subject)?;
    executable(email_binary).context("installed Email wrapper is unavailable")?;
    let body = render(argv)
        .await
        .with_context(|| format!("production run {id}"))?;
    if body.is_empty() {
        return Ok(json!({"job_id":id,"outcome":"skipped_empty","body_bytes":0}));
    }
    let message = email::api::Message {
        subject: subject.to_owned(),
        body,
        // Email generates a fresh key for this invocation and owns its bounded
        // transport retries. Paperboy retains no key or message to replay.
        idempotency_key: None,
    };
    let receipt = email::api::Client::new(email_binary)
        .send_with_options(&message, &[], &email::api::ReplyOptions::default())
        .await
        .context(
            "Email acceptance is not confirmed; inspect the provider before a manual resend",
        )?;
    Ok(
        json!({"job_id":id,"outcome":"accepted","provider_message_id":receipt.id,"body_bytes":message.body.len()}),
    )
}

async fn capture_body(mut stream: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 8_192];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(bytes);
        }
        if count > MAX_BODY_BYTES.saturating_sub(bytes.len()) {
            return Err(std::io::Error::other(
                "renderer stdout exceeds 64000 bytes; no email submitted",
            ));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
}

struct Diagnostic {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn capture_diagnostic(mut stream: impl AsyncRead + Unpin) -> std::io::Result<Diagnostic> {
    let mut diagnostic = Diagnostic {
        bytes: Vec::new(),
        truncated: false,
    };
    let mut buffer = vec![0_u8; 8_192];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(diagnostic);
        }
        let retained = count.min(MAX_DIAGNOSTIC_BYTES.saturating_sub(diagnostic.bytes.len()));
        diagnostic.bytes.extend_from_slice(&buffer[..retained]);
        diagnostic.truncated |= retained < count;
    }
}

fn decode_body(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!("renderer stdout must be UTF-8; no email submitted"))
}

/// Also handles a cancelled future. No renderer PID or body is stored on disk.
struct RendererGroup(u32);

impl Drop for RendererGroup {
    fn drop(&mut self) {
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{}", self.0)])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn retains_exact_body_and_rejects_overflow_without_truncation() {
        let input = b"\r\n  Report\n\n";
        assert_eq!(
            decode_body(capture_body(&input[..]).await.unwrap())
                .unwrap()
                .as_bytes(),
            input
        );
        assert!(capture_body(&vec![b'x'; MAX_BODY_BYTES][..]).await.is_ok());
        assert!(
            capture_body(&vec![b'x'; MAX_BODY_BYTES + 1][..])
                .await
                .is_err()
        );
        assert!(decode_body(vec![0xff]).is_err());
        assert!(decode_body(Vec::new()).unwrap().is_empty());
        assert_eq!(decode_body(b" \n".to_vec()).unwrap(), " \n");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn drains_stderr_after_retaining_only_the_bounded_prefix() {
        let bytes = vec![b'e'; MAX_DIAGNOSTIC_BYTES + 8_193];
        let mut stream = &bytes[..];
        let result = capture_diagnostic(&mut stream).await.unwrap();
        assert_eq!(result.bytes.len(), MAX_DIAGNOSTIC_BYTES);
        assert!(result.truncated);
        assert!(stream.is_empty());
    }
}
