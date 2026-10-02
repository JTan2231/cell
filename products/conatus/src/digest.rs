//! Deterministic daily mail from retained wants and existing evidence.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Local, Timelike as _, Utc};
use email::api::{Client, Message, ReplyOptions};
use fs2::FileExt as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::annals::EmailContext;
use crate::store::{Record, Store, WantState};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Digest {
    pub subject: String,
    pub body: String,
    pub want_count: usize,
    pub context_revision: Option<i64>,
    pub context_available: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Delivery {
    occurrence: String,
    digest: Digest,
    idempotency_key: String,
    attempted_at: Option<i64>,
    accepted_id: Option<String>,
}

pub fn render(records: &[Record], context: Option<&EmailContext>) -> Result<Digest> {
    let wants: Vec<_> = records
        .iter()
        .filter(|record| record.kind == "want" && record.state != Some(WantState::Archived))
        .collect();
    let mut body = String::new();
    for (index, want) in wants.iter().enumerate() {
        if index > 0 {
            body.push_str("\n----------------------------------------\n\n");
        }
        writeln!(
            body,
            "{}.\n\n{}\n\n{}",
            index + 1,
            want.wording,
            capture_date(want.captured_at)?
        )?;
        if let Some(quotes) = context.and_then(|c| c.quotes.get(&want.work_name)) {
            let mut count = 0;
            // Intake is already ordered by capture time and ID, newest first.
            for decision in records.iter().filter(|record| record.kind == "decision") {
                if let Some(quote) = quotes.get(&decision.work_name) {
                    if count == 0 {
                        body.push_str("\nRelated:\n");
                    }
                    writeln!(body, "\n{quote}\n{}", capture_date(decision.captured_at)?)?;
                    count += 1;
                    if count == 2 {
                        break;
                    }
                }
            }
        }
    }
    if wants.is_empty() {
        body.push_str("No active wants.\n");
    } else if context.is_none() {
        body.push_str("\nSupporting context was unavailable. All active wants are included.\n");
    }
    Ok(Digest {
        subject: format!("Conatus — {} wants", wants.len()),
        body,
        want_count: wants.len(),
        context_revision: context.map(|c| c.revision),
        context_available: context.is_some(),
    })
}

fn capture_date(timestamp: i64) -> Result<String> {
    Ok(DateTime::<Utc>::from_timestamp(timestamp, 0)
        .context("invalid Conatus capture timestamp")?
        .format("%Y-%m-%d")
        .to_string())
}

fn prepare(store: &Store) -> Result<Digest> {
    let records = store.email_records()?;
    if !records.iter().any(|record| record.kind == "want") {
        return render(&records, Some(&EmailContext::default()));
    }
    let context = store.config()?.library().email_context();
    if context.is_err() {
        eprintln!(
            "Conatus email: supporting context unavailable; using the complete active wants list"
        );
    }
    render(&records, context.as_ref().ok())
}

pub fn preview(root: &Path, occurrence: Option<&str>) -> Result<Value> {
    let store = Store::open(root)?;
    let digest = match occurrence {
        Some(id) => {
            delivery(&store, id)?
                .context("unknown email occurrence")?
                .digest
        }
        None => prepare(&store)?,
    };
    Ok(json!({"from":email::api::sender(),"to":email::api::recipient(),"digest":digest}))
}

fn delivery(store: &Store, occurrence: &str) -> Result<Option<Delivery>> {
    ensure!(
        occurrence.len() <= 100
            && occurrence
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-".contains(&b)),
        "invalid email occurrence"
    );
    store
        .setting(&format!("email/{occurrence}"))?
        .map(|text| serde_json::from_str(&text).map_err(Into::into))
        .transpose()
}

fn save(store: &Store, delivery: &Delivery) -> Result<()> {
    store.set(
        &format!("email/{}", delivery.occurrence),
        &serde_json::to_string(delivery)?,
    )
}

fn scheduled_occurrence(now: DateTime<Local>) -> Result<String> {
    let date = if now.hour() < 9 {
        now.date_naive()
            .pred_opt()
            .context("local date has no predecessor")?
    } else {
        now.date_naive()
    };
    Ok(format!("daily/{date}"))
}

/// An explicit retry selects the retained bytes after provider acceptance inspection.
pub fn send(root: &Path, scheduled: bool, retry: Option<&str>) -> Result<Value> {
    let gate = crate::gate(root);
    if scheduled && !gate.status()?.holds.is_empty() {
        return Ok(json!({"scheduled":true,"skipped":"deployment_maintenance"}));
    }
    let _admission = match gate.enter() {
        Ok(admission) => admission,
        Err(_) if scheduled && !gate.status()?.holds.is_empty() => {
            return Ok(json!({"scheduled":true,"skipped":"deployment_maintenance"}));
        }
        Err(error) => return Err(error.into()),
    };
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(root.join("email.lock"))?;
    lock.try_lock_exclusive()
        .context("another Conatus email send is running")?;
    let store = Store::open(root)?;
    let occurrence = if let Some(id) = retry {
        id.to_owned()
    } else if scheduled {
        scheduled_occurrence(Local::now())?
    } else {
        format!("manual/{}", uuid::Uuid::now_v7())
    };
    let mut outgoing = if let Some(existing) = delivery(&store, &occurrence)? {
        existing
    } else {
        ensure!(retry.is_none(), "unknown email occurrence");
        let outgoing = Delivery {
            idempotency_key: format!("conatus-email/{}/{occurrence}", store.config()?.library_id),
            occurrence: occurrence.clone(),
            digest: prepare(&store)?,
            attempted_at: None,
            accepted_id: None,
        };
        save(&store, &outgoing)?;
        outgoing
    };
    if let Some(id) = &outgoing.accepted_id {
        return Ok(json!({"occurrence":occurrence,"accepted_id":id,"already_accepted":true}));
    }
    if let Some(attempted) = outgoing.attempted_at {
        ensure!(
            retry.is_some(),
            "email occurrence {occurrence} has uncertain acceptance; inspect the provider before using email send --retry {occurrence}"
        );
        let age = crate::now()? - attempted;
        ensure!(
            (0..23 * 3600).contains(&age),
            "email retry is outside the safe provider idempotency window; no message was sent"
        );
    }
    let home = std::path::PathBuf::from(std::env::var_os("HOME").context("HOME is required")?);
    ensure!(home.is_absolute(), "HOME must be absolute");
    let client = Client::new(home.join(".local/bin/email"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    outgoing.attempted_at.get_or_insert(crate::now()?);
    save(&store, &outgoing)?;
    let receipt = runtime.block_on(client.send_with_options(&Message {
        subject: outgoing.digest.subject.clone(),
        body: outgoing.digest.body.clone(),
        idempotency_key: Some(outgoing.idempotency_key.clone()),
    }, &[], &ReplyOptions::default()))
        .with_context(|| format!("email occurrence {occurrence} has uncertain acceptance; inspect the provider before retrying"))?;
    outgoing.accepted_id = Some(receipt.id.clone());
    save(&store, &outgoing)?;
    Ok(
        json!({"occurrence":occurrence,"accepted_id":receipt.id,"want_count":outgoing.digest.want_count,"already_accepted":false}),
    )
}
