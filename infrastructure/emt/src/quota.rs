//! Deterministic quota notices. This path never starts a Nucleus job.
use crate::Result;
use crate::store::{Config, private_directory, private_file, write_private};
use email::api::{Client, Message};
use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{QuotaStateV1, QuotaStatusV1};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

const UNKNOWN_NOTICE_DELAY_SECONDS: i64 = 300;
const PENDING_NOTICE_FILE: &str = "quota-notice-pending.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingNotice {
    version: u32,
    condition_id: String,
    unknown_since: i64,
}

#[derive(Serialize, Deserialize)]
struct Notice {
    version: u32,
    condition_id: String,
    message: Message,
    first_send_at: Option<i64>,
    last_send_at: Option<i64>,
    attempts: u8,
    receipt: Option<String>,
    uncertain: bool,
}

fn notice(quota: &QuotaStatusV1) -> Option<Notice> {
    if !quota.is_blocked() {
        return None;
    }
    let condition_id = quota.condition_id.clone()?;
    let observation = match quota.state {
        QuotaStateV1::Unknown => "Codex weekly quota could not be verified.".to_string(),
        QuotaStateV1::Exhausted => "Codex reported an exhausted usage limit.".to_string(),
        _ => format!(
            "Observed Codex weekly quota: {}% remaining.",
            quota.remaining_percent?
        ),
    };
    let reset = quota
        .resets_at
        .and_then(|at| time::OffsetDateTime::from_unix_timestamp(at).ok())
        .and_then(|at| {
            at.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .map_or_else(
            || "The reset time is unavailable.".to_string(),
            |at| format!("Reported reset: {at}."),
        );
    Some(Notice {
        version: 1,
        message: Message {
            subject: "Codex quota: model work paused".into(),
            body: format!(
                "{observation}\n\nNucleus paused new model work for this condition. Pending work is retained. {reset}\nNucleus rechecks quota and releases only this quota pause after recovery. Existing operator pauses and unrelated failure halts remain.\n\nInspect current state with `nucleus quota`.\nCondition: {condition_id}"
            ),
            idempotency_key: Some(format!("emt/quota/{condition_id}")),
        },
        condition_id,
        first_send_at: None,
        last_send_at: None,
        attempts: 0,
        receipt: None,
        uncertain: false,
    })
}

pub async fn tick(root: &Path, config: &Config, discover: bool) -> Result<bool> {
    let observation = if let Ok(client) = NucleusClient::for_current_user() {
        tokio::time::timeout(Duration::from_secs(5), client.quota_status())
            .await
            .ok()
    } else {
        None
    };
    let legacy = matches!(
        &observation,
        Some(Err(ClientError::Api { status: 404, .. }))
    );
    let quota = observation
        .and_then(std::result::Result::ok)
        .filter(|quota| quota.version == 1);
    let directory = root.join("quota-notifications");
    let retained = retain_notice(root, quota.as_ref(), discover, crate::now());
    deliver_notices(&directory, &Client::new(&config.email_executable)).await?;
    retained?;
    Ok(!legacy && quota.as_ref().is_none_or(QuotaStatusV1::is_blocked))
}

fn clear_pending(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => std::fs::File::open(
            path.parent()
                .ok_or_else(|| crate::fail("pending notice has no parent"))?,
        )?
        .sync_all()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn retain_notice(
    root: &Path,
    quota: Option<&QuotaStatusV1>,
    discover: bool,
    now: i64,
) -> Result<()> {
    let pending_path = root.join(PENDING_NOTICE_FILE);
    let Some(quota) = quota.filter(|quota| discover && quota.version == 1 && quota.is_blocked())
    else {
        return clear_pending(&pending_path);
    };
    let Some(condition_id) = quota
        .condition_id
        .as_deref()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
    else {
        return clear_pending(&pending_path);
    };
    let directory = root.join("quota-notifications");
    let path = directory.join(format!("{condition_id}.json"));
    // Frozen mail wins even if a later observation changes the pause reason.
    if path.try_exists()? {
        return clear_pending(&pending_path);
    }
    if quota.state == QuotaStateV1::Low
        && quota
            .remaining_percent
            .is_none_or(|percent| !percent.is_finite() || !(0.0..=100.0).contains(&percent))
    {
        return clear_pending(&pending_path);
    }
    if quota.state == QuotaStateV1::Unknown {
        let previous = if pending_path.try_exists()? {
            let pending: PendingNotice =
                serde_json::from_reader(private_file(&pending_path, false)?)?;
            if pending.version != 1 {
                return Err(crate::fail("unsupported pending quota notice"));
            }
            uuid::Uuid::parse_str(&pending.condition_id)?;
            Some(pending)
        } else {
            None
        };
        let pending = if let Some(pending) = previous
            .filter(|pending| pending.condition_id == condition_id && pending.unknown_since <= now)
        {
            pending
        } else {
            let pending = PendingNotice {
                version: 1,
                condition_id: condition_id.to_string(),
                unknown_since: now,
            };
            write_private(&pending_path, &serde_json::to_vec(&pending)?)?;
            pending
        };
        if now.saturating_sub(pending.unknown_since) < UNKNOWN_NOTICE_DELAY_SECONDS {
            return Ok(());
        }
    }
    if let Some(notice) = notice(quota) {
        private_directory(&directory)?;
        write_private(&path, &serde_json::to_vec(&notice)?)?;
    }
    // A retained frozen notice suppresses duplicates after an interrupted clear.
    clear_pending(&pending_path)
}

async fn deliver_notices(directory: &Path, email: &Client) -> Result<()> {
    if directory.try_exists()? {
        let mut paths = std::fs::read_dir(directory)?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        for path in paths {
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let mut notice: Notice = serde_json::from_slice(&std::fs::read(&path)?)?;
            if notice.version != 1 {
                return Err(crate::fail("unsupported quota notice"));
            }
            if notice.receipt.is_some() || notice.uncertain {
                continue;
            }
            let now = crate::now();
            if notice.attempts >= 2
                || notice
                    .first_send_at
                    .is_some_and(|first| now < first || now - first >= 23 * 3600)
            {
                notice.uncertain = true;
                write_private(&path, &serde_json::to_vec(&notice)?)?;
                continue;
            }
            if notice.last_send_at.is_some_and(|last| now - last < 300) {
                continue;
            }
            notice.first_send_at.get_or_insert(now);
            notice.last_send_at = Some(now);
            notice.attempts += 1;
            write_private(&path, &serde_json::to_vec(&notice)?)?;
            if let Ok(receipt) = email
                .send_with_options(&notice.message, &[], &email::api::ReplyOptions::default())
                .await
            {
                notice.receipt = Some(receipt.id);
                write_private(&path, &serde_json::to_vec(&notice)?)?;
            }
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucleus_core::QuotaPolicyV1;
    use std::path::PathBuf;

    const CONDITION: &str = "00000000-0000-4000-8000-000000000001";
    const NEXT_CONDITION: &str = "00000000-0000-4000-8000-000000000002";

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Result<Self> {
            let root = std::env::temp_dir().join(format!("emt-quota-{}", uuid::Uuid::now_v7()));
            private_directory(&root)?;
            Ok(Self(root))
        }

        fn notice_path(&self, condition: &str) -> PathBuf {
            self.0
                .join("quota-notifications")
                .join(format!("{condition}.json"))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn quota(state: QuotaStateV1) -> QuotaStatusV1 {
        QuotaStatusV1 {
            version: 1,
            policy: QuotaPolicyV1::default(),
            state,
            account_key: None,
            limit_id: "codex".into(),
            remaining_percent: (state == QuotaStateV1::Low).then_some(8.0),
            observed_at: Some(900),
            resets_at: Some(10_000),
            condition_id: Some(CONDITION.into()),
            // The pause may be old when its reason first becomes unknown.
            condition_started_at: Some(1),
        }
    }

    #[test]
    fn unknown_notice_waits_five_minutes_from_first_observation() -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = Fixture::new()?;
        let quota = quota(QuotaStateV1::Unknown);
        retain_notice(&fixture.0, Some(&quota), true, 1000)?;
        let pending_path = fixture.0.join(PENDING_NOTICE_FILE);
        let pending: PendingNotice = serde_json::from_slice(&std::fs::read(&pending_path)?)?;
        assert_eq!(pending.unknown_since, 1000);
        assert_eq!(
            std::fs::metadata(&pending_path)?.permissions().mode() & 0o777,
            0o600
        );
        assert!(!fixture.0.join("quota-notifications").try_exists()?);
        retain_notice(&fixture.0, Some(&quota), true, 1299)?;
        assert!(!fixture.notice_path(CONDITION).try_exists()?);
        retain_notice(&fixture.0, Some(&quota), true, 1300)?;
        assert!(!pending_path.try_exists()?);
        let notice: Notice =
            serde_json::from_slice(&std::fs::read(fixture.notice_path(CONDITION))?)?;
        assert!(
            notice
                .message
                .body
                .starts_with("Codex weekly quota could not be verified.")
        );
        assert_eq!(
            notice.message.idempotency_key.as_deref(),
            Some(format!("emt/quota/{CONDITION}").as_str())
        );
        assert_eq!(notice.attempts, 0);
        Ok(())
    }

    #[test]
    fn recovery_read_failure_and_disabled_discovery_restart_the_wait() -> Result<()> {
        let unknown = quota(QuotaStateV1::Unknown);
        for (observation, discover) in [
            (None, true),
            (Some(quota(QuotaStateV1::Open)), true),
            (Some(quota(QuotaStateV1::Disabled)), true),
            (Some(quota(QuotaStateV1::NotApplicable)), true),
            (Some(unknown.clone()), false),
        ] {
            let fixture = Fixture::new()?;
            retain_notice(&fixture.0, Some(&unknown), true, 1000)?;
            retain_notice(&fixture.0, observation.as_ref(), discover, 1200)?;
            assert!(!fixture.0.join(PENDING_NOTICE_FILE).try_exists()?);
            assert!(!fixture.notice_path(CONDITION).try_exists()?);
            retain_notice(&fixture.0, Some(&unknown), true, 1300)?;
            retain_notice(&fixture.0, Some(&unknown), true, 1599)?;
            assert!(!fixture.notice_path(CONDITION).try_exists()?);
            retain_notice(&fixture.0, Some(&unknown), true, 1600)?;
            assert!(fixture.notice_path(CONDITION).try_exists()?);
        }
        Ok(())
    }

    #[test]
    fn malformed_observations_clear_the_candidate_without_freezing_mail() -> Result<()> {
        let unknown = quota(QuotaStateV1::Unknown);
        let mut missing_id = unknown.clone();
        missing_id.condition_id = None;
        let mut invalid_id = unknown.clone();
        invalid_id.condition_id = Some("invalid".into());
        let mut unsupported = unknown.clone();
        unsupported.version = 2;
        let mut missing_percent = quota(QuotaStateV1::Low);
        missing_percent.remaining_percent = None;
        let mut invalid_percent = quota(QuotaStateV1::Low);
        invalid_percent.remaining_percent = Some(101.0);
        for malformed in [
            missing_id,
            invalid_id,
            unsupported,
            missing_percent,
            invalid_percent,
        ] {
            let fixture = Fixture::new()?;
            retain_notice(&fixture.0, Some(&unknown), true, 1000)?;
            retain_notice(&fixture.0, Some(&malformed), true, 1300)?;
            assert!(!fixture.0.join(PENDING_NOTICE_FILE).try_exists()?);
            assert!(!fixture.notice_path(CONDITION).try_exists()?);
        }
        Ok(())
    }

    #[test]
    fn confirmed_low_and_exhausted_quota_are_immediately_eligible() -> Result<()> {
        for state in [QuotaStateV1::Low, QuotaStateV1::Exhausted] {
            let fixture = Fixture::new()?;
            retain_notice(&fixture.0, Some(&quota(state)), true, 1000)?;
            assert!(fixture.notice_path(CONDITION).try_exists()?);
            assert!(!fixture.0.join(PENDING_NOTICE_FILE).try_exists()?);
        }
        Ok(())
    }

    #[test]
    fn confirmation_during_the_wait_freezes_the_confirmed_notice() -> Result<()> {
        let fixture = Fixture::new()?;
        retain_notice(&fixture.0, Some(&quota(QuotaStateV1::Unknown)), true, 1000)?;
        retain_notice(&fixture.0, Some(&quota(QuotaStateV1::Low)), true, 1001)?;
        let notice: Notice =
            serde_json::from_slice(&std::fs::read(fixture.notice_path(CONDITION))?)?;
        assert!(
            notice
                .message
                .body
                .starts_with("Observed Codex weekly quota: 8% remaining.")
        );
        assert!(!fixture.0.join(PENDING_NOTICE_FILE).try_exists()?);
        Ok(())
    }

    #[test]
    fn frozen_payload_and_transport_state_win_after_interrupted_pending_clear() -> Result<()> {
        let fixture = Fixture::new()?;
        let unknown = quota(QuotaStateV1::Unknown);
        retain_notice(&fixture.0, Some(&unknown), true, 1000)?;
        let pending_bytes = std::fs::read(fixture.0.join(PENDING_NOTICE_FILE))?;
        retain_notice(&fixture.0, Some(&unknown), true, 1300)?;
        let path = fixture.notice_path(CONDITION);
        let mut notice: Notice = serde_json::from_slice(&std::fs::read(&path)?)?;
        notice.first_send_at = Some(1300);
        notice.last_send_at = Some(1300);
        notice.attempts = 1;
        notice.receipt = Some("accepted-receipt".into());
        write_private(&path, &serde_json::to_vec(&notice)?)?;
        let frozen = std::fs::read(&path)?;
        // Simulate a restart after the notice was frozen but before pending clear.
        write_private(&fixture.0.join(PENDING_NOTICE_FILE), &pending_bytes)?;
        let mut later = quota(QuotaStateV1::Low);
        later.remaining_percent = None;
        retain_notice(&fixture.0, Some(&later), true, 1301)?;
        assert_eq!(std::fs::read(&path)?, frozen);
        assert!(!fixture.0.join(PENDING_NOTICE_FILE).try_exists()?);
        retain_notice(&fixture.0, Some(&unknown), true, 1600)?;
        assert_eq!(std::fs::read(&path)?, frozen);
        assert_eq!(
            std::fs::read_dir(fixture.0.join("quota-notifications"))?.count(),
            1
        );
        Ok(())
    }

    #[test]
    fn retained_candidate_survives_restart_but_a_new_condition_gets_a_new_wait() -> Result<()> {
        let fixture = Fixture::new()?;
        let pending = PendingNotice {
            version: 1,
            condition_id: CONDITION.into(),
            unknown_since: 1000,
        };
        write_private(
            &fixture.0.join(PENDING_NOTICE_FILE),
            &serde_json::to_vec(&pending)?,
        )?;
        let mut unknown = quota(QuotaStateV1::Unknown);
        retain_notice(&fixture.0, Some(&unknown), true, 1300)?;
        assert!(fixture.notice_path(CONDITION).try_exists()?);
        unknown.condition_id = Some(NEXT_CONDITION.into());
        retain_notice(&fixture.0, Some(&unknown), true, 1301)?;
        retain_notice(&fixture.0, Some(&unknown), true, 1600)?;
        assert!(!fixture.notice_path(NEXT_CONDITION).try_exists()?);
        retain_notice(&fixture.0, Some(&unknown), true, 1601)?;
        assert!(fixture.notice_path(NEXT_CONDITION).try_exists()?);
        Ok(())
    }

    #[test]
    fn changed_condition_before_eligibility_and_backward_time_reset_the_wait() -> Result<()> {
        let fixture = Fixture::new()?;
        let mut unknown = quota(QuotaStateV1::Unknown);
        retain_notice(&fixture.0, Some(&unknown), true, 1000)?;
        unknown.condition_id = Some(NEXT_CONDITION.into());
        retain_notice(&fixture.0, Some(&unknown), true, 1299)?;
        retain_notice(&fixture.0, Some(&unknown), true, 1200)?;
        retain_notice(&fixture.0, Some(&unknown), true, 1499)?;
        assert!(!fixture.notice_path(CONDITION).try_exists()?);
        assert!(!fixture.notice_path(NEXT_CONDITION).try_exists()?);
        retain_notice(&fixture.0, Some(&unknown), true, 1500)?;
        assert!(fixture.notice_path(NEXT_CONDITION).try_exists()?);
        Ok(())
    }

    #[test]
    fn unsupported_or_corrupt_pending_state_cannot_make_a_notice_eligible() -> Result<()> {
        let fixture = Fixture::new()?;
        let path = fixture.0.join(PENDING_NOTICE_FILE);
        for bytes in [
            br#"{"version":2,"condition_id":"00000000-0000-4000-8000-000000000001","unknown_since":1}"#.as_slice(),
            b"invalid-json".as_slice(),
        ] {
            write_private(&path, bytes)?;
            assert!(retain_notice(&fixture.0, Some(&quota(QuotaStateV1::Unknown)), true, 1300).is_err());
            assert!(!fixture.notice_path(CONDITION).try_exists()?);
        }
        Ok(())
    }
}
