use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{ThreadTokenUsage, TokenUsageBreakdown};

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RateLimitWindow {
    pub(crate) used_percent: i32,
    pub(crate) window_duration_mins: Option<i64>,
    pub(crate) resets_at: Option<i64>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreditsSnapshot {
    pub(crate) has_credits: bool,
    pub(crate) unlimited: bool,
    pub(crate) balance: Option<String>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpendControlLimitSnapshot {
    pub(crate) limit: String,
    pub(crate) used: String,
    pub(crate) remaining_percent: i32,
    pub(crate) resets_at: i64,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RateLimitSnapshot {
    pub(crate) limit_id: Option<String>,
    pub(crate) limit_name: Option<String>,
    pub(crate) primary: Option<RateLimitWindow>,
    pub(crate) secondary: Option<RateLimitWindow>,
    pub(crate) credits: Option<CreditsSnapshot>,
    pub(crate) individual_limit: Option<SpendControlLimitSnapshot>,
    pub(crate) spend_control_reached: Option<bool>,
    pub(crate) plan_type: Option<String>,
    pub(crate) rate_limit_reached_type: Option<String>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RateLimitResetCreditsSummary {
    pub(crate) available_count: i64,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)]
pub(crate) struct AccountRateLimits {
    pub(crate) rate_limits: RateLimitSnapshot,
    pub(crate) rate_limits_by_limit_id: Option<BTreeMap<String, RateLimitSnapshot>>,
    pub(crate) rate_limit_reset_credits: Option<RateLimitResetCreditsSummary>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountTokenUsageSummary {
    pub(crate) lifetime_tokens: Option<i64>,
    pub(crate) peak_daily_tokens: Option<i64>,
    pub(crate) longest_running_turn_sec: Option<i64>,
    pub(crate) current_streak_days: Option<i64>,
    pub(crate) longest_streak_days: Option<i64>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DailyTokenUsage {
    pub(crate) start_date: String,
    pub(crate) tokens: i64,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountTokenUsage {
    pub(crate) summary: AccountTokenUsageSummary,
    pub(crate) daily_usage_buckets: Option<Vec<DailyTokenUsage>>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountSnapshot {
    pub(crate) rate_limits: AccountRateLimits,
    pub(crate) token_activity: Option<AccountTokenUsage>,
    pub(crate) token_activity_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProtocolEvent {
    TokenUsageUpdated {
        thread_id: String,
        turn_id: String,
        usage: ThreadTokenUsage,
    },
    RawResponseCompleted {
        thread_id: String,
        turn_id: String,
        response_id: String,
        usage: Option<TokenUsageBreakdown>,
    },
    TurnCompleted {
        thread_id: String,
        turn_id: String,
        status: String,
    },
}

/// Decode one exact Codex output record retained by Nucleus.
pub(crate) fn decode_output(message: &Value) -> Option<ProtocolEvent> {
    match message.get("method").and_then(Value::as_str) {
        Some("thread/tokenUsage/updated") => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Params {
                thread_id: String,
                turn_id: String,
                token_usage: ThreadTokenUsage,
            }
            parse_params::<Params>(message).map(|params| ProtocolEvent::TokenUsageUpdated {
                thread_id: params.thread_id,
                turn_id: params.turn_id,
                usage: params.token_usage,
            })
        }
        Some("rawResponse/completed") => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Params {
                thread_id: String,
                turn_id: String,
                response_id: String,
                usage: Option<TokenUsageBreakdown>,
            }
            parse_params::<Params>(message).map(|params| ProtocolEvent::RawResponseCompleted {
                thread_id: params.thread_id,
                turn_id: params.turn_id,
                response_id: params.response_id,
                usage: params.usage,
            })
        }
        Some("turn/completed") => {
            let thread_id = string_at(message, "/params/threadId")?;
            let turn_id = string_at(message, "/params/turn/id")?;
            let status = string_at(message, "/params/turn/status")?;
            Some(ProtocolEvent::TurnCompleted {
                thread_id,
                turn_id,
                status,
            })
        }
        _ => None,
    }
}

fn parse_params<T: for<'de> Deserialize<'de>>(message: &Value) -> Option<T> {
    serde_json::from_value(message.get("params")?.clone()).ok()
}

fn string_at(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer)?.as_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ProtocolEvent, decode_output};

    #[test]
    fn decoder_reads_output_notifications_without_harness_input() {
        assert!(matches!(
            decode_output(&json!({
                "method": "turn/completed",
                "params": {
                    "threadId": "thread-1",
                    "turn": { "id": "turn-1", "status": "completed" }
                }
            })),
            Some(ProtocolEvent::TurnCompleted { thread_id, turn_id, status })
                if thread_id == "thread-1" && turn_id == "turn-1" && status == "completed"
        ));
    }
}
