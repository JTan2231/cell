use std::io::{Read as _, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult, Context as _};
use crate::model::DigestSnapshot;

const HELP_LIMIT: u64 = 64 * 1024;
const HELP_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn doctor(email_binary: &Path) -> AppResult<()> {
    let metadata = std::fs::metadata(email_binary).map_err(|_error| {
        AppError::new(
            "email_binary_missing",
            "configured Email CLI target is unavailable",
        )
    })?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(AppError::new(
            "email_binary_invalid",
            "configured Email CLI target is not an executable regular file",
        ));
    }
    let mut child = Command::new(email_binary)
        .arg("--help")
        .env_clear()
        .env("HOME", std::env::var_os("HOME").unwrap_or_default())
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_error| {
            AppError::new(
                "email_probe_failed",
                "unable to start the Email capability probe",
            )
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::new("email_probe_failed", "Email probe stdout was unavailable"))?;
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        stdout
            .take(HELP_LIMIT + 1)
            .read_to_end(&mut output)
            .map(|_| output)
    });
    let deadline = Instant::now() + HELP_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_error| {
            AppError::new("email_probe_failed", "unable to observe the Email probe")
        })? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::new(
                "email_probe_timeout",
                "Email capability probe exceeded five seconds",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = reader
        .join()
        .map_err(|_| AppError::new("email_probe_failed", "Email probe reader failed"))?
        .map_err(|_error| {
            AppError::new("email_probe_failed", "unable to read Email probe output")
        })?;
    if !status.success() || output.len() > usize::try_from(HELP_LIMIT).unwrap_or(usize::MAX) {
        return Err(AppError::new(
            "email_probe_failed",
            "Email capability probe did not return a bounded successful response",
        ));
    }
    let help = std::str::from_utf8(&output).map_err(|_error| {
        AppError::new("email_probe_failed", "Email probe output was not UTF-8")
    })?;
    if !help.contains("--idempotency-key") {
        return Err(AppError::new(
            "email_capability_missing",
            "Email CLI does not expose caller-supplied idempotency; install Email contract v2",
        ));
    }
    Ok(())
}

pub(crate) fn send(
    email_binary: &str,
    idempotency_key: &str,
    snapshot: &DigestSnapshot,
) -> AppResult<String> {
    let mut child = Command::new(email_binary)
        .env("CHANCERY_USAGE_INTERNAL", "1")
        .args(["--idempotency-key", idempotency_key, &snapshot.subject, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("email_spawn_failed", "unable to start installed email CLI")?;
    child
        .stdin
        .take()
        .ok_or_else(|| AppError::new("email_spawn_failed", "email stdin was unavailable"))?
        .write_all(snapshot.body.as_bytes())
        .context("email_write_failed", "unable to write frozen email body")?;
    let output = child
        .wait_with_output()
        .context("email_wait_failed", "unable to wait for email CLI")?;
    if !output.status.success() {
        return Err(AppError::new(
            "email_send_failed",
            format!(
                "email exited with {}; inspect Email diagnostics",
                output.status
            ),
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .context("email_response_invalid", "email output was not UTF-8")?;
    let email_id = stdout
        .trim()
        .strip_prefix("Accepted ")
        .or_else(|| stdout.trim().strip_prefix("Sent "))
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        })
        .ok_or_else(|| {
            AppError::new(
                "email_response_invalid",
                "email did not report its accepted message ID",
            )
        })?;
    Ok(email_id.to_owned())
}
