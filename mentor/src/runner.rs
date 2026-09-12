//! One short pass that stops work admission at the first failed stage.

use crate::grading::{Grader, Progress};
use crate::mail::{self, FrozenEmail};
use crate::store::{Assignment, Config, Incoming, SEND_WINDOW, Store, WORK_LIFETIME};
use crate::{Result, fail};
use email::api::{Client, ReceivedMessage, ReceivedPageRequest};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
use tokio::time::timeout;

const MAX_PAGES_PER_TICK: usize = 4;

/// Advance one bounded worker pass and report content-free stage outcomes.
///
/// # Errors
/// Returns admission, storage or content-free stage failures. Scheduled errors
/// reach Clockwork as a nonzero exit; Clockwork owns the durable scheduling halt.
pub async fn tick(root: &Path, recovery: bool) -> Result<Value> {
    let _admission = if recovery {
        crate::gate(root).recover()?
    } else {
        crate::gate(root).enter()?
    };
    let _lock = crate::store::runner_lock(root)?;
    let store = Store::open(root)?;
    store.expire(crate::now())?;
    let config = store.config()?;
    let result = advance(root, &store, &config, recovery).await;
    // Cleanup never admits a successor job or sends a message after an abend.
    store.expire(crate::now())?;
    store.set_meta("last_tick_completed_at", &crate::now().to_string())?;
    store.set_meta(
        "last_tick_errors",
        result.as_ref().err().copied().unwrap_or(""),
    )?;
    if let Err(code) = result {
        return Err(fail(code));
    }
    Ok(
        json!({"paused":config.paused,"recovery":recovery,"completed_at":crate::now(),"error_codes":[]}),
    )
}

async fn advance(
    root: &Path,
    store: &Store,
    config: &Config,
    recovery: bool,
) -> std::result::Result<(), &'static str> {
    let email = Client::new(config.email_executable.clone());
    let grader = Grader::for_current_user().map_err(|_| "grading_client_unavailable")?;
    timeout(
        Duration::from_secs(15),
        cancel_expired(root, store, &grader),
    )
    .await
    .map_err(|_| "grading_cancellation_timed_out")?
    .map_err(|_| "grading_cancellation_failed")?;

    // A hold or pause stops new admission. Existing exchanges can still finish.
    if !config.paused && !recovery {
        select_daily(store, config, crate::now()).map_err(|_| "daily_selection_failed")?;
        timeout(Duration::from_secs(20), poll(store, &email, crate::now()))
            .await
            .map_err(|_| "reply_poll_timed_out")?
            .map_err(|_| "reply_poll_failed")?;
    }
    timeout(
        Duration::from_secs(20),
        advance_grading(root, store, &grader),
    )
    .await
    .map_err(|_| "grading_timed_out")?
    .map_err(|_| "grading_failed")?;
    timeout(Duration::from_secs(20), send_pending(store, &email))
        .await
        .map_err(|_| "email_submission_unresolved")?
        .map_err(|_| "email_submission_unresolved")?;
    Ok(())
}

/// Expire local content and settle expired-job cancellation without practice work.
///
/// # Errors
/// Returns storage, locking or cancellation errors. No send or model admission occurs.
pub async fn cleanup(root: &Path) -> Result<Value> {
    let _admission = crate::gate(root).recover()?;
    let _lock = crate::store::runner_lock(root)?;
    let store = Store::open(root)?;
    store.expire(crate::now())?;
    if !store.cancellation_jobs()?.is_empty() {
        let grader = Grader::for_current_user()?;
        timeout(
            Duration::from_secs(15),
            cancel_expired(root, &store, &grader),
        )
        .await
        .map_err(|_| fail("grading_cancellation_timed_out"))??;
    }
    Ok(
        json!({"completed_at":crate::now(),"pending_cancellation_count":store.cancellation_jobs()?.len()}),
    )
}

async fn cancel_expired(root: &Path, store: &Store, grader: &Grader) -> Result<()> {
    for (incoming, job) in store.cancellation_jobs()? {
        if grader.cancel_job(&job).await? {
            store.finish_cancellation(&incoming)?;
            remove_scratch(root, &job);
        }
    }
    Ok(())
}

fn select_daily(store: &Store, config: &Config, now: i64) -> Result<()> {
    let Some(date) = config.due_date(now)? else {
        return Ok(());
    };
    if store.assigned_today(&date)? {
        return Ok(());
    }
    let (corpus_id, corpus) = store.active_corpus()?;
    let used = store.used_problem_ids()?;
    let seed = store
        .meta("shuffle_seed")?
        .ok_or_else(|| fail("problem selection seed is absent"))?;
    let problem = corpus
        .problems
        .iter()
        .filter(|problem| !used.contains(&problem.id))
        .min_by_key(|problem| {
            let mut hash = Sha256::new();
            hash.update(seed.as_bytes());
            hash.update([0]);
            hash.update(problem.id.as_bytes());
            hex::encode(hash.finalize())
        });
    let Some(problem) = problem else {
        return Ok(());
    };
    let token = crate::random_token()?;
    let assignment = Assignment {
        date,
        problem_id: problem.id.clone(),
        corpus_id,
        reply_to: format!("mentor.{token}@{}", config.receiving_domain),
        token,
    };
    let payload = serde_json::to_string(&mail::problem_email(
        &assignment,
        &problem.title,
        &problem.markdown,
    ))?;
    store.reserve_assignment(&assignment, &payload, now)
}

async fn poll(store: &Store, email: &Client, now: i64) -> Result<()> {
    // A scan covers all retained provider pages. On completion, the next tick
    // starts a new scan. Provider cursors are never used as acknowledgements.
    let mut after = store.meta("poll_after")?.filter(|s| !s.is_empty());
    let mut visited = HashSet::new();
    for _ in 0..MAX_PAGES_PER_TICK {
        let Ok(page) = email
            .list_received(&ReceivedPageRequest {
                limit: 100,
                after: after.clone(),
            })
            .await
        else {
            // An expired page cursor can be recovered by starting again.
            store.set_meta("poll_after", "")?;
            return Err(fail("Email receiving list failed"));
        };
        for reference in &page.data {
            if store.seen(&reference.id)? {
                continue;
            }
            if let Some(assignment) = match_assignment(store, reference)? {
                // Save the provider ID before moving the page cursor. Even a
                // retrieval failure is retried from the same provider page.
                let full = email
                    .get_received(&reference.id)
                    .await
                    .map_err(|_| fail("Email receiving retrieval failed"))?;
                if full.id != reference.id {
                    return Err(fail("Email returned a different received record"));
                }
                if match_assignment(store, &full)?.as_ref().map(|a| &a.date)
                    != Some(&assignment.date)
                {
                    return Err(fail("received routing metadata changed during retrieval"));
                }
                capture_reply(store, &assignment, &full, now)?;
            }
        }
        if !page.has_more {
            store.set_meta("poll_after", "")?;
            store.set_meta("last_poll_completed_at", &now.to_string())?;
            return Ok(());
        }
        let next = page
            .data
            .last()
            .ok_or_else(|| fail("Email returned an empty unfinished page"))?
            .id
            .clone();
        if after.as_ref() == Some(&next) || !visited.insert(next.clone()) {
            return Err(fail("Email receiving pagination did not advance"));
        }
        store.set_meta("poll_after", &next)?;
        after = Some(next);
    }
    Ok(())
}

fn match_assignment(store: &Store, message: &ReceivedMessage) -> Result<Option<Assignment>> {
    let mut matched: Option<Assignment> = None;
    for recipient in message.to.iter().chain(message.received_for.iter()) {
        let Some(token) = mail::assignment_token(recipient) else {
            continue;
        };
        let Some(assignment) = store.assignment(&token)? else {
            continue;
        };
        if mail::mailbox(recipient).as_deref() != Some(assignment.reply_to.as_str()) {
            continue;
        }
        if matched
            .as_ref()
            .is_some_and(|old| old.date != assignment.date)
        {
            // An ambiguous message is outside the one-problem reply protocol.
            return Ok(None);
        }
        matched = Some(assignment);
    }
    Ok(matched)
}

fn capture_reply(
    store: &Store,
    assignment: &Assignment,
    message: &ReceivedMessage,
    now: i64,
) -> Result<()> {
    let timestamp = mail::received_at(message)
        .ok()
        .filter(|value| *value <= now + 300);
    let received = timestamp.unwrap_or(now);
    let authorized_sender =
        mail::mailbox(&message.from).as_deref() == Some(email::api::recipient());
    let ignored = !authorized_sender || mail::automated(message) || timestamp.is_none();
    let expired = received + WORK_LIFETIME <= now;
    let references = mail::references(message).unwrap_or_default();
    let usable_parent = !references.is_empty();
    let extracted = if ignored || expired || !usable_parent {
        None
    } else {
        Some(mail::extract_answer(message))
    };
    let state = if ignored {
        "ignored"
    } else if expired {
        "expired"
    } else if !usable_parent {
        "failed"
    } else {
        "pending"
    };
    let incoming = Incoming {
        id: message.id.clone(),
        assignment_date: assignment.date.clone(),
        state: state.into(),
        received_at: received,
        expires_at: received + WORK_LIFETIME,
        message_id: message.message_id.clone(),
        references,
        answer: extracted
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .cloned(),
        request_json: None,
        job_id: None,
    };
    let response = if let Some(Err(extraction)) = extracted {
        let corpus = store.corpus(&assignment.corpus_id)?;
        let problem = corpus
            .problem(&assignment.problem_id)
            .ok_or_else(|| fail("assigned problem is absent"))?;
        let reply = mail::response_email(
            assignment,
            &incoming,
            &problem.title,
            &extraction.to_string(),
        );
        Some(serde_json::to_string(&reply)?)
    } else {
        None
    };
    store.capture(&incoming, now, response.as_deref())
}

async fn advance_grading(root: &Path, store: &Store, grader: &Grader) -> Result<()> {
    let pending = store.pending()?;
    // One active model attempt for this personal service. A queued item behind
    // an existing grading item is revisited after that item settles.
    let selected = pending
        .iter()
        .find(|item| item.state == "grading")
        .or_else(|| pending.first());
    let Some(incoming) = selected else {
        return Ok(());
    };
    if incoming.state != "grading" && !store.cancellation_jobs()?.is_empty() {
        return Ok(());
    }
    if incoming.expires_at <= crate::now() {
        return Ok(());
    }
    let assignment = store.assignment_date(&incoming.assignment_date)?;
    let corpus = store.corpus(&assignment.corpus_id)?;
    let problem = corpus
        .problem(&assignment.problem_id)
        .ok_or_else(|| fail("assigned problem is absent"))?;
    let job_id = incoming
        .job_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let scratch = root.join("scratch").join(&job_id);
    crate::store::private_directory(&scratch)?;
    let request = if let Some(request) = &incoming.request_json {
        request.clone()
    } else {
        let Some(answer) = &incoming.answer else {
            store.fail_incoming(&incoming.id, "answer_unavailable", false)?;
            return Err(fail("pending answer is unavailable"));
        };
        let request = grader.prepare_request(
            &job_id,
            &problem.markdown,
            &corpus.rubric.markdown,
            answer,
            &scratch,
        )?;
        store.start_grading(&incoming.id, &job_id, &request)?;
        request
    };
    match grader.advance(&request).await? {
        Progress::Pending => Ok(()),
        Progress::QuotaExhausted => {
            store.connection.execute("UPDATE incoming SET state='failed',error_code='quota_exhausted',request_json=NULL WHERE id=?1", [&incoming.id])?;
            remove_scratch(root, &job_id);
            Ok(())
        }
        Progress::Complete(critique) => {
            let payload = serde_json::to_string(&mail::response_email(
                &assignment,
                incoming,
                &problem.title,
                &critique,
            ))?;
            store.queue_response(incoming, &payload, crate::now())?;
            remove_scratch(root, &job_id);
            Ok(())
        }
        Progress::Failed(_) => {
            let body = "I could not complete this critique. Please reply with your complete answer to try again.";
            let payload = serde_json::to_string(&mail::response_email(
                &assignment,
                incoming,
                &problem.title,
                body,
            ))?;
            store.queue_response(incoming, &payload, crate::now())?;
            remove_scratch(root, &job_id);
            Err(fail("grading failed"))
        }
    }
}

async fn send_pending(store: &Store, email: &Client) -> Result<()> {
    for outgoing in store.outgoing(crate::now())? {
        let now = crate::now();
        if outgoing.expires_at <= now
            || outgoing
                .first_attempt
                .is_some_and(|first| first + SEND_WINDOW <= now)
        {
            continue;
        }
        let frozen: FrozenEmail = serde_json::from_str(&outgoing.payload)
            .map_err(|_| fail("pending mail payload is invalid"))?;
        store.begin_send(&outgoing.id, now)?;
        if let Ok(receipt) = email
            .send_with_options(&frozen.message, &[], &frozen.reply)
            .await
        {
            store.sent(&outgoing, &receipt.id, crate::now())?;
        } else {
            store.unresolved_send(&outgoing.id)?;
            return Err(fail("email submission is unresolved"));
        }
    }
    Ok(())
}

fn remove_scratch(root: &Path, job_id: &str) {
    if uuid::Uuid::parse_str(job_id).is_ok() {
        // The grader has no filesystem access, so this is an empty directory.
        let _ = std::fs::remove_dir(root.join("scratch").join(job_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    fn private_temp() -> Result<tempfile::TempDir> {
        let temporary = tempfile::tempdir()?;
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))?;
        Ok(temporary)
    }

    fn queued_mail(store: &Store, id: &str, now: i64) -> Result<String> {
        let payload = serde_json::to_string(&FrozenEmail {
            message: email::api::Message {
                subject: "Fixture".into(),
                body: "Synthetic fixture body".into(),
                idempotency_key: Some(format!("mentor/test/{id}")),
            },
            reply: email::api::ReplyOptions::default(),
        })?;
        store.connection.execute(
            "INSERT INTO outbox(id,payload,state,created_at,expires_at,next_attempt) VALUES(?1,?2,'pending',?3,?4,?3)",
            rusqlite::params![id, payload, now, now + WORK_LIFETIME],
        )?;
        Ok(payload)
    }

    fn failing_email(root: &Path) -> Result<std::path::PathBuf> {
        let executable = root.join("fake-email");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$0.calls\"\n/bin/cat >/dev/null\nexit 1\n",
        )?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        Ok(executable)
    }

    #[tokio::test]
    async fn unresolved_send_stops_before_the_next_message_and_keeps_exact_payload() -> Result<()> {
        let temporary = private_temp()?;
        let store = Store::initialize(temporary.path())?;
        let now = crate::now();
        let payload = queued_mail(&store, "first", now - 1)?;
        queued_mail(&store, "second", now)?;
        let executable = failing_email(temporary.path())?;
        assert!(
            send_pending(&store, &Client::new(&executable))
                .await
                .is_err()
        );
        let queued = store.outgoing(crate::now())?;
        assert_eq!(queued.len(), 2);
        assert_eq!(queued[0].payload, payload);
        assert_eq!(queued[0].attempts, 1);
        assert!(queued[0].first_attempt.is_some());
        assert_eq!(queued[1].attempts, 0);
        assert_eq!(
            std::fs::read_to_string(executable.with_extension("calls"))?,
            "--payload-stdin\n"
        );
        store.expire(crate::now() + SEND_WINDOW)?;
        let state: String =
            store
                .connection
                .query_row("SELECT state FROM outbox WHERE id='first'", [], |row| {
                    row.get(0)
                })?;
        assert_eq!(state, "unknown");
        Ok(())
    }

    #[tokio::test]
    async fn polling_failure_returns_an_error_before_any_pending_mail_is_sent() -> Result<()> {
        let temporary = private_temp()?;
        let store = Store::initialize(temporary.path())?;
        let mut config = store.config()?;
        config.paused = false;
        config.first_delivery_date = Some("2999-01-01".into());
        config.email_executable = failing_email(temporary.path())?;
        store.set_config(&config)?;
        queued_mail(&store, "pending", crate::now())?;
        assert_eq!(
            tick(temporary.path(), false)
                .await
                .err()
                .map(|e| e.to_string())
                .as_deref(),
            Some("reply_poll_failed")
        );
        assert_eq!(store.outgoing(crate::now())?[0].attempts, 0);
        assert_eq!(
            store.meta("last_tick_errors")?.as_deref(),
            Some("reply_poll_failed")
        );
        assert_eq!(
            std::fs::read_to_string(config.email_executable.with_extension("calls"))?,
            "receive\n"
        );
        Ok(())
    }

    #[tokio::test]
    async fn cleanup_expires_content_without_sending_pending_mail() -> Result<()> {
        let temporary = private_temp()?;
        let store = Store::initialize(temporary.path())?;
        queued_mail(&store, "expired", crate::now() - WORK_LIFETIME)?;
        queued_mail(&store, "pending", crate::now())?;
        cleanup(temporary.path()).await?;
        assert_eq!(store.outgoing(crate::now())?.len(), 1);
        assert_eq!(store.outgoing(crate::now())?[0].attempts, 0);
        let content: Option<String> = store.connection.query_row(
            "SELECT payload FROM outbox WHERE id='expired'",
            [],
            |row| row.get(0),
        )?;
        assert!(content.is_none());
        Ok(())
    }
}
