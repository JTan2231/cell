use crate::{
    Result, adapters,
    http::HttpClient,
    models::{Budgets, Config, Job, JobDraft, VerificationResult},
    now,
    store::Store,
    timestamp,
};
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

/// Resolves a retained job or observes only the supplied public posting.
///
/// # Errors
/// Returns an error when the URL does not identify one supported posting or
/// collection cannot retain that posting.
pub async fn collect_job(store: &Store, url: &str) -> Result<Job> {
    collect_job_with(store, url, adapters::verify).await
}

async fn collect_job_with(
    store: &Store,
    url: &str,
    verify: impl AsyncFnMut(
        &HttpClient,
        &str,
        Option<&Value>,
    ) -> std::result::Result<VerificationResult, String>,
) -> Result<Job> {
    adapters::validate_job_url(url)?;
    let _lock = store.lock()?;
    if let Some(job) = selected_job(store, url)? {
        return Ok(job);
    }
    let config = store.config()?;
    let source = store.add_target_source(url)?;
    let run_id = store.start_run()?;
    let client = collection_client(store, &run_id, &config.budgets)?;
    let result = tokio::time::timeout(
        Duration::from_secs(config.budgets.runtime_seconds),
        collect_target(&client, url, config.max_verifications_per_run, verify),
    )
    .await
    .map_err(|_| "budget_exhausted: runtime limit".into())
    .and_then(|result| result)
    .and_then(|(draft, pages)| Ok((store.record_job_observation(&source, &draft)?, pages)));
    let mut note = json!({
        "scope": "job", "url": crate::normalize_url(url)?, "source_id": source.id,
    });
    match result {
        Ok((job, pages)) => {
            note["job_id"] = json!(job.id);
            note["verification_pages"] = json!(pages);
            store.finish_run(&run_id, "complete", &note.to_string())?;
            Ok(job)
        }
        Err(error) => {
            note["error"] = json!(error.to_string());
            store.finish_run(&run_id, "failed", &note.to_string())?;
            Err(error)
        }
    }
}

async fn collect_target(
    client: &HttpClient,
    url: &str,
    max_pages: u64,
    mut verify: impl AsyncFnMut(
        &HttpClient,
        &str,
        Option<&Value>,
    ) -> std::result::Result<VerificationResult, String>,
) -> Result<(JobDraft, u64)> {
    let mut cursor = None;
    let mut selected = None;
    for page in 1..=max_pages {
        let result = verify(client, url, cursor.as_ref()).await?;
        if !matches!(
            result.outcome.as_str(),
            "complete" | "partial" | "observed" | "resolved"
        ) {
            return Err(format!("targeted collection failed: {}", result.outcome).into());
        }
        for draft in result.jobs {
            if adapters::job_draft_matches(&draft, url)? {
                if selected.is_some() {
                    return Err("selected URL matches multiple extracted postings".into());
                }
                selected = Some(draft);
            }
        }
        if result.complete || result.next_cursor.is_none() {
            return selected.map(|job| (job, page)).ok_or_else(|| {
                "selected URL did not resolve to one supported public job posting".into()
            });
        }
        cursor = result.next_cursor;
    }
    Err("budget_exhausted: targeted collection page limit".into())
}

fn selected_job(store: &Store, url: &str) -> Result<Option<Job>> {
    let mut selected = store
        .snapshot()?
        .jobs
        .into_iter()
        .filter_map(|job| match adapters::job_url_matches(&job, url) {
            Ok(true) => Some(Ok(job)),
            Ok(false) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if selected.len() > 1 {
        return Err("selected URL matches multiple retained jobs".into());
    }
    Ok(selected.pop())
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
    let client = collection_client(store, &run_id, &config.budgets)?;
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

fn collection_client(store: &Store, run_id: &str, budgets: &Budgets) -> Result<HttpClient> {
    let reserve_store = store.clone();
    let reserve_run = run_id.to_owned();
    let budgets = budgets.clone();
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
    Ok(HttpClient::new(reserve, settle)?)
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
                    && config.allows_automatic_url(&s.url)
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
        .filter(|s| {
            s.enabled
                && config.allows_automatic_url(&s.url)
                && s.next_due_at <= started
                && !checked.contains(&s.id)
                && options.only_source.as_ref().is_none_or(|id| id == &s.id)
        })
        .count();
    if pending > 0 {
        partial = true;
        reasons.push(format!("{pending} careers sources remain due"));
    }
    Ok(
        json!({"partial":partial,"company_observations":discovered,"verification_pages":verifications,"remaining_sources":pending,"reasons":reasons}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CompanyDraft;

    fn posting(id: &str) -> JobDraft {
        JobDraft {
            source_key: format!("ashby:hadrian-automation:{id}"),
            url: format!("https://jobs.ashbyhq.com/hadrian-automation/{id}"),
            title: format!("Engineer {id}"),
            is_listed: true,
            ..JobDraft::default()
        }
    }

    fn board(jobs: Vec<JobDraft>) -> VerificationResult {
        VerificationResult {
            jobs,
            complete: true,
            outcome: "complete".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn targeted_observation_preserves_disabled_source_neighbors_and_pending_scan()
    -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::init(directory.path())?;
        let mut config = Config {
            automatic_excluded_ats: vec![],
            ..Default::default()
        };
        store.set_config(&config)?;
        let source = store.add_manual_source(&posting("one").url, None)?;
        store.record_verification(&source, &board(vec![posting("one"), posting("two")]), 86400)?;
        store.record_verification(
            &store.source(&source.id)?,
            &VerificationResult {
                jobs: vec![posting("one")],
                next_cursor: Some(json!({"skip": 1})),
                outcome: "partial".into(),
                ..Default::default()
            },
            86400,
        )?;
        store.disable_source(&source.id)?;
        config.automatic_excluded_ats = vec!["ashby".into()];
        store.set_config(&config)?;
        let before = store.snapshot()?;
        let url = posting("selected").url;
        let selected = collect_job_with(&store, &url, async |_, requested, cursor| {
            assert_eq!(requested, url);
            assert!(
                cursor.is_none(),
                "targeted work must not resume the board scan"
            );
            Ok(board(vec![
                posting("selected"),
                posting("unrequested"),
                JobDraft {
                    title: "Changed neighbor".into(),
                    ..posting("one")
                },
            ]))
        })
        .await?;
        assert_eq!(selected.source_key, "ashby:hadrian-automation:selected");
        assert_eq!(selected.company_id, source.company_id);
        assert_eq!(selected.availability, "listed");
        let after = store.snapshot()?;
        assert_eq!(after.jobs.len(), before.jobs.len() + 1);
        assert_eq!(
            serde_json::to_value(&after.source_health)?,
            serde_json::to_value(&before.source_health)?
        );
        for original in &before.jobs {
            let retained = after
                .jobs
                .iter()
                .find(|job| job.id == original.id)
                .ok_or("neighbor lost")?;
            assert_eq!(
                serde_json::to_value(retained)?,
                serde_json::to_value(original)?
            );
        }
        let last_run = store.status()?["last_run"].clone();
        let repeat = collect_job_with(&store, &url, async |_, _, _| {
            panic!("retained job must not fetch")
        })
        .await?;
        assert_eq!(repeat.id, selected.id);
        assert_eq!(store.status()?["last_run"], last_run);

        // Finish the pending ordinary scan after reopening to prove that its seen set survived.
        config.automatic_excluded_ats.clear();
        store.set_config(&config)?;
        drop(store);
        let store = Store::open(directory.path())?;
        store.record_verification(
            &store.source(&source.id)?,
            &board(vec![posting("two")]),
            86400,
        )?;
        for original in before.jobs {
            let current = store
                .snapshot()?
                .jobs
                .into_iter()
                .find(|job| job.id == original.id)
                .ok_or("neighbor lost")?;
            assert_eq!(current.availability, "listed");
            assert_eq!(current.missing_complete_snapshots, 0);
        }
        Ok(())
    }

    #[tokio::test]
    async fn targeted_sources_preserve_enrollment_and_filter_across_pages() -> Result<()> {
        for existing_enabled in [None, Some(true), Some(false)] {
            let directory = tempfile::tempdir()?;
            let store = Store::init(directory.path())?;
            let url = posting("selected").url;
            if let Some(enabled) = existing_enabled {
                let source = store.add_manual_source(&url, None)?;
                if !enabled {
                    store.disable_source(&source.id)?;
                }
            }
            let mut calls = 0;
            let job = collect_job_with(&store, &url, async |_, _, cursor| {
                calls += 1;
                if calls == 1 {
                    assert!(cursor.is_none());
                    Ok(VerificationResult {
                        jobs: vec![posting("other")],
                        next_cursor: Some(json!({"skip": 100})),
                        outcome: "partial".into(),
                        ..Default::default()
                    })
                } else {
                    assert_eq!(cursor, Some(&json!({"skip": 100})));
                    Ok(board(vec![posting("selected"), posting("another")]))
                }
            })
            .await?;
            assert_eq!(calls, 2);
            assert_eq!(store.snapshot()?.jobs.len(), 1);
            assert_eq!(
                store.source(&job.source_id)?.enabled,
                existing_enabled.unwrap_or(false)
            );
            let run = store.status()?["last_run"].clone();
            assert_eq!(run["status"], "complete");
            let note: Value = serde_json::from_str(run["note"].as_str().ok_or("run note")?)?;
            assert_eq!(note["scope"], "job");
            assert_eq!(note["job_id"], job.id);
            assert_eq!(note["verification_pages"], 2);
        }
        Ok(())
    }

    #[tokio::test]
    async fn targeted_failure_retains_no_jobs_or_board_progress() -> Result<()> {
        for failure in ["absent", "ambiguous", "transport", "page_limit"] {
            let directory = tempfile::tempdir()?;
            let store = Store::init(directory.path())?;
            store.set_config(&Config {
                max_verifications_per_run: 1,
                ..Default::default()
            })?;
            let url = posting("selected").url;
            let source = store.add_target_source(&url)?;
            let result = collect_job_with(&store, &url, async |_, _, _| match failure {
                "absent" => Ok(board(vec![posting("other")])),
                "ambiguous" => Ok(board(vec![posting("selected"), posting("selected")])),
                "transport" => Err("HTTP transport failed".into()),
                _ => Ok(VerificationResult {
                    jobs: vec![posting("selected")],
                    next_cursor: Some(json!({"skip": 100})),
                    outcome: "partial".into(),
                    ..Default::default()
                }),
            })
            .await;
            let error = result
                .err()
                .ok_or("targeted collection must fail")?
                .to_string();
            let expected = match failure {
                "absent" => "did not resolve",
                "ambiguous" => "multiple extracted",
                "transport" => "HTTP transport",
                _ => "page limit",
            };
            assert!(error.contains(expected), "{error}");
            assert!(store.snapshot()?.jobs.is_empty());
            assert_eq!(
                serde_json::to_value(store.source(&source.id)?)?,
                serde_json::to_value(source)?
            );
            assert_eq!(store.status()?["last_run"]["status"], "failed");
        }
        Ok(())
    }

    #[tokio::test]
    async fn targeted_collection_keeps_real_http_budgets_and_writer_lock() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::init(directory.path())?;
        let url = posting("selected").url;
        let mut config = Config::default();
        config.budgets.http_per_run = 0;
        store.set_config(&config)?;
        let lock = store.lock()?;
        let error = collect_job(&store, &url)
            .await
            .err()
            .ok_or("writer must be blocked")?;
        assert!(error.to_string().contains("another Cast mutation"));
        assert!(store.snapshot()?.source_health.is_empty());
        drop(lock);
        for daily in [false, true] {
            config.budgets.http_per_run = if daily { 500 } else { 0 };
            config.budgets.http_daily = if daily { 0 } else { 3000 };
            store.set_config(&config)?;
            let error = collect_job(&store, &url)
                .await
                .err()
                .ok_or("budget must block HTTP")?;
            assert!(error.to_string().contains("budget_exhausted"));
            assert!(store.snapshot()?.jobs.is_empty());
            assert_eq!(store.status()?["http_requests_today"], 0);
            assert_eq!(store.status()?["last_run"]["status"], "failed");
        }
        Ok(())
    }

    #[tokio::test]
    async fn ordinary_collection_excludes_existing_and_new_ashby_sources_and_postings() -> Result<()>
    {
        let directory = tempfile::tempdir()?;
        let store = Store::init(directory.path())?;
        let mut config = Config {
            queries: vec![],
            ..Default::default()
        };
        config.budgets.http_per_run = 0;
        store.set_config(&config)?;
        let source = store.add_manual_source(&posting("existing").url, None)?;
        let query = Config::default().queries.remove(0);
        let mut coverage = store.coverage(&query)?;
        coverage.status = "complete".into();
        let allowed = JobDraft {
            url: "https://employer.example/jobs/allowed".into(),
            source_key: "allowed".into(),
            ..posting("allowed")
        };
        store.record_discovery(
            &[CompanyDraft {
                name: "Employer".into(),
                website_url: Some(source.url.clone()),
                careers_urls: vec!["https://jobs.ashbyhq.com/new-employer".into()],
                jobs: vec![
                    posting("discovered"),
                    allowed.clone(),
                    JobDraft {
                        url: "https://employer.example/jobs/ashby".into(),
                        apply_url: Some(posting("apply").url),
                        ..posting("apply")
                    },
                ],
                ..Default::default()
            }],
            "discovery:fixture",
            &coverage,
        )?;
        assert_eq!(store.snapshot()?.jobs.len(), 1);
        assert_eq!(store.snapshot()?.jobs[0].url, allowed.url);
        assert_eq!(store.snapshot()?.source_health.len(), 2);
        let before = store.snapshot()?.source_health;
        let result = run(
            &store,
            RunOptions {
                force: true,
                ..Default::default()
            },
        )
        .await?;
        assert_eq!(result["status"], "complete");
        assert_eq!(result["summary"]["verification_pages"], 0);
        assert_eq!(result["summary"]["remaining_sources"], 0);
        assert_eq!(
            serde_json::to_value(store.snapshot()?.source_health)?,
            serde_json::to_value(before)?
        );
        assert_eq!(store.status()?["http_requests_today"], 0);
        config.automatic_excluded_ats.clear();
        store.set_config(&config)?;
        let result = run(
            &store,
            RunOptions {
                force: true,
                ..Default::default()
            },
        )
        .await?;
        assert_eq!(
            result["status"], "partial",
            "removing the exclusion should admit budget-limited requests"
        );
        Ok(())
    }
}
