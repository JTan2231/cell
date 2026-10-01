//! Deterministic quota notices. This path never starts a Nucleus job.
use crate::Result;
use crate::store::{Config, private_directory, write_private};
use email::api::{Client, Message};
use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{QuotaStateV1, QuotaStatusV1};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

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
    let client = NucleusClient::for_current_user()?;
    let observation = tokio::time::timeout(Duration::from_secs(5), client.quota_status()).await;
    let legacy = matches!(&observation, Ok(Err(ClientError::Api { status: 404, .. })));
    let quota = observation.ok().and_then(std::result::Result::ok);
    let directory = root.join("quota-notifications");
    if discover && let Some(quota) = &quota {
        retain_notice(&directory, quota)?;
    }
    deliver_notices(&directory, &Client::new(&config.email_executable)).await?;
    Ok(!legacy && quota.as_ref().is_none_or(QuotaStatusV1::is_blocked))
}

fn retain_notice(directory: &Path, quota: &QuotaStatusV1) -> Result<()> {
    if let Some(notice) = notice(quota) {
        uuid::Uuid::parse_str(&notice.condition_id)?;
        private_directory(directory)?;
        let path = directory.join(format!("{}.json", notice.condition_id));
        if !path.try_exists()? {
            write_private(&path, &serde_json::to_vec(&notice)?)?;
        }
    }
    Ok(())
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
