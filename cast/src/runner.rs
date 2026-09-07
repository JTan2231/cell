use crate::{Result, adapters, http::HttpClient, models::Config, now, store::Store, timestamp};
use serde_json::{Value, json};
use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    pub force: bool,
    pub provider: Option<String>,
    pub max_requests: Option<u64>,
    pub only_source: Option<String>,
}

/// Runs due discovery and careers collection within persistent request and time budgets.
///
/// # Errors
/// Returns an error when state locking, configuration, or durable checkpointing fails.
pub async fn run(store: &Store, options: RunOptions) -> Result<Value> {
    let _lock = store.lock()?;
    let mut config = store.config()?;
    if let Some(limit) = options.max_requests {
        config.budgets.http_per_run = config.budgets.http_per_run.min(limit);
    }
    let run_id = store.start_run()?;
    let reserve_store = store.clone();
    let reserve_run = run_id.clone();
    let budgets = config.budgets.clone();
    let reserve = Arc::new(move |provider: &str, units: u64| {
        reserve_store
            .reserve_request(&reserve_run, provider, units, &budgets)
            .map_err(|e| e.to_string())
    });
    let settle_store = store.clone();
    let settle = Arc::new(move |id: &str, actual: Option<u64>| {
        settle_store
            .settle_request(id, actual)
            .map_err(|e| e.to_string())
    });
    let client = HttpClient::new(reserve, settle)?;
    let outcome = tokio::time::timeout(
        Duration::from_secs(config.budgets.runtime_seconds),
        collect(store, &client, &config, &options),
    )
    .await;
    let (status, note) = match outcome {
        Ok(Ok(note)) => (
            if note["partial"].as_bool().unwrap_or(false) {
                "partial"
            } else {
                "complete"
            },
            note,
        ),
        Ok(Err(error)) => {
            store.finish_run(&run_id, "failed", &error.to_string())?;
            return Err(error);
        }
        Err(_) => (
            "partial",
            json!({"partial":true,"reason":"runtime_budget_exhausted"}),
        ),
    };
    store.finish_run(&run_id, status, &note.to_string())?;
    Ok(json!({"run_id":run_id,"status":status,"summary":note,"state":store.status()?}))
}

#[allow(clippy::too_many_lines)]
async fn collect(
    store: &Store,
    client: &HttpClient,
    config: &Config,
    options: &RunOptions,
) -> Result<Value> {
    let mut queries = VecDeque::new();
    let started = timestamp();
    if options.only_source.is_none() {
        for query in &config.queries {
            if !query.enabled
                || options
                    .provider
                    .as_ref()
                    .is_some_and(|p| p != &query.provider && p != &query.id)
            {
                continue;
            }
            let coverage = store.coverage(query)?;
            if options.force || coverage.next_due_at <= started {
                let mut query = query.clone();
                query.cursor = coverage.cursor;
                queries.push_back(query);
            }
        }
    }
    let mut checked = HashSet::new();
    let mut verifications = 0_u64;
    let mut partial = false;
    let mut reasons = Vec::new();
    let mut discovered = 0_u64;
    loop {
        let mut progressed = false;
        if let Some(query) = queries.pop_front() {
            progressed = true;
            let mut coverage = store.coverage(&query)?;
            let query = adapters::prepare_query(&query, coverage.high_watermark.as_deref())?;
            coverage.cursor = query.cursor.clone();
            coverage.last_attempt_at = Some(now());
            coverage.status = "running".into();
            store.save_coverage(&coverage)?;
            match adapters::discover(client, &query).await {
                Ok(result) => {
                    discovered += u64::try_from(result.companies.len())?;
                    let discovery_source = format!("discovery:{}", query.id);
                    coverage.status = if result.outcome.is_empty() {
                        if result.complete {
                            "complete"
                        } else {
                            "partial"
                        }
                        .into()
                    } else {
                        result.outcome
                    };
                    coverage.cursor = result.next_cursor.clone();
                    coverage.note =
                        (!result.warnings.is_empty()).then(|| result.warnings.join("; "));
                    if result.complete {
                        if query.provider == "theirstack" {
                            coverage.high_watermark = query
                                .cursor
                                .as_ref()
                                .and_then(|v| v.get("body"))
                                .and_then(|v| v.get("discovered_at_lte"))
                                .and_then(Value::as_str)
                                .map(str::to_owned);
                        }
                        coverage.last_complete_at = Some(now());
                        coverage.next_due_at = timestamp() + i64::try_from(query.interval_seconds)?;
                    } else if result.next_cursor.is_some()
                        && !coverage.status.contains("budget")
                        && !coverage.status.contains("error")
                        && !coverage.status.contains("capped")
                    {
                        let mut next = query;
                        next.cursor = result.next_cursor;
                        queries.push_back(next);
                    } else {
                        partial = true;
                        coverage.next_due_at = timestamp() + 3600;
                        reasons.push(format!("{}: {}", query.id, coverage.status));
                    }
                    store.record_discovery(&result.companies, &discovery_source, &coverage)?;
                }
                Err(error) => {
                    partial = true;
                    coverage.status = if error.contains("budget_exhausted") {
                        "partial_budget"
                    } else if error.contains("credential") || error.contains("API_KEY") {
                        "credentials_unavailable"
                    } else {
                        "partial_error"
                    }
                    .into();
                    coverage.note = Some(error);
                    coverage.next_due_at = timestamp() + 3600;
                    reasons.push(format!("{}: {}", query.id, coverage.status));
                    store.save_coverage(&coverage)?;
                }
            }
        }
        if verifications < config.max_verifications_per_run {
            let source = store.sources()?.into_iter().find(|s| {
                s.enabled
                    && !checked.contains(&s.id)
                    && (options.force || s.next_due_at <= started)
                    && options.only_source.as_ref().is_none_or(|id| id == &s.id)
            });
            if let Some(mut source) = source {
                progressed = true;
                verifications += 1;
                source.last_attempt_at = Some(now());
                source.status = "running".into();
                store.save_source(&source)?;
                match adapters::verify(client, &source.url, source.cursor.as_ref()).await {
                    Ok(result) => {
                        let continuation = result.next_cursor.is_some() && !result.complete;
                        store.record_verification(
                            &source,
                            &result,
                            config.careers_interval_seconds,
                        )?;
                        if !continuation {
                            checked.insert(source.id.clone());
                            if !result.complete {
                                partial = true;
                                reasons.push(format!("{}: {}", source.id, result.outcome));
                                let mut saved = store.source(&source.id)?;
                                saved.next_due_at =
                                    timestamp() + i64::try_from(config.careers_interval_seconds)?;
                                store.save_source(&saved)?;
                            }
                        }
                        if result.outcome.contains("error") || result.outcome.contains("budget") {
                            partial = true;
                            checked.insert(source.id);
                        }
                    }
                    Err(error) => {
                        partial = true;
                        source.status = if error.contains("budget_exhausted") {
                            "partial_budget"
                        } else {
                            "partial_error"
                        }
                        .into();
                        source.note = Some(error);
                        source.next_due_at = timestamp() + 3600;
                        checked.insert(source.id.clone());
                        store.save_source(&source)?;
                    }
                }
            }
        }
        if !progressed {
            break;
        }
    }
    let pending = store
        .sources()?
        .iter()
        .filter(|s| s.enabled && s.next_due_at <= started && !checked.contains(&s.id))
        .count();
    if pending > 0 {
        partial = true;
        reasons.push(format!("{pending} careers sources remain due"));
    }
    Ok(
        json!({"partial":partial,"company_observations":discovered,"verification_pages":verifications,"remaining_sources":pending,"reasons":reasons}),
    )
}
