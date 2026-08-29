use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BudgetReport {
    observed_at: String,
    scope: &'static str,
    attribution: &'static str,
    snapshot: Value,
}

impl BudgetReport {
    pub(crate) fn new(snapshot: Value) -> Self {
        Self {
            observed_at: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_else(|_| "unavailable".to_owned()),
            scope: "account-global Codex subscription state",
            attribution: "not uniquely attributable to Annals or one source delivery",
            snapshot,
        }
    }
}

pub(crate) fn print_human(report: &BudgetReport) {
    println!("Codex subscription budget");
    println!("Observed: {}", report.observed_at);
    println!("Scope:    {}", report.scope);
    println!("Warning:  {}", report.attribution);

    let rate_limits = report
        .snapshot
        .get("rateLimits")
        .unwrap_or(&report.snapshot);
    if let Some(limits) = rate_limits
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
    {
        for (limit_id, snapshot) in limits {
            print_limit(limit_id, snapshot);
        }
    } else if let Some(snapshot) = rate_limits.get("rateLimits") {
        let limit_id = snapshot
            .get("limitId")
            .and_then(Value::as_str)
            .unwrap_or("codex");
        print_limit(limit_id, snapshot);
    } else {
        println!("\nNo rate-limit bucket was returned by Codex.");
    }

    if let Some(count) = report
        .snapshot
        .get("rateLimits")
        .unwrap_or(&report.snapshot)
        .pointer("/rateLimitResetCredits/availableCount")
        .and_then(Value::as_i64)
    {
        println!("\nAvailable reset credits: {count}");
    }
    print_token_activity(report.snapshot.get("tokenActivity"));
    if let Some(error) = report
        .snapshot
        .get("tokenActivityError")
        .and_then(Value::as_str)
    {
        println!("\nAccount-global token activity unavailable: {error}");
    }
    println!(
        "\nThe backend does not expose a token denominator, so a delivery's exact share of this allowance cannot be calculated."
    );
}

fn print_token_activity(activity: Option<&Value>) {
    let Some(activity) = activity else {
        return;
    };
    let summary = activity.get("summary");
    let lifetime = summary
        .and_then(|summary| summary.get("lifetimeTokens"))
        .and_then(Value::as_i64);
    let peak = summary
        .and_then(|summary| summary.get("peakDailyTokens"))
        .and_then(Value::as_i64);
    let latest = activity
        .get("dailyUsageBuckets")
        .and_then(Value::as_array)
        .and_then(|buckets| buckets.last());
    if lifetime.is_none() && peak.is_none() && latest.is_none() {
        return;
    }
    println!("\nAccount-global token activity (context only; not allowance units):");
    if let Some(lifetime) = lifetime {
        println!("  Lifetime: {}", grouped(lifetime));
    }
    if let Some(peak) = peak {
        println!("  Peak day: {}", grouped(peak));
    }
    if let Some(latest) = latest {
        let date = latest
            .get("startDate")
            .and_then(Value::as_str)
            .unwrap_or("unknown date");
        let tokens = latest
            .get("tokens")
            .and_then(Value::as_i64)
            .map_or_else(|| "unknown".to_owned(), grouped);
        println!("  Latest daily bucket: {date}; {tokens} tokens");
    }
}

fn print_limit(fallback_id: &str, snapshot: &Value) {
    let limit_id = snapshot
        .get("limitId")
        .and_then(Value::as_str)
        .unwrap_or(fallback_id);
    let limit_name = snapshot.get("limitName").and_then(Value::as_str);
    println!();
    match limit_name {
        Some(name) => println!("Limit: {limit_id} ({name})"),
        None => println!("Limit: {limit_id}"),
    }
    if let Some(plan) = snapshot.get("planType").and_then(Value::as_str) {
        println!("  Plan: {plan}");
    }
    print_window("Primary", snapshot.get("primary"));
    print_window("Secondary", snapshot.get("secondary"));
    if let Some(credits) = snapshot.get("credits").filter(|value| !value.is_null()) {
        let balance = credits
            .get("balance")
            .and_then(Value::as_str)
            .unwrap_or("unavailable");
        let has_credits = credits
            .get("hasCredits")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let unlimited = credits
            .get("unlimited")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        println!(
            "  Purchased credits: balance {balance}; has credits {has_credits}; unlimited {unlimited}"
        );
    }
    if let Some(reached) = snapshot.get("spendControlReached").and_then(Value::as_bool) {
        println!("  Spend control reached: {reached}");
    }
    if let Some(individual) = snapshot.get("individualLimit").and_then(Value::as_object) {
        let used = individual
            .get("used")
            .and_then(Value::as_str)
            .unwrap_or("unavailable");
        let limit = individual
            .get("limit")
            .and_then(Value::as_str)
            .unwrap_or("unavailable");
        let remaining = individual
            .get("remainingPercent")
            .and_then(Value::as_i64)
            .map_or_else(|| "unavailable".to_owned(), |value| format!("{value}%"));
        println!("  Spend control: {used} of {limit}; {remaining} remaining");
    }
    if let Some(reached_type) = snapshot.get("rateLimitReachedType").and_then(Value::as_str) {
        println!("  Reached limit type: {reached_type}");
    }
}

fn print_window(label: &str, window: Option<&Value>) {
    let Some(window) = window.filter(|value| !value.is_null()) else {
        return;
    };
    let Some(used) = window.get("usedPercent").and_then(Value::as_i64) else {
        return;
    };
    let duration = window
        .get("windowDurationMins")
        .and_then(Value::as_i64)
        .map_or_else(|| "unknown window".to_owned(), duration_label);
    let reset = window
        .get("resetsAt")
        .and_then(Value::as_i64)
        .map_or_else(|| "unknown reset".to_owned(), format_seconds);
    println!("  {label}: {used}% used; {duration}; resets {reset}");
}

fn duration_label(minutes: i64) -> String {
    if minutes % (24 * 60) == 0 {
        format!("{} days", minutes / (24 * 60))
    } else if minutes % 60 == 0 {
        format!("{} hours", minutes / 60)
    } else {
        format!("{minutes} minutes")
    }
}

fn format_seconds(seconds: i64) -> String {
    let Ok(timestamp) = OffsetDateTime::from_unix_timestamp(seconds) else {
        return seconds.to_string();
    };
    timestamp
        .format(&Rfc3339)
        .unwrap_or_else(|_| seconds.to_string())
}

fn grouped(value: i64) -> String {
    let text = value.to_string();
    let mut result = String::with_capacity(text.len() + text.len() / 3);
    for (index, character) in text.chars().enumerate() {
        if index > 0 && (text.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(character);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::duration_label;

    #[test]
    fn renders_known_window_durations() {
        assert_eq!(duration_label(300), "5 hours");
        assert_eq!(duration_label(10_080), "7 days");
        assert_eq!(duration_label(17), "17 minutes");
    }
}
