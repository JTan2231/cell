//! Bounded discovery, receiving and observation. No agent activity is mirrored here.
use crate::agent::{self, Progress};
use crate::store::{Config, Store};
use crate::{Result, fail};
use email::api::{Client, ReceivedMessage, ReceivedPageRequest};
use rusqlite::params;
use serde_json::{Value, json};
use std::path::Path;

pub async fn tick(root: &Path, recovery: bool) -> Result<Value> {
    let gate = crate::gate(root);
    let _admission = if recovery {
        gate.recover()?
    } else {
        gate.enter()?
    };
    let _lock = crate::store::runner_lock(root)?;
    let mut store = Store::open(root)?;
    let mut config = Config::load(root)?;
    config.validate()?;
    let mut waiting = Vec::new();
    if !recovery && !config.paused {
        ingest(&mut store, &config)?;
        if poll(root, &store, &mut config).await.is_err() {
            waiting.push("email_receiving_unavailable");
        }
    }
    if let Some(mut exchange) = store.next()? {
        if exchange.request_digest.is_none() {
            if crate::now() >= exchange.deadline_at {
                finish(root, &store, &exchange, false).await?;
                return Ok(json!({"waiting":waiting,"status":store.status()?}));
            }
            agent::prepare(root, &store, &exchange, &config)?;
            exchange = store.exchange(&exchange.id)?;
        }
        match agent::advance(&store, &exchange).await {
            Ok(Progress::Waiting) => {}
            Ok(Progress::Complete) => finish(root, &store, &exchange, true).await?,
            Ok(Progress::Failed) => finish(root, &store, &exchange, false).await?,
            Err(_) => waiting.push("nucleus_unavailable_or_submission_unresolved"),
        }
    }
    let pending: Vec<String> = {
        let mut query = store.connection.prepare(
            "SELECT id FROM exchanges WHERE send_state='pending' ORDER BY created_at LIMIT 1",
        )?;
        query
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?
    };
    for id in pending {
        if crate::mail::deliver(root, &id).await.is_err() {
            waiting.push("email_submission_unresolved");
        }
    }
    Ok(json!({"waiting":waiting,"status":store.status()?}))
}

fn ingest(store: &mut Store, config: &Config) -> Result<()> {
    let client = clockwork::api::Client::new(&config.clockwork_executable);
    let page = client.incident_feed(store.cursor()?, 100)?;
    for incident in &page.items {
        let notification = client.notification(&incident.id)?;
        // Advance the page cursor only after every item has been committed.
        store.capture_incident(incident, 0, &notification, config)?;
    }
    if let Some(last) = page.items.last() {
        store.connection.execute(
            "UPDATE incidents SET feed_cursor=?2 WHERE id=?1",
            params![last.id, i64::try_from(page.next_cursor)?],
        )?;
    }
    Ok(())
}

fn route(store: &Store, message: &ReceivedMessage) -> Result<Option<String>> {
    let mut matched = None;
    for address in message.to.iter().chain(&message.received_for) {
        if let Some(id) = store.incident_for_route(&crate::mail::mailbox(address))? {
            if matched.as_ref().is_some_and(|old| old != &id) {
                return Ok(None);
            }
            matched = Some(id);
        }
    }
    Ok(matched)
}

async fn poll(root: &Path, store: &Store, config: &mut Config) -> Result<()> {
    let client = Client::new(&config.email_executable);
    for _ in 0..4 {
        let Ok(page) = client
            .list_received(&ReceivedPageRequest {
                limit: 100,
                after: config.poll_after.clone(),
            })
            .await
        else {
            config.poll_after = None;
            config.save(root)?;
            return Err(fail("receiving scan is unavailable"));
        };
        for metadata in &page.data {
            if store.seen(&metadata.id)? {
                continue;
            }
            if let Some(incident) = route(store, metadata)? {
                let message = client.get_received(&metadata.id).await?;
                if route(store, &message)?.as_deref() != Some(incident.as_str()) {
                    return Err(fail("received routing changed during retrieval"));
                }
                if !crate::mail::automated(&message) {
                    store.capture_reply(&incident, &message)?;
                }
            }
        }
        let next = if page.has_more {
            Some(
                page.data
                    .last()
                    .ok_or_else(|| fail("empty unfinished receiving page"))?
                    .id
                    .clone(),
            )
        } else {
            None
        };
        if page.has_more && next == config.poll_after {
            return Err(fail("receiving cursor did not advance"));
        }
        config.poll_after = next;
        config.save(root)?;
        if !page.has_more {
            break;
        }
    }
    Ok(())
}

async fn finish(
    root: &Path,
    store: &Store,
    exchange: &crate::store::Exchange,
    completed: bool,
) -> Result<()> {
    // Nucleus owns the agent's output and activity. EMT only needs to know whether
    // its email exists; a later runtime failure does not erase a submitted email.
    let current = store.exchange(&exchange.id)?;
    if current.mail_json.is_none() {
        let body = if completed {
            format!(
                "The EMT agent finished without submitting an email for this exchange. Inspect Nucleus job {} for its activity. EMT has not started a replacement attempt.",
                exchange.nucleus_job_id
            )
        } else {
            format!(
                "EMT could not complete this exchange. Inspect Nucleus job {} and the affected product before repeating an intervention. A partial action may have occurred. EMT has not started a replacement attempt.",
                exchange.nucleus_job_id
            )
        };
        crate::mail::freeze(store, &current, "EMT exchange needs attention", &body)?;
    }
    store.connection.execute(
        "UPDATE exchanges SET state=?2,request_json=NULL,error=?3 WHERE id=?1",
        params![
            exchange.id,
            if completed && current.mail_json.is_some() {
                "finished"
            } else {
                "failed"
            },
            if completed {
                None
            } else {
                Some("agent_failed_or_expired")
            }
        ],
    )?;
    // Transport failure remains on the exchange and does not create another job.
    let _ = crate::mail::deliver(root, &exchange.id).await;
    Ok(())
}
