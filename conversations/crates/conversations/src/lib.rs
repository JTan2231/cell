//! Read-only, normalized access to local Codex conversation history.
//!
//! The crate speaks the documented Codex App Server JSON-RPC protocol. It does
//! not inspect Codex JSONL logs or `SQLite` state directly.

mod model;
mod protocol;

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
#[cfg(target_os = "macos")]
use std::process::Stdio;
use std::time::Duration;

pub use model::{
    ALL_SOURCE_KINDS, ArchiveScope, CompletedFileChange, Conversation, ItemRef, ListOptions,
    Message, Role, SearchHit, ThreadRef, ThreadSummary, TimestampPrecision, Turn, TurnActivity,
    TurnRef,
};
use model::{parse_thread, parse_turn_activity, parse_turns};
use protocol::Protocol;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(target_os = "macos")]
use sha2::{Digest as _, Sha256};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("unable to start {path}: {source}")]
    Spawn {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("I/O error: {0}")]
    Io(#[source] std::io::Error),
    #[error("invalid JSON in {context}: {source}")]
    InvalidJson {
        context: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("Codex App Server protocol error: {message}")]
    Protocol { message: String },
    #[error("Codex App Server {method} failed ({code}): {message}")]
    Rpc {
        method: String,
        code: i64,
        message: String,
    },
    #[error("Codex App Server timed out after {seconds}s during {method}")]
    Timeout { method: String, seconds: u64 },
    #[error("conversation not found: {0}")]
    NotFound(String),
    #[error("turn {turn_id} was not found for session or thread hint {session_hint}")]
    TurnNotFound {
        session_hint: String,
        turn_id: String,
    },
    #[error("turn {turn_id} in thread {thread_id} is not completed (status: {status})")]
    TurnNotCompleted {
        thread_id: String,
        turn_id: String,
        status: String,
    },
    #[error(
        "turn {turn_id} for session or thread hint {session_hint} is ambiguous across threads: {thread_ids}"
    )]
    AmbiguousTurn {
        session_hint: String,
        turn_id: String,
        thread_ids: String,
    },
    #[error("Codex version command failed: {0}")]
    Version(String),
    #[error("stable local host identity is unavailable; set CONVERSATIONS_HOST_ID explicitly")]
    HostIdentityUnavailable,
    #[error(
        "thread reference belongs to host {reference_host_id}, but this client represents host {client_host_id}"
    )]
    ThreadHostMismatch {
        reference_host_id: String,
        client_host_id: String,
    },
}

/// Process and identity settings for one App Server connection.
#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub codex_path: PathBuf,
    pub codex_args: Vec<OsString>,
    pub host_id: String,
    pub request_timeout: Duration,
    pub stderr_policy: StderrPolicy,
}

/// Where the spawned Codex App Server writes its diagnostic stream.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StderrPolicy {
    /// Preserve diagnostics for an interactive caller.
    #[default]
    Inherit,
    /// Route diagnostics to the null device for privacy-sensitive embedding.
    Suppress,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            codex_path: std::env::var_os("CONVERSATIONS_CODEX")
                .map_or_else(|| PathBuf::from("codex"), PathBuf::from),
            codex_args: Vec::new(),
            host_id: local_host_id(),
            request_timeout: Duration::from_secs(30),
            stderr_policy: StderrPolicy::Inherit,
        }
    }
}

/// Compatibility and connectivity facts observed by `doctor`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub ok: bool,
    pub host_id: String,
    pub codex_path: String,
    pub executable_version: String,
    pub app_server_user_agent: Option<String>,
    pub visible_threads: usize,
    pub thread_cli_versions: Vec<String>,
    pub warnings: Vec<String>,
}

/// Report from an explicit scan-and-repair metadata refresh.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshReport {
    pub active_threads: usize,
    pub archived_threads: usize,
    pub total_threads: usize,
}

/// Blocking client for a short-lived `codex app-server --stdio` process.
pub struct AppServerClient {
    protocol: Protocol,
    config: ClientConfig,
}

impl AppServerClient {
    /// Start and initialize one short-lived App Server process.
    ///
    /// # Errors
    ///
    /// Returns an error when the process cannot start or complete its protocol
    /// handshake.
    pub fn spawn(config: ClientConfig) -> Result<Self> {
        if config.host_id.is_empty() {
            return Err(Error::HostIdentityUnavailable);
        }
        let protocol = Protocol::spawn(
            &config.codex_path,
            &config.codex_args,
            config.request_timeout,
            config.stderr_policy,
        )?;
        Ok(Self { protocol, config })
    }

    /// Enumerate every requested page. Active and archived stores are queried
    /// separately because App Server defines them as separate list filters.
    ///
    /// # Errors
    ///
    /// Returns an error for an App Server, transport, pagination, or malformed
    /// response failure.
    pub fn list(&mut self, options: &ListOptions) -> Result<Vec<ThreadSummary>> {
        let mut threads = Vec::new();
        match options.archive {
            ArchiveScope::Active => self.list_archive(false, options, &mut threads)?,
            ArchiveScope::Archived => self.list_archive(true, options, &mut threads)?,
            ArchiveScope::All => {
                self.list_archive(false, options, &mut threads)?;
                self.list_archive(true, options, &mut threads)?;
            }
        }
        threads.sort_by(|left, right| {
            right
                .updated_at
                .or(right.created_at)
                .cmp(&left.updated_at.or(left.created_at))
                .then_with(|| left.reference.thread_id.cmp(&right.reference.thread_id))
        });
        if let Some(updated_after) = options.updated_after {
            threads.retain(|thread| {
                thread
                    .updated_at
                    .or(thread.created_at)
                    .is_some_and(|timestamp| timestamp >= updated_after)
            });
        }
        if let Some(limit) = options.limit {
            threads.truncate(limit);
        }
        Ok(threads)
    }

    /// Read the persisted metadata for one exact machine-local thread reference.
    ///
    /// This metadata-only lookup includes every App Server source kind and both
    /// active and archived stores. It uses the state database without invoking
    /// metadata repair and returns the recorded working directory in the
    /// existing [`ThreadSummary`] shape.
    ///
    /// # Errors
    ///
    /// Returns an error when the reference belongs to another host, the thread
    /// is absent or duplicated across stores, or App Server cannot enumerate a
    /// complete metadata view.
    pub fn read_thread_summary(&mut self, reference: &ThreadRef) -> Result<ThreadSummary> {
        if reference.host_id != self.config.host_id {
            return Err(Error::ThreadHostMismatch {
                reference_host_id: reference.host_id.clone(),
                client_host_id: self.config.host_id.clone(),
            });
        }
        let mut matches = self
            .list(&ListOptions {
                include_subagents: true,
                include_exec: true,
                ..ListOptions::default()
            })?
            .into_iter()
            .filter(|summary| summary.reference.thread_id == reference.thread_id);
        let summary = matches
            .next()
            .ok_or_else(|| Error::NotFound(reference.thread_id.clone()))?;
        if matches.next().is_some() {
            return Err(Error::Protocol {
                message: format!(
                    "thread {} occurred more than once across active and archived metadata",
                    reference.thread_id
                ),
            });
        }
        Ok(summary)
    }

    /// Read a full normalized conversation, preferring turn pagination and
    /// falling back only when the installed App Server lacks that method.
    ///
    /// # Errors
    ///
    /// Returns an error when full history is unavailable or malformed.
    pub fn read(&mut self, summary: &ThreadSummary) -> Result<Conversation> {
        let thread_id = &summary.reference.thread_id;
        let values = match self.list_turns(thread_id) {
            Ok(turns) => turns,
            Err(error) if method_unavailable(&error) => self.read_legacy_turns(thread_id)?,
            Err(error) => return Err(error),
        };
        Ok(Conversation {
            thread: summary.clone(),
            turns: parse_turns(&self.config.host_id, thread_id, &values)?,
        })
    }

    /// Find a summary in either persisted archive and then read it.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is absent or cannot be read fully.
    pub fn read_thread(&mut self, thread_id: &str) -> Result<Conversation> {
        let options = ListOptions {
            include_subagents: true,
            include_exec: true,
            ..ListOptions::default()
        };
        let summary = self
            .list(&options)?
            .into_iter()
            .find(|thread| thread.reference.thread_id == thread_id)
            .ok_or_else(|| Error::NotFound(thread_id.to_owned()))?;
        self.read(&summary)
    }

    /// Read one exact turn and its content-free completed file-change evidence.
    ///
    /// This operation does not enumerate thread metadata. It pages newest-first
    /// and stops as soon as the requested turn is found.
    ///
    /// # Errors
    ///
    /// Returns an error when the turn is absent, incomplete, malformed, or
    /// unavailable through App Server.
    pub fn read_turn_activity(&mut self, thread_id: &str, turn_id: &str) -> Result<TurnActivity> {
        self.maybe_read_turn_activity(thread_id, turn_id)?
            .ok_or_else(|| Error::TurnNotFound {
                session_hint: thread_id.to_owned(),
                turn_id: turn_id.to_owned(),
            })
    }

    /// Read every completed turn activity for one selected thread.
    ///
    /// This opt-in reconciliation read preserves full turn pagination and the
    /// legacy method-unavailable fallback without changing the ordinary
    /// message-only corpus.
    ///
    /// # Errors
    ///
    /// Returns an error when any completed turn is incomplete or malformed, or
    /// when App Server cannot provide the complete selected history.
    pub fn read_completed_turn_activities(
        &mut self,
        summary: &ThreadSummary,
    ) -> Result<Vec<TurnActivity>> {
        let thread_id = &summary.reference.thread_id;
        let values = match self.list_turns(thread_id) {
            Ok(turns) => turns,
            Err(error) if method_unavailable(&error) => self.read_legacy_turns(thread_id)?,
            Err(error) => return Err(error),
        };
        let mut activities = Vec::new();
        for value in values {
            let value_turn_id =
                value
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Protocol {
                        message: format!("turn in {thread_id} has no string id"),
                    })?;
            let status =
                value
                    .get("status")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Protocol {
                        message: format!("turn {value_turn_id} has no string status"),
                    })?;
            match status {
                "completed" => activities.push(parse_turn_activity(
                    &self.config.host_id,
                    thread_id,
                    &value,
                )?),
                "inProgress" | "interrupted" | "failed" => {}
                status => {
                    return Err(Error::Protocol {
                        message: format!("turn {value_turn_id} has unknown status {status}"),
                    });
                }
            }
        }
        Ok(activities)
    }

    /// Resolve a hook-style session hint to the unique thread containing a turn.
    ///
    /// Root sessions, forks, and subagents can share App Server's `sessionId`.
    /// This method therefore tests exact turn membership across the exact thread
    /// hint and every visible active or archived member of the same lineage. It
    /// never guesses when the turn appears in more than one thread.
    ///
    /// # Errors
    ///
    /// Returns an error when no matching turn is visible, more than one thread
    /// contains the turn, or any required App Server read is incomplete.
    pub fn resolve_turn_activity(
        &mut self,
        session_hint: &str,
        turn_id: &str,
    ) -> Result<TurnActivity> {
        if let Some(activity) = self.maybe_read_turn_activity(session_hint, turn_id)? {
            return Ok(activity);
        }
        let summaries = self.list(&ListOptions {
            include_subagents: true,
            include_exec: true,
            ..ListOptions::default()
        })?;
        let candidates = session_candidate_thread_ids(&summaries, session_hint);
        let mut matches = Vec::new();
        for thread_id in candidates
            .into_iter()
            .filter(|thread_id| thread_id != session_hint)
        {
            if let Some(activity) = self.maybe_read_turn_activity(&thread_id, turn_id)? {
                matches.push(activity);
            }
        }
        match matches.len() {
            0 => Err(Error::TurnNotFound {
                session_hint: session_hint.to_owned(),
                turn_id: turn_id.to_owned(),
            }),
            1 => matches.pop().ok_or_else(|| Error::Protocol {
                message: "unique turn activity disappeared during resolution".to_owned(),
            }),
            _ => {
                let mut thread_ids = matches
                    .iter()
                    .map(|activity| activity.turn.reference.thread_id.clone())
                    .collect::<Vec<_>>();
                thread_ids.sort();
                Err(Error::AmbiguousTurn {
                    session_hint: session_hint.to_owned(),
                    turn_id: turn_id.to_owned(),
                    thread_ids: thread_ids.join(", "),
                })
            }
        }
    }

    /// Materialize a normalized corpus. Copied items shared by forks are kept
    /// once, on the most recently updated thread returned by `list`.
    ///
    /// # Errors
    ///
    /// Returns an error if enumeration or any selected full-history read fails.
    pub fn snapshot(&mut self, options: &ListOptions) -> Result<Vec<Conversation>> {
        let summaries = self.list(options)?;
        let mut seen_item_ids = HashSet::new();
        let mut conversations = Vec::with_capacity(summaries.len());
        for summary in summaries {
            let mut conversation = self.read(&summary)?;
            for turn in &mut conversation.turns {
                turn.messages
                    .retain(|message| seen_item_ids.insert(message.reference.item_id.clone()));
            }
            conversations.push(conversation);
        }
        Ok(conversations)
    }

    /// Search titles through App Server and message text client-side. The
    /// title request is deliberately separate because App Server searchTerm
    /// does not search turn content.
    ///
    /// # Errors
    ///
    /// Returns an error if title enumeration or any selected history fails.
    pub fn search(&mut self, query: &str, options: &ListOptions) -> Result<Vec<SearchHit>> {
        let mut title_options = options.clone();
        title_options.title_query = Some(query.to_owned());
        let title_matches = self.list(&title_options)?;
        let title_ids = title_matches
            .into_iter()
            .map(|thread| thread.reference.thread_id)
            .collect::<HashSet<_>>();

        let mut all_options = options.clone();
        all_options.title_query = None;
        let query_folded = query.to_lowercase();
        let mut hits = Vec::new();
        let mut seen_items = HashSet::new();
        for conversation in self.snapshot(&all_options)? {
            let title_match = title_ids.contains(&conversation.thread.reference.thread_id)
                || conversation
                    .thread
                    .title()
                    .to_lowercase()
                    .contains(&query_folded);
            for turn in conversation.turns {
                for message in turn.messages {
                    if (title_match || message.text.to_lowercase().contains(&query_folded))
                        && seen_items.insert(message.reference.item_id.clone())
                    {
                        hits.push(SearchHit {
                            thread: conversation.thread.clone(),
                            message,
                        });
                    }
                }
            }
        }
        Ok(hits)
    }

    /// Ask App Server to perform its normal metadata scan-and-repair path.
    ///
    /// # Errors
    ///
    /// Returns an error when either active or archived enumeration fails.
    pub fn refresh(&mut self) -> Result<RefreshReport> {
        let options = ListOptions {
            include_subagents: true,
            include_exec: true,
            use_state_db_only: false,
            archive: ArchiveScope::Active,
            ..ListOptions::default()
        };
        let active_threads = self.list(&options)?.len();
        let archived_threads = self
            .list(&ListOptions {
                archive: ArchiveScope::Archived,
                ..options
            })?
            .len();
        Ok(RefreshReport {
            active_threads,
            archived_threads,
            total_threads: active_threads + archived_threads,
        })
    }

    /// Inspect executable, handshake, visible-history, and version facts.
    ///
    /// # Errors
    ///
    /// Returns an error when the Codex version or history cannot be read.
    pub fn doctor(&mut self) -> Result<DoctorReport> {
        let executable_version = codex_version(&self.config.codex_path)?;
        let threads = self.list(&ListOptions::default())?;
        let mut versions = threads
            .iter()
            .filter_map(|thread| thread.cli_version.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        versions.sort();
        let mut warnings = Vec::new();
        if versions
            .iter()
            .any(|version| !executable_version.contains(version))
        {
            warnings.push(format!(
                "stored threads were written by Codex versions [{}], while this client launched {executable_version}",
                versions.join(", ")
            ));
        }
        warnings.push(
            "runtime status is scoped to this App Server process; notLoaded does not prove another Codex client is idle"
                .to_owned(),
        );
        Ok(DoctorReport {
            ok: true,
            host_id: self.config.host_id.clone(),
            codex_path: self.config.codex_path.display().to_string(),
            executable_version,
            app_server_user_agent: self
                .protocol
                .initialize_result
                .get("userAgent")
                .and_then(Value::as_str)
                .map(str::to_owned),
            visible_threads: threads.len(),
            thread_cli_versions: versions,
            warnings,
        })
    }

    fn list_archive(
        &mut self,
        archived: bool,
        options: &ListOptions,
        threads: &mut Vec<ThreadSummary>,
    ) -> Result<()> {
        let mut cursor: Option<String> = None;
        let mut seen_cursors = HashSet::new();
        loop {
            let mut params = json!({
                "limit": 100,
                "sortKey": "updated_at",
                "sortDirection": "desc",
                "sourceKinds": ALL_SOURCE_KINDS,
                "archived": archived,
                "useStateDbOnly": options.use_state_db_only
            });
            if let Some(value) = &cursor {
                params["cursor"] = Value::String(value.clone());
            }
            if let Some(value) = &options.cwd {
                params["cwd"] = Value::String(value.clone());
            }
            if let Some(value) = &options.title_query {
                params["searchTerm"] = Value::String(value.clone());
            }
            let result = self.protocol.request("thread/list", &params)?;
            let page = result
                .get("data")
                .and_then(Value::as_array)
                .ok_or_else(|| Error::Protocol {
                    message: "thread/list response has no data array".to_owned(),
                })?;
            for value in page {
                let summary = parse_thread(&self.config.host_id, value, archived)?;
                let is_subagent = summary.parent_thread_id.is_some()
                    || summary.source_kind.starts_with("subAgent");
                let include = (options.include_exec || summary.source_kind != "exec")
                    && (options.include_subagents || !is_subagent);
                if include {
                    threads.push(summary);
                }
            }
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            match &cursor {
                Some(cursor) if !seen_cursors.insert(cursor.clone()) => {
                    return Err(Error::Protocol {
                        message: format!("thread/list repeated pagination cursor {cursor}"),
                    });
                }
                Some(_) => {}
                None => break,
            }
        }
        Ok(())
    }

    fn list_turns(&mut self, thread_id: &str) -> Result<Vec<Value>> {
        let mut cursor: Option<String> = None;
        let mut seen_cursors = HashSet::new();
        let mut turns = Vec::new();
        loop {
            let mut params = json!({
                "threadId": thread_id,
                "limit": 100,
                "sortDirection": "asc",
                "itemsView": "full"
            });
            if let Some(value) = &cursor {
                params["cursor"] = Value::String(value.clone());
            }
            let result = self.protocol.request("thread/turns/list", &params)?;
            let page = result
                .get("data")
                .and_then(Value::as_array)
                .ok_or_else(|| Error::Protocol {
                    message: "thread/turns/list response has no data array".to_owned(),
                })?;
            turns.extend(page.iter().cloned());
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            match &cursor {
                Some(cursor) if !seen_cursors.insert(cursor.clone()) => {
                    return Err(Error::Protocol {
                        message: format!(
                            "thread/turns/list repeated pagination cursor {cursor} for {thread_id}"
                        ),
                    });
                }
                Some(_) => {}
                None => break,
            }
        }
        turns.sort_by(|left, right| {
            left.get("startedAt")
                .and_then(Value::as_i64)
                .cmp(&right.get("startedAt").and_then(Value::as_i64))
                .then_with(|| {
                    left.get("id")
                        .and_then(Value::as_str)
                        .cmp(&right.get("id").and_then(Value::as_str))
                })
        });
        Ok(turns)
    }

    fn maybe_read_turn_activity(
        &mut self,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<Option<TurnActivity>> {
        let value = match self.find_turn(thread_id, turn_id) {
            Ok(value) => value,
            Err(error) if method_unavailable(&error) => self
                .read_legacy_turns(thread_id)?
                .into_iter()
                .find(|value| value.get("id").and_then(Value::as_str) == Some(turn_id)),
            Err(error) => return Err(error),
        };
        value
            .as_ref()
            .map(|value| parse_turn_activity(&self.config.host_id, thread_id, value))
            .transpose()
    }

    fn find_turn(&mut self, thread_id: &str, turn_id: &str) -> Result<Option<Value>> {
        let mut cursor: Option<String> = None;
        let mut seen_cursors = HashSet::new();
        loop {
            let mut params = json!({
                "threadId": thread_id,
                "limit": 100,
                "sortDirection": "desc",
                "itemsView": "full"
            });
            if let Some(value) = &cursor {
                params["cursor"] = Value::String(value.clone());
            }
            let result = self.protocol.request("thread/turns/list", &params)?;
            let page = result
                .get("data")
                .and_then(Value::as_array)
                .ok_or_else(|| Error::Protocol {
                    message: "thread/turns/list response has no data array".to_owned(),
                })?;
            for value in page {
                let page_turn_id = value.get("id").and_then(Value::as_str).ok_or_else(|| {
                    Error::Protocol {
                        message: format!(
                            "thread/turns/list returned a turn without a string id for {thread_id}"
                        ),
                    }
                })?;
                if page_turn_id == turn_id {
                    return Ok(Some(value.clone()));
                }
            }
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            match &cursor {
                Some(cursor) if !seen_cursors.insert(cursor.clone()) => {
                    return Err(Error::Protocol {
                        message: format!(
                            "thread/turns/list repeated pagination cursor {cursor} for {thread_id}"
                        ),
                    });
                }
                Some(_) => {}
                None => return Ok(None),
            }
        }
    }

    fn read_legacy_turns(&mut self, thread_id: &str) -> Result<Vec<Value>> {
        let result = self.protocol.request(
            "thread/read",
            &json!({"threadId": thread_id, "includeTurns": true}),
        )?;
        result
            .get("thread")
            .and_then(|thread| thread.get("turns"))
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| Error::Protocol {
                message: "thread/read response has no thread.turns array".to_owned(),
            })
    }
}

fn session_candidate_thread_ids(summaries: &[ThreadSummary], session_hint: &str) -> Vec<String> {
    let mut selected = HashSet::from([session_hint.to_owned()]);
    let mut session_ids = HashSet::from([session_hint.to_owned()]);
    loop {
        let mut changed = false;
        let selected_parents = summaries
            .iter()
            .filter(|summary| selected.contains(&summary.reference.thread_id))
            .flat_map(|summary| {
                [
                    summary.parent_thread_id.as_deref(),
                    summary.forked_from_id.as_deref(),
                ]
            })
            .flatten()
            .collect::<HashSet<_>>();
        for summary in summaries {
            let thread_id = &summary.reference.thread_id;
            let related = selected.contains(thread_id)
                || summary
                    .session_id
                    .as_ref()
                    .is_some_and(|session_id| session_ids.contains(session_id))
                || summary
                    .parent_thread_id
                    .as_ref()
                    .is_some_and(|parent| selected.contains(parent))
                || summary
                    .forked_from_id
                    .as_ref()
                    .is_some_and(|parent| selected.contains(parent))
                || selected_parents.contains(thread_id.as_str());
            if related && selected.insert(thread_id.clone()) {
                changed = true;
            }
            if related
                && summary
                    .session_id
                    .as_ref()
                    .is_some_and(|session_id| session_ids.insert(session_id.clone()))
            {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut candidates = summaries
        .iter()
        .map(|summary| &summary.reference.thread_id)
        .filter(|thread_id| selected.contains(*thread_id))
        .cloned()
        .collect::<Vec<_>>();
    if !candidates.iter().any(|thread_id| thread_id == session_hint) {
        candidates.push(session_hint.to_owned());
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn method_unavailable(error: &Error) -> bool {
    matches!(error, Error::Rpc { code: -32601, .. })
}

#[cfg(test)]
mod fallback_tests {
    use super::{Error, method_unavailable};

    #[test]
    fn legacy_fallback_requires_method_not_found_code() {
        assert!(method_unavailable(&Error::Rpc {
            method: "thread/turns/list".to_owned(),
            code: -32601,
            message: "Unknown method".to_owned(),
        }));
        assert!(!method_unavailable(&Error::Rpc {
            method: "thread/turns/list".to_owned(),
            code: -32000,
            message: "this thread source is unsupported".to_owned(),
        }));
    }
}

fn codex_version(path: &PathBuf) -> Result<String> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|source| Error::Spawn {
            path: path.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(Error::Version(format!(
            "{} exited with {}",
            path.display(),
            output.status
        )));
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if version.is_empty() {
        return Err(Error::Version(format!(
            "{} returned an empty version",
            path.display()
        )));
    }
    Ok(version)
}

fn local_host_id() -> String {
    if let Some(host) = std::env::var_os("CONVERSATIONS_HOST_ID") {
        let host = host.to_string_lossy().trim().to_owned();
        if !host.is_empty() {
            return host;
        }
    }
    #[cfg(target_os = "macos")]
    {
        macos_platform_uuid()
            .map(|platform_uuid| {
                let digest = format!("{:x}", Sha256::digest(platform_uuid.as_bytes()));
                format!("mac_{}", &digest[..32])
            })
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Command::new("hostname")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .filter(|host| !host.is_empty())
            .unwrap_or_else(|| "local".to_owned())
    }
}

#[cfg(target_os = "macos")]
fn macos_platform_uuid() -> Option<String> {
    let output = Command::new("/usr/sbin/ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_platform_uuid(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "macos")]
fn parse_platform_uuid(output: &str) -> Option<String> {
    let value = output
        .lines()
        .find_map(|line| {
            line.split_once("\"IOPlatformUUID\" = \"")
                .map(|(_, value)| value)
        })?
        .split('"')
        .next()?
        .trim();
    let valid = value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        });
    valid.then(|| value.to_ascii_lowercase())
}

/// Remove duplicate copied items from already-loaded conversations. This is
/// useful to callers that load threads incrementally rather than via snapshot.
pub fn deduplicate_copied_items(conversations: &mut [Conversation]) {
    let mut owners = HashMap::new();
    for (conversation_index, conversation) in conversations.iter().enumerate() {
        for turn in &conversation.turns {
            for message in &turn.messages {
                owners
                    .entry(message.reference.item_id.clone())
                    .or_insert(conversation_index);
            }
        }
    }
    for (conversation_index, conversation) in conversations.iter_mut().enumerate() {
        for turn in &mut conversation.turns {
            turn.messages.retain(|message| {
                owners.get(&message.reference.item_id) == Some(&conversation_index)
            });
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod host_id_tests {
    use super::{AppServerClient, ClientConfig, Error, parse_platform_uuid};

    #[test]
    fn parses_only_the_platform_uuid_value() {
        let output = "    \"IOPlatformUUID\" = \"123E4567-E89B-12D3-A456-426614174000\"";
        assert_eq!(
            parse_platform_uuid(output).as_deref(),
            Some("123e4567-e89b-12d3-a456-426614174000")
        );
        assert_eq!(parse_platform_uuid("no platform identity"), None);
        assert_eq!(
            parse_platform_uuid("\"IOPlatformUUID\" = \"not-a-uuid\""),
            None
        );
    }

    #[test]
    fn empty_platform_identity_fails_before_process_start() {
        let config = ClientConfig {
            host_id: String::new(),
            ..ClientConfig::default()
        };
        let Err(error) = AppServerClient::spawn(config) else {
            panic!("empty host identity unexpectedly started App Server");
        };
        assert!(matches!(error, Error::HostIdentityUnavailable));
    }
}
