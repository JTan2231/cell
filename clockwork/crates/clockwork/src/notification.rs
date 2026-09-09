//! One bounded Email attempt per broker visit. Product admission never depends on delivery.
use std::process::Stdio;
use std::time::Duration;

use rusqlite::params;
use tokio::io::AsyncWriteExt as _;
use tokio::process::Command;

use crate::error::{Context as _, Error, Result};
use crate::lock::KeyLock;
use crate::paths::Layout;
use crate::store::{Store, now_unix};

const RETRY_INTERVAL: i64 = 300;
// Leave an hour of margin before the provider's 24-hour idempotency expiry.
const DEDUP_WINDOW: i64 = 23 * 60 * 60;
#[allow(clippy::duration_suboptimal_units)] // Keep the workspace Rust 1.89 baseline.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) async fn send_pending(store: &mut Store, layout: &Layout) -> Result<usize> {
    send_selected(store, layout, None).await
}

pub(crate) async fn send_selected(
    store: &mut Store,
    layout: &Layout,
    selected: Option<&str>,
) -> Result<usize> {
    let Some(_lock) = KeyLock::try_acquire_notifications(layout)? else {
        return Ok(0);
    };
    let now = now_unix()?;
    store.connection.execute(
        "UPDATE incidents SET notification_status='uncertain' WHERE notification_status='pending' AND first_attempt_at IS NOT NULL AND (first_attempt_at > ?1 OR ?1 - first_attempt_at >= ?2)",
        params![now, DEDUP_WINDOW],
    ).context("database_write_failed", "retain notifications outside the deduplication window")?;
    let pending: Option<(String, String, i64)> = {
        use rusqlite::OptionalExtension as _;
        store.connection.query_row(
            "SELECT id,email_cli,notification_generation FROM incidents WHERE notification_status='pending' AND (?2 IS NULL OR id = ?2) AND (last_attempt_at IS NULL OR last_attempt_at <= ?1) ORDER BY created_at,id LIMIT 1",
            params![now - RETRY_INTERVAL, selected], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional().context("database_read_failed", "select pending pause notification")?
    };
    let Some((id, email_cli, generation)) = pending else {
        return Ok(0);
    };
    let incident = store.incident(&id)?;
    let (subject, body) = payload(&incident);
    store.connection.execute(
        "UPDATE incidents SET first_attempt_at=COALESCE(first_attempt_at,?2),last_attempt_at=?2,notification_attempts=notification_attempts+1 WHERE id=?1 AND notification_status='pending'",
        params![id,now],
    ).context("database_write_failed", "record notification attempt before transport")?;
    // The fixed rendering version and stored incident metadata freeze this payload.
    let idempotency_key = format!("clockwork/pause/{id}/{generation}/v1");
    let accepted = send_email(&email_cli, layout, &idempotency_key, &subject, &body).await;
    if accepted {
        store
            .connection
            .execute(
                "UPDATE incidents SET notification_status='accepted' WHERE id=?1",
                [&id],
            )
            .context(
                "database_write_failed",
                "record Email submission acceptance",
            )?;
    }
    Ok(1)
}

pub(crate) fn approve_retry(store: &mut Store, layout: &Layout, id: &str) -> Result<()> {
    let Some(_lock) = KeyLock::try_acquire_notifications(layout)? else {
        return Err(Error::new(
            "notification_busy",
            "another notification attempt is in progress",
        ));
    };
    let incident = store.incident(id)?;
    if incident.notification_status != "uncertain" {
        return Err(Error::new(
            "notification_state_invalid",
            "explicit duplicate-risk approval requires an uncertain notification",
        ));
    }
    store.connection.execute(
        "UPDATE incidents SET notification_status='pending', first_attempt_at=NULL,last_attempt_at=NULL,notification_generation=notification_generation+1 WHERE id=?1 AND notification_status='uncertain'", [id],
    ).context("database_write_failed", "record explicit notification retry approval")?;
    Ok(())
}

fn payload(incident: &clockwork::api::IncidentRecord) -> (String, String) {
    let subject = format!("Scheduling paused: {}", incident.key);
    let activation = incident
        .activation_id
        .as_deref()
        .unwrap_or("imported product failure");
    let body = format!(
        "Clockwork halted scheduling for {} at Unix time {}.\n\nFailure code: {}\nOccurrence: {}\nActivation: {}\nIncident: {}\n\nInspect the product's retained result and recovery instructions. No further work will start through this binding until you explicitly approve continuation.\n\nInspect: clockwork incident show {}\nApprove future scheduling: clockwork binding resume {} {}\n\nContinuation does not retry or undo the failed product work. This message describes the halt when it was recorded.\n",
        incident.key,
        incident.created_at,
        incident.code,
        incident.occurrence,
        activation,
        incident.id,
        incident.id,
        incident.key,
        incident.id,
    );
    (subject, body)
}

async fn send_email(
    executable: &str,
    layout: &Layout,
    idempotency_key: &str,
    subject: &str,
    body: &str,
) -> bool {
    let mut command = Command::new(executable);
    command
        .args(["--idempotency-key", idempotency_key, "--", subject, "-"])
        .env_clear()
        .env("HOME", layout.home())
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .kill_on_drop(true);
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let Some(mut input) = child.stdin.take() else {
        let _ = child.kill().await;
        return false;
    };
    let result = tokio::time::timeout(ATTEMPT_TIMEOUT, async {
        input.write_all(body.as_bytes()).await?;
        input.shutdown().await?;
        drop(input);
        child.wait().await
    })
    .await;
    if let Ok(Ok(status)) = result {
        status.success()
    } else {
        if let Some(pid) = child.id() {
            let _ = Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{pid}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        let _ = child.kill().await;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::payload;
    use clockwork::api::IncidentRecord;

    #[test]
    fn payload_stays_identical_after_resume_and_attempt_updates() {
        let mut incident = IncidentRecord {
            id: "incident-1".into(),
            key: "example/worker".into(),
            activation_id: Some("activation-1".into()),
            definition_digest: None,
            code: "timed_out".into(),
            occurrence: "activation/one".into(),
            created_at: 42,
            resumed_at: None,
            notification_status: "pending".into(),
            first_attempt_at: None,
            last_attempt_at: None,
            notification_attempts: 0,
        };
        let first = payload(&incident);
        incident.resumed_at = Some(80);
        incident.first_attempt_at = Some(50);
        incident.notification_attempts = 2;
        incident.notification_status = "accepted".into();
        assert_eq!(first, payload(&incident));
    }
    #[tokio::test]
    async fn failed_email_stays_pending_without_reopening_a_disabled_halt_and_expiry_needs_approval()
     {
        use crate::{paths::Layout, store::Store};
        use std::os::unix::fs::PermissionsExt as _;
        let temporary = tempfile::tempdir().expect("temporary");
        let layout = Layout::isolated(temporary.path());
        let mut store = Store::open(&layout).expect("store");
        let incident = store
            .import_halt("example/worker", "failed", "job/one")
            .expect("import halt");
        let email = temporary.path().join("email-double");
        std::fs::write(&email, "#!/bin/sh\n/bin/cat >/dev/null\nexit 1\n").expect("email double");
        std::fs::set_permissions(&email, std::fs::Permissions::from_mode(0o700))
            .expect("executable");
        store
            .connection
            .execute(
                "UPDATE incidents SET email_cli=?2 WHERE id=?1",
                rusqlite::params![incident.id, email.to_string_lossy()],
            )
            .expect("fixture transport");
        assert_eq!(
            super::send_pending(&mut store, &layout)
                .await
                .expect("attempt"),
            1
        );
        assert_eq!(
            store
                .incident(&incident.id)
                .expect("incident")
                .notification_status,
            "pending"
        );
        assert_eq!(
            super::send_pending(&mut store, &layout)
                .await
                .expect("spacing"),
            0
        );
        let binding = store.binding("example/worker").expect("binding");
        assert!(!binding.enabled);
        assert_eq!(
            binding.halted_incident.as_deref(),
            Some(incident.id.as_str())
        );
        store
            .connection
            .execute(
                "UPDATE incidents SET first_attempt_at=1,last_attempt_at=1 WHERE id=?1",
                [&incident.id],
            )
            .expect("elapsed fixture");
        assert_eq!(
            super::send_pending(&mut store, &layout)
                .await
                .expect("expire"),
            0
        );
        assert_eq!(
            store
                .incident(&incident.id)
                .expect("incident")
                .notification_status,
            "uncertain"
        );
        super::approve_retry(&mut store, &layout, &incident.id)
            .expect("explicit duplicate-risk approval");
        std::fs::write(
            &email,
            "#!/bin/sh\n/bin/cat >/dev/null\nprintf 'Accepted test-message\\n'\n",
        )
        .expect("accepted double");
        assert_eq!(
            super::send_selected(&mut store, &layout, Some(&incident.id))
                .await
                .expect("approved attempt"),
            1
        );
        let accepted = store.incident(&incident.id).expect("incident");
        assert_eq!(accepted.notification_status, "accepted");
        assert_eq!(accepted.notification_attempts, 2);
        assert!(store.require_unhalted("example/worker").is_err());
    }
}
