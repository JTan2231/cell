//! The agent writes the email; EMT retains and transports that exact exchange.
use crate::store::{Config, Exchange, Store};
use crate::{Result, fail};
use email::api::{Client, Message, ReceivedMessage, ReplyOptions};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Letter {
    pub message: Message,
    pub reply: ReplyOptions,
}

/// Send one agent-authored email. Repeating the same exchange reuses its frozen payload.
pub async fn compose(root: &Path, id: &str, subject: &str, body: &str) -> Result<Value> {
    uuid::Uuid::parse_str(id).map_err(|_| fail("exchange ID must be a UUID"))?;
    let _admission = crate::gate(root).recover()?;
    let _lock = crate::store::lock(root, &format!("mail-{id}.lock"))?;
    let store = Store::open(root)?;
    let exchange = store.exchange(id)?;
    if exchange.state != "running" || crate::now() >= exchange.deadline_at {
        return Err(fail("this exchange is not accepting an agent email"));
    }
    freeze(&store, &exchange, subject, body)?;
    deliver_locked(root, &store, &store.exchange(id)?).await?;
    Ok(json!({"exchange_id":id,"email_id":store.exchange(id)?.email_id}))
}

pub fn freeze(store: &Store, exchange: &Exchange, subject: &str, body: &str) -> Result<()> {
    if subject.trim().is_empty()
        || subject.len() > 256
        || subject.contains(['\n', '\r'])
        || body.trim().is_empty()
        || body.len() > 64 * 1024
    {
        return Err(fail(
            "email requires a one-line subject and a body of at most 64 KiB",
        ));
    }
    let incident = store.incident(&exchange.incident_id)?;
    let parent = exchange
        .incoming_json
        .as_ref()
        .map(|value| serde_json::from_str::<ReceivedMessage>(value))
        .transpose()?
        .map(|message| message.message_id)
        .filter(|id| !id.is_empty());
    let letter = Letter {
        message: Message {
            subject: subject.into(),
            body: body.into(),
            idempotency_key: Some(format!("emt/exchange/{}", exchange.id)),
        },
        reply: ReplyOptions {
            reply_to: Some(
                incident["reply_to"]
                    .as_str()
                    .ok_or_else(|| fail("incident reply route is absent"))?
                    .into(),
            ),
            in_reply_to: parent.clone(),
            references: parent.into_iter().collect(),
        },
    };
    if let Some(frozen) = &exchange.mail_json {
        if serde_json::from_str::<Letter>(frozen)? != letter {
            return Err(fail("this exchange already has a different frozen email"));
        }
        return Ok(());
    }
    store.connection.execute(
        "UPDATE exchanges SET mail_json=?2,send_state='pending' WHERE id=?1 AND mail_json IS NULL",
        params![exchange.id, serde_json::to_string(&letter)?],
    )?;
    Ok(())
}

pub async fn deliver(root: &Path, id: &str) -> Result<()> {
    uuid::Uuid::parse_str(id)?;
    let _lock = crate::store::lock(root, &format!("mail-{id}.lock"))?;
    let store = Store::open(root)?;
    deliver_locked(root, &store, &store.exchange(id)?).await
}

async fn deliver_locked(root: &Path, store: &Store, exchange: &Exchange) -> Result<()> {
    if exchange.send_state.as_deref() == Some("accepted")
        || exchange.send_state.as_deref() == Some("uncertain")
    {
        return Ok(());
    }
    let Some(payload) = &exchange.mail_json else {
        return Ok(());
    };
    let now = crate::now();
    // One recovery attempt, with the same bytes and identity, inside Email's horizon.
    if exchange.send_attempts >= 2
        || exchange
            .first_send_at
            .is_some_and(|first| now < first || now - first >= 23 * 60 * 60)
    {
        store.connection.execute(
            "UPDATE exchanges SET send_state='uncertain' WHERE id=?1",
            [&exchange.id],
        )?;
        return Ok(());
    }
    if exchange.last_send_at.is_some_and(|last| now - last < 300) {
        return Ok(());
    }
    let config = Config::load(root)?;
    let clockwork = clockwork::api::Client::new(&config.clockwork_executable);
    if exchange.kind == "diagnosis" {
        match clockwork.claim_notification(&exchange.incident_id, &exchange.id) {
            Ok(view) if view.delivery_id.as_deref() == Some(exchange.id.as_str()) => {}
            Ok(_) => return Err(fail("Clockwork did not confirm notification ownership")),
            Err(error)
                if error
                    .to_string()
                    .starts_with("notification_not_delegatable:") =>
            {
                // Clockwork's basic alert is already underway or this is an older incident.
                // This frozen report is a follow-up, under its own stable send identity.
            }
            Err(_) => return Err(fail("Clockwork notification ownership is unresolved")),
        }
    }
    let letter: Letter = serde_json::from_str(payload)?;
    store.connection.execute("UPDATE exchanges SET first_send_at=COALESCE(first_send_at,?2),last_send_at=?2,send_attempts=send_attempts+1 WHERE id=?1",
        params![exchange.id,now])?;
    match Client::new(&config.email_executable)
        .send_with_options(&letter.message, &[], &letter.reply)
        .await
    {
        Ok(receipt) => {
            store.connection.execute(
                "UPDATE exchanges SET send_state='accepted',email_id=?2 WHERE id=?1",
                params![exchange.id, receipt.id],
            )?;
            Ok(())
        }
        Err(_) => Err(fail(
            "email submission is unresolved; the exact exchange remains retained",
        )),
    }
}

#[must_use]
pub fn automated(message: &ReceivedMessage) -> bool {
    message.headers.iter().any(|(name, value)| {
        (name.eq_ignore_ascii_case("auto-submitted") && !value.eq_ignore_ascii_case("no"))
            || (name.eq_ignore_ascii_case("precedence")
                && ["bulk", "list", "junk"]
                    .iter()
                    .any(|v| value.eq_ignore_ascii_case(v)))
    })
}

#[must_use]
pub fn mailbox(value: &str) -> String {
    let value = value.trim();
    value
        .split_once('<')
        .and_then(|(_, rest)| rest.split_once('>'))
        .map_or(value, |(address, _)| address)
        .trim()
        .to_ascii_lowercase()
}
