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
        .strip_prefix("Sent ")
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    use crate::model::DigestSnapshot;

    use super::{doctor, send};

    #[test]
    fn doctor_requires_caller_idempotency_capability() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let legacy = directory.path().join("legacy-email");
        fs::write(
            &legacy,
            "#!/bin/sh\nprintf '%s\\n' 'Usage: email SUBJECT BODY'\n",
        )?;
        fs::set_permissions(&legacy, fs::Permissions::from_mode(0o755))?;
        let error = doctor(&legacy).err().ok_or("expected missing capability")?;
        assert_eq!(error.code, "email_capability_missing");

        let current = directory.path().join("current-email");
        fs::write(
            &current,
            "#!/bin/sh\nprintf '%s\\n' 'Usage: email --idempotency-key KEY SUBJECT BODY'\n",
        )?;
        fs::set_permissions(&current, fs::Permissions::from_mode(0o755))?;
        doctor(&current)?;
        Ok(())
    }

    #[test]
    fn sends_frozen_body_on_stdin() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let script = directory.path().join("email");
        let capture = directory.path().join("capture");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >'{}'\ncat >>'{}'\nprintf '%s\\n' 'Sent email_123'\n",
                capture.display(),
                capture.display()
            ),
        )?;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
        let snapshot = DigestSnapshot {
            run_id: "run_1".to_owned(),
            report_date: "2026-08-31".to_owned(),
            content_revision: 0,
            subject: "Subject".to_owned(),
            body: "Frozen body".to_owned(),
            digest_hash: "hash".to_owned(),
        };
        assert_eq!(
            send(script.to_str().ok_or("path")?, "key/1", &snapshot)?,
            "email_123"
        );
        let capture = fs::read_to_string(capture)?;
        assert!(capture.contains("--idempotency-key key/1 Subject -"));
        assert!(capture.contains("Frozen body"));
        Ok(())
    }

    #[test]
    fn does_not_forward_email_stderr() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let script = directory.path().join("email");
        fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' 'SECRET_BODY /Users/person/private' >&2\nexit 1\n",
        )?;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
        let snapshot = DigestSnapshot {
            run_id: "run_1".to_owned(),
            report_date: "2026-08-31".to_owned(),
            content_revision: 0,
            subject: "Subject".to_owned(),
            body: "Frozen body".to_owned(),
            digest_hash: "hash".to_owned(),
        };
        let error = send(script.to_str().ok_or("path")?, "key/1", &snapshot)
            .err()
            .ok_or("expected email failure")?;
        assert_eq!(error.code, "email_send_failed");
        assert!(!error.message.contains("SECRET_BODY"));
        assert!(!error.message.contains("/Users/person"));
        Ok(())
    }

    #[test]
    fn refuses_unbounded_email_stdout() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let script = directory.path().join("email");
        fs::write(
            &script,
            "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' 'Sent email_123' 'SECRET_BODY /Users/person/private'\n",
        )?;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
        let snapshot = DigestSnapshot {
            run_id: "run_1".to_owned(),
            report_date: "2026-08-31".to_owned(),
            content_revision: 0,
            subject: "Subject".to_owned(),
            body: "Frozen body".to_owned(),
            digest_hash: "hash".to_owned(),
        };
        let error = send(script.to_str().ok_or("path")?, "key/1", &snapshot)
            .err()
            .ok_or("expected invalid response")?;
        assert_eq!(error.code, "email_response_invalid");
        assert!(!error.message.contains("SECRET_BODY"));
        assert!(!error.message.contains("/Users/person"));
        Ok(())
    }
}
