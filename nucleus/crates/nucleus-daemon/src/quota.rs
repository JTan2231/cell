use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use nucleus_codex::CodexHarness;
use nucleus_core::{QuotaPolicyV1, QuotaStateV1, QuotaStatusV1, codex_weekly_window};
use tokio::sync::Mutex;
use uuid::Uuid;

pub const POLL_SECONDS: u64 = 60;
const MAX_AGE_SECONDS: i64 = 120;

pub struct QuotaGate {
    path: PathBuf,
    generation: AtomicU64,
    value: Mutex<QuotaStatusV1>,
    refresh_lock: Mutex<()>,
    last_check: Mutex<Option<i64>>,
}

impl QuotaGate {
    pub fn open(root: &Path) -> io::Result<Self> {
        let policy: QuotaPolicyV1 = read_json(&root.join("quota-policy.json"))?.unwrap_or_default();
        if policy.pause_at_remaining_percent >= policy.resume_above_remaining_percent
            || policy.resume_above_remaining_percent >= 100
        {
            return Err(io::Error::other(
                "quota policy requires 0 <= pause < resume < 100",
            ));
        }
        let path = root.join("quota-state.json");
        let mut value = read_json::<QuotaStatusV1>(&path)?.unwrap_or(QuotaStatusV1 {
            version: 1,
            policy: policy.clone(),
            state: QuotaStateV1::Unknown,
            account_key: None,
            limit_id: "codex".into(),
            remaining_percent: None,
            observed_at: None,
            resets_at: None,
            condition_id: None,
            condition_started_at: None,
        });
        if value.version != 1 || value.limit_id != "codex" {
            return Err(io::Error::other("unsupported quota admission state"));
        }
        value.policy = policy;
        if !value.policy.enabled {
            value.state = QuotaStateV1::Disabled;
        }
        Ok(Self {
            path,
            generation: AtomicU64::new(0),
            value: Mutex::new(value),
            refresh_lock: Mutex::new(()),
            last_check: Mutex::new(None),
        })
    }

    pub async fn snapshot(&self) -> io::Result<QuotaStatusV1> {
        let mut value = self.value.lock().await;
        let now = unix_now();
        if value.policy.enabled
            && !matches!(
                value.state,
                QuotaStateV1::NotApplicable | QuotaStateV1::Exhausted
            )
            && !fresh(&value, now)
        {
            transition(&mut value, QuotaStateV1::Unknown, now);
            self.save(&value)?;
        }
        Ok(value.clone())
    }

    pub async fn refresh(&self, harness: &CodexHarness) -> io::Result<()> {
        let Ok(_refresh) = self.refresh_lock.try_lock() else {
            return Ok(());
        };
        let now = unix_now();
        let enabled = self.value.lock().await.policy.enabled;
        if !enabled {
            return Ok(());
        }
        let account = harness.quota_account_key();
        let same_account = account.as_ref().ok().is_some_and(|key| {
            self.value
                .try_lock()
                .is_ok_and(|value| value.account_key.as_ref() == key.as_ref())
        });
        if same_account
            && self
                .last_check
                .lock()
                .await
                .is_some_and(|last| now >= last && now - last < 60)
        {
            return Ok(());
        }
        *self.last_check.lock().await = Some(now);
        match account {
            Ok(None) => {
                let mut value = self.value.lock().await;
                value.account_key = None;
                value.remaining_percent = None;
                value.observed_at = None;
                value.resets_at = None;
                transition(&mut value, QuotaStateV1::NotApplicable, now);
                self.save(&value)
            }
            Ok(Some(key)) => {
                {
                    let mut value = self.value.lock().await;
                    if value.account_key.as_ref() != Some(&key) {
                        value.account_key = Some(key);
                        value.observed_at = None;
                        value.remaining_percent = None;
                        value.resets_at = None;
                        value.condition_id = None;
                        value.condition_started_at = None;
                        value.state = QuotaStateV1::Unknown;
                    }
                }
                let generation = self.generation.load(Ordering::SeqCst);
                let sample = harness
                    .read_account_snapshot(false, Duration::ZERO, Duration::from_secs(10))
                    .await;
                let mut value = self.value.lock().await;
                if generation != self.generation.load(Ordering::SeqCst) {
                    return Ok(());
                }
                if let Ok(sample) = sample {
                    observe(&mut value, &sample.rate_limits, unix_now());
                } else if !fresh(&value, unix_now()) {
                    transition(&mut value, QuotaStateV1::Unknown, unix_now());
                }
                self.save(&value)
            }
            Err(_) => {
                let mut value = self.value.lock().await;
                // An unavailable identity cannot authorize use of a previous account's allowance.
                transition(&mut value, QuotaStateV1::Unknown, now);
                self.save(&value)
            }
        }
    }

    pub async fn exhausted(&self) -> io::Result<()> {
        let mut value = self.value.lock().await;
        if value.policy.enabled && value.state != QuotaStateV1::NotApplicable {
            self.generation.fetch_add(1, Ordering::SeqCst);
            value.remaining_percent = None;
            value.observed_at = None;
            transition(&mut value, QuotaStateV1::Exhausted, unix_now());
            *self.last_check.lock().await = Some(unix_now());
            self.save(&value)?;
        }
        Ok(())
    }

    fn save(&self, value: &QuotaStatusV1) -> io::Result<()> {
        let temporary = self.path.with_extension(format!("{}.tmp", Uuid::now_v7()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&serde_json::to_vec(value).map_err(io::Error::other)?)?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)?;
            if let Some(parent) = self.path.parent() {
                fs::File::open(parent)?.sync_all()?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
                return Err(io::Error::other(
                    "quota files must be private regular files",
                ));
            }
            serde_json::from_slice(&fs::read(path)?)
                .map(Some)
                .map_err(io::Error::other)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn fresh(value: &QuotaStatusV1, now: i64) -> bool {
    value
        .observed_at
        .is_some_and(|at| now >= at && now - at <= MAX_AGE_SECONDS)
        && value.resets_at.is_some_and(|reset| now < reset)
}

fn observe(value: &mut QuotaStatusV1, payload: &serde_json::Value, now: i64) {
    let Some((remaining, reset)) = codex_weekly_window(payload, now) else {
        value.remaining_percent = None;
        value.observed_at = None;
        transition(value, QuotaStateV1::Unknown, now);
        return;
    };
    let was_paused = value.condition_id.is_some();
    value.remaining_percent = Some(remaining);
    value.resets_at = Some(reset);
    value.observed_at = Some(now);
    let state = if remaining <= f64::from(value.policy.pause_at_remaining_percent)
        || (was_paused && remaining <= f64::from(value.policy.resume_above_remaining_percent))
    {
        if remaining == 0.0 {
            QuotaStateV1::Exhausted
        } else {
            QuotaStateV1::Low
        }
    } else {
        QuotaStateV1::Open
    };
    transition(value, state, now);
}

fn transition(value: &mut QuotaStatusV1, state: QuotaStateV1, now: i64) {
    value.state = state;
    if value.is_blocked() {
        if value.condition_id.is_none() {
            value.condition_id = Some(Uuid::now_v7().to_string());
            value.condition_started_at = Some(now);
        }
    } else {
        value.condition_id = None;
        value.condition_started_at = None;
    }
}

fn unix_now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn pause_survives_restart_and_only_fresh_recovery_reopens()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let gate = QuotaGate::open(root.path())?;
        let sample = |used| json!({"rateLimitsByLimitId":{"codex":{"limitId":"codex","primary":{"usedPercent":used,"windowDurationMins":10080,"resetsAt":1000}}}});
        let mut value = gate.value.lock().await;
        observe(&mut value, &sample(90), 100);
        assert_eq!(value.state, QuotaStateV1::Low);
        let id = value.condition_id.clone();
        gate.save(&value)?;
        drop(value);
        let restored = QuotaGate::open(root.path())?;
        let mut value = restored.value.lock().await;
        assert_eq!(value.condition_id, id);
        observe(&mut value, &sample(85), 110);
        assert!(value.is_blocked());
        observe(&mut value, &serde_json::Value::Null, 120);
        assert_eq!(value.state, QuotaStateV1::Unknown);
        assert_eq!(value.condition_id, id);
        observe(&mut value, &sample(84), 130);
        assert_eq!(value.state, QuotaStateV1::Open);
        assert!(value.condition_id.is_none());
        Ok(())
    }
}
