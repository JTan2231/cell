use std::collections::{BTreeMap, BTreeSet};

use conversations::{
    AppServerClient, ClientConfig, ListOptions, Message, Role, StderrPolicy, ThreadSummary,
    TimestampPrecision as ConversationPrecision, TurnActivity,
};
use sha2::{Digest as _, Sha256};

use crate::error::{AppError, AppResult};
use crate::model::{MessageRole, Precision, SourceMessage, ThreadTranscript};
use crate::store::Store;

const MAX_CLASSIFICATION_MESSAGES: usize = 64;
const MAX_CLASSIFICATION_TEXT_BYTES: usize = 262_144;

#[derive(Debug)]
pub(crate) struct ObservationSource {
    pub(crate) transcript: ThreadTranscript,
    pub(crate) authorities: Vec<SourceMessage>,
    pub(crate) source_digest: String,
    pub(crate) source_completed_at: i64,
}

#[derive(Debug)]
pub(crate) enum ObservationLoad {
    Eligible(ObservationSource),
    NotEligible {
        host_id: String,
        thread_id: String,
        authority_occurred_at: Option<i64>,
        source_completed_at: i64,
    },
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct ReconcileResult {
    pub(crate) threads_scanned: usize,
    pub(crate) activities_scanned: usize,
    pub(crate) observations_enqueued: usize,
}

pub(crate) fn load_observation(
    session_hint: &str,
    exact_thread_id: Option<&str>,
    turn_id: &str,
    scope_level: i64,
    baseline: i64,
) -> AppResult<ObservationLoad> {
    let mut client = source_client()?;
    let activity = if let Some(thread_id) = exact_thread_id {
        client.read_turn_activity(thread_id, turn_id)
    } else {
        client.resolve_turn_activity(session_hint, turn_id)
    }
    .map_err(observation_source_error)?;
    observation_from_activity(&mut client, activity, scope_level, baseline)
}

pub(crate) fn reconcile_window(
    store: &mut Store,
    baseline: i64,
    window_start: i64,
    window_end: i64,
    completed_cutoff: i64,
) -> AppResult<ReconcileResult> {
    let coverage_start = window_start.max(baseline);
    if coverage_start >= window_end {
        return Ok(ReconcileResult {
            threads_scanned: 0,
            activities_scanned: 0,
            observations_enqueued: 0,
        });
    }
    let mut client = source_client()?;
    let summaries = client
        .list(&ListOptions {
            use_state_db_only: false,
            ..ListOptions::default()
        })
        .map_err(source_error)?;
    let mut activities_scanned = 0_usize;
    let mut observations_enqueued = 0_usize;
    let candidates = reconciliation_summaries(&summaries, coverage_start);
    let mut canonical_activities =
        BTreeMap::<(String, String), (&ThreadSummary, TurnActivity)>::new();
    for summary in &candidates {
        let activities = client
            .read_completed_turn_activities(summary)
            .map_err(source_error)?;
        for activity in activities {
            activities_scanned += 1;
            if activity
                .turn
                .completed_at
                .is_none_or(|completed_at| completed_at > completed_cutoff)
            {
                continue;
            }
            retain_canonical_activity(&mut canonical_activities, summary, activity);
        }
    }
    for (_key, (summary, activity)) in canonical_activities {
        let user_messages = activity
            .turn
            .messages
            .iter()
            .filter(|message| message.role == Role::User && !message.text.trim().is_empty())
            .collect::<Vec<_>>();
        if user_messages.is_empty() {
            continue;
        }
        let has_unknown_user_time = user_messages.iter().any(|message| {
            message.timestamp.is_none()
                || message.timestamp_precision == ConversationPrecision::Unknown
        });
        let has_authority_in_window = user_messages.iter().any(|message| {
            message.timestamp.is_some_and(|occurred_at| {
                occurred_at >= coverage_start && occurred_at < window_end
            })
        });
        let unknown_may_overlap = has_unknown_user_time
            && activity
                .turn
                .started_at
                .is_none_or(|started_at| started_at < window_end)
            && activity
                .turn
                .completed_at
                .is_none_or(|completed_at| completed_at >= coverage_start);
        if !has_authority_in_window && !unknown_may_overlap {
            continue;
        }
        let completed_at = activity.turn.completed_at.ok_or_else(|| {
            AppError::new(
                "conversation_source_failed",
                "a reconciled completed turn has no completion timestamp",
            )
        })?;
        let (_observation, inserted) = store.ingest_reconciled_observation(
            &summary.reference.host_id,
            &summary.reference.thread_id,
            &activity.turn.reference.turn_id,
            completed_at,
        )?;
        observations_enqueued += usize::from(inserted);
    }
    Ok(ReconcileResult {
        threads_scanned: candidates.len(),
        activities_scanned,
        observations_enqueued,
    })
}

fn source_client() -> AppResult<AppServerClient> {
    AppServerClient::spawn(ClientConfig {
        stderr_policy: StderrPolicy::Suppress,
        ..ClientConfig::default()
    })
    .map_err(source_error)
}

#[allow(clippy::too_many_lines)]
fn observation_from_activity(
    client: &mut AppServerClient,
    activity: TurnActivity,
    scope_level: i64,
    baseline: i64,
) -> AppResult<ObservationLoad> {
    let source_completed_at = activity.turn.completed_at.ok_or_else(|| {
        AppError::new(
            "conversation_source_failed",
            "a completed turn has no authoritative completion timestamp",
        )
    })?;
    let host_id = activity.turn.reference.host_id.clone();
    let thread_id = activity.turn.reference.thread_id.clone();
    let target_users = activity
        .turn
        .messages
        .iter()
        .filter(|message| message.role == Role::User && !message.text.trim().is_empty())
        .collect::<Vec<_>>();
    let latest_known_authority = target_users
        .iter()
        .filter_map(|message| message.timestamp)
        .max();
    if target_users.is_empty() {
        return Ok(ObservationLoad::NotEligible {
            host_id,
            thread_id,
            authority_occurred_at: latest_known_authority,
            source_completed_at,
        });
    }
    let conversation = client
        .read_thread(&activity.turn.reference.thread_id)
        .map_err(source_error)?;
    if !summary_is_root_interactive(&conversation.thread) {
        return Ok(ObservationLoad::NotEligible {
            host_id,
            thread_id,
            authority_occurred_at: latest_known_authority,
            source_completed_at,
        });
    }
    if target_users.iter().any(|message| {
        message.timestamp.is_none() || message.timestamp_precision == ConversationPrecision::Unknown
    }) {
        return Err(AppError::new(
            "source_timestamp_incomplete",
            format!(
                "completed turn {} contains a user message without an authoritative item or turn timestamp",
                activity.turn.reference.turn_id
            ),
        ));
    }
    let authorities = target_users
        .into_iter()
        .filter(|message| message.timestamp.is_some_and(|time| time >= baseline))
        .map(normalize_message)
        .collect::<AppResult<Vec<_>>>()?;
    if authorities.is_empty() {
        return Ok(ObservationLoad::NotEligible {
            host_id,
            thread_id,
            authority_occurred_at: latest_known_authority,
            source_completed_at,
        });
    }
    let target_index = conversation
        .turns
        .iter()
        .position(|turn| turn.reference.turn_id == activity.turn.reference.turn_id)
        .ok_or_else(|| {
            AppError::new(
                "conversation_source_failed",
                "the completed authority turn disappeared while its context was read",
            )
        })?;
    verify_authorities(&conversation.turns[target_index].messages, &authorities)?;
    let messages = if scope_level == 0 {
        bounded_observation_messages(&conversation.turns, target_index, &activity, &authorities)?
    } else if scope_level == 1 {
        let prefix = conversation.turns[..=target_index]
            .iter()
            .flat_map(|turn| turn.messages.iter())
            .map(normalize_message)
            .collect::<AppResult<Vec<_>>>()?;
        bounded_expanded_messages(
            prefix,
            &bounded_observation_messages(
                &conversation.turns,
                target_index,
                &activity,
                &authorities,
            )?,
        )?
    } else {
        return Err(AppError::new(
            "observation_scope_invalid",
            "observation scope must be level 0 or level 1",
        ));
    };
    let source_digest = observation_source_digest(
        &activity,
        &authorities,
        &conversation.turns[..=target_index],
    );
    Ok(ObservationLoad::Eligible(ObservationSource {
        transcript: ThreadTranscript {
            host_id: activity.turn.reference.host_id,
            thread_id: activity.turn.reference.thread_id,
            messages,
        },
        authorities,
        source_digest,
        source_completed_at,
    }))
}

fn bounded_observation_messages(
    turns: &[conversations::Turn],
    target_index: usize,
    activity: &TurnActivity,
    authorities: &[SourceMessage],
) -> AppResult<Vec<SourceMessage>> {
    let prefix = turns[..=target_index]
        .iter()
        .flat_map(|turn| turn.messages.iter())
        .collect::<Vec<_>>();
    let authority_by_id = authorities
        .iter()
        .map(|authority| (authority.item_id.as_str(), authority))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut selected_context_ids = std::collections::BTreeSet::new();
    for authority in authorities {
        let Some(index) = prefix
            .iter()
            .position(|message| message.reference.item_id == authority.item_id)
        else {
            return Err(AppError::new(
                "conversation_source_failed",
                "an authority item disappeared while bounded context was selected",
            ));
        };
        if let Some(preceding) = prefix[..index]
            .iter()
            .rev()
            .find(|message| message.role == Role::Assistant && !message.text.trim().is_empty())
        {
            selected_context_ids.insert(preceding.reference.item_id.as_str());
        }
    }
    let result = activity
        .turn
        .messages
        .iter()
        .rev()
        .find(|message| message.role == Role::Assistant && !message.text.trim().is_empty());
    if let Some(result) = result {
        let verified = prefix
            .iter()
            .find(|message| message.reference.item_id == result.reference.item_id)
            .ok_or_else(|| {
                AppError::new(
                    "conversation_source_incomplete",
                    "the completed-turn result disappeared while its context was read",
                )
            })?;
        if !same_source_message(&normalize_message(result)?, &normalize_message(verified)?) {
            return Err(AppError::new(
                "conversation_source_incomplete",
                "the completed-turn result changed while its context was read",
            ));
        }
        selected_context_ids.insert(verified.reference.item_id.as_str());
    }
    let messages = prefix
        .into_iter()
        .filter(|message| {
            authority_by_id.contains_key(message.reference.item_id.as_str())
                || selected_context_ids.contains(message.reference.item_id.as_str())
        })
        .map(|message| {
            authority_by_id
                .get(message.reference.item_id.as_str())
                .map_or_else(
                    || normalize_message(message),
                    |authority| Ok((*authority).clone()),
                )
        })
        .collect::<AppResult<Vec<_>>>()?;
    ensure_classification_bounds(&messages, "mandatory level-zero scope")?;
    Ok(messages)
}

fn same_source_message(left: &SourceMessage, right: &SourceMessage) -> bool {
    left.host_id == right.host_id
        && left.thread_id == right.thread_id
        && left.turn_id == right.turn_id
        && left.item_id == right.item_id
        && left.role == right.role
        && left.text == right.text
        && left.occurred_at == right.occurred_at
        && left.precision == right.precision
}

fn verify_authorities(messages: &[Message], authorities: &[SourceMessage]) -> AppResult<()> {
    for authority in authorities {
        let source = messages
            .iter()
            .find(|message| message.reference.item_id == authority.item_id)
            .ok_or_else(|| {
                AppError::new(
                    "conversation_source_incomplete",
                    "an authority item disappeared while its source was verified",
                )
            })?;
        let normalized = normalize_message(source)?;
        if !same_source_message(&normalized, authority) {
            return Err(AppError::new(
                "conversation_source_incomplete",
                "completed-turn authority changed while its context was read",
            ));
        }
    }
    Ok(())
}

fn bounded_expanded_messages(
    prefix: Vec<SourceMessage>,
    mandatory: &[SourceMessage],
) -> AppResult<Vec<SourceMessage>> {
    ensure_classification_bounds(mandatory, "mandatory level-zero scope")?;
    let mandatory_ids = mandatory
        .iter()
        .map(|message| message.item_id.clone())
        .collect::<BTreeSet<_>>();
    let mut selected_ids = mandatory_ids.clone();
    let mut count = mandatory.len();
    let mut bytes = mandatory
        .iter()
        .map(|message| message.text.len())
        .sum::<usize>();
    for message in prefix.iter().rev() {
        if selected_ids.contains(&message.item_id) {
            continue;
        }
        if count == MAX_CLASSIFICATION_MESSAGES
            || bytes
                .checked_add(message.text.len())
                .is_none_or(|next| next > MAX_CLASSIFICATION_TEXT_BYTES)
        {
            break;
        }
        selected_ids.insert(message.item_id.clone());
        count += 1;
        bytes += message.text.len();
    }
    let messages = prefix
        .into_iter()
        .filter(|message| selected_ids.contains(&message.item_id))
        .collect::<Vec<_>>();
    ensure_classification_bounds(&messages, "expanded level-one scope")?;
    Ok(messages)
}

fn ensure_classification_bounds(messages: &[SourceMessage], scope: &str) -> AppResult<()> {
    let bytes = messages.iter().try_fold(0_usize, |total, message| {
        total.checked_add(message.text.len()).ok_or_else(|| {
            AppError::new(
                "classification_scope_too_large",
                format!("{scope} text size exceeds the supported range"),
            )
        })
    })?;
    if messages.len() > MAX_CLASSIFICATION_MESSAGES || bytes > MAX_CLASSIFICATION_TEXT_BYTES {
        return Err(AppError::new(
            "classification_scope_too_large",
            format!(
                "{scope} exceeds {MAX_CLASSIFICATION_MESSAGES} messages or {MAX_CLASSIFICATION_TEXT_BYTES} UTF-8 bytes"
            ),
        ));
    }
    Ok(())
}

fn normalize_message(message: &Message) -> AppResult<SourceMessage> {
    let occurred_at = message.timestamp.ok_or_else(|| {
        AppError::new(
            "source_timestamp_incomplete",
            format!(
                "message {} has no authoritative timestamp",
                message.reference.item_id
            ),
        )
    })?;
    let precision = match message.timestamp_precision {
        ConversationPrecision::Item => Precision::Item,
        ConversationPrecision::Turn => Precision::Turn,
        ConversationPrecision::Unknown => {
            return Err(AppError::new(
                "source_timestamp_incomplete",
                format!(
                    "message {} has unknown precision",
                    message.reference.item_id
                ),
            ));
        }
    };
    Ok(SourceMessage {
        host_id: message.reference.host_id.clone(),
        thread_id: message.reference.thread_id.clone(),
        turn_id: message.reference.turn_id.clone(),
        item_id: message.reference.item_id.clone(),
        role: match message.role {
            Role::User => MessageRole::User,
            Role::Assistant => MessageRole::Assistant,
        },
        text: message.text.clone(),
        occurred_at,
        precision,
    })
}

fn observation_source_digest(
    activity: &TurnActivity,
    authorities: &[SourceMessage],
    prefix: &[conversations::Turn],
) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        b"host",
        activity.turn.reference.host_id.as_bytes(),
    );
    hash_field(
        &mut hasher,
        b"thread",
        activity.turn.reference.thread_id.as_bytes(),
    );
    hash_field(
        &mut hasher,
        b"turn",
        activity.turn.reference.turn_id.as_bytes(),
    );
    hash_field(
        &mut hasher,
        b"completed-at",
        &activity.turn.completed_at.unwrap_or_default().to_le_bytes(),
    );
    for authority in authorities {
        hash_field(&mut hasher, b"authority-item", authority.item_id.as_bytes());
        hash_field(
            &mut hasher,
            b"authority-time",
            &authority.occurred_at.to_le_bytes(),
        );
        hash_field(&mut hasher, b"authority-text", authority.text.as_bytes());
    }
    for turn in prefix {
        hash_field(
            &mut hasher,
            b"prefix-turn",
            turn.reference.turn_id.as_bytes(),
        );
        for message in &turn.messages {
            hash_field(
                &mut hasher,
                b"prefix-item",
                message.reference.item_id.as_bytes(),
            );
            hash_field(
                &mut hasher,
                b"prefix-role",
                match message.role {
                    Role::User => b"user",
                    Role::Assistant => b"assistant",
                },
            );
            hash_field(
                &mut hasher,
                b"prefix-time",
                &message.timestamp.unwrap_or_default().to_le_bytes(),
            );
            hash_field(
                &mut hasher,
                b"prefix-time-known",
                &[u8::from(message.timestamp.is_some())],
            );
            hash_field(
                &mut hasher,
                b"prefix-precision",
                match message.timestamp_precision {
                    ConversationPrecision::Item => b"item",
                    ConversationPrecision::Turn => b"turn",
                    ConversationPrecision::Unknown => b"unknown",
                },
            );
            hash_field(&mut hasher, b"prefix-text", message.text.as_bytes());
        }
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
fn canonicalize_transcripts(transcripts: &mut [ThreadTranscript]) {
    transcripts.sort_by(|left, right| {
        (&left.host_id, &left.thread_id).cmp(&(&right.host_id, &right.thread_id))
    });
}

#[cfg(test)]
fn source_manifest_hash(transcripts: &[ThreadTranscript]) -> String {
    let mut hasher = Sha256::new();
    for transcript in transcripts {
        hasher.update(b"transcript;");
        hash_field(&mut hasher, b"host", transcript.host_id.as_bytes());
        hash_field(&mut hasher, b"thread", transcript.thread_id.as_bytes());
        for message in &transcript.messages {
            hasher.update(b"message;");
            hash_field(&mut hasher, b"host", message.host_id.as_bytes());
            hash_field(&mut hasher, b"thread", message.thread_id.as_bytes());
            hash_field(&mut hasher, b"turn", message.turn_id.as_bytes());
            hash_field(&mut hasher, b"item", message.item_id.as_bytes());
            hash_field(&mut hasher, b"role", message.role.as_str().as_bytes());
            hash_field(
                &mut hasher,
                b"occurred-at",
                &message.occurred_at.to_le_bytes(),
            );
            hash_field(
                &mut hasher,
                b"precision",
                message.precision.as_str().as_bytes(),
            );
            hash_field(&mut hasher, b"text", message.text.as_bytes());
        }
    }
    format!("{:x}", hasher.finalize())
}

fn hash_field(hasher: &mut Sha256, label: &[u8], value: &[u8]) {
    hasher.update(label);
    hasher.update(b"=");
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b":");
    hasher.update(value);
    hasher.update(b";");
}

#[cfg(test)]
fn normalize_selected_thread(
    messages: Vec<Message>,
    thread_id: &str,
    window_start: i64,
    window_end: i64,
) -> AppResult<Option<Vec<SourceMessage>>> {
    if messages.iter().any(|message| {
        message.timestamp.is_none() || message.timestamp_precision == ConversationPrecision::Unknown
    }) {
        return Err(AppError::new(
            "source_timestamp_incomplete",
            format!(
                "root thread {thread_id} overlaps the report window but contains a user or assistant message without an authoritative item or turn timestamp"
            ),
        ));
    }
    let has_user_authority = messages.iter().any(|message| {
        message.role == Role::User
            && message
                .timestamp
                .is_some_and(|time| time >= window_start && time < window_end)
    });
    if !has_user_authority {
        return Ok(None);
    }
    messages
        .into_iter()
        .filter(|message| message.timestamp.is_some_and(|time| time < window_end))
        .map(|message| {
            let occurred_at = message.timestamp.ok_or_else(|| {
                AppError::new(
                    "source_timestamp_incomplete",
                    format!("message {} lost its timestamp", message.reference.item_id),
                )
            })?;
            let precision = match message.timestamp_precision {
                ConversationPrecision::Item => Precision::Item,
                ConversationPrecision::Turn => Precision::Turn,
                ConversationPrecision::Unknown => {
                    return Err(AppError::new(
                        "source_timestamp_incomplete",
                        format!(
                            "message {} has unknown precision",
                            message.reference.item_id
                        ),
                    ));
                }
            };
            Ok(SourceMessage {
                host_id: message.reference.host_id,
                thread_id: message.reference.thread_id,
                turn_id: message.reference.turn_id,
                item_id: message.reference.item_id,
                role: match message.role {
                    Role::User => MessageRole::User,
                    Role::Assistant => MessageRole::Assistant,
                },
                text: message.text,
                occurred_at,
                precision,
            })
        })
        .collect::<AppResult<Vec<_>>>()
        .map(Some)
}

#[cfg(test)]
fn summary_may_overlap(summary: &ThreadSummary, window_start: i64, window_end: i64) -> bool {
    if !summary_is_root_interactive(summary) {
        return false;
    }
    summary.created_at.is_none_or(|time| time < window_end)
        && summary.updated_at.is_none_or(|time| time >= window_start)
}

fn summary_is_root_interactive(summary: &ThreadSummary) -> bool {
    summary.parent_thread_id.is_none()
        && summary.source_kind != "exec"
        && !summary.source_kind.starts_with("subAgent")
}

fn reconciliation_summaries(
    summaries: &[ThreadSummary],
    coverage_start: i64,
) -> Vec<&ThreadSummary> {
    let by_id = summaries
        .iter()
        .map(|summary| (summary.reference.thread_id.as_str(), summary))
        .collect::<BTreeMap<_, _>>();
    let mut selected = summaries
        .iter()
        .filter(|summary| {
            summary_is_root_interactive(summary)
                && summary_may_contain_completion(summary, coverage_start)
        })
        .map(|summary| summary.reference.thread_id.as_str())
        .collect::<BTreeSet<_>>();

    // A recently updated fork can contain copied ancestor turns even when its
    // ancestor's summary is known-stale. Read that lineage too so the copied
    // turn is deterministically owned by the earliest visible ancestor.
    loop {
        let ancestors = selected
            .iter()
            .filter_map(|thread_id| by_id.get(thread_id))
            .filter_map(|summary| summary.forked_from_id.as_deref())
            .filter(|ancestor| by_id.contains_key(ancestor))
            .collect::<Vec<_>>();
        let mut changed = false;
        for ancestor in ancestors {
            changed |= selected.insert(ancestor);
        }
        if !changed {
            break;
        }
    }
    selected
        .into_iter()
        .filter_map(|thread_id| by_id.get(thread_id).copied())
        .collect()
}

fn canonical_owner_rank(summary: &ThreadSummary) -> (bool, &str) {
    (
        summary.forked_from_id.is_some(),
        summary.reference.thread_id.as_str(),
    )
}

fn retain_canonical_activity<'a>(
    activities: &mut BTreeMap<(String, String), (&'a ThreadSummary, TurnActivity)>,
    summary: &'a ThreadSummary,
    activity: TurnActivity,
) {
    let key = (
        activity.turn.reference.host_id.clone(),
        activity.turn.reference.turn_id.clone(),
    );
    let replace = activities
        .get(&key)
        .is_none_or(|(owner, _)| canonical_owner_rank(summary) < canonical_owner_rank(owner));
    if replace {
        activities.insert(key, (summary, activity));
    }
}

fn summary_may_contain_completion(summary: &ThreadSummary, coverage_start: i64) -> bool {
    summary
        .updated_at
        .or(summary.created_at)
        .is_none_or(|updated_at| updated_at >= coverage_start)
}

#[allow(clippy::needless_pass_by_value)]
fn source_error(_error: conversations::Error) -> AppError {
    AppError::new(
        "conversation_source_failed",
        "unable to read the complete Codex conversation source; inspect Conversations diagnostics",
    )
}

fn observation_source_error(error: conversations::Error) -> AppError {
    match error {
        conversations::Error::TurnNotCompleted { .. } => AppError::new(
            "conversation_source_not_completed",
            "the Stop-hook turn is not yet durably complete; it remains queued",
        ),
        conversations::Error::TurnNotFound { .. } => AppError::new(
            "conversation_source_pending",
            "the Stop-hook turn is not yet visible in the complete conversation source",
        ),
        error => source_error(error),
    }
}

#[cfg(test)]
mod tests {
    use conversations::{
        CompletedFileChange, ItemRef, Message, Role, ThreadRef, ThreadSummary, TimestampPrecision,
        Turn, TurnActivity, TurnRef,
    };

    use crate::model::{MessageRole, Precision, SourceMessage, ThreadTranscript};

    use super::{
        bounded_expanded_messages, bounded_observation_messages, canonicalize_transcripts,
        normalize_message, normalize_selected_thread, observation_source_error,
        reconciliation_summaries, retain_canonical_activity, source_manifest_hash,
        summary_may_contain_completion, summary_may_overlap,
    };

    fn summary(created_at: Option<i64>, updated_at: Option<i64>) -> ThreadSummary {
        ThreadSummary {
            reference: ThreadRef {
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
            },
            session_id: None,
            name: None,
            preview: String::new(),
            cwd: None,
            source_kind: "appServer".to_owned(),
            parent_thread_id: None,
            forked_from_id: None,
            cli_version: None,
            archived: false,
            ephemeral: false,
            created_at,
            updated_at,
            runtime_status: "notLoaded".to_owned(),
        }
    }

    #[test]
    fn repaired_summary_metadata_prunes_obviously_disjoint_threads() {
        assert!(!summary_may_overlap(&summary(Some(0), Some(9)), 10, 20));
        assert!(!summary_may_overlap(&summary(Some(20), Some(30)), 10, 20));
        assert!(summary_may_overlap(&summary(Some(0), Some(10)), 10, 20));
        assert!(summary_may_overlap(&summary(None, None), 10, 20));
        assert!(!summary_may_contain_completion(
            &summary(Some(0), Some(9)),
            10
        ));
        assert!(summary_may_contain_completion(&summary(None, None), 10));
    }

    #[test]
    fn reconciliation_keeps_stale_ancestors_of_recent_forks() {
        let mut root = summary(Some(0), Some(9));
        root.reference.thread_id = "root".to_owned();
        let mut fork = summary(Some(0), Some(20));
        fork.reference.thread_id = "fork".to_owned();
        fork.forked_from_id = Some("root".to_owned());
        let summaries = [root, fork];
        let selected = reconciliation_summaries(&summaries, 10);
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn excludes_noninteractive_threads() {
        let mut value = summary(Some(0), Some(10));
        value.source_kind = "exec".to_owned();
        assert!(!summary_may_overlap(&value, 10, 20));
        value.source_kind = "appServer".to_owned();
        value.parent_thread_id = Some("parent".to_owned());
        assert!(!summary_may_overlap(&value, 10, 20));
    }

    fn message(index: i64, role: Role, timestamp: Option<i64>) -> Message {
        Message {
            reference: ItemRef {
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
                turn_id: "turn".to_owned(),
                item_id: format!("item-{index}"),
            },
            role,
            text: format!("message {index}"),
            timestamp,
            timestamp_precision: timestamp
                .map_or(TimestampPrecision::Unknown, |_| TimestampPrecision::Item),
        }
    }

    fn activity(thread_id: &str, turn_id: &str) -> TurnActivity {
        TurnActivity {
            turn: Turn {
                reference: TurnRef {
                    host_id: "host".to_owned(),
                    thread_id: thread_id.to_owned(),
                    turn_id: turn_id.to_owned(),
                },
                started_at: Some(1),
                completed_at: Some(2),
                status: "completed".to_owned(),
                messages: Vec::new(),
            },
            completed_file_changes: vec![CompletedFileChange {
                reference: ItemRef {
                    host_id: "host".to_owned(),
                    thread_id: thread_id.to_owned(),
                    turn_id: turn_id.to_owned(),
                    item_id: format!("change-{turn_id}"),
                },
                change_count: 1,
            }],
        }
    }

    #[test]
    fn copied_fork_turn_has_one_owner_but_fork_only_turn_is_retained() {
        let mut root = summary(Some(0), Some(10));
        root.reference.thread_id = "root".to_owned();
        let mut fork = summary(Some(0), Some(20));
        fork.reference.thread_id = "fork".to_owned();
        fork.forked_from_id = Some("root".to_owned());
        let mut selected = std::collections::BTreeMap::new();
        retain_canonical_activity(&mut selected, &fork, activity("fork", "copied"));
        retain_canonical_activity(&mut selected, &root, activity("root", "copied"));
        retain_canonical_activity(&mut selected, &fork, activity("fork", "fork-only"));
        assert_eq!(selected.len(), 2);
        assert_eq!(
            selected
                .get(&("host".to_owned(), "copied".to_owned()))
                .map(|(owner, _)| owner.reference.thread_id.as_str()),
            Some("root")
        );
        assert_eq!(
            selected
                .get(&("host".to_owned(), "fork-only".to_owned()))
                .map(|(owner, _)| owner.reference.thread_id.as_str()),
            Some("fork")
        );
    }

    #[test]
    fn keeps_all_pre_window_context_for_deictic_authority() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut messages = (0..8)
            .map(|index| message(index, Role::Assistant, Some(index)))
            .collect::<Vec<_>>();
        messages.push(message(8, Role::User, Some(10)));
        let normalized = normalize_selected_thread(messages, "thread", 10, 20)?
            .ok_or("expected selected thread")?;
        assert_eq!(normalized.len(), 9);
        assert_eq!(normalized[0].item_id, "item-0");
        Ok(())
    }

    #[test]
    fn fails_when_assistant_context_has_no_timestamp() {
        let messages = vec![
            message(0, Role::Assistant, None),
            message(1, Role::User, Some(10)),
        ];
        let error = normalize_selected_thread(messages, "thread", 10, 20)
            .err()
            .map(|error| error.code);
        assert_eq!(error, Some("source_timestamp_incomplete"));
    }

    #[test]
    fn level_zero_omits_hundreds_of_intermediate_assistant_messages()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut proposal = message(0, Role::Assistant, Some(1));
        proposal.reference.turn_id = "previous".to_owned();
        proposal.reference.item_id = "proposal".to_owned();
        let previous = Turn {
            reference: TurnRef {
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
                turn_id: "previous".to_owned(),
            },
            started_at: Some(1),
            completed_at: Some(1),
            status: "completed".to_owned(),
            messages: vec![proposal],
        };
        let mut first_user = message(1, Role::User, Some(10));
        first_user.reference.item_id = "user-1".to_owned();
        let mut target_messages = vec![first_user];
        for index in 0..300 {
            let mut assistant = message(100 + index, Role::Assistant, Some(11 + index));
            assistant.reference.item_id = format!("assistant-{index}");
            target_messages.push(assistant);
        }
        let mut second_user = message(500, Role::User, Some(400));
        second_user.reference.item_id = "user-2".to_owned();
        target_messages.push(second_user);
        let mut result = message(501, Role::Assistant, Some(401));
        result.reference.item_id = "result".to_owned();
        target_messages.push(result);
        let target = Turn {
            reference: TurnRef {
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
                turn_id: "turn".to_owned(),
            },
            started_at: Some(10),
            completed_at: Some(402),
            status: "completed".to_owned(),
            messages: target_messages,
        };
        let activity = TurnActivity {
            turn: target.clone(),
            completed_file_changes: vec![CompletedFileChange {
                reference: ItemRef {
                    host_id: "host".to_owned(),
                    thread_id: "thread".to_owned(),
                    turn_id: "turn".to_owned(),
                    item_id: "change".to_owned(),
                },
                change_count: 1,
            }],
        };
        let authorities = [
            normalize_message(&target.messages[0])?,
            normalize_message(&target.messages[301])?,
        ];
        let selected =
            bounded_observation_messages(&[previous, target], 1, &activity, &authorities)?;
        assert_eq!(
            selected
                .iter()
                .filter(|message| message.role == crate::model::MessageRole::User)
                .count(),
            2
        );
        assert_eq!(
            selected
                .iter()
                .filter(|message| message.role == crate::model::MessageRole::Assistant)
                .count(),
            3
        );
        assert!(selected.iter().any(|message| message.item_id == "proposal"));
        assert!(
            selected
                .iter()
                .any(|message| message.item_id == "assistant-299")
        );
        assert!(selected.iter().any(|message| message.item_id == "result"));
        assert!(
            !selected
                .iter()
                .any(|message| message.item_id == "assistant-0")
        );
        Ok(())
    }

    #[test]
    fn level_zero_rejects_an_unverified_activity_result() -> Result<(), Box<dyn std::error::Error>>
    {
        let authority = message(1, Role::User, Some(10));
        let read_turn = Turn {
            reference: TurnRef {
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
                turn_id: "turn".to_owned(),
            },
            started_at: Some(10),
            completed_at: Some(12),
            status: "completed".to_owned(),
            messages: vec![authority.clone()],
        };
        let mut completed_activity = activity("thread", "turn");
        completed_activity.turn.messages =
            vec![authority.clone(), message(2, Role::Assistant, Some(11))];
        let authorities = [normalize_message(&authority)?];

        let error =
            bounded_observation_messages(&[read_turn], 0, &completed_activity, &authorities)
                .err()
                .ok_or("expected an incomplete source")?;
        assert_eq!(error.code, "conversation_source_incomplete");
        Ok(())
    }

    #[test]
    fn canonical_thread_order_makes_manifest_invariant() {
        let transcripts = [
            ThreadTranscript {
                host_id: "host-b".to_owned(),
                thread_id: "thread-a".to_owned(),
                messages: Vec::new(),
            },
            ThreadTranscript {
                host_id: "host-a".to_owned(),
                thread_id: "thread-z".to_owned(),
                messages: Vec::new(),
            },
        ];
        let mut forward = transcripts.clone();
        let mut reverse = transcripts.into_iter().rev().collect::<Vec<_>>();
        canonicalize_transcripts(&mut forward);
        canonicalize_transcripts(&mut reverse);
        assert_eq!(
            source_manifest_hash(&forward),
            source_manifest_hash(&reverse)
        );
    }

    #[test]
    fn source_errors_do_not_forward_rpc_detail() {
        let error = super::source_error(conversations::Error::Rpc {
            method: "thread/read".to_owned(),
            code: -1,
            message: "SECRET_TRANSCRIPT /Users/person/private".to_owned(),
        });
        assert!(!error.message.contains("SECRET_TRANSCRIPT"));
        assert!(!error.message.contains("/Users/person"));
    }

    #[test]
    fn expanded_scope_keeps_the_newest_messages_within_both_limits() {
        let prefix = (0..100)
            .map(|index| SourceMessage {
                host_id: "host".to_owned(),
                thread_id: "thread".to_owned(),
                turn_id: format!("turn-{index}"),
                item_id: format!("item-{index}"),
                role: MessageRole::Assistant,
                text: "x".repeat(4_096),
                occurred_at: i64::from(index),
                precision: Precision::Item,
            })
            .collect::<Vec<_>>();
        let mandatory = vec![prefix[99].clone()];
        let bounded =
            bounded_expanded_messages(prefix, &mandatory).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(bounded.len(), 64);
        assert_eq!(
            bounded.first().map(|message| message.item_id.as_str()),
            Some("item-36")
        );
        assert_eq!(
            bounded.last().map(|message| message.item_id.as_str()),
            Some("item-99")
        );
        assert_eq!(
            bounded
                .iter()
                .map(|message| message.text.len())
                .sum::<usize>(),
            262_144
        );
    }

    #[test]
    fn mandatory_scope_fails_instead_of_truncating() {
        let mandatory = vec![SourceMessage {
            host_id: "host".to_owned(),
            thread_id: "thread".to_owned(),
            turn_id: "turn".to_owned(),
            item_id: "item".to_owned(),
            role: MessageRole::User,
            text: "x".repeat(262_145),
            occurred_at: 1,
            precision: Precision::Item,
        }];
        assert_eq!(
            bounded_expanded_messages(mandatory.clone(), &mandatory)
                .err()
                .map(|error| error.code),
            Some("classification_scope_too_large")
        );
    }

    #[test]
    fn incomplete_stop_source_is_retryable_without_detail() {
        let error = observation_source_error(conversations::Error::TurnNotCompleted {
            thread_id: "private-thread".to_owned(),
            turn_id: "private-turn".to_owned(),
            status: "inProgress".to_owned(),
        });
        assert_eq!(error.code, "conversation_source_not_completed");
        assert!(!error.message.contains("private"));
    }
}
