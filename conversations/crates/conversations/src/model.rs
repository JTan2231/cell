use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

/// Every source kind currently accepted by Codex App Server `thread/list`.
pub const ALL_SOURCE_KINDS: [&str; 10] = [
    "cli",
    "vscode",
    "exec",
    "appServer",
    "subAgent",
    "subAgentReview",
    "subAgentCompact",
    "subAgentThreadSpawn",
    "subAgentOther",
    "unknown",
];

/// Which persisted thread sets should be enumerated.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArchiveScope {
    Active,
    Archived,
    #[default]
    All,
}

/// Options shared by list and snapshot operations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListOptions {
    pub archive: ArchiveScope,
    pub include_subagents: bool,
    pub include_exec: bool,
    pub cwd: Option<String>,
    pub title_query: Option<String>,
    /// Keep threads updated at or after this Unix timestamp.
    pub updated_after: Option<i64>,
    /// Limit merged summaries before any snapshot performs full-history reads.
    pub limit: Option<usize>,
    pub use_state_db_only: bool,
}

impl Default for ListOptions {
    fn default() -> Self {
        Self {
            archive: ArchiveScope::All,
            include_subagents: false,
            include_exec: false,
            cwd: None,
            title_query: None,
            updated_after: None,
            limit: None,
            use_state_db_only: true,
        }
    }
}

/// A stable machine-local thread reference.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRef {
    pub host_id: String,
    pub thread_id: String,
}

/// A stable machine-local turn reference.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRef {
    pub host_id: String,
    pub thread_id: String,
    pub turn_id: String,
}

/// A stable machine-local item reference.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemRef {
    pub host_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
}

/// App Server's persisted metadata for one thread, normalized for callers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub reference: ThreadRef,
    pub session_id: Option<String>,
    pub name: Option<String>,
    pub preview: String,
    pub cwd: Option<String>,
    pub source_kind: String,
    pub parent_thread_id: Option<String>,
    pub forked_from_id: Option<String>,
    pub cli_version: Option<String>,
    pub archived: bool,
    pub ephemeral: bool,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    /// Status as observed by this new App Server process.
    pub runtime_status: String,
}

impl ThreadSummary {
    #[must_use]
    pub fn title(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.preview)
    }
}

/// Normalized content role. Tool and reasoning items never enter this enum.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// Authority of a normalized message timestamp.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimestampPrecision {
    Item,
    Turn,
    Unknown,
}

/// One normalized user or assistant message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub reference: ItemRef,
    pub role: Role,
    pub text: String,
    pub timestamp: Option<i64>,
    pub timestamp_precision: TimestampPrecision,
}

/// One normalized turn.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub reference: TurnRef,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub status: String,
    pub messages: Vec<Message>,
}

/// One successfully completed App Server file-change item, reduced to
/// content-free metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedFileChange {
    pub reference: ItemRef,
    pub change_count: usize,
}

/// One exact turn plus content-free evidence of its completed file changes.
///
/// `turn.messages` retains the ordinary user/assistant-only normalization.
/// Paths, diffs, commands, tool output, and other internal item payloads never
/// enter this type.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnActivity {
    pub turn: Turn,
    pub completed_file_changes: Vec<CompletedFileChange>,
}

impl TurnActivity {
    /// Whether this turn contains at least one completed, nonempty file change.
    #[must_use]
    pub fn has_completed_file_change(&self) -> bool {
        !self.completed_file_changes.is_empty()
    }
}

/// A thread summary plus its normalized, full user/assistant history.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub thread: ThreadSummary,
    pub turns: Vec<Turn>,
}

/// One client-side full-text match.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub thread: ThreadSummary,
    pub message: Message,
}

pub(crate) fn parse_thread(host_id: &str, value: &Value, archived: bool) -> Result<ThreadSummary> {
    let thread_id = required_string(value, "id", "thread")?;
    Ok(ThreadSummary {
        reference: ThreadRef {
            host_id: host_id.to_owned(),
            thread_id,
        },
        session_id: optional_string(value, "sessionId"),
        name: optional_string(value, "name"),
        preview: optional_string(value, "preview").unwrap_or_default(),
        cwd: optional_string(value, "cwd"),
        source_kind: source_kind(value.get("source")),
        parent_thread_id: optional_string(value, "parentThreadId"),
        forked_from_id: optional_string(value, "forkedFromId"),
        cli_version: optional_string(value, "cliVersion"),
        archived,
        ephemeral: value
            .get("ephemeral")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        created_at: optional_i64(value, "createdAt"),
        updated_at: optional_i64(value, "updatedAt"),
        runtime_status: value
            .get("status")
            .and_then(|status| status.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned(),
    })
}

pub(crate) fn parse_turns(host_id: &str, thread_id: &str, values: &[Value]) -> Result<Vec<Turn>> {
    let mut seen_items = HashSet::new();
    let mut turns = Vec::with_capacity(values.len());
    for value in values {
        let turn_id = required_string(value, "id", "turn")?;
        if let Some(items_view) = value.get("itemsView") {
            match items_view.as_str() {
                Some("full") => {}
                Some(items_view) => {
                    return Err(Error::Protocol {
                        message: format!(
                            "turn {turn_id} returned itemsView {items_view}, not full"
                        ),
                    });
                }
                None => {
                    return Err(Error::Protocol {
                        message: format!("turn {turn_id} has a non-string itemsView"),
                    });
                }
            }
        }
        let started_at = optional_i64(value, "startedAt");
        let items = value
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Protocol {
                message: format!("turn {turn_id} has no items array in a full-history response"),
            })?;
        let mut messages = Vec::new();
        for item in items {
            let Some(role) = item_role(item) else {
                continue;
            };
            let item_id = required_string(item, "id", "thread item")?;
            if !seen_items.insert(item_id.clone()) {
                continue;
            }
            let text = match role {
                Role::User => user_text(item, &item_id)?,
                Role::Assistant => message_text(item, &item_id)?,
            };
            let item_timestamp =
                optional_i64(item, "createdAt").or_else(|| optional_i64(item, "timestamp"));
            let (timestamp, timestamp_precision) = if let Some(timestamp) = item_timestamp {
                (Some(timestamp), TimestampPrecision::Item)
            } else if let Some(timestamp) = started_at {
                (Some(timestamp), TimestampPrecision::Turn)
            } else {
                (None, TimestampPrecision::Unknown)
            };
            messages.push(Message {
                reference: ItemRef {
                    host_id: host_id.to_owned(),
                    thread_id: thread_id.to_owned(),
                    turn_id: turn_id.clone(),
                    item_id,
                },
                role,
                text,
                timestamp,
                timestamp_precision,
            });
        }
        turns.push(Turn {
            reference: TurnRef {
                host_id: host_id.to_owned(),
                thread_id: thread_id.to_owned(),
                turn_id,
            },
            started_at,
            completed_at: optional_i64(value, "completedAt"),
            status: optional_string(value, "status").unwrap_or_else(|| "unknown".to_owned()),
            messages,
        });
    }
    Ok(turns)
}

pub(crate) fn parse_turn_activity(
    host_id: &str,
    thread_id: &str,
    value: &Value,
) -> Result<TurnActivity> {
    let mut turns = parse_turns(host_id, thread_id, std::slice::from_ref(value))?;
    let turn = turns.pop().ok_or_else(|| Error::Protocol {
        message: format!("turn activity for {thread_id} contained no turn"),
    })?;
    let turn_id = &turn.reference.turn_id;
    if turn.status != "completed" {
        return Err(Error::TurnNotCompleted {
            thread_id: thread_id.to_owned(),
            turn_id: turn_id.clone(),
            status: turn.status.clone(),
        });
    }
    if turn.completed_at.is_none() {
        return Err(Error::Protocol {
            message: format!("completed turn {turn_id} has no completedAt timestamp"),
        });
    }
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Protocol {
            message: format!("turn {turn_id} has no items array in a full-history response"),
        })?;
    let mut seen_file_changes = HashSet::new();
    let mut completed_file_changes = Vec::new();
    for item in items {
        if item.get("type").and_then(Value::as_str) != Some("fileChange") {
            continue;
        }
        let item_id = required_string(item, "id", "fileChange")?;
        let status = required_string(item, "status", "fileChange")?;
        let changes = item
            .get("changes")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Protocol {
                message: format!("fileChange {item_id} has no changes array"),
            })?;
        for (index, change) in changes.iter().enumerate() {
            validate_file_change(change, &item_id, index)?;
        }
        match status.as_str() {
            "completed" if !changes.is_empty() && seen_file_changes.insert(item_id.clone()) => {
                completed_file_changes.push(CompletedFileChange {
                    reference: ItemRef {
                        host_id: host_id.to_owned(),
                        thread_id: thread_id.to_owned(),
                        turn_id: turn_id.clone(),
                        item_id,
                    },
                    change_count: changes.len(),
                });
            }
            "inProgress" | "failed" | "declined" | "completed" => {}
            status => {
                return Err(Error::Protocol {
                    message: format!("fileChange {item_id} has unknown status {status}"),
                });
            }
        }
    }
    Ok(TurnActivity {
        turn,
        completed_file_changes,
    })
}

fn validate_file_change(change: &Value, item_id: &str, index: usize) -> Result<()> {
    let object = change.as_object().ok_or_else(|| Error::Protocol {
        message: format!("fileChange {item_id} change {index} is not an object"),
    })?;
    for field in ["path", "diff"] {
        if object.get(field).and_then(Value::as_str).is_none() {
            return Err(Error::Protocol {
                message: format!("fileChange {item_id} change {index} has no string {field}"),
            });
        }
    }
    let kind = object
        .get("kind")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Protocol {
            message: format!("fileChange {item_id} change {index} has no object kind"),
        })?;
    let kind_type = kind
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Protocol {
            message: format!("fileChange {item_id} change {index} kind has no string type"),
        })?;
    match kind_type {
        "add" | "delete" => Ok(()),
        "update" => match kind.get("move_path") {
            None | Some(Value::Null | Value::String(_)) => Ok(()),
            Some(_) => Err(Error::Protocol {
                message: format!(
                    "fileChange {item_id} change {index} update kind has invalid move_path"
                ),
            }),
        },
        kind_type => Err(Error::Protocol {
            message: format!("fileChange {item_id} change {index} has unknown kind {kind_type}"),
        }),
    }
}

fn item_role(item: &Value) -> Option<Role> {
    match item.get("type").and_then(Value::as_str) {
        Some("userMessage") => Some(Role::User),
        Some("agentMessage") => Some(Role::Assistant),
        _ => None,
    }
}

fn user_text(item: &Value, item_id: &str) -> Result<String> {
    let content = item
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Protocol {
            message: format!("userMessage {item_id} has no content array"),
        })?;
    let mut text_parts = Vec::new();
    for part in content {
        if part.get("type").and_then(Value::as_str) == Some("text") {
            let text = part
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Protocol {
                    message: format!("userMessage {item_id} has malformed text content"),
                })?;
            text_parts.push(text);
        }
    }
    let text = text_parts.join("\n");
    if text.trim().is_empty() {
        return Err(Error::Protocol {
            message: format!("userMessage {item_id} has no nonempty text content"),
        });
    }
    Ok(text)
}

fn message_text(item: &Value, item_id: &str) -> Result<String> {
    let text = item
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Protocol {
            message: format!("agentMessage {item_id} has no string text"),
        })?;
    if text.trim().is_empty() {
        return Err(Error::Protocol {
            message: format!("agentMessage {item_id} has empty text"),
        });
    }
    Ok(text.to_owned())
}

fn source_kind(source: Option<&Value>) -> String {
    match source {
        Some(Value::String(source)) => source.clone(),
        Some(Value::Object(source)) if source.contains_key("subAgent") => {
            match source.get("subAgent") {
                Some(Value::String(value)) if value == "review" => "subAgentReview".to_owned(),
                Some(Value::String(value)) if value == "compact" => "subAgentCompact".to_owned(),
                Some(Value::Object(value)) if value.contains_key("thread_spawn") => {
                    "subAgentThreadSpawn".to_owned()
                }
                _ => "subAgentOther".to_owned(),
            }
        }
        Some(Value::Object(source)) if source.contains_key("custom") => "unknown".to_owned(),
        _ => "unknown".to_owned(),
    }
}

fn required_string(value: &Value, field: &str, context: &str) -> Result<String> {
    optional_string(value, field).ok_or_else(|| Error::Protocol {
        message: format!("{context} has no string {field}"),
    })
}

fn optional_string(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn optional_i64(value: &Value, field: &str) -> Option<i64> {
    value.get(field).and_then(Value::as_i64)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn normalizes_only_user_and_assistant_content() {
        let turns = parse_turns(
            "host",
            "thread",
            &[json!({
                "id": "turn",
                "startedAt": 42,
                "status": "completed",
                "items": [
                    {"id":"user", "type":"userMessage", "content":[
                        {"type":"text", "text":"Choose A"},
                        {"type":"image", "url":"file:///secret.png"}
                    ]},
                    {"id":"reasoning", "type":"reasoning", "summary":[]},
                    {"id":"tool", "type":"commandExecution", "command":"secret"},
                    {"id":"agent", "type":"agentMessage", "text":"A it is"},
                    {"id":"agent", "type":"agentMessage", "text":"duplicate"}
                ]
            })],
        );
        let turns = match turns {
            Ok(turns) => turns,
            Err(error) => panic!("fixture is invalid: {error}"),
        };

        assert_eq!(turns[0].messages.len(), 2);
        assert_eq!(turns[0].messages[0].text, "Choose A");
        assert_eq!(turns[0].messages[0].timestamp, Some(42));
        assert_eq!(
            turns[0].messages[0].timestamp_precision,
            TimestampPrecision::Turn
        );
        assert_eq!(turns[0].messages[1].role, Role::Assistant);
    }

    #[test]
    fn activity_reduces_completed_file_changes_without_retaining_payloads() {
        let value = json!({
            "id":"turn",
            "startedAt":42,
            "completedAt":43,
            "status":"completed",
            "items":[
                {"id":"user", "type":"userMessage", "content":[
                    {"type":"text", "text":"Make the change"}
                ]},
                {"id":"agent", "type":"agentMessage", "text":"Done"},
                {"id":"file", "type":"fileChange", "status":"completed", "changes":[
                    {"path":"/private/secret-a", "diff":"SECRET DIFF A", "kind":{"type":"add"}},
                    {"path":"/private/secret-b", "diff":"SECRET DIFF B", "kind":{"type":"update", "move_path":null}}
                ]},
                {"id":"failed", "type":"fileChange", "status":"failed", "changes":[
                    {"path":"/private/failed", "diff":"FAILED SECRET", "kind":{"type":"delete"}}
                ]},
                {"id":"empty", "type":"fileChange", "status":"completed", "changes":[]}
            ]
        });

        let activity = match parse_turn_activity("host", "thread", &value) {
            Ok(activity) => activity,
            Err(error) => panic!("fixture is invalid: {error}"),
        };
        assert!(activity.has_completed_file_change());
        assert_eq!(activity.turn.messages.len(), 2);
        assert_eq!(activity.completed_file_changes.len(), 1);
        assert_eq!(activity.completed_file_changes[0].change_count, 2);
        let serialized = match serde_json::to_string(&activity) {
            Ok(serialized) => serialized,
            Err(error) => panic!("activity did not serialize: {error}"),
        };
        for secret in [
            "/private/secret-a",
            "/private/secret-b",
            "SECRET DIFF A",
            "SECRET DIFF B",
            "FAILED SECRET",
        ] {
            assert!(!serialized.contains(secret));
        }
    }

    #[test]
    fn activity_requires_a_completed_turn_with_a_completion_timestamp() {
        for (status, completed_at) in [
            ("inProgress", Some(43)),
            ("interrupted", Some(43)),
            ("failed", Some(43)),
        ] {
            let result = parse_turn_activity(
                "host",
                "thread",
                &json!({
                    "id":"turn",
                    "startedAt":42,
                    "completedAt":completed_at,
                    "status":status,
                    "items":[]
                }),
            );
            assert!(matches!(result, Err(Error::TurnNotCompleted { .. })));
        }
        let missing_timestamp = parse_turn_activity(
            "host",
            "thread",
            &json!({"id":"turn", "status":"completed", "items":[]}),
        );
        assert!(
            matches!(missing_timestamp, Err(Error::Protocol { message }) if message.contains("completedAt"))
        );
    }

    #[test]
    fn activity_rejects_malformed_file_change_entries_and_statuses() {
        let malformed_changes = [
            json!("not an object"),
            json!({"diff":"x", "kind":{"type":"add"}}),
            json!({"path":"x", "kind":{"type":"add"}}),
            json!({"path":"x", "diff":"x", "kind":"add"}),
            json!({"path":"x", "diff":"x", "kind":{"type":"unknown"}}),
            json!({"path":"x", "diff":"x", "kind":{"type":"update", "move_path":7}}),
        ];
        for malformed in malformed_changes {
            let result = parse_turn_activity(
                "host",
                "thread",
                &json!({
                    "id":"turn",
                    "completedAt":43,
                    "status":"completed",
                    "items":[{
                        "id":"file",
                        "type":"fileChange",
                        "status":"completed",
                        "changes":[malformed]
                    }]
                }),
            );
            assert!(matches!(result, Err(Error::Protocol { .. })));
        }

        let unknown_status = parse_turn_activity(
            "host",
            "thread",
            &json!({
                "id":"turn",
                "completedAt":43,
                "status":"completed",
                "items":[{
                    "id":"file",
                    "type":"fileChange",
                    "status":"newStatus",
                    "changes":[]
                }]
            }),
        );
        assert!(
            matches!(unknown_status, Err(Error::Protocol { message }) if message.contains("unknown status"))
        );
    }

    #[test]
    fn recognizes_structured_subagent_sources() {
        let value = json!({
            "id":"thread",
            "preview":"",
            "source":{"subAgent":{"thread_spawn":{"parent_thread_id":"root","depth":1}}},
            "parentThreadId":"root"
        });
        let thread = match parse_thread("host", &value, false) {
            Ok(thread) => thread,
            Err(error) => panic!("fixture is invalid: {error}"),
        };
        assert_eq!(thread.source_kind, "subAgentThreadSpawn");
        assert_eq!(thread.parent_thread_id.as_deref(), Some("root"));
    }

    #[test]
    fn rejects_turns_without_full_items() {
        let result = parse_turns(
            "host",
            "thread",
            &[json!({"id":"turn", "status":"completed"})],
        );
        assert!(
            matches!(result, Err(Error::Protocol { message }) if message.contains("items array"))
        );
    }

    #[test]
    fn rejects_malformed_recognized_messages() {
        let malformed_user = parse_turns(
            "host",
            "thread",
            &[json!({
                "id":"turn",
                "status":"completed",
                "items":[{"id":"user", "type":"userMessage", "content":[
                    {"type":"text"}
                ]}]
            })],
        );
        assert!(
            matches!(malformed_user, Err(Error::Protocol { message }) if message.contains("malformed text"))
        );

        let malformed_agent = parse_turns(
            "host",
            "thread",
            &[json!({
                "id":"turn",
                "status":"completed",
                "items":[{"id":"agent", "type":"agentMessage"}]
            })],
        );
        assert!(
            matches!(malformed_agent, Err(Error::Protocol { message }) if message.contains("string text"))
        );
    }

    #[test]
    fn rejects_explicit_non_full_turn_views() {
        for items_view in [json!("summary"), json!("notLoaded"), json!(false)] {
            let result = parse_turns(
                "host",
                "thread",
                &[json!({
                    "id":"turn",
                    "status":"completed",
                    "itemsView":items_view,
                    "items":[]
                })],
            );
            assert!(
                matches!(result, Err(Error::Protocol { message }) if message.contains("itemsView"))
            );
        }

        let full = parse_turns(
            "host",
            "thread",
            &[json!({
                "id":"turn",
                "status":"completed",
                "itemsView":"full",
                "items":[]
            })],
        );
        assert!(full.is_ok());
    }
}
