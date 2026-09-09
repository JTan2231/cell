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
    let candidates: Vec<(String, String, i64)> = {
        let mut query = store.connection.prepare(
            "SELECT id,email_cli,notification_generation FROM incidents WHERE notification_status='pending' AND (?2 IS NULL OR id = ?2) AND (last_attempt_at IS NULL OR last_attempt_at <= ?1) ORDER BY created_at,id",
        ).context("database_read_failed", "prepare pending notifications")?;
        query
            .query_map(params![now - RETRY_INTERVAL, selected], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .context("database_read_failed", "read pending notifications")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("database_read_failed", "decode pending notifications")?
    };
    let mut routing = Routing::load(layout)?;
    let mut pending = None;
    for (id, executable, generation) in candidates {
        let incident = store.incident(&id)?;
        let route = routing.route(layout, &incident)?;
        if route
            .as_ref()
            .is_some_and(|route| route.delivery_id.is_some() || now < route.due_at)
        {
            continue;
        }
        pending = Some((
            incident,
            executable,
            generation,
            route.and_then(|route| route.reply_to),
        ));
        break;
    }
    let Some((incident, email_cli, generation, reply_to)) = pending else {
        return Ok(0);
    };
    let id = incident.id.clone();
    let (subject, body) = payload(&incident);
    store.connection.execute(
        "UPDATE incidents SET first_attempt_at=COALESCE(first_attempt_at,?2),last_attempt_at=?2,notification_attempts=notification_attempts+1 WHERE id=?1 AND notification_status='pending'",
        params![id,now],
    ).context("database_write_failed", "record notification attempt before transport")?;
    // The fixed rendering version and stored incident metadata freeze this payload.
    let idempotency_key = format!("clockwork/pause/{id}/{generation}/v1");
    let accepted = send_email(
        &email_cli,
        layout,
        &idempotency_key,
        &subject,
        &body,
        reply_to.as_deref(),
    )
    .await;
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
    reply_to: Option<&str>,
) -> bool {
    let mut command = Command::new(executable);
    if let Some(address) = reply_to {
        command.args(["--reply-to", address]);
    }
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

// Versioned metadata beside the unchanged schema-two incident database.
// Diagnostic text and incoming mail remain in EMT.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Routing {
    version: u32,
    domain: Option<String>,
    enabled_at: i64,
    routes: std::collections::BTreeMap<String, Route>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    reply_to: Option<String>,
    due_at: i64,
    delivery_id: Option<String>,
}

impl Routing {
    fn load(layout: &Layout) -> Result<Self> {
        let path = layout.state_root().join("notification-routing.json");
        match std::fs::read(&path) {
            Ok(bytes) => {
                let value: Self = serde_json::from_slice(&bytes)
                    .context("notification_routing_invalid", "read notification routing")?;
                if value.version != 1 {
                    return Err(Error::new(
                        "notification_routing_invalid",
                        "unsupported notification routing version",
                    ));
                }
                Ok(value)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                version: 1,
                ..Self::default()
            }),
            Err(error) => Err(Error::new(
                "notification_routing_unavailable",
                error.to_string(),
            )),
        }
    }

    fn save(&self, layout: &Layout) -> Result<()> {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let temporary = layout
            .state_root()
            .join(format!(".notification-routing-{}", uuid::Uuid::now_v7()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .context(
                "notification_routing_write_failed",
                "prepare notification routing",
            )?;
        file.write_all(&serde_json::to_vec(self).context(
            "notification_routing_invalid",
            "encode notification routing",
        )?)
        .context(
            "notification_routing_write_failed",
            "write notification routing",
        )?;
        file.sync_all().context(
            "notification_routing_write_failed",
            "sync notification routing",
        )?;
        std::fs::rename(
            &temporary,
            layout.state_root().join("notification-routing.json"),
        )
        .context(
            "notification_routing_write_failed",
            "commit notification routing",
        )?;
        std::fs::File::open(layout.state_root())
            .and_then(|file| file.sync_all())
            .context(
                "notification_routing_write_failed",
                "sync routing directory",
            )
    }

    fn route(
        &mut self,
        layout: &Layout,
        incident: &clockwork::api::IncidentRecord,
    ) -> Result<Option<Route>> {
        if let Some(route) = self.routes.get(&incident.id) {
            return Ok(Some(route.clone()));
        }
        let Some(domain) = &self.domain else {
            return Ok(None);
        };
        if incident.key == "emt/worker"
            || incident.created_at < self.enabled_at
            || incident.first_attempt_at.is_some()
        {
            return Ok(None);
        }
        let route = Route {
            reply_to: Some(format!("emt.{}@{}", incident.id.replace('-', ""), domain)),
            due_at: incident.created_at.saturating_add(120),
            delivery_id: None,
        };
        self.routes.insert(incident.id.clone(), route.clone());
        self.save(layout)?;
        Ok(Some(route))
    }
}

pub(crate) fn configure_emt(layout: &Layout, domain: Option<&str>) -> Result<()> {
    let Some(_lock) = KeyLock::try_acquire_notifications(layout)? else {
        return Err(Error::new(
            "notification_busy",
            "another notification operation is in progress",
        ));
    };
    if let Some(domain) = domain
        && (domain.len() + 37 > 254
            || !domain.contains('.')
            || domain.split('.').any(|part| {
                part.is_empty()
                    || part.len() > 63
                    || part.starts_with('-')
                    || part.ends_with('-')
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            }))
    {
        return Err(Error::new(
            "receiving_domain_invalid",
            "invalid EMT receiving domain",
        ));
    }
    let mut routing = Routing::load(layout)?;
    if routing.domain.as_deref() != domain {
        routing.domain = domain.map(str::to_owned);
        routing.enabled_at = now_unix()?;
        routing.save(layout)?;
    }
    Ok(())
}

pub(crate) fn view(
    store: &Store,
    layout: &Layout,
    id: &str,
    delivery_id: Option<&str>,
) -> Result<clockwork::api::NotificationView> {
    let Some(_lock) = KeyLock::try_acquire_notifications(layout)? else {
        return Err(Error::new(
            "notification_busy",
            "another notification operation is in progress",
        ));
    };
    let incident = store.incident(id)?;
    let mut routing = Routing::load(layout)?;
    let mut route = routing.route(layout, &incident)?;
    if let Some(delivery) = delivery_id {
        if uuid::Uuid::parse_str(delivery).is_err() {
            return Err(Error::new(
                "delivery_id_invalid",
                "delivery ID must be a UUID",
            ));
        }
        let current = route.as_mut().ok_or_else(|| {
            Error::new(
                "notification_not_delegatable",
                "this incident uses a basic notification",
            )
        })?;
        if current.delivery_id.as_deref() != Some(delivery) {
            if current.delivery_id.is_some()
                || incident.first_attempt_at.is_some()
                || incident.notification_status != "pending"
            {
                return Err(Error::new(
                    "notification_not_delegatable",
                    "initial notification already belongs to another delivery",
                ));
            }
            current.delivery_id = Some(delivery.to_owned());
            routing.routes.insert(id.to_owned(), current.clone());
            routing.save(layout)?;
        }
    }
    let (subject, body) = payload(&incident);
    Ok(clockwork::api::NotificationView {
        incident_id: id.to_owned(),
        delivery_id: route.as_ref().and_then(|route| route.delivery_id.clone()),
        reply_to: route.and_then(|route| route.reply_to),
        subject,
        body,
    })
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
